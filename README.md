# Zero-KV

Zero-KV is a hyper-optimized, disk-backed key-value storage engine engineered for ultra-low latency local environments. Built around the principle of **Mechanical Sympathy**, the engine aligns software execution boundaries directly with CPU architecture and Linux kernel primitives to maximize hardware saturation.

## Features

* **Zero-Copy Pipeline:** Leverages `zerocopy` and `memmap2` to stream data directly from virtual memory to the network layer without user-space allocation or duplication.
* **Kernel Transport Bypassing:** Utilizes Unix Domain Sockets (UDS) and 16-byte fixed-width binary frames to entirely bypass the TCP/IP stack overhead.
* **Cache-Line Alignment:** Enforces strict 8-byte boundaries on disk to prevent fields from straddling 64-byte hardware blocks, ensuring single-cycle register loads.
* **Syscall Amortization:** Combines 4KB buffered batch reads with vectored I/O (`write_vectored`) to clear thousands of transactions per context switch.
* **Zero-Allocation Hot Path:** Decouples execution loop lifetimes to safely reuse pre-allocated memory pools without hitting the global heap allocator.

## Performance Metrics

*Evaluated under a full-saturation loop via WSL2 with 100 concurrent connections:*

* **Throughput:** **3,681,180 requests per second**
* **P50 Latency (Median):** **22.30µs**
* **P99 Latency (Tail):** **66.30µs**
* **Max Spike:** **90.62µs**

## Performance Telemetry & Verification

To audit steady-state determinism under peak load, the execution footprint was captured via `samply`. 

* **[📊 View Live Interactive Profile Trace](https://share.firefox.dev/42LOBK8)**

*Trace verification highlights:* Zero CPU cycles spent inside the global allocator (`malloc`/`free`) on the hot path, with over 85% of thread runtimes directly confined to core execution and poll states.

---

## Getting Started

### 1. Build the Binary Dataset

The engine requires an immutable, pre-sorted data file. Serialize your source dataset using the compiler utility:

```bash
cargo run --release --bin baker

```

### 2. Launch the Storage Server

Initialize the UDS server at `/tmp/zero-kv.sock`. Run with standard I/O logging suppressed to avoid blocking the runtime event loop:

```bash
RUST_LOG=error cargo run --release --bin kv_store

```

### 3. Run the Load Benchmarker

Saturate the engine with the high-concurrency pipelined traffic generator to calculate real-time latency distributions:

```bash
cargo run --release --bin benchmarker

```

---

## Architecture Decision Records (ADR)

### ADR 1: Use of Unix Domain Sockets over TCP

* **Context:** Localhost TCP loopback introduces packet sequence validation, window tracking states, and network checksum noise.
* **Decision:** Default to Unix Domain Sockets (UDS) for local inter-process communication.
* **Consequence:** Bypasses IP headers entirely, converting the network transport into a direct, kernel-mediated memory pipe.

### ADR 2: 8-Byte Data Alignment

* **Context:** Unaligned data layouts force the CPU to execute multiple memory bus transactions whenever a primitive straddles two cache lines.
* **Decision:** Force the compilation stage (`the baker`) to pack records strictly along 8-byte boundaries.
* **Consequence:** Guarantees spatial locality; data is loaded cleanly into registers inside a single clock cycle.

### ADR 3: Memory-Mapped I/O with Random Advice

* **Context:** Standard file descriptor system calls require duplicate buffer copies across the kernel-user boundary and trigger expensive kernel read-ahead heuristics.
* **Decision:** Map files using `memmap2` paired with an explicit `madvise(Advice::Random)` hint.
* **Consequence:** Moves I/O directly into the OS Page Cache with zero user-space memory overhead while completely disabling sequential page-cache prefetch churn during binary search jumps.

### ADR 4: Syscall Batching & Vectored I/O

* **Context:** Frequent unbatched reading/writing traps the CPU inside expensive user-to-kernel context switches, causing quick saturation under load.
* **Decision:** Implement 4KB read buffering on pipelined requests and flush response flights atomically via `write_vectored`.
* **Consequence:** Amortizes the cost of a single kernel trap over dozens of distinct queries, shifting the engine's bottleneck to raw hardware limits.

### ADR 5: Decoupling Descriptor Lifetimes from Pre-allocated Buffers

* **Context:** The Rust borrow checker blocks clearing mutable response headers if transient `IoSlice` objects holding immutable references to them still exist inside the sibling loop scope.
* **Decision:** Declare the temporary `IoSlice` descriptor arrays directly inside the loop block while preserving the heavy vector storage capacities outside.
* **Consequence:** Satisfies compilation lifetime constraints, allowing total memory reuse on every batch transaction without hitting the global allocator.

## License

MIT