use anyhow::{Context, Result};
use kv_store::storage::Storage;
use kv_store::{DEFAULT_SOCKET_PATH, DEFAULT_STORAGE_PATH, OpCode, ResponseStatus};
use kv_store::{Request, ResponseHeader};
use tracing::{debug, info};
// we bring in standard ioslice for our non-contiguous memory gather writes
use std::io::IoSlice;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use zerocopy::AsBytes; // we bring in the trait for direct .as_bytes() access

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("zero-kv engine initializing...");

    let storage = Storage::new(DEFAULT_STORAGE_PATH).expect("failed to load storage file");
    let engine = Arc::new(storage);

    // i originally used tcp, but the overhead of the loopback stack (checksums, ip headers)
    // was adding 30-40µs of noise. i switched to unix domain sockets because they
    // bypass all that baggage and act like a direct pipe between processes.
    let path = DEFAULT_SOCKET_PATH;
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).context("failed to bind to unix socket")?;
    info!("server listening on {}", path);

    loop {
        let (mut socket, _addr) = listener.accept().await?;
        debug!("accepted connection from {:?}", _addr);
        let engine_clone = Arc::clone(&engine);

        tokio::spawn(async move {
            // i used to read 16 bytes at a time, but i hit the "syscall wall."
            // the cpu was spending more time context-switching to the kernel than
            // actually doing work. now i pull 4kb chunks, which lets me process
            // dozens of requests in a single kernel transition.
            let mut read_buf = [0u8; 4096];
            let mut leftover = 0;

            // i pre-allocated these to avoid hitting the heap in the hot loop.
            // every time you touch the allocator, you risk a mutex lock or a
            // latency spike, so i'm keeping the memory "warm" and reused.
            // since our read buffer is 4kb and requests are 16 bytes, we can have up to 256 requests
            // in a single batch. We use stack-allocated arrays to avoid heap allocations entirely
            // within the connection handling task.
            let mut response_headers = [ResponseHeader::default(); 256];
            let mut data_slices = [None; 256];

            loop {
                let n = match socket.read(&mut read_buf[leftover..]).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };

                let total_bytes = leftover + n;
                let mut consumed = 0;

                {
                    // to achieve a truly zero-allocation hot path, we use a stack-allocated
                    // array of IoSlices. 512 slices * 16 bytes is only 8kb—well within stack limits.
                    let mut response_slices = [IoSlice::new(&[]); 512];
                    let mut current_slice_count = 0;
                    let mut current_response_count = 0;

                    // we process every 16-byte frame in our current batch.
                    // by doing lookups in a tight loop, we keep the cpu's
                    // instruction cache very happy.
                    while total_bytes - consumed >= 16 {
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
                                    Some(_) if opcode == OpCode::Exists => {
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::Ok as u32).into(),
                                            length: 0.into(),
                                        };
                                        data_slices[current_response_count] = None;
                                    }
                                    Some(data) => {
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::Ok as u32).into(),
                                            length: (data.len() as u32).into(),
                                        };
                                        data_slices[current_response_count] = Some(data);
                                    }
                                    None => {
                                        response_headers[current_response_count] = ResponseHeader {
                                            status: (ResponseStatus::NotFound as u32).into(),
                                            length: 0.into(),
                                        };
                                        data_slices[current_response_count] = None;
                                    }
                                }
                            }
                            current_response_count += 1;
                        }
                    }

                    // vectored i/o is how i avoid the "final copy."
                    for i in 0..current_response_count {
                        response_slices[current_slice_count] =
                            IoSlice::new(response_headers[i].as_bytes());
                        current_slice_count += 1;
                        if let Some(data) = data_slices[i] {
                            response_slices[current_slice_count] = IoSlice::new(data);
                            current_slice_count += 1;
                        }
                    }

                    if current_slice_count > 0 {
                        if socket
                            .write_vectored(&response_slices[..current_slice_count])
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                leftover = total_bytes - consumed;
                if leftover > 0 {
                    read_buf.copy_within(consumed..total_bytes, 0);
                }
            }
        });
    }
}
