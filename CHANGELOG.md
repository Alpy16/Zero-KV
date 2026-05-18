# Changelog

All notable changes and architectural milestones for the Zero-KV storage engine project are documented in this file.

## [Stage 5] - The Zero-Copy Path

### Added
* Integrated vectored I/O (`write_vectored`) into the hot path of the server connection loop using standard library `IoSlice` handles.
* Implemented an allocation-free stack structure to arrange non-contiguous memory segments (the stack-allocated response header and the memory-mapped value slice) for atomic transmission.
* Introduced a bulletproof partial-write defense mechanism to intercept network backpressure conditions without dropping bytes or causing runtime slicing panics.
* Added a dedicated integration testing suite (`tests/smoke_test.rs`) to evaluate protocol behavior, socket connectivity, numeric key lookups, and missing key responses over local network streams.

### Changed
* Refactored the network success path inside the asynchronous task runner, replacing sequential, multi-step socket writes with a single unified kernel context switch.
* Cleaned up the error handling logic in the connection loop, shifting raw expressions into contextual, lowercase tracing blocks.

---

## [Stage 4] - The Async Server

### Added
* Implemented a performance-first TCP listening server leveraging the `tokio` multi-threaded runtime.
* Enforced connection hardening by wrapping socket reads inside an asynchronous 5-second `timeout` wrapper to actively prevent slowloris resource exhaustion attacks.
* Added structural token tracking by wrapping the core memory-mapped storage controller inside an atomic reference counter (`Arc`) for safe cross-thread distribution.

---

## [Stage 3] - The Protocol

### Added
* Designed a rigorous, fixed-width binary frame specification (16 bytes for requests, 8 bytes for response headers) to eliminate stream delimiter scanning.
* Integrated the `zerocopy` crate ecosystem to safely cast raw network buffer arrays directly into structural data types without allocation overhead.
* Introduced big-endian network alignment types (`U32`, `U64`) to stabilize data consistency across differing hardware architectures.

---

## [Stage 2] - The Mmap Reader

### Added
* Constructed the core storage engine backend mapping database storage files straight into virtual memory using the `memmap2` driver interface.
* Authored an unsafe transient slice pointer mechanism (`std::slice::from_raw_parts`) to generate zero-cost, safe slice views over raw binary offsets.
* Implemented the runtime index binary search engine to locate target key offsets in logarithmic time.

---

## [Stage 1] - The Baker

### Added
* Built the file compiler CLI utility to pre-compile raw key-value pairs into optimized binary structures.
* Implemented a data pre-sorting layer to ensure perfect binary search index sequencing.
* Enforced structural 8-byte padding and alignment algorithms on written data blocks to preserve modern CPU cache efficiency.