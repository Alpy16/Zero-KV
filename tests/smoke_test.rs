use anyhow::Result;
use kv_store::{Request, ResponseHeader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zerocopy::byteorder::network_endian::{U32, U64};

// this integration test assumes your server is running in another terminal
// you can run your server with 'cargo run' and execute this with 'cargo test'
#[tokio::test]
async fn test_successful_lookup() -> Result<()> {
    // 1. the handshake
    // we open a raw tcp connection to our running server port
    let mut socket = TcpStream::connect("127.0.0.1:5500")
        .await
        .expect("failed to connect to server. is 'cargo run' running in another tab?");

    // 2. forging the request frame
    // your protocol keys are pure u64 values. make sure this matches a key you baked!
    // we use zero-copy network-endian wrappers to set up our 16-byte fixed layout
    // inside tests/smoke_test.rs
    let req = Request {
        op: U32::new(0),    // 0 stands for our Get opcode
        _padding: 0,        // explicit structural alignment padding
        key: U64::new(100), // we use a real key that our baker actually wrote!
    };

    // 3. the outbound transmission
    // we extract the raw byte view of our request struct via zerocopy
    let req_bytes = zerocopy::AsBytes::as_bytes(&req);

    // we push the exact 16-byte frame down the socket pipe
    socket.write_all(req_bytes).await?;

    // 4. parsing the response header
    // our protocol guarantees the server will reply with an 8-byte header first
    let mut header_buf = [0u8; 8];
    socket.read_exact(&mut header_buf).await?;

    // we cast those raw network bytes back into our response header struct
    let response_header = ResponseHeader::from_bytes(&header_buf)
        .expect("server sent an invalid or malformed header frame");

    // 5. asserting the systems logic
    // check if the server verified the transaction as successful (status 0)
    assert_eq!(
        response_header.status.get(),
        0,
        "server returned an error status code"
    );

    // 6. pulling the data payload
    // read the value length from the header so we know exactly how many bytes to pull
    let value_len = response_header.length.get() as usize;
    assert!(
        value_len > 0,
        "database returned an empty or missing value payload"
    );

    // we allocate a dynamic heap buffer matching that exact size to catch the stream
    let mut value_buf = vec![0u8; value_len];
    socket.read_exact(&mut value_buf).await?;

    println!("smoke test success! received {} bytes of data", value_len);
    Ok(())
}

#[tokio::test]
async fn test_lookup_miss() -> Result<()> {
    // we test the boundary condition: what happens when a numeric key doesn't exist?
    let mut socket = TcpStream::connect("127.0.0.1:5500").await?;

    // we forge a massive key id that your baker definitely hasn't seen
    let req = Request {
        op: U32::new(0),
        _padding: 0,
        key: U64::new(999999999999),
    };

    // send the 16-byte frame
    socket.write_all(zerocopy::AsBytes::as_bytes(&req)).await?;

    // read the 8-byte response header
    let mut header_buf = [0u8; 8];
    socket.read_exact(&mut header_buf).await?;

    let response_header =
        ResponseHeader::from_bytes(&header_buf).expect("failed to decode response header");

    // our engine must reply with status code 1 (not found) and 0 data length
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
