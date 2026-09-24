use anyhow::{Context, Result};
use kv_store::storage::Storage;
use kv_store::{DEFAULT_SOCKET_PATH, DEFAULT_STORAGE_PATH, OpCode, ResponseStatus};
use kv_store::{Request, ResponseHeader};
use std::io::IoSlice;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tracing::{debug, error, info};
use zerocopy::AsBytes; // we bring in the trait for direct .as_bytes() access

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("zero-kv engine initializing...");

    // Load the immutable storage file into memory via Mmap.
    let storage = Storage::new(DEFAULT_STORAGE_PATH).expect("failed to load storage file");
    let engine = Arc::new(storage);

    // Setup the Unix Domain Socket for low-latency local communication.
    // We utilize Unix Domain Sockets to bypass the overhead of the loopback network stack,
    // facilitating direct, low-latency IPC.
    let path = DEFAULT_SOCKET_PATH;
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).context("failed to bind to unix socket")?;
    info!("server listening on {}", path);

    loop {
        let (mut socket, _addr) = listener.accept().await?;
        debug!("accepted connection from {:?}", _addr);
        let engine_clone = Arc::clone(&engine);

        tokio::spawn(async move {
            // We use a 4KB buffer to ingest batches of 16-byte request frames.
            let mut read_buf = [0u8; 4096];
            let mut leftover = 0;

            // Pre-allocated response metadata buffers to avoid heap allocations in the hot path.
            let mut response_headers = [ResponseHeader::default(); 256];
            let mut data_slices = [None; 256];

            'connection: loop {
                // Read data into the buffer, starting after any leftover bytes from the previous read.
                let n = match socket.read(&mut read_buf[leftover..]).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };

                let total_bytes = leftover + n;
                let mut consumed = 0;

                {
                    // We prepare IoSlices for vectored writes to achieve zero-copy transmission.
                    // source_slices: Keeps track of the original &[u8] for offset calculation during partial writes.
                    // response_slices: The actual IoSlices passed to the kernel.

                    // We process requests in batches to amortize the cost of the `write_vectored` syscall.
                    // By using stack-allocated arrays here, we ensure that the per-batch overhead
                    // involves no heap allocations, keeping the latency predictable (P99 hardening).

                    // A single request can result in at most 2 slices: the 8-byte response header
                    // and the actual value data from the mmap.

                    let mut source_slices: [&[u8]; 512] = [&[]; 512];
                    let mut response_slices = [IoSlice::new(&[]); 512];
                    let mut current_slice_count = 0;
                    let mut current_response_count = 0;

                    while total_bytes - consumed >= 16 && current_response_count < 256 {
                        let frame = &read_buf[consumed..consumed + 16];
                        consumed += 16;

                        if let Some(req) = Request::from_bytes(frame) {
                            let opcode = req.opcode();
                            if opcode == OpCode::Unknown {
                                response_headers[current_response_count] = ResponseHeader {
                                    status: (ResponseStatus::Error as u32).into(),
                                    length: 0.into(),
                                };
                                data_slices[current_response_count] = None;
                            } else {
                                let result = engine_clone.get(req.key.get());
                                match result {
                                    Ok(Some(_)) if opcode == OpCode::Exists => {
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::Ok as u32).into(),
                                            length: 0.into(),
                                        };
                                        data_slices[current_response_count] = None;
                                    }
                                    Ok(Some(data)) => {
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::Ok as u32).into(),
                                            length: (data.len() as u32).into(),
                                        };
                                        data_slices[current_response_count] = Some(data);
                                    }
                                    Ok(None) => {
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::NotFound as u32).into(),
                                            length: 0.into(),
                                        };
                                        data_slices[current_response_count] = None;
                                    }
                                    Err(e) => {
                                        error!("Engine error for key {}: {}", req.key.get(), e);
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::Error as u32).into(),
                                            length: 0.into(),
                                        };
                                        data_slices[current_response_count] = None;
                                    }
                                }
                            }
                            current_response_count += 1;
                        }
                    }

                    // We assemble the vectored I/O batch. This avoids copying data into a
                    // contiguous response buffer before sending.
                    for i in 0..current_response_count {
                        let header_bytes = response_headers[i].as_bytes();
                        source_slices[current_slice_count] = header_bytes;
                        response_slices[current_slice_count] = IoSlice::new(header_bytes);
                        current_slice_count += 1;
                        if let Some(data) = data_slices[i] {
                            source_slices[current_slice_count] = data;
                            response_slices[current_slice_count] = IoSlice::new(data);
                            current_slice_count += 1;
                        }
                    }

                    // We execute the vectored write loop until the entire batch is sent.
                    if current_slice_count > 0 {
                        let mut current_idx = 0;
                        while current_idx < current_slice_count {
                            // write_vectored transmits non-contiguous memory in a single syscall.
                            match socket
                                .write_vectored(&response_slices[current_idx..current_slice_count])
                                .await
                            {
                                Ok(0) | Err(_) => break 'connection,
                                Ok(n) => {
                                    // Handle partial writes: advance the slice pointers by the
                                    // number of bytes actually written by the kernel.

                                    let mut remaining = n;
                                    while remaining > 0 && current_idx < current_slice_count {
                                        if source_slices[current_idx].len() <= remaining {
                                            remaining -= source_slices[current_idx].len();
                                            current_idx += 1;
                                        } else {
                                            source_slices[current_idx] =
                                                &source_slices[current_idx][remaining..];
                                            response_slices[current_idx] =
                                                IoSlice::new(source_slices[current_idx]);
                                            remaining = 0;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Move any unparsed bytes to the start of the buffer for the next iteration.
                leftover = total_bytes - consumed;
                if leftover > 0 {
                    read_buf.copy_within(consumed..total_bytes, 0);
                }
            }
        });
    }
}
