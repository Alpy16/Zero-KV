## CHANGELOG.md

All notable changes to this project will be documented in this file. This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

### [Unreleased] - 2026-05-10

**Status: Post-Hardening / Pre-Stage 4**

#### Added

* **zero-copy protocol:** we integrated `zerocopy` for `Request` and `ResponseHeader` structs to enable direct memory mapping of network frames.
* **concurrency support:** we explicitly implemented `Send` and `Sync` for `Storage` to allow multi-threaded access.
* **contextual errors:** we integrated `anyhow` into the baker and server binaries for detailed error reporting.
* **structured logging:** we added the `tracing` infrastructure for future request observability.

#### Changed

* **memory alignment:** we refactored the binary layout to 16-byte frames with 8-byte internal alignment for cpu optimization.
* **storage architecture:** we swapped self-referential lifetimes for a stable **raw pointer + private helper** pattern in the storage reader.
* **baker efficiency:** we implemented `BufWriter` in the baker to reduce disk i/o overhead during database construction.
* **standardized endianness:** we locked the protocol to big-endian (network byte order) using `zerocopy` native types.

#### Fixed

* **alignment panics:** we resolved "unaligned reference" errors by ensuring all `u64` fields sit on 8-byte boundaries.
* **unsafe surface area:** we moved `unsafe` pointer math out of the `get()` hot path and into encapsulated initialization/helper functions.

---

### [0.0.1] - 2026-05-01

#### Added

* **initial storage engine:** support for memory-mapped file lookups.
* **binary baker:** basic cli to convert json datasets into sorted binary files.
* **index search:** binary search implementation over the disk-backed index.