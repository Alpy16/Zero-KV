# Zero-KV

Zero-KV is a hyper-optimized, zero-copy, disk-backed key-value storage engine designed for ultra-low latency local environments. It is built with a focus on "Mechanical Sympathy"—aligning software behavior with CPU and OS kernel characteristics to minimize overhead.

## Features

- **Zero-Copy Architecture:** Utilizes the `zerocopy` and `memmap2` crates to ensure data is never copied between the disk, the application user-space, and the network socket.
- **Unix Domain Sockets (UDS):** Bypasses the TCP/IP stack overhead entirely for sub-microsecond local process communication.
- **Mechanical Sympathy:** Data structures are strictly 8-byte aligned on disk to ensure optimal CPU cache-line efficiency.
- **Syscall Reduction:** Aggressive request batching (4KB reads) and response batching (vectored I/O) minimize kernel context-switching overhead.
- **Pipelined Protocol:** High-concurrency support using fixed-width binary frames (16-byte requests, 8-byte response headers) to eliminate parsing costs.

## Performance Metrics

Running on local benchmark loops (WSL2 environment, 100 concurrent connections):
- **Throughput:** ~3.2 Million requests per second
- **P50 Latency (Median):** ~24.63µs
- **P99 Latency (Tail):** ~124.00µs
- **Max Spike:** ~160.41µs

## Getting Started

### 1. Dataset Compilation (The Baker)
Zero-KV requires an immutable, pre-sorted binary database file. Use the baker utility to generate this from your source dataset:
```bash
cargo run --release --bin baker

```

### 2. Running the Server

The server initializes a Unix Domain Socket at `/tmp/zero-kv.sock`. Run with `RUST_LOG=error` to ensure standard I/O logging operations do not bottleneck the runtime execution loop:

```bash
RUST_LOG=error cargo run --release --bin kv_store

```

### 3. Benchmarking

To launch the high-concurrency pipelined load tester and capture latency distributions:

```bash
cargo run --release --bin benchmarker

```

## Architecture Decision Records (ADR)

### ADR 1: Use of Unix Domain Sockets over TCP

**Context:** Initial benchmarks showed TCP loopback overhead adding unnecessary processing noise and buffering latency per request.
**Decision:** Switched to Unix Domain Sockets (UDS) for all local communications.
**Consequence:** Bypasses IP headers, sequence validation, tracking states, and network checksums, resulting in a direct, kernel-mediated memory pipe for localhost workloads.

### ADR 2: 8-Byte Data Alignment in Storage

**Context:** Unaligned data access can cause a single read operation to straddle two separate CPU cache lines, doubling memory bus transactions.
**Decision:** The compilation utility (`the baker`) forces every serialized record to align strictly on an 8-byte boundary.
**Consequence:** Guarantees that the CPU can load index fields into registers within a single clock cycle, eliminating cache-line splitting overhead.

### ADR 3: Memory-Mapped I/O with Random Advice

**Context:** Standard file descriptor read system calls require copying data blocks across the kernel-to-user-space memory boundary.
**Decision:** Employed `memmap2` to map the database file into the process's virtual address space, combined with an explicit `madvise(Advice::Random)` hint.
**Consequence:** Allows the operating system's Page Cache to handle hardware I/O directly without user-space buffer copies. The random access hint explicitly disables the kernel's sequential read-ahead heuristics, preventing massive page cache churn during non-contiguous binary search jumps.

### ADR 4: Syscall Batching & Vectored I/O

**Context:** Under heavy load, the server hit a performance wall where the CPU spent more time executing context switches to the kernel than processing lookups.
**Decision:** Implemented 4KB batch reads on the incoming socket, paired with pipelining on the client. Integrated `write_vectored` to flush multi-response flights atomically.
**Consequence:** Amortizes the entry cost of a single system call across dozens of distinct queries, shifting the bottleneck from context-switching limitations to maximum hardware capabilities.

### ADR 5: Decoupling Descriptor Lifetimes from Pre-allocated Buffers

**Context:** Rust's borrow checker prevents mutably clearing response header allocations while `IoSlice` objects (which hold immutable references to those headers) still live in a sibling vector within the loop.
**Decision:** Declared the transient `IoSlice` array layout inside the hot loop, while maintaining the underlying vector capacities inside a persistent allocation layer outside the loop.
**Consequence:** Decouples the lifetimes cleanly. This allows the server to clear and reuse memory blocks on every batch transaction without hitting the global allocator heap, satisfying both compilation safety and performance constraints.

## License

MIT
