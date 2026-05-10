use std::io;
use thiserror::Error;
pub mod storage;
use zerocopy::{
    AsBytes, FromBytes, FromZeroes,
    byteorder::network_endian::{U32, U64},
};

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ResponseStatus {
    Ok = 0,
    NotFound = 1,
    Error = 2,
}

/// Custom Error type for the entire storage engine.
/// We use 'thiserror' to keep our error handling concise but descriptive.
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("File collision: {0}")]
    FileCollision(String),

    #[error("Invalid storage file: Magic number or version mismatch")]
    InvalidFormat,

    #[error("Key not found: {0}")]
    KeyNotFound(u64),

    #[error("Memory mapping failed: {0}")]
    MmapFailed(String),
}

/// The 'Header' sits at byte 0 of your file.
/// We use repr(C) to prevent Rust from reordering fields.
#[repr(C)]
// we add these derives so the baker can use .as_bytes() safely
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Header {
    pub magic: u64,
    pub version: u64,
    pub count: u64,
    pub padding: u64,
}

#[repr(C)]
// we do the same for IndexEntry so the baker can write the whole list at once
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct IndexEntry {
    pub key: u64,
    pub val_offset: u64,
    pub val_len: u32,
    pub _padding: u32,
}

// Constant sizes to avoid magic numbers in your math
pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
pub const INDEX_ENTRY_SIZE: usize = std::mem::size_of::<IndexEntry>();

/// A helper to verify the file's "Magic Number"
impl Header {
    pub fn is_valid(&self) -> bool {
        self.magic == 0xA016 && self.version == 1
    }
}

// a fixed-width binary frame for network communication. This is how clients will talk to our server.

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]

// OpCode is the exact same concept of how solidity talks to the EVM so this was pretty easy for me to get.
pub enum OpCode {
    Get = 0,
    Exists = 1,
    Unknown = 99,
}

#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Request {
    pub op: U32,       // Automatically handles Big-Endian
    pub _padding: u32, // Explicit padding for 8-byte alignment
    pub key: U64,      // Automatically handles Big-Endian
}

#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct ResponseHeader {
    // we use U32 here just like we did in the Request struct
    pub status: U32,
    pub length: U32,
}

impl Request {
    /// REPLACEMENT FOR from_be_bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        // zerocopy handles the length check and the mapping in one go.
        // It returns None if the slice is the wrong size.
        // my manual implementation was very long and prone to errors, this is much cleaner.
        Self::read_from(bytes)
    }

    /// REPLACEMENT FOR to_be_bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        // copies the whole struct into the buffer in one move.
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    // just to make my life easier when we want the OpCode as an enum when we build the server logic
    pub fn opcode(&self) -> OpCode {
        match self.op.get() {
            0 => OpCode::Get,
            1 => OpCode::Exists,
            _ => OpCode::Unknown,
        }
    }
}

impl ResponseHeader {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from(bytes)
    }

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
    fn test_protocol_roundtrip() {
        // create a request using zerocopy types
        let original_req = Request {
            op: U32::new(OpCode::Get as u32), // wrap the enum as u32
            _padding: 0,
            key: U64::new(0xDEADBEEFCAFEBABE), // wrap the raw u64
        };

        // encode
        let bytes = original_req.to_bytes();

        // decode
        let decoded_req = Request::from_bytes(&bytes).expect("Failed to decode");

        // assert match (using .get() to compare values)
        assert_eq!(original_req.op.get(), decoded_req.op.get());
        assert_eq!(original_req.key.get(), decoded_req.key.get());
    }
}
