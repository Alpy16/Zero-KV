# High-Performance Disk-Backed Storage Engine

A specialized key-value storage engine built in Rust, designed for sub-microsecond lookups. This project focuses on low-level systems programming, zero-copy data transfer, and memory-mapped file I/O.

## Architecture

The engine utilizes `mmap(2)` to treat disk storage as an extension of application memory, allowing the Operating System to handle page caching and hardware-level optimizations.

### Technical Specifications

* **Zero-Copy:** Data is served directly from the OS page cache to the caller without being copied into intermediate buffers.
* **O(log n) Lookups:** The index is pre-sorted, enabling lightning-fast binary searches.
* **Memory-Aligned:** Structs use `#[repr(C)]` with natural padding to ensure 8-byte alignment, maximizing CPU throughput and satisfying Rust's safety requirements.
* **Memory Efficiency:** The engine maps the file address space rather than loading the entire dataset into RAM.

---

## The 6-Stage Roadmap

### Stage 1: The Baker (Complete)

A CLI tool that sorts a dataset by key and serializes it into a custom binary format.

* **Header:** A 32-byte metadata block containing magic numbers and item counts.
* **Index:** A contiguous block of fixed-width `IndexEntry` structs.

### Stage 2: The Mmap Reader (Complete)

The core retrieval logic. This stage involves:

* Mapping the binary file into the process address space using `memmap2`.
* A robust `get()` API returning `Option<&[u8]>`.

### Stage 3: The Protocol (Complete)

Defined a rigorous, fixed-width binary frame for network communication.

* **Request:** 16-byte frame (4-byte OpCode + 4-byte Padding + 8-byte Key).
* **Response:** 8-byte header (4-byte Status + 4-byte Length).
* **Endianness:** All values use Network Byte Order (Big-Endian) for cross-platform compatibility.

### Stage 4: The Async Server (Pending)

Building a high-concurrency TCP listener using `tokio` to allow multiple clients to query the engine simultaneously.

### Stage 5: The Zero-Copy Path (Pending)

Implementing `writev` (vectored I/O) to stream data directly from the memory map to the network socket.

### Stage 6: The Benchmarker (Pending)

A load-testing suite designed to generate flamegraphs and prove P99 latencies under 50 microseconds.

---

## Technical Note: The "Packed vs. Aligned" Shift

Initially, i utilized `#[repr(packed)]` to achieve a 12-byte request size. However, the project shifted to a 16-byte **aligned** layout for two critical reasons:

1. **Safety:** Rust strictly forbids taking references to unaligned fields in packed structs, as this constitutes Undefined Behavior (UB). Even calling conversion methods on unaligned fields can trigger these protections.
2. **Performance:** Modern CPUs are optimized to read 64-bit integers from addresses that are multiples of 8. By embracing 4 bytes of padding between the OpCode and the Key, we allow the CPU to fetch keys in a single cycle without the heavy penalty of shifted or split memory reads.

---

## Binary Format Layout

| Section | Size | Description |
| --- | --- | --- |
| **Header** | 32 Bytes | Magic (0xA016), Version, Entry Count |
| **Index** | 24 Bytes * N | Sorted Keys, Val Offsets, Val Lengths |
| **Data** | Variable | Raw value bytes |

---

**Author:** [Alpy16](https://github.com/Alpy16)

**License:** MIT