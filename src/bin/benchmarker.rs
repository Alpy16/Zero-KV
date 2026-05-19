use anyhow::Result;
use kv_store::{Request, ResponseHeader};
use std::time::{Duration, Instant};
// we bring in `AsyncReadExt` and `AsyncWriteExt` for asynchronous socket I/O.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use zerocopy::{
    AsBytes,
    byteorder::network_endian::{U32, U64},
};

#[tokio::main]
async fn main() -> Result<()> {
    // i found 100 tasks to be the sweet spot. it's enough to keep my
    // cpu cores busy without drowning the tokio scheduler in
    // context-switching noise.
    let concurrency = 100;
    let reqs_per_conn = 1000;
    let total_expected = concurrency * reqs_per_conn;

    println!("stage 6: the benchmarker initializing...");
    println!(
        "target: {} concurrent connections, {} requests each (total: {})",
        concurrency, reqs_per_conn, total_expected
    );

    // i'm using a channel for the metrics so the workers don't have to
    // fight over a global mutex. if they did, the lock contention
    // would actually ruin our latency measurements.
    let (tx, mut rx) = mpsc::channel::<Vec<Duration>>(concurrency);

    let keys = [50, 100, 200];
    let benchmark_start = Instant::now();

    for _ in 0..concurrency {
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let mut socket = match UnixStream::connect("/tmp/zero-kv.sock").await {
                Ok(s) => s,
                Err(_) => return,
            };

            let mut data_buf = vec![0u8; 1024];
            let mut local_timings = Vec::with_capacity(reqs_per_conn);

            // i'm using pipelining here—sending 10 requests at once before
            // reading any responses. it's the only way to really push the
            // server's batch-reading code to its limit.
            let pipeline_depth = 10;
            let mut i = 0;
            while i < reqs_per_conn {
                let start = Instant::now();
                let mut request_batch = Vec::with_capacity(pipeline_depth * 16);
                for j in 0..pipeline_depth {
                    if i + j >= reqs_per_conn {
                        break;
                    }
                    let req = Request {
                        op: U32::new(0),
                        _padding: 0,
                        key: U64::new(keys[(i + j) % 3]),
                    };
                    request_batch.extend_from_slice(req.as_bytes());
                }

                if socket.write_all(&request_batch).await.is_err() {
                    break;
                }

                for j in 0..pipeline_depth {
                    if i + j >= reqs_per_conn {
                        break;
                    }

                    let mut header_buf = [0u8; 8];
                    if socket.read_exact(&mut header_buf).await.is_err() {
                        break;
                    }

                    let header = ResponseHeader::from_bytes(&header_buf).unwrap();
                    let len = header.length.get() as usize;

                    if len > 0 {
                        if len > data_buf.len() {
                            data_buf.resize(len, 0);
                        }
                        if socket.read_exact(&mut data_buf[..len]).await.is_err() {
                            break;
                        }
                    }

                    local_timings.push(start.elapsed() / pipeline_depth as u32);
                }

                i += pipeline_depth;
            }

            let _ = tx_clone.send(local_timings).await;
        });
    }

    drop(tx);

    let mut timings = Vec::with_capacity(total_expected);
    while let Some(mut batch) = rx.recv().await {
        timings.append(&mut batch);
    }

    let total_time = benchmark_start.elapsed();
    // we calculate the total time taken for the entire benchmark.

    // 6. the math analyzer
    if timings.is_empty() {
        println!("benchmark failed: zero successful requests recorded.");
        return Ok(());
    }

    // we sort the vector so we can calculate percentiles by array index boundaries
    // `sort_unstable` is generally faster than stable sort when order of equal elements doesn't matter.
    timings.sort_unstable();

    let len = timings.len();
    let avg = timings.iter().sum::<Duration>() / len as u32;
    let p50 = timings[len / 2];
    let p95 = timings[(len as f64 * 0.95) as usize];
    let p99 = timings[(len as f64 * 0.99) as usize];
    let p99_9 = timings[(len as f64 * 0.999) as usize];

    println!("\n--- benchmark results ---");
    println!("total time:   {:.2?}", total_time);
    println!("success rate: {}/{}", len, total_expected);
    println!(
        "throughput:   {:.0} req/sec",
        (len as f64) / total_time.as_secs_f64()
    );

    println!("\n--- latency distribution ---");
    println!("average:      {:.2?}", avg);
    println!("p50 (median): {:.2?}", p50);
    println!("p95:          {:.2?}", p95);
    println!("p99:          {:.2?}", p99);
    println!("p99.9:        {:.2?}", p99_9);
    println!("max spike:    {:.2?}", timings[len - 1]);

    Ok(())
}
