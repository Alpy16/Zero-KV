# Changelog

All notable changes to the Zero-KV project will be documented in this file.

## [Unreleased]

### Added

* Planned implementation of Vectored I/O (writev) for Stage 5 optimization.
* Planned high-concurrency benchmarking suite for Stage 6.

## [0.4.0] - 2026-05-10

### Changed

* **Architectural Refactor**: Comprehensive restructuring of the codebase to support asynchronous task spawning and thread-safe state management.
* **Library Hardening**: Refactored the storage engine to be Send + Sync, enabling integration with the Tokio runtime.

### Added

* **Project Documentation**: Initialized the changelog to track versioned progress and architectural shifts.

## [0.3.0] - 2026-05-08

### Added

* **Stage 3 Protocol**: Defined the fixed-width binary request and response frames.
* **Alignment Specifications**: Implementation of 8-byte natural alignment for network-delivered keys to ensure single-cycle CPU fetches.
* **Zero-Copy Deserialization**: Integration of the zerocopy crate for non-allocating frame interpretation.

## [0.2.0] - 2026-05-07

### Added

* **Stage 2 Retrieval**: Integration of memory-mapped file I/O via memmap2 for disk-backed storage.
* **Core API**: Implementation of the binary search lookup logic over memory-mapped index offsets.
* **Project Roadmap**: Added a comprehensive README outlining the 6-stage development plan and technical architecture.

## [0.1.0] - 2026-05-04

### Added

* **Initial Commit**: Established the project structure and the foundational Baker CLI for data serialization.
* **Error Handling**: Implemented custom error types and centralized error propagation on top of the initial key-value store logic.
* **Index Serialization**: Initial implementation of sorted binary index generation for O(log n) lookup efficiency.