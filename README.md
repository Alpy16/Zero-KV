# Zero-KV

[![Rust](https://img.shields.io/badge/Rust-2024-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Runtime-Tokio-blue.svg)](https://tokio.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

## About

Zero-KV is a disk-backed, read-optimized key-value storage engine for low-latency local environments.

The project explores systems-level performance techniques in Rust: immutable binary storage, memory-mapped reads, fixed-width request frames, Unix Domain Sockets, batched request handling, and vectored writes.

The engine is intentionally scoped around a simple workload: fast local lookups from a pre-baked immutable dataset.

## Features

* **Memory-mapped storage:** Uses `memmap2` to expose the baked database file through the OS page cache.
* **Fixed binary protocol:** Uses 16-byte request frames and 8-byte response headers for predictable parsing.
* **Unix Domain Socket transport:** Uses UDS for local inter-process communication without TCP loopback overhead.
* **Sorted immutable index:** Stores fixed-width index entries and performs binary search over the mapped index region.
* **Batched request handling:** Reads 4KB request batches and processes up to 256 pipelined requests per batch.
* **Vectored responses:** Uses `write_vectored` to write response headers and value slices without assembling a separate contiguous response buffer.
* **Cache-conscious layout:** Aligns stored value regions to 8-byte boundaries to simplify offset calculation and reduce unaligned access concerns.
* **Reusable hot-path buffers:** Reuses stack-allocated response metadata inside the server loop to reduce allocator pressure during steady-state handling.

## Quickstart

### Requirements

* Rust stable
* Linux or WSL2
* Unix Domain Socket support

### Installation

Clone the repository and build the release binaries:

```
git clone https://github.com/Alpy16/Zero-KV.git
cd Zero-KV
cargo build --release
```

### Environment Setup

Generate a benchmark input dataset:

```
seq 1 100000 | awk '{print $1 ",value_content_at_key_" $1}' > bench_input.csv
```

Bake the immutable storage file:

```
cargo run --release --bin baker -- bench_input.csv
```

This creates `storage.db`, which is loaded by the server at startup.

## Usage

### Build

```
cargo build --release
```

### Bake Dataset

Input format:

```
key,value
```

Example:

```
1,hello
2,world
100,First value content
```

Bake it:

```
cargo run --release --bin baker -- bench_input.csv
```

### Run Server

```
RUST_LOG=error cargo run --release --bin kv_store
```

By default, the server listens on:

```
/tmp/zero-kv.sock
```

### Run Benchmark

In another terminal:

```
cargo run --release --bin benchmarker
```

The benchmarker launches 100 concurrent Unix socket clients, each sending 256-request pipelines against keys in the baked dataset.

### Full Lifecycle Script

If using the automation script:

```
chmod +x run.sh
./run.sh
```

The script performs the full cycle:

1. Build release binaries
2. Generate a 100,000-entry dataset
3. Bake `storage.db`
4. Start the server
5. Run the saturation benchmark
6. Clean up temporary input files

## Performance Metrics

The current benchmark is designed to measure saturated local read throughput under pipelined Unix Domain Socket traffic.

Example benchmark configuration:

| Parameter          |                      Value |
| ------------------ | -------------------------: |
| Dataset size       |            100,000 entries |
| Concurrent clients |                        100 |
| Pipeline depth     |               256 requests |
| Run duration       |                 10 seconds |
| Transport          |        Unix Domain Sockets |
| Storage mode       | Immutable mmap-backed file |
| Workload           |         Existing-key reads |

Example result from WSL2:

| Metric            |                 Result |
| ----------------- | ---------------------: |
| Throughput        | 3,681,180 requests/sec |
| Batch P50 latency |                22.30µs |
| Batch P99 latency |                66.30µs |
| Max batch spike   |                90.62µs |

Latency values represent completion time for a full 256-request pipeline, not individual request latency.

## Performance Telemetry

Runtime behavior was inspected with `samply` and Firefox Profiler.

[View interactive profile trace](https://share.firefox.dev/42LOBK8)

In the captured steady-state profile, allocator activity was not visible in the request hot path. Most runtime was concentrated around socket polling, batched request handling, and mmap-backed lookup logic.

## Architecture

```
Zero-KV
├── src/
│   ├── lib.rs              # Protocol types, binary frame layout, shared constants
│   ├── storage.rs          # Mmap-backed storage engine and index lookup
│   ├── main.rs             # UDS server and request handling loop
│   └── bin/
│       ├── baker.rs        # Dataset compiler: CSV -> immutable storage.db
│       └── benchmarker.rs  # Saturation benchmark client
├── Cargo.toml
├── run.sh                  # Build, bake, serve, benchmark lifecycle script
└── README.md
```

## Storage Format

The baked database is immutable and laid out as:

```
+------------------+
| Header           |
+------------------+
| IndexEntry[0]    |
| IndexEntry[1]    |
| ...              |
| IndexEntry[n]    |
+------------------+
| Value bytes      |
| 8-byte padding   |
| Value bytes      |
| 8-byte padding   |
| ...              |
+------------------+
```

Each index entry stores:

```
key:        u64
val_offset: u64
val_len:    u32
padding:    u32
```

Because index entries are fixed-width and sorted by key, lookups use binary search over the mapped index region.

## Protocol

Each request is a fixed 16-byte frame:

```
op:      u32
padding: u32
key:     u64
```

Each response begins with an 8-byte header:

```
status: u32
length: u32
```

If `length > 0`, the response header is followed by the value bytes.

## Architecture Decision Records

### ADR 1: Unix Domain Sockets over TCP

**Context:** The engine is designed for local IPC workloads where TCP loopback introduces unnecessary protocol overhead.

**Decision:** Use Unix Domain Sockets as the default transport.

**Consequence:** The transport remains local-only, but avoids TCP/IP framing and checksum overhead for same-machine clients.

### ADR 2: Immutable Baked Storage

**Context:** Supporting writes, deletes, and compaction would require additional synchronization and recovery logic.

**Decision:** Compile input data into a sorted immutable binary file before server startup.

**Consequence:** Runtime lookup logic stays simple and fast, while mutation support is intentionally out of scope.

### ADR 3: Memory-Mapped I/O

**Context:** Standard read calls require explicit user-space buffers and repeated syscall interaction.

**Decision:** Use `memmap2` to map the database file and let the OS page cache manage file-backed memory.

**Consequence:** Lookups can return borrowed slices from the mapped file without copying values into a separate storage buffer.

### ADR 4: Fixed-Width Binary Frames

**Context:** Text protocols and variable-width request formats add parsing overhead and allocation pressure.

**Decision:** Use fixed-width request and response headers with explicit binary layouts.

**Consequence:** Request decoding becomes predictable and cheap, but the protocol is less flexible than a self-describing format.

### ADR 5: Batched Reads and Vectored Writes

**Context:** Handling one request per syscall limits throughput under high request rates.

**Decision:** Read 4KB request batches and flush response headers/value slices through vectored I/O.

**Consequence:** Syscall cost is amortized across many pipelined requests.

## Safety Notes

The storage engine uses a raw pointer internally to reference the mapped index region.

This is done to avoid self-referential lifetime issues while allowing the read-only storage object to be shared across worker tasks. The pointer is derived from the owned mmap, the mmap length is validated before pointer construction, and the mapped file is never mutated by the engine after startup.

## Limitations

* Read-only storage engine
* No writes, deletes, compaction, or replication
* No crash-recovery layer beyond immutable file loading
* Local IPC only through Unix Domain Sockets
* Benchmark is optimized for local saturated read throughput
* Batch latency is reported per 256-request pipeline, not per individual request
* Current input format is simple CSV and does not support escaping commas inside values

## License

MIT
