use std::io;
use thiserror::Error;
pub mod storage;
// we bring in `zerocopy` traits to enable safe, allocation-free byte-to-struct conversions.
use zerocopy::{
    AsBytes, FromBytes, FromZeroes,
    byteorder::network_endian::{U32, U64},
};

/// Default configuration constants used across the engine and tools.
pub const DEFAULT_STORAGE_PATH: &str = "storage.db";
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/zero-kv.sock";

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
// i'm using an explicit u32 discriminant here so the status fits perfectly into
// our 8-byte response header without any padding or alignment surprises.
pub enum ResponseStatus {
    Ok = 0,
    NotFound = 1,
    Error = 2,
}

/// Custom Error type for the entire storage engine.
// i'm centralizing everything in this enum. using thiserror makes it easy to
// wrap low-level io or mmap issues into something that fits our engine's logic.
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("File collision: {0}")]
    FileCollision(String),

    // updated to match your new hardening logic
    #[error("Invalid storage file: Magic number mismatch")]
    MagicMismatch,

    #[error("Invalid storage file: Version mismatch (expected 1, got {0})")]
    VersionMismatch(u64),

    #[error("Invalid storage file: Index exceeds file bounds")]
    IndexSizeMismatch,

    #[error("Invalid storage file: Header is too small or corrupted")]
    InvalidHeader,

    #[error("Key not found: {0}")]
    KeyNotFound(u64),

    #[error("Memory mapping failed: {0}")]
    MmapFailed(String),
}

/// The 'Header' sits at byte 0 of your file.
#[repr(C)]
// i'm using repr(c) to force a stable memory layout. since i'm using zerocopy
// to map raw file bytes directly to these structs, i can't let the compiler
// shuffle the fields around during optimization.
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Header {
    pub magic: u64,
    pub version: u64,
    pub count: u64,
    pub padding: u64,
}

#[repr(C)]
// i designed the index entry to be a fixed 24 bytes. this is the key to
// our performance—it lets me treat the whole index section as a simple
// array and jump to any entry using basic math.
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct IndexEntry {
    pub key: u64,
    pub val_offset: u64,
    pub val_len: u32,
    pub _padding: u32,
}

pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
pub const INDEX_ENTRY_SIZE: usize = std::mem::size_of::<IndexEntry>();

impl Header {
    // just a quick sanity check to make sure i'm actually opening a zero-kv file.
    pub fn is_valid(&self) -> bool {
        self.magic == 0xA016 && self.version == 1
    }
}

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum OpCode {
    Get = 0,
    Exists = 1,
    Unknown = 99,
}

// i made this request frame exactly 16 bytes. using zerocopy's big-endian
// types (U32/U64) handles the network byte order automatically, so i don't
// have to manually call .to_be_bytes() and risk a mistake.
#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Request {
    pub op: U32,       // automatically handles big-endian conversion for the operation code.
    pub _padding: u32, // explicit padding to ensure the `key` field is 8-byte aligned.
    pub key: U64,      // automatically handles big-endian conversion for the 64-bit key.
}

// every response starts with these 8 bytes. it's the "contract" with
// the client—telling them if the key exists and how much data follows.
#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone, Default)]
pub struct ResponseHeader {
    pub status: U32, // the status code of the response (e.g., ok, not found).
    pub length: U32, // the length of the data payload that follows the header.
}

impl Request {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    pub fn opcode(&self) -> OpCode {
        match self.op.get() {
            0 => OpCode::Get,
            1 => OpCode::Exists,
            _ => OpCode::Unknown,
        }
    }
}

impl ResponseHeader {
    /// similar to `Request::from_bytes`, this method safely decodes a byte slice
    /// into a `ResponseHeader` struct, used by clients to parse server responses.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from(bytes)
    }

    /// converts the `ResponseHeader` struct into a fixed-size byte array for network transmission.
    /// this is used by the `main.rs` server to send response headers.
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.as_bytes());
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// we test the roundtrip conversion of our `Request` struct to bytes and back.
    /// this ensures our `zerocopy` implementation and byte order handling are correct.
    fn test_protocol_roundtrip() {
        // we create an original request using `zerocopy` types for op code and key.
        let original_req = Request {
            op: U32::new(OpCode::Get as u32), // we wrap the enum variant as a `U32` for network endianness.
            _padding: 0,
            key: U64::new(0xDEADBEEFCAFEBABE), // we wrap the raw `u64` key as a `U64`.
        };

        // we encode the request struct into its byte representation.
        let bytes = original_req.to_bytes();

        // we decode the bytes back into a `Request` struct.
        let decoded_req = Request::from_bytes(&bytes).expect("Failed to decode");

        // we assert that the decoded request matches the original, using `.get()`
        // to retrieve the native endian values for comparison.
        assert_eq!(original_req.op.get(), decoded_req.op.get());
        assert_eq!(original_req.key.get(), decoded_req.key.get());
    }
}
