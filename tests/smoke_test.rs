use anyhow::Result;
use kv_store::{Request, ResponseHeader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
// we bring in `TcpStream` for establishing network connections.
use tokio::net::UnixStream;
use zerocopy::byteorder::network_endian::{U32, U64};

// this integration test assumes my server is running in another terminal.
// i can run the server with 'cargo run' and execute this with 'cargo test'.
#[tokio::test]
async fn test_successful_lookup() -> Result<()> {
    // i'm opening a raw unix socket connection here. since i moved the server
    // to uds for performance, i need to make sure my tests follow suit.
    let mut socket = UnixStream::connect("/tmp/zero-kv.sock")
        .await
        .expect("failed to connect to unix socket. is 'cargo run' running in another tab?");

    // i'm forging a 16-byte request frame manually. i'm using a key i know
    // is in the baked database so i can verify the full data path works.
    let req = Request {
        op: U32::new(0), // 0 is the 'get' opcode i defined in lib.rs.
        _padding: 0,
        key: U64::new(100), // i'm using a real key i baked into the test storage.
    };

    // i'm extracting the raw bytes directly via zerocopy. there's no serialization
    // logic here—i'm just taking the memory layout of the struct and pushing
    // it straight down the socket pipe.
    socket.write_all(zerocopy::AsBytes::as_bytes(&req)).await?;

    let mut header_buf = [0u8; 8];
    socket.read_exact(&mut header_buf).await?;

    // i'm casting the raw bytes back into my struct to see what the server said.
    let response_header = ResponseHeader::from_bytes(&header_buf)
        .expect("server sent an invalid or malformed header frame");

    // i'm asserting that the server found the key (status 0).
    assert_eq!(
        response_header.status.get(),
        0,
        "server returned an error status code"
    );

    // i read the value length from the header so i know exactly how much
    // data i need to pull from the stream next.
    let value_len = response_header.length.get() as usize;
    assert!(
        value_len > 0,
        "database returned an empty or missing value payload"
    );

    // i'm allocating a heap buffer to catch the data payload.
    let mut value_buf = vec![0u8; value_len];
    socket.read_exact(&mut value_buf).await?;

    println!("smoke test success! received {} bytes of data", value_len);
    Ok(())
}

#[tokio::test]
async fn test_lookup_miss() -> Result<()> {
    // i'm testing the "not found" path here using the unix socket.
    let mut socket = UnixStream::connect("/tmp/zero-kv.sock").await?;

    // i'm forging a request with a key that i definitely didn't bake,
    // which should trigger a lookup miss in the storage engine.
    let req = Request {
        op: U32::new(0),
        _padding: 0,
        key: U64::new(999999999999),
    };

    // i'm sending the 16-byte frame just like before.
    socket.write_all(zerocopy::AsBytes::as_bytes(&req)).await?;

    let mut header_buf = [0u8; 8];
    socket.read_exact(&mut header_buf).await?;

    let response_header =
        ResponseHeader::from_bytes(&header_buf).expect("failed to decode response header");

    // i'm checking that the engine correctly returns status 1 (not found)
    // and ensures no data bytes are sent.
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
