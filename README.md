# High-Performance Disk-Backed Storage Engine

A specialized key-value storage engine built in Rust, designed for sub-microsecond lookups. This project focuses on low-level systems programming, zero-copy data transfer, and memory-mapped file I/O.

## Architecture
The engine utilizes `mmap(2)` to treat disk storage as an extension of application memory, allowing the Operating System to handle page caching and hardware-level optimizations.

### Technical Specifications
*   **Zero-Copy:** Data is served directly from the OS page cache to the caller without being copied into intermediate buffers.
*   **O(log n) Lookups:** The index is pre-sorted, enabling lightning-fast binary searches.
*   **8-Byte Alignment:** Structs use `#[repr(C)]` to ensure a predictable memory layout compatible with direct pointer casting.
*   **Memory Efficiency:** The engine maps the file address space rather than loading the entire dataset into RAM.

---

## The 6-Stage Roadmap

### Stage 1: The Baker (Complete)
A CLI tool that sorts a dataset by key and serializes it into a custom binary format.
*   **Header:** A 32-byte metadata block containing magic numbers and item counts.
*   **Index:** A contiguous block of fixed-width `IndexEntry` structs.

### Stage 2: The Mmap Reader (Complete)
The core retrieval logic. This stage involves:
*   Mapping the binary file into the process address space using `memmap2`.
*   Performing unsafe pointer arithmetic to overlay `IndexEntry` blueprints onto raw bytes.
*   A robust `get()` API returning `Option<&[u8]>`.

### Stage 3: The Protocol (Pending)
Defining a rigorous, fixed-width binary frame for network communication to eliminate the overhead of parsing text-based formats like JSON.

### Stage 4: The Async Server (Pending)
Building a high-concurrency TCP listener using `tokio` to allow multiple clients to query the engine simultaneously.

### Stage 5: The Zero-Copy Path (Pending)
Implementing `writev` (vectored I/O) to stream data directly from the memory map to the network socket.

### Stage 6: The Benchmarker (Pending)
A load-testing suite designed to generate flamegraphs and prove P99 latencies under 50 microseconds.

---

## Getting Started

### Installation
```bash
git clone https://github.com/Alpy16/kv_store.git
cd kv_store
```

### Running Tests
The engine includes a clean-room unit test that bakes a temporary database, performs lookups, and verifies data integrity.
```bash
cargo test
```

---

## Binary Format Layout
| Section | Size | Description |
| :--- | :--- | :--- |
| **Header** | 32 Bytes | Magic (0xA016), Version, Entry Count |
| **Index** | 24 Bytes * N | Sorted Keys, Val Offsets, Val Lengths |
| **Data** | Variable | Raw value bytes |

---

**Author:** [Alpy16](https://github.com/Alpy16)  
**License:** MIT