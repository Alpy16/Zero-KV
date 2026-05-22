use std::io;
use thiserror::Error;
pub mod storage;
/// We utilize `zerocopy` traits to facilitate safe, allocation-free casting of raw byte slices
/// into structured memory layouts, minimizing CPU overhead during serialization and deserialization.
use zerocopy::{
    AsBytes, FromBytes, FromZeroes,
    byteorder::network_endian::{U32, U64},
};

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

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Data corruption: Checksum mismatch")]
    ChecksumMismatch,

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("File collision: {0}")]
    FileCollision(String),

    // updated to match your new hardening logic
    #[error("Invalid storage file: Magic number mismatch")]
    MagicMismatch,

    #[error("Invalid storage file: Version mismatch (Expected 1, Got {0})")]
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
/// We use `repr(C)` to ensure a stable memory layout across different compiler versions,
/// which is critical for direct memory mapping of the database file.
#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Header {
    pub magic: u64,
    pub version: u64,
    pub count: u64,
    pub header_checksum: u32,
    pub _padding: u32, // Adjusted to 32-bit to maintain 8-byte alignment for the total struct (32 bytes).
}

/// Each `IndexEntry` represents a fixed-width pointer to a value in the data section.
/// We repurposed the previous padding field to store a CRC32C checksum of the value data,
/// enabling O(1) integrity verification during the retrieval hot path.
#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct IndexEntry {
    pub key: u64,
    pub val_offset: u64,
    pub val_len: u32,
    pub val_checksum: u32, // Checksum of the value payload.
}

pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
pub const INDEX_ENTRY_SIZE: usize = std::mem::size_of::<IndexEntry>();

impl Header {
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

/// We enforce a 16-byte fixed-width request frame. By utilizing Big-Endian types,
/// we ensure protocol compatibility across different CPU architectures.
#[repr(C, align(8))]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Request {
    pub op: U32,       // automatically handles big-endian conversion for the operation code.
    pub _padding: u32, // explicit padding to ensure the `key` field is 8-byte aligned.
    pub key: U64,      // automatically handles big-endian conversion for the 64-bit key.
}

/// The response header establishes a contract with the client, providing the
/// operation status and the exact length of the trailing payload.
#[repr(C, align(8))]
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
        let original_req = Request {
            op: U32::new(OpCode::Get as u32),
            _padding: 0,
            key: U64::new(0xDEADBEEFCAFEBABE),
        };

        let bytes = original_req.to_bytes();

        // we decode the bytes back into a `Request` struct.
        let decoded_req = Request::from_bytes(&bytes).expect("Failed to decode");

        // we assert that the decoded request matches the original, using `.get()`
        // to retrieve the native endian values for comparison.
        assert_eq!(original_req.op.get(), decoded_req.op.get());
        assert_eq!(original_req.key.get(), decoded_req.key.get());
    }
}
