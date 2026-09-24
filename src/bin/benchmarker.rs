use anyhow::Result;
use kv_store::{DEFAULT_SOCKET_PATH, Request, ResponseHeader};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use zerocopy::AsBytes;
use zerocopy::byteorder::network_endian::U64;

const CONCURRENT_CONNECTIONS: usize = 100;
const PIPELINE_DEPTH: usize = 256;
const RUN_DURATION: u64 = 10;
const MAX_KEY: u64 = 100_000;
const MAX_LATENCY_SAMPLES: usize = 10_000; // Maximum recorded batches per connection.

async fn run_client(task_id: usize) -> Result<(u64, Vec<Duration>)> {
    let mut socket = UnixStream::connect(DEFAULT_SOCKET_PATH).await?;
    let mut total_requests = 0;
    let mut latencies = Vec::with_capacity(MAX_LATENCY_SAMPLES);

    // Pre-allocate the batch buffer outside the loop to keep the client hot-path allocation-free.
    let mut batch_buf = Vec::with_capacity(PIPELINE_DEPTH * 16);

    // Simple LCG for pseudo-random keys without external dependencies
    let mut seed = (task_id as u64 + 42) * 1103515245;
    let mut next_key = || {
        seed = (seed.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
        // Sample 1..=MAX_KEY; all keys exist in the generated benchmark dataset.
        1 + (seed % MAX_KEY)
    };

    let start = Instant::now();
    let mut header_buf = [0u8; 8];
    let mut val_scratch = Vec::with_capacity(1024); // Grows to fit the largest response seen.

    while start.elapsed().as_secs() < RUN_DURATION {
        batch_buf.clear();
        for _ in 0..PIPELINE_DEPTH {
            let req = Request {
                op: 0.into(),
                _padding: 0,
                key: U64::new(next_key()),
            };
            batch_buf.extend_from_slice(req.as_bytes());
        }

        let batch_start = Instant::now();
        socket.write_all(&batch_buf).await?;

        // Read each header before its variable-length payload.
        for _ in 0..PIPELINE_DEPTH {
            socket.read_exact(&mut header_buf).await?;
            let h = ResponseHeader::from_bytes(&header_buf)
                .ok_or_else(|| anyhow::anyhow!("Malformed response header"))?;
            let len = h.length.get() as usize;
            if len > 0 {
                if val_scratch.len() < len {
                    val_scratch.resize(len, 0);
                }
                socket.read_exact(&mut val_scratch[..len]).await?;
            }
        }

        if latencies.len() < MAX_LATENCY_SAMPLES {
            latencies.push(batch_start.elapsed());
        }
        total_requests += PIPELINE_DEPTH as u64;
    }

    Ok((total_requests, latencies))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Launching {} concurrent clients...", CONCURRENT_CONNECTIONS);

    let mut handles = Vec::new();
    for i in 0..CONCURRENT_CONNECTIONS {
        handles.push(tokio::spawn(run_client(i)));
    }

    let start = Instant::now();
    let mut total_requests = 0u64;
    let mut all_latencies = Vec::new();

    for handle in handles {
        let (reqs, mut lats) = handle.await??;
        total_requests += reqs;
        all_latencies.append(&mut lats);
    }

    let elapsed = start.elapsed().as_secs_f64();
    all_latencies.sort_unstable();

    // Percentiles cover recorded batch completion times, capped per connection.
    let p50 = all_latencies[all_latencies.len() / 2];
    let p99 = all_latencies[(all_latencies.len() * 99) / 100];

    println!(
        "\nSaturation Results (Real-world keys & Concurrency):\n------------------\nThroughput:             {:.2} req/s\nBatch (256req) P50 Latency: {:?}\nBatch (256req) P99 Latency: {:?}\nTotal Reqs:             {}",
        total_requests as f64 / elapsed,
        p50,
        p99,
        total_requests
    );

    println!(
        "\n*Note: P50/P99 metrics represent the completion of a {} request pipeline.",
        PIPELINE_DEPTH
    );

    Ok(())
}
