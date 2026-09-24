use anyhow::{Context, Result};
use kv_store::storage::Storage;
use kv_store::{DEFAULT_SOCKET_PATH, DEFAULT_STORAGE_PATH, OpCode, ResponseStatus};
use kv_store::{Request, ResponseHeader};
use std::io::IoSlice;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tracing::{debug, error, info};
use zerocopy::AsBytes;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("zero-kv engine initializing...");

    // Map the immutable storage file; pages are loaded on demand.
    let storage = Storage::new(DEFAULT_STORAGE_PATH).expect("failed to load storage file");
    let engine = Arc::new(storage);

    // Bind the local Unix domain socket.
    let path = DEFAULT_SOCKET_PATH;
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).context("failed to bind to unix socket")?;
    info!("server listening on {}", path);

    loop {
        let (mut socket, _addr) = listener.accept().await?;
        debug!("accepted connection from {:?}", _addr);
        let engine_clone = Arc::clone(&engine);

        tokio::spawn(async move {
            // Buffer up to 256 fixed-size request frames.
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
                    // Each response uses at most two slices: header and mapped value.
                    // Retain source slices to rebuild IoSlices after partial writes.

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

                    // Gather headers and values without copying into a contiguous buffer.
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

                    // Continue until the batch is sent or the connection fails.
                    if current_slice_count > 0 {
                        let mut current_idx = 0;
                        while current_idx < current_slice_count {
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
