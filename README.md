# ⚡ TurboCopy

A blazing-fast, multi-threaded file copy engine built in **Rust**, designed to reduce I/O bottlenecks when transferring tens of thousands of small files.


\

---

## 🛑 The Problem

Transferring large directories containing tens of thousands of small, loose files — such as `node_modules`, build artifacts, virtual environments, or datasets — can be surprisingly slow with standard file managers.

Traditional copy routines can be limited by:

* **Sequential file processing:** Files may spend significant time waiting on filesystem operations.
* **Filesystem metadata overhead:** File creation, directory updates, indexing, and security checks add latency.
* **Small-file I/O:** Thousands of tiny files create substantial per-file overhead compared with a single large transfer.
* **Progress/UI overhead:** Frequent progress updates can unnecessarily compete with the actual transfer workload.

For example, a 5 GB directory containing more than 20,000 files can take **35–40 minutes** to transfer using standard Windows Explorer on some systems.

---

## 🚀 The Solution: TurboCopy

**TurboCopy** is built from the ground up in Rust and uses concurrent systems-programming techniques to improve throughput for large collections of small files.

### Key Features

* ⚡ **Parallel Directory Traversal (****`jwalk`****)**
  Walks nested directory trees in parallel across CPU cores, allowing the transfer workload to be discovered quickly.

* 🔄 **Multi-Threaded Worker Pool (****`crossbeam-channel`****)**
  Uses a bounded channel to distribute file-copy tasks across a configurable worker pool.

* ⏱️ **Decoupled Progress Tracking (****`AtomicU64`****)**
  Worker threads update shared atomic counters while a dedicated UI loop renders progress independently.

* 🛡️ **Windows Long Path Support**
  Supports Windows extended paths using the `\\?\` path prefix to avoid traditional `MAX_PATH` limitations.

* 📅 **Timestamp Preservation (****`filetime`****)**
  Preserves source file timestamps during transfers.

* 🔁 **Locked File Retries**
  Temporarily locked files can be retried with exponential backoff before the application falls back gracefully.

---

## 📊 Real-World Benchmarks

> **Test Environment:** Intel Core i5/i7 • 4 Cores / 8 Threads • Windows 11
> **Target Payload:** 20,045 files • ~5.0 GB

| Workload                  | Windows Explorer |                 TurboCopy | Performance Gain |
| :------------------------ | ---------------: | ------------------------: | ---------------: |
| **Local SSD Transfer**    |       ~35–40 min |    **3.78 min** (226.87s) | **~8.5× faster** |
| **USB Flash Drive Write** |          ~40 min | **19.63 min** (1,178.22s) |   **~2× faster** |
| **USB Flash Drive Read**  |       ~12–15 min |    **6.28 min** (376.85s) | **~2.2× faster** |

### USB Read Performance

The USB read benchmark achieved approximately **13.21 MB/s sustained throughput**, demonstrating that TurboCopy can substantially reduce software-side overhead even when the storage device itself becomes the limiting factor.

> **Note:** Benchmark results depend heavily on the filesystem, storage device, CPU, antivirus software, directory structure, and number/size of files. Results should therefore be treated as workload-specific rather than universal performance guarantees.

---

## 🛠️ Installation & Building

### Prerequisites

* [Rust & Cargo](https://rustup.rs/) — Rust 1.70+ recommended

### Build from Source

Clone the repository and compile the optimized release binary:

```bash
git clone https://github.com/your-username/turbo_copy.git
cd turbo_copy

cargo build --release
```

The optimized executable will be located at:

**Windows**

```text
target/release/turbo_copy.exe
```

**Linux / macOS**

```text
target/release/turbo_copy
```

---

## 💻 Usage

### Basic CLI Command

#### Windows

```cmd
.\target\release\turbo_copy.exe -s "D:\SourceFolder" -d "E:\DestinationFolder"
```

#### Linux / macOS

```bash
./target/release/turbo_copy -s "/path/to/source" -d "/path/to/destination"
```

---

## ⚙️ CLI Options

```text
Options:
  -s, --source <PATH>        Source directory path [required]
  -d, --destination <PATH>   Destination directory path [required]
  -t, --threads <NUMBER>     Number of worker threads
                             (default: 2x logical CPU cores)
  -h, --help                 Print help information
  -V, --version              Print version information
```

---

## 🔧 Tuning Worker Threads

For workloads containing massive numbers of very small files, increasing the number of worker threads can help hide per-file filesystem and metadata latency.

For example:

```cmd
.\target\release\turbo_copy.exe -s "D:\Source" -d "E:\Dest" -t 32
```

### Recommended Approach

Start with the default configuration and increase the thread count gradually.

For example:

```text
2× logical cores
        ↓
16 threads
        ↓
32 threads
        ↓
64 threads
```

The optimal value depends on the storage device and workload. More threads do **not** necessarily mean better performance, particularly when the destination storage is already saturated.

---

## 🖱️ Windows Context Menu Integration

TurboCopy can be integrated into the Windows right-click menu for convenient folder transfers.

### 1. Create the Registry File

Create a file named:

```text
install_context_menu.reg
```

### 2. Add the Following

Update the executable path to match your installation:

```registry
Windows Registry Editor Version 5.00

