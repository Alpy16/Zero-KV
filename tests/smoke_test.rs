// Requires a running server with a nonempty value at key 100 and no key 999999999999.
// Start it with `cargo run --bin kv_store`, then run `cargo test --test smoke_test`.
use anyhow::Result;
use kv_store::{Request, ResponseHeader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use zerocopy::byteorder::network_endian::{U32, U64};

#[tokio::test]
async fn test_successful_lookup() -> Result<()> {
    let mut socket = UnixStream::connect("/tmp/zero-kv.sock")
        .await
        .expect("failed to connect to unix socket. is 'cargo run' running in another tab?");
    let req = Request {
        op: U32::new(0),
        _padding: 0,
        key: U64::new(100),
    };
    socket.write_all(zerocopy::AsBytes::as_bytes(&req)).await?;

    let mut header_buf = [0u8; 8];
    socket.read_exact(&mut header_buf).await?;
    let response_header = ResponseHeader::from_bytes(&header_buf)
        .expect("server sent an invalid or malformed header frame");
    assert_eq!(
        response_header.status.get(),
        0,
        "server returned an error status code"
    );
    let value_len = response_header.length.get() as usize;
    assert!(
        value_len > 0,
        "database returned an empty or missing value payload"
    );
    let mut value_buf = vec![0u8; value_len];
    socket.read_exact(&mut value_buf).await?;

    println!("smoke test success! received {} bytes of data", value_len);
    Ok(())
}

#[tokio::test]
async fn test_lookup_miss() -> Result<()> {
    let mut socket = UnixStream::connect("/tmp/zero-kv.sock").await?;
    let req = Request {
        op: U32::new(0),
        _padding: 0,
        key: U64::new(999999999999),
    };
    socket.write_all(zerocopy::AsBytes::as_bytes(&req)).await?;

    let mut header_buf = [0u8; 8];
    socket.read_exact(&mut header_buf).await?;

    let response_header =
        ResponseHeader::from_bytes(&header_buf).expect("failed to decode response header");
    assert_eq!(
        response_header.status.get(),
        1,
        "engine failed to declare a key miss"
    );
    assert_eq!(
        response_header.length.get(),
        0,
        "engine sent data bytes on a miss"
    );

    Ok(())
}
