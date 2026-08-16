use clap::Parser;
use crossbeam_channel::{bounded, Receiver, Sender};
use filetime::{set_file_times, FileTime};
use indicatif::{ProgressBar, ProgressStyle};
use jwalk::WalkDir;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// TurboCopy: Bare-metal multi-threaded file copy engine tuned for Windows/Unix.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Source directory path
    #[arg(short, long)]
    source: PathBuf,

    /// Destination directory path
    #[arg(short, long)]
    destination: PathBuf,

    /// Number of worker threads (Defaults to 2x logical cores)
    #[arg(short, long)]
    threads: Option<usize>,
}

#[derive(Clone)]
struct CopyTask {
    src: PathBuf,
    dst: PathBuf,
    size: u64,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    if !args.source.exists() {
        eprintln!("❌ Error: Source path {:?} does not exist.", args.source);
        std::process::exit(1);
    }

    let worker_count = args.threads.unwrap_or_else(|| num_cpus::get() * 2);

    println!("🚀 TurboCopy Engine Initialized");
    println!("   Source:       {:?}", args.source);
    println!("   Destination:  {:?}", args.destination);
    println!("   Worker Pool:  {} threads", worker_count);
    println!("--------------------------------------------------");

    let start_time = Instant::now();

    // ------------------------------------------------------------------
    // PHASE 1: Parallel Directory Scan
    // ------------------------------------------------------------------
    println!("🔍 Indexing source directory tree...");
    let mut tasks = Vec::new();
    let mut total_bytes: u64 = 0;

    for entry in WalkDir::new(&args.source).skip_hidden(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_file() {
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let rel_path = match path.strip_prefix(&args.source) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let dest_path = args.destination.join(rel_path);
            let size = metadata.len();

            total_bytes += size;
            tasks.push(CopyTask {
                src: path,
                dst: dest_path,
                size,
            });
        }
    }

    let total_files = tasks.len();
    println!(
        "✔ Indexed {} files ({:.2} GB) in {:.2?}",
        total_files,
        total_bytes as f64 / 1_073_741_824.0,
        start_time.elapsed()
    );

    if total_files == 0 {
        println!("Nothing to copy!");
        return Ok(());
    }

    // ------------------------------------------------------------------
    // PHASE 2: UI Progress Bar Setup
    // ------------------------------------------------------------------
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, eta {eta})")
            .unwrap()
            .progress_chars("#>-")
    );

    let bytes_copied_atomic = Arc::new(AtomicU64::new(0));
    let files_copied_atomic = Arc::new(AtomicU64::new(0));

    // ------------------------------------------------------------------
    // PHASE 3: Concurrent Worker Pipeline
    // ------------------------------------------------------------------
    let (sender, receiver): (Sender<CopyTask>, Receiver<CopyTask>) = bounded(20_000);

    // Spawn Worker Threads
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let rx = receiver.clone();
        let bytes_counter = Arc::clone(&bytes_copied_atomic);
        let files_counter = Arc::clone(&files_copied_atomic);

        let handle = thread::spawn(move || {
            let mut buffer = [0u8; 65536]; // Pre-allocated 64KB buffer per thread

            while let Ok(task) = rx.recv() {
                let win_src = prepare_path(&task.src);
                let win_dst = prepare_path(&task.dst);

                if let Some(parent) = win_dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                // File write with 3 retries for locked/busy file handles
                let mut retries = 3;
                let mut success = false;

                while retries > 0 {
                    if copy_file_chunked(&win_src, &win_dst, &mut buffer, &bytes_counter).is_ok() {
                        success = true;
                        break;
                    }
                    retries -= 1;
                    thread::sleep(Duration::from_millis(20));
                }

                // Fallback attempt
                if !success {
                    let _ = fs::copy(&win_src, &win_dst);
                }

                // Preserve original timestamps
                if let Ok(metadata) = fs::metadata(&win_src) {
                    let atime = FileTime::from_last_access_time(&metadata);
                    let mtime = FileTime::from_last_modification_time(&metadata);
                    let _ = set_file_times(&win_dst, atime, mtime);
                }

                files_counter.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }

    // Spawn Progress Bar Sync Thread (~30 FPS render frequency)
    let pb_clone = pb.clone();
    let bytes_counter_clone = Arc::clone(&bytes_copied_atomic);
    let sync_handle = thread::spawn(move || {
        while bytes_counter_clone.load(Ordering::Relaxed) < total_bytes {
            let current_bytes = bytes_counter_clone.load(Ordering::Relaxed);
            pb_clone.set_position(current_bytes);
            thread::sleep(Duration::from_millis(33));
        }
        pb_clone.set_position(total_bytes);
    });

    // Feed Tasks into Channel Pipeline
    for task in tasks {
        let _ = sender.send(task);
    }
    drop(sender); // Signal workers to finish and exit

    // Wait for all workers and UI thread to complete
    for handle in handles {
        let _ = handle.join().unwrap();
    }
    let _ = sync_handle.join().unwrap();

    pb.finish_with_message("Transfer Complete!");

    let duration = start_time.elapsed();
    let avg_speed_mb = (total_bytes as f64 / 1_048_576.0) / duration.as_secs_f64();

    println!("--------------------------------------------------");
    println!("🎉 Transfer Finished Successfully!");
    println!("Total Files:   {}", files_copied_atomic.load(Ordering::Relaxed));
    println!("Total Time:    {:.2?}", duration);
    println!("Average Speed: {:.2} MB/s", avg_speed_mb);

    Ok(())
}

/// Chunked copy function that updates atomic byte counter in real time
fn copy_file_chunked(
    src: &Path,
    dst: &Path,
    buffer: &mut [u8],
    bytes_counter: &Arc<AtomicU64>,
) -> io::Result<()> {
    let mut reader = File::open(src)?;
    let mut writer = File::create(dst)?;

    loop {
        let bytes_read = reader.read(buffer)?;
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buffer[..bytes_read])?;
        bytes_counter.fetch_add(bytes_read as u64, Ordering::Relaxed);
    }

    Ok(())
}

/// Normalizes paths with \\?\ prefix on Windows to bypass 260-char MAX_PATH limit
fn prepare_path(path: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        let path_str = path.to_string_lossy();
        if !path_str.starts_with(r"\\?\") {
            return PathBuf::from(format!(r"\\?\{}", path_str));
        }
    }
    path.to_path_buf()
}