[HKEY_CLASSES_ROOT\Directory\shell\TurboCopy]
@="⚡ TurboCopy to..."
"Icon"="D:\\Path\\To\\turbo_copy\\target\\release\\turbo_copy.exe"

[HKEY_CLASSES_ROOT\Directory\shell\TurboCopy\command]
@="\"D:\\Path\\To\\turbo_copy\\target\\release\\turbo_copy.exe\" --source \"%1\""
```

### 3. Register the Context Menu

Double-click the `.reg` file and accept the Windows Registry prompts.

After installation, TurboCopy will appear when right-clicking supported directories in Windows Explorer.

> **Note:** The context-menu command must match the CLI interface implemented by the current TurboCopy version. If the application requires both source and destination arguments, the registry integration should be adapted accordingly.

---

## 🏗️ Architecture

TurboCopy separates directory discovery, file-copy workers, progress tracking, and UI rendering.

```text
┌────────────────────────────────────────────────────────┐
│                   Parallel Scanner                     │
│                (jwalk Directory Walk)                  │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼
             [ Lock-Free Bounded Queue ]
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
  [ Worker 1 ]       [ Worker 2 ]       [ Worker N ]
   (64KB Buff)        (64KB Buff)        (64KB Buff)
        │                  │                  │
        └──────────────────┼──────────────────┘
                           │
                           │ Updates
                           │
                           ▼
               [ Atomic Byte/File Counters ]
                           │
                           ▼
               [ UI Render Loop (30 FPS) ]
                     (indicatif CLI)
```

### Data Flow

```text
Source Directory
       │
       ▼
Parallel Directory Scanner
       │
       ▼
File Transfer Queue
       │
       ├──────────────┬──────────────┐
       ▼              ▼              ▼
   Worker 1       Worker 2       Worker N
       │              │              │
       └──────────────┴──────────────┘
                      │
                      ▼
               Destination
                      │
                      ▼
             Atomic Statistics
                      │
                      ▼
                Progress UI
```

---

## 🧩 Core Technologies

| Component            | Technology            | Purpose                               |
| :------------------- | :-------------------- | :------------------------------------ |
| Language             | **Rust**              | Systems-level performance and safety  |
| Directory traversal  | **jwalk**             | Parallel filesystem traversal         |
| Worker communication | **crossbeam-channel** | Efficient bounded task queue          |
| Progress display     | **indicatif**         | Terminal progress UI                  |
| Atomic counters      | **AtomicU64**         | Low-overhead shared statistics        |
| Timestamp handling   | **filetime**          | File timestamp preservation           |
| Build system         | **Cargo**             | Dependency management and compilation |

---

## 📈 Why Rust?

TurboCopy is intentionally implemented in Rust because the workload is heavily dependent on filesystem operations, concurrency, memory management, and efficient system-level execution.

Rust provides:

* Low runtime overhead
* Native compiled performance
* Memory safety without garbage collection
* Strong concurrency guarantees
* Excellent cross-platform support
* Fine-grained control over I/O and threading

This makes Rust a strong fit for a high-throughput file-transfer utility.

---

## 🗺️ Roadmap

* [x] Parallel directory scanner
* [x] Multi-threaded file-copy engine
* [x] Configurable worker thread count
* [x] Windows long-path (`\\?\`) support
* [x] Timestamp preservation
* [x] Locked-file retry mechanism
* [x] Context-menu integration via Windows Registry
* [ ] Native desktop GUI using `eframe` / `egui`
* [ ] Optional in-memory ZIP/TAR streaming
* [ ] CRC32 integrity verification
* [ ] SHA-256 integrity verification
* [ ] Transfer resume support
* [ ] Copy cancellation
* [ ] Detailed transfer statistics
* [ ] Cross-platform context-menu integration

---

## 🔐 Reliability & Data Integrity

TurboCopy is designed to improve transfer throughput while maintaining normal filesystem semantics.

Future integrity-verification support will provide optional post-copy validation using:

```text
Source File
     │
     ▼
SHA-256 / CRC32
     │
     ▼
Destination File
     │
     ▼
Hash Comparison
     │
 ┌───┴────┐
 ▼        ▼
MATCH   MISMATCH
```

This will allow users to trade additional processing time for stronger verification guarantees.

---

## 🤝 Contributing

Contributions, bug reports, benchmarks, and performance improvements are welcome.

If you discover an issue:

1. Reproduce the problem.
2. Record your operating system and storage configuration.
3. Include the relevant TurboCopy command.
4. Open an issue with the details.

For performance contributions, benchmark results are especially useful.

---

## 📄 License

TurboCopy is licensed under the **MIT License**.

See the [`LICENSE`](LICENSE) file for the complete license text.

---

## ⭐ Project Goals

TurboCopy is built around a simple idea:

> **Make copying thousands of small files as fast and efficient as possible.**

By combining Rust's low-level performance with parallel directory traversal, concurrent workers, efficient task distribution, and decoupled progress reporting, TurboCopy aims to provide a faster alternative for workloads where traditional file-copy tools struggle.

**Fast. Concurrent. Lightweight. Built in Rust. ⚡**
