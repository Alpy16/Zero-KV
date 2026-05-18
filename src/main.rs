use anyhow::{Context, Result};
use kv_store::storage::Storage;
use kv_store::{Request, ResponseHeader};
use std::sync::Arc;
use tracing::{error, info, warn};
// we bring in standard ioslice for our non-contiguous memory gather writes
use std::io::IoSlice;
// we add Duration and timeout for our security hardening
use std::time::Duration;
use tokio::time::timeout;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("zero-kv engine initializing...");

    // 1. the engine setup
    // we load the database from the disk once
    let storage = Storage::new("storage.db").expect("failed to load storage.db");

    // we wrap the engine in an arc (atomic reference counter)
    // this lets us safely hand out read access tokens to multiple async tasks
    let engine = Arc::new(storage);

    // 2. the open sign
    // we bind our listener to a local port to await client incoming streams
    let listener = TcpListener::bind("127.0.0.1:5500")
        .await
        .context("failed to bind to port 5500")?;
    info!("server listening on 127.0.0.1:5500");

    // 3. the receptionist loop
    loop {
        let (mut socket, _addr) = listener.accept().await?;
        info!("accepted connection from {}", _addr);

        let engine_clone = Arc::clone(&engine);

        // we spawn a new "researcher" task to handle this specific client
        tokio::spawn(async move {
            let mut buf = [0u8; 16];

            // we give the client exactly 5 seconds to send their 16-byte request frame
            // if they hang, the 'timeout' future will return an error, and we drop the connection
            let read_result = timeout(Duration::from_secs(5), socket.read_exact(&mut buf)).await;

            match read_result {
                Ok(Ok(_)) => {
                    // the client sent the bytes in time!
                    if let Some(req) = Request::from_bytes(&buf) {
                        let result = engine_clone.get(req.key.get());

                        match result {
                            Some(data) => {
                                info!(
                                    "lookup success: key {} ({} bytes)",
                                    req.key.get(),
                                    data.len()
                                );

                                // we found the data, so we initialize an ok (0) response header
                                let head = ResponseHeader {
                                    status: 0.into(),
                                    length: (data.len() as u32).into(),
                                };

                                // STAGE 5: The Zero-Copy Path
                                // we cast our fixed header struct into a safe raw byte slice view
                                let header_bytes = zerocopy::AsBytes::as_bytes(&head);

                                // we gather our separate memory spaces into an array of io descriptors
                                // slice 1 points to our stack header, slice 2 points directly into the mmap
                                let bufs = [IoSlice::new(header_bytes), IoSlice::new(&data)];

                                // we sum up the total expected packet size to catch any partial writes
                                let total_expected = header_bytes.len() + data.len();

                                // we issue exactly one system call to let the kernel stream both regions
                                match socket.write_vectored(&bufs).await {
                                    Ok(n) => {
                                        info!("sent {} bytes to {}", n, _addr);

                                        // systems defense: if the network chokes, we handle partial flushes
                                        if n < total_expected {
                                            warn!(
                                                "partial write to {}: sent {} of {} bytes",
                                                _addr, n, total_expected
                                            );

                                            if n < header_bytes.len() {
                                                // case A: the header itself got cut short.
                                                // we finish sending the remainder of the header, then all the data
                                                let _ = socket.write_all(&header_bytes[n..]).await;
                                                let _ = socket.write_all(data).await;
                                            } else {
                                                // case B: the header cleared, but the data payload fractured.
                                                // we calculate our progress into the mmap slice and flush the rest
                                                let data_written = n - header_bytes.len();
                                                let _ =
                                                    socket.write_all(&data[data_written..]).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("failed to send response to {}: {}", _addr, e);
                                    }
                                }
                            }
                            None => {
                                warn!("lookup miss: key {} not found", req.key.get());
                                // key not found: we send back status 1 (not found)
                                let head = ResponseHeader {
                                    status: 1.into(),
                                    length: 0.into(),
                                };
                                if let Err(e) =
                                    socket.write_all(zerocopy::AsBytes::as_bytes(&head)).await
                                {
                                    error!("failed to send response to {}: {}", _addr, e);
                                }
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    // a standard network error occurred
                    error!("read error from {}: {}", _addr, e);
                }
                Err(_) => {
                    // the 5-second timer expired
                    warn!("client {} timed out while sending request", _addr);
                }
            }
        });
    }
}
