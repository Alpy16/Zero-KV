# Zero-KV: High-Performance Disk-Backed Storage Engine

**A specialized key-value retrieval engine optimized for sub-microsecond latency through memory-mapped I/O and zero-copy abstractions.**

Zero-KV is engineered to bridge the performance gap between persistent disk storage and volatile RAM. By leveraging the `mmap(2)` system call, the engine treats on-disk binary structures as an extension of the process address space. This architecture allows the operating system kernel to manage page caching and demand-paging, enabling the engine to serve data directly from the kernel page cache to the network interface without intermediate CPU copies or heap allocations.

---

## Core Engineering Principles

* **Zero-Copy Architecture**: Utilizing the `zerocopy` crate, incoming network frames and on-disk structures are interpreted as typed references rather than being parsed or deserialized.
* **Mechanical Sympathy**: The binary schema is strictly aligned to 8-byte boundaries. This ensures that 64-bit keys and offsets reside on natural CPU word boundaries, preventing the significant performance penalties associated with unaligned memory access.
* **Asynchronous Concurrency**: Built on the `tokio` runtime, the engine utilizes a non-blocking multi-threaded executor. A shared-state architecture managed via `Arc` (Atomic Reference Counting) allows thousands of concurrent "researcher" tasks to query the memory-mapped index without lock contention.
* **Deterministic Retrieval**: The engine utilizes a pre-sorted binary index, enabling $O(\log n)$ lookups via binary search. This provides predictable P99 latency profiles compared to hash-based engines that may suffer from re-indexing or collision overhead.

---

## Technical Roadmap

### Stage 1: The Baker (Complete)

A high-efficiency serialization tool that transforms raw datasets into a memory-aligned binary format. It performs the necessary sorting and padding during the build phase to eliminate runtime overhead.

### Stage 2: The Mmap Reader (Complete)

The retrieval core that maps the binary database into virtual memory. It supports datasets larger than physical RAM by relying on the OS page cache for intelligent data persistence.

### Stage 3: The Protocol (Complete)

A rigorous, fixed-width binary specification for network communication.

* **Request Frame**: 16-byte aligned (4-byte OpCode, 4-byte Padding, 8-byte Key).
* **Response Frame**: 8-byte header (4-byte Status, 4-byte Length) followed by raw data.

### Stage 4: The Async Server (Complete)

A multi-threaded TCP listener utilizing `tokio`. This stage includes structured logging via the `tracing` ecosystem for real-time observability of connection lifecycles and lookup performance.

### Stage 5: The Zero-Copy Path (Current Focus)

Optimization of the write path using Vectored I/O (`writev`). This eliminates multiple system calls by aggregating the response header and data payload into a single atomic kernel operation.

### Stage 6: The Benchmarker (Planned)

A high-concurrency load-testing suite designed to validate sub-50 microsecond P99 latencies and generate flamegraphs for kernel-level performance profiling.

---

## Design Rationale: Memory Alignment and Safety

A critical architectural transition occurred during Stage 3, moving from a 12-byte packed request format to a 16-byte aligned format.

| Feature | Packed Layout | Aligned Layout |
| --- | --- | --- |
| **Request Size** | 12 Bytes | 16 Bytes |
| **Memory Access** | Unaligned (Shifted) | **Natural (Single-Cycle)** |
| **Safety Profile** | Potential Undefined Behavior | **Memory Safe / Repr(C)** |

**Engineering Trade-off**: While a packed layout reduces network bandwidth by 25%, it introduces significant CPU overhead and risks hardware-level exceptions on strict-alignment architectures. By introducing 4 bytes of internal padding, the engine ensures that 8-byte keys are naturally aligned, allowing the hardware to fetch data in a single clock cycle.

---

## Binary Format Specification

| Section | Size | Description |
| --- | --- | --- |
| **Header** | 32 Bytes | Magic ID (`0xA016`), Versioning, and Entry Metadata. |
| **Index** | 24 Bytes * N | Contiguous block of sorted 8-byte Keys and Data Offsets. |
| **Data** | Variable | Raw value segments, 8-byte padded to maintain alignment. |

---

## Observability

The engine utilizes structured logging to provide granular visibility into server operations. Log levels are categorized to distinguish between routine lifecycle events and performance-critical warnings:

* **INFO**: Connection establishment, engine initialization, and successful lookups.
* **WARN**: Cache misses or lookups for non-existent keys.
* **ERROR**: Protocol violations or network I/O failures.

---

## Usage

### Build and Serialize

```bash
cargo run --bin baker

```

### Initialize Server

```bash
cargo run --bin kv_store

```

The engine will bind to `127.0.0.1:5500`.

---

**Author**: [Alpy16](https://github.com/Alpy16)

**License**: MIT