use anyhow::{Context, Result};
use kv_store::OpCode; // we bring in OpCode to handle different request types
use kv_store::storage::Storage;
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

    let storage = Storage::new("storage.db").expect("failed to load storage.db");
    let engine = Arc::new(storage);

    // i originally used tcp, but the overhead of the loopback stack (checksums, ip headers)
    // was adding 30-40µs of noise. i switched to unix domain sockets because they
    // bypass all that baggage and act like a direct pipe between processes.
    let path = "/tmp/zero-kv.sock";
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
            // since our read buffer is 4kb and requests are 16 bytes, we can
            // have up to 256 requests in a single batch.
            let mut response_headers = Vec::with_capacity(256);
            let mut data_slices = Vec::with_capacity(256);

            loop {
                let n = match socket.read(&mut read_buf[leftover..]).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };

                let total_bytes = leftover + n;
                let mut consumed = 0;

                {
                    let mut response_slices = Vec::with_capacity(512);
                    response_headers.clear();
                    data_slices.clear();

                    // we process every 16-byte frame in our current batch.
                    // by doing lookups in a tight loop, we keep the cpu's
                    // instruction cache very happy.
                    while total_bytes - consumed >= 16 {
                        let frame = &read_buf[consumed..consumed + 16];
                        consumed += 16;

                        if let Some(req) = Request::from_bytes(frame) {
                            let opcode = req.opcode();
                            let result = engine_clone.get(req.key.get());

                            match result {
                                Some(_) if opcode == OpCode::Exists => {
                                    response_headers.push(ResponseHeader {
                                        status: 0.into(),
                                        length: 0.into(),
                                    });
                                    data_slices.push(None);
                                }
                                Some(data) => {
                                    response_headers.push(ResponseHeader {
                                        status: 0.into(),
                                        length: (data.len() as u32).into(),
                                    });
                                    data_slices.push(Some(data));
                                }
                                None => {
                                    response_headers.push(ResponseHeader {
                                        status: 1.into(),
                                        length: 0.into(),
                                    });
                                    data_slices.push(None);
                                }
                            }
                        }
                    }

                    // vectored i/o is how i avoid the "final copy."
                    for (i, header) in response_headers.iter().enumerate() {
                        response_slices.push(IoSlice::new(header.as_bytes()));
                        if let Some(data) = data_slices[i] {
                            response_slices.push(IoSlice::new(data));
                        }
                    }

                    if !response_slices.is_empty() {
                        if socket.write_vectored(&response_slices).await.is_err() {
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
