use std::io;
use thiserror::Error;
pub mod storage;
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
#[derive(Debug, Copy, Clone)]
pub struct Header {
    pub magic: u64,   // Identifier (e.g., 0xA016)
    pub version: u64, // Format version
    pub count: u64,   // Total number of IndexEntries
    pub padding: u64, // Ensures 32-byte alignment
}

/// The 'IndexEntry' allows for O(log n) lookups via binary search.
/// Each entry is exactly 24 bytes.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct IndexEntry {
    pub key: u64,        // The lookup ID
    pub val_offset: u64, // Absolute position in the file
    pub val_len: u32,    // Length of the data blob
    pub _padding: u32,   // Ensures 24-byte alignment
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
pub struct Request {
    pub op: OpCode, // 0 = GET, 1 = PUT, etc.
    pub key: u64,   // the key for the operation
}

#[repr(C)]
pub struct ResponseHeader {
    pub status: u32, // 0 = OK, 1 = NOT_FOUND, 2 = ERR
    pub length: u32, // the length of the actual value following this header
}

impl Request {
    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != std::mem::size_of::<Request>() {
            //basically if it isnt exactly 12 bytes dont even bother reading it
            return None;
        } else {
            // get opcode from bytes 0 to 4, big endian because we want a consistent format across different architectures
            let op_raw = u32::from_be_bytes(bytes[0..4].try_into().ok()?);

            // get the key from bytes 8 to 16
            let key = u64::from_be_bytes(bytes[8..16].try_into().ok()?);

            // this ensures that even if the network sends a weird number like 99,
            // we handle it safely rather than crashing.
            let op = match op_raw {
                0 => OpCode::Get,
                1 => OpCode::Exists,
                _ => OpCode::Unknown,
            };
            // ... (your extraction logic)
            Some(Self { op: op, key: key })
        }
    }
    pub fn to_be_bytes(&self) -> [u8; 16] {
        let mut buffer = [0u8; 16];

        // slot the 4-byte OpCode into the first part
        buffer[0..4].copy_from_slice(&(self.op as u32).to_be_bytes());

        // slot the 8-byte Key into the remaining part
        // We start at index 4 because that's where the OpCode ends
        buffer[8..16].copy_from_slice(&self.key.to_be_bytes());

        // Return the completed buffer
        buffer
    }
}

impl ResponseHeader {
    pub fn to_be_bytes(&self) -> [u8; 8] {
        let mut buffer = [0u8; 8];

        buffer[0..4].copy_from_slice(&self.status.to_be_bytes());
        buffer[4..8].copy_from_slice(&self.length.to_be_bytes());
        buffer
        // same logic but for an 8 byte header instead of a 12 byte request
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != std::mem::size_of::<ResponseHeader>() {
            return None;
        } else {
            let status = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
            let length = u32::from_be_bytes(bytes[4..8].try_into().ok()?);
            Some(Self { status, length })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_roundtrip() {
        // 1. Create a request
        let original_req = Request {
            op: OpCode::Get,
            key: 0xDEADBEEFCAFEBABE,
        };

        // 2. Encode it to bytes
        let bytes = original_req.to_be_bytes();

        // 3. Decode it back
        let decoded_req = Request::from_be_bytes(&bytes).unwrap();

        // 4. Assert they match
        assert_eq!(original_req.op, decoded_req.op);
        assert_eq!(original_req.key, decoded_req.key);
    }
}
