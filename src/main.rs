use anyhow::{Context, Result};
// we bring in our protocol blueprints from the library
use kv_store::{Request, ResponseHeader};
// we bring in the storage engine
use kv_store::storage::Storage;
use tracing::{error, info, warn};

// we use arc to share our engine safely across many tasks
use std::sync::Arc;
// we use tokio's async tools for non-blocking networking
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("zero-kv engine initializing...");
    // 1. the engine setup
    // we load the database from the disk once
    // we use unwrap here because if the file is missing, the server can't start anyway
    let storage = Storage::new("storage.db").unwrap();

    // we wrap the engine in an arc (atomic reference counter)
    // this allows us to give every client connection a "key" to the same data
    let engine = Arc::new(storage);

    // 2. the open sign
    // we tell the operating system to listen for incoming bytes on port 5500
    let listener = TcpListener::bind("127.0.0.1:5500")
        .await
        .context("failed to bind to port 5500")?;
    info!("server listening on 127.0.0.1:5500");

    // 3. the receptionist loop
    loop {
        // we wait here (without using cpu) until a client connects
        let (mut socket, _addr) = listener.accept().await?;
        info!("accepted connection from {}", _addr);

        // we make a fast clone of the engine's pointer
        // this doesn't copy the database, it just increments a counter
        let engine_clone = Arc::clone(&engine);

        // we spawn a new "researcher" task to handle this specific client
        // the 'move' keyword lets this task take ownership of its own socket and engine clone
        tokio::spawn(async move {
            // we create a 16-byte buffer to hold the incoming request frame
            let mut buf = [0u8; 16];

            // we read exactly 16 bytes from the client socket
            if socket.read_exact(&mut buf).await.is_ok() {
                // we map those 16 bytes directly onto our Request struct
                // this is our zero-copy path: no parsing, just reinterpreting memory
                if let Some(req) = Request::from_bytes(&buf) {
                    // we ask the engine to find the value for the requested key
                    // req.key.get() handles the big-endian to native conversion for us
                    let result = engine_clone.get(req.key.get());

                    // we decide what to send back based on what the engine found
                    match result {
                        Some(data) => {
                            info!(
                                "lookup success: key {} ({} bytes)",
                                req.key.get(),
                                data.len()
                            );
                            // we found the key! we prepare a header with status 0 (ok)
                            let head = ResponseHeader {
                                status: 0.into(),
                                length: (data.len() as u32).into(),
                            };
                            // we write the 8-byte header first
                            let _ = socket.write_all(zerocopy::AsBytes::as_bytes(&head)).await;
                            // then we write the raw value bytes directly from the mmap
                            let _ = socket.write_all(data).await;
                        }
                        None => {
                            warn!("lookup miss: key {} not found", req.key.get());
                            // key not found: we send back status 1 (not found)
                            let head = ResponseHeader {
                                status: 1.into(),
                                length: 0.into(),
                            };
                            let _ = socket.write_all(zerocopy::AsBytes::as_bytes(&head)).await;
                        }
                    }
                }
            }
        });
        // after spawning the task, we immediately loop back to wait for the next client
    }
}
