use anyhow::Result;
use kv_store::Request;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use zerocopy::byteorder::network_endian::U64;

#[tokio::main]
async fn main() -> Result<()> {
    let mut socket = UnixStream::connect("/tmp/zero-kv.sock").await?;

    // pre-build a batch of requests to flood the server.
    // this tests the server's 4KB read buffer efficiency.
    let mut batch = Vec::with_capacity(256 * 16);
    for _ in 0..256 {
        let req = Request {
            op: 0.into(),
            _padding: 0,
            key: U64::new(100), // using a known key from the baker
        };
        batch.extend_from_slice(zerocopy::AsBytes::as_bytes(&req));
    }

    println!("starting flamegraph load generation...");
    let start = Instant::now();
    let mut total_requests = 0;

    // run for 10 seconds or until interrupted
    while start.elapsed().as_secs() < 10 {
        socket.write_all(&batch).await?;

        // we expect 256 responses. each response header is 8 bytes.
        // we know key 100 has a value, so we must account for that in the read.
        // for simplicity in this benchmarker, we just pull the expected byte count.
        let mut response_buf = vec![0u8; 256 * (8 + 19)]; // 8 byte header + "First value content"
        socket.read_exact(&mut response_buf).await?;

        total_requests += 256;
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "finished: {} requests in {:.2}s ({:.2} req/s)",
        total_requests,
        elapsed,
        total_requests as f64 / elapsed
    );

    Ok(())
}
