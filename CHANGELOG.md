# Changelog

All notable changes to the Zero-KV storage engine will be documented in this file. This project adheres to a performance-driven development lifecycle.

## [Stage 6] - Mechanical Sympathy & Syscall Optimization

### Added
- **Unix Domain Sockets (UDS):** Replaced TCP with UDS to eliminate loopback networking overhead, bypassing IP headers, checksums, and Nagle's algorithm.
- **Request Batching:** Implemented 4KB buffered reads to ingest multiple 16-byte frames in a single kernel context switch.
- **Response Batching:** Integrated vectored I/O (`write_vectored`) to aggregate multiple responses into single system calls.
- **Pipelining:** Updated the benchmarker to support request pipelining, enabling higher saturation of the server's batch-processing logic.

### Changed
- **Hot-path Logging:** Migrated per-request tracing from `info!` to `debug!` to eliminate `stdout` lock contention and associated latency jitter.
- **Allocation Strategy:** Moved response buffers outside the hot loop and pre-allocated capacities to prevent heap allocations during the request lifecycle.
- **Borrow Checker Refactoring:** Decoupled `IoSlice` lifetimes from header buffers to facilitate memory reuse without data copying.

### Fixed
- **Tail Latency Spikes:** Reduced P99.9 latency by removing synchronous logging and high-frequency timer registrations.

---

## [Stage 5] - Zero-Copy Path

### Added
- **Vectored I/O:** Initial implementation of `write_vectored` for atomic transmission of non-contiguous memory (stack headers and mmap values).
- **Smoke Tests:** Introduced `smoke_test.rs` for protocol compliance and boundary condition verification.

---

## [Stage 4] - Asynchronous Runtime

### Added
- **Tokio Integration:** Migrated to an asynchronous multi-threaded architecture.
- **Atomic Storage Sharing:** Wrapped the storage controller in `Arc` for lock-free read access across concurrent tasks.

---

## [Stage 3] - Protocol Specification

### Added
- **Fixed-Width Frames:** Established 16-byte request and 8-byte response frame specifications.
- **Zerocopy Casting:** Integrated `zerocopy` for allocation-free byte-to-struct mapping.
- **Endian Stability:** Implemented big-endian alignment for network interoperability.

---

## [Stage 2] - Memory-Mapped Storage

### Added
- **Mmap Backend:** Implemented file-backed storage using `mmap2`.
- **Kernel Hinting:** Added `Advice::Random` (madvise) to optimize the kernel's page cache management for binary search patterns.

---

## [Stage 1] - Data Baking

### Added
- **The Baker Utility:** Created a CLI tool for pre-compiling and sorting database files.
- **Structural Alignment:** Enforced 8-byte boundary alignment for all data entries to optimize CPU cache line utilization.