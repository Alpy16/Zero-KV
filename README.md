# Zero-KV

[![Rust](https://img.shields.io/badge/Rust-2024-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Runtime-Tokio-blue.svg)](https://tokio.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

## About

Zero-KV is a disk-backed, read-optimized key-value storage engine for low-latency local environments.

The project explores systems-level performance techniques in Rust: immutable binary storage, memory-mapped reads, fixed-width binary frames, Unix Domain Sockets, batched request handling, and vectored writes.

The engine is intentionally scoped around a simple workload: fast local lookups from a pre-baked immutable dataset.

## Features

* **Memory-mapped storage:** Uses `memmap2` to expose the baked database file through the OS page cache.
* **Fixed binary protocol:** Uses 16-byte request frames and 8-byte response headers for predictable parsing.
* **Unix Domain Socket transport:** Uses UDS for local inter-process communication without TCP loopback overhead.
* **Sorted immutable index:** Stores fixed-width index entries and performs binary search over the mapped index region.
* **Input-driven baking:** Compiles a simple `key,value` input file into an immutable binary `storage.db`.
* **Batched request handling:** Reads 4KB request batches and processes up to 256 pipelined requests per batch.
* **Vectored responses:** Uses `write_vectored` to write response headers and value slices without assembling a separate contiguous response buffer.
* **Partial-write safety:** Handles partial vectored writes by advancing through written slices until the full response batch is flushed.
* **Aligned value layout:** Pads stored values to 8-byte boundaries.
* **Reusable response buffers:** Reuses fixed-size response metadata arrays within each connection task.

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

### Generate a Dataset

Zero-KV expects a simple CSV-like input file:

```
key,value
```

There is no header row. Keys are unsigned 64-bit integers. The first comma separates the key from the value; additional commas remain part of the value. Leading and trailing whitespace is trimmed from both fields. Blank lines are ignored, lines without a comma are skipped with a warning, and invalid keys stop the bake.

Example:

```
1,hello
2,world
100,First value content
```

Generate a 100,000-entry benchmark dataset:

```
seq 1 100000 | awk '{print $1 ",value_content_at_key_" $1}' > bench_input.csv
```

### Bake the Storage File

Compile the input dataset into an immutable binary database:

```
cargo run --release --bin baker -- bench_input.csv
```

This creates:

```
storage.db
```

The server maps this file at startup. The baker refuses to overwrite an existing `storage.db`; stop the server and remove the old file before re-baking.

### Run the Server

```
cargo run --release --bin kv_store
```

By default, the server listens on:

```
/tmp/zero-kv.sock
```

### Run the Benchmark

In another terminal:

```
cargo run --release --bin benchmarker
```

The benchmarker launches 100 concurrent Unix socket clients, each sending 256-request pipelines against keys 1 through 100,000. Bake the generated benchmark dataset above for an all-hit workload; the client does not check response statuses.

## Full Lifecycle Script

The repository includes `cycle.sh`, which runs the full build, bake, serve, and benchmark cycle.

```
chmod +x cycle.sh
./cycle.sh
```

The script performs the following steps:

1. Builds release binaries
2. Generates a 100,000-entry dataset
3. Bakes `storage.db`
4. Starts the server in the background
5. Waits for the Unix socket to become available
6. Runs the saturation benchmark
7. Checks that the server survived the benchmark
8. Removes the generated input file and stops the server on exit

The script replaces `storage.db` and `server.log` and removes the existing socket path. Run it with the server stopped. It leaves the baked database, log, and socket path behind.

## Performance Metrics

The benchmark is designed to measure saturated local read throughput under pipelined Unix Domain Socket traffic.

Benchmark configuration for the generated dataset:

| Parameter          |                      Value |
| ------------------ | -------------------------: |
| Dataset size       |            100,000 entries |
| Concurrent clients |                        100 |
| Pipeline depth     |               256 requests |
| Run duration       |                 10 seconds |
| Transport          |        Unix Domain Sockets |
| Storage mode       | Immutable mmap-backed file |
| Workload           |         Existing-key reads |
| Hit rate           |                       100% |

Previously recorded result (not re-measured for the current implementation):

| Metric            |                  Result |
| ----------------- | ----------------------: |
| Throughput        | 10,405,284 requests/sec |
| Batch P50 latency |                 2.387ms |
| Batch P99 latency |                 5.413ms |
| Total requests    |             104,117,504 |

Latency values represent completion time for a full 256-request pipeline, not individual request latency. Percentiles use at most the first 10,000 batches per client.

Benchmark results are environment-dependent. The result above was collected in a local Linux/WSL2 environment using Unix Domain Sockets.

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
│       ├── baker.rs        # Dataset compiler: input file -> immutable storage.db
│       └── benchmarker.rs  # Saturation benchmark client
├── Cargo.toml
├── cycle.sh                # Build, bake, serve, benchmark lifecycle script
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
| IndexEntry[n-1]  |
+------------------+
| Value bytes      |
| 8-byte padding   |
| Value bytes      |
| 8-byte padding   |
| ...              |
+------------------+
```

The 32-byte header stores:

```
magic:           u64  # 0xA016
version:         u64  # 1
count:           u64
header_checksum: u32
padding:         u32
```

The header checksum is CRC32 over all 32 bytes with `header_checksum` set to zero.

Each 24-byte index entry stores:

```
key:          u64
val_offset:   u64
val_len:      u32
val_checksum: u32
```

Storage integers use native endianness. Offsets are absolute file offsets, lengths exclude padding, and `val_checksum` is CRC32 over the value bytes. Each successful lookup verifies the payload checksum, so its cost is O(log n + value length). The index itself has no checksum.

Because index entries are fixed-width and sorted by key, lookups use binary search over the mapped index region.

## Protocol

Protocol operation codes, keys, statuses, and lengths use big-endian encoding. Each request is a fixed 16-byte frame:

```
op:      u32
padding: u32
key:     u64
```

Supported operations:

| Opcode | Operation |
| -----: | --------- |
|      0 | Get       |
|      1 | Exists    |

Each response begins with an 8-byte header:

```
status: u32
length: u32
```

Response statuses:

| Status | Meaning   |
| -----: | --------- |
|      0 | Ok        |
|      1 | Not found |
|      2 | Error     |

If `length > 0`, the response header is followed by the value bytes. `Exists` returns `Ok` or `Not found` with no payload, but still verifies the stored value checksum. Unknown operations and storage errors return `Error` with no payload.

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

### ADR 6: Partial-Write-Safe Vectored Output

**Context:** A single vectored write is not guaranteed to flush every byte in the provided slice list.

**Decision:** Track the number of bytes written, advance through completed slices, trim partially written slices, and continue writing until the full response batch is sent.

**Consequence:** The server preserves response framing correctness while avoiding an intermediate contiguous response buffer. Socket writes still copy data into kernel buffers.

## Safety Notes

The storage engine uses a raw pointer internally to reference the mapped index region.

The pointer is derived from the owned mmap and avoids storing a self-referential slice. The engine checks header magic, version, CRC32, and index size at startup, and value bounds and CRC32 during lookup. The engine does not modify the file.

The backing file must not be modified or truncated while mapped. The raw index pointer also requires the mapping and header count to remain unchanged; both are currently public fields. Size calculations use unchecked arithmetic, so these checks do not make arbitrary malformed files safe to load.

The server response path uses borrowed slices into preallocated response headers and mmap-backed values. Vectored writes are advanced carefully so partial writes do not corrupt the client-visible response stream.

## Limitations

* Read-only storage engine
* No writes, deletes, compaction, or replication
* No crash-recovery layer; baking writes directly to the destination and flushes without `fsync` or atomic publication
* Local IPC only through Unix Domain Sockets
* Benchmark is optimized for local saturated read throughput
* Batch latency is reported per 256-request pipeline, not per individual request
* Input is UTF-8 `key,value` text split at the first comma, without CSV quoting or multiline values
* Duplicate keys are accepted; lookup does not specify which duplicate is returned
* Storage files use native endianness and require a compatible host layout
* Benchmark assumes keys 1 through 100,000 exist and does not validate response statuses

## License

MIT
