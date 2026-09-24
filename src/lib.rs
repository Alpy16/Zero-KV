use std::io;
use thiserror::Error;
pub mod storage;
use zerocopy::{
    AsBytes, FromBytes, FromZeroes,
    byteorder::network_endian::{U32, U64},
};

pub const DEFAULT_STORAGE_PATH: &str = "storage.db";
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/zero-kv.sock";

/// `ResponseStatus` defines the machine-readable outcome of a request.
#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ResponseStatus {
    Ok = 0,
    NotFound = 1,
    Error = 2,
}

/// Errors reported by the storage engine.
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Data corruption: Checksum mismatch")]
    ChecksumMismatch,

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("File collision: {0}")]
    FileCollision(String),

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

/// A 32-byte storage header at file offset zero, using native-endian integers.
#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Header {
    pub magic: u64,
    pub version: u64,
    pub count: u64,
    pub header_checksum: u32,
    pub _padding: u32, // Explicit padding keeps the header at 32 bytes.
}

/// A 24-byte native-endian index entry, stored in key order.
/// Lookups use binary search and verify CRC32 over the full value payload.
#[repr(C)]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct IndexEntry {
    pub key: u64,
    pub val_offset: u64,
    pub val_len: u32,
    pub val_checksum: u32, // Checksum of the value payload.
}

/// Fixed sizes derived from the struct layouts to assist in offset calculations.
pub const HEADER_SIZE: usize = std::mem::size_of::<Header>();
pub const INDEX_ENTRY_SIZE: usize = std::mem::size_of::<IndexEntry>();

impl Header {
    /// Checks the magic number and version, without validating the checksum or bounds.
    pub fn is_valid(&self) -> bool {
        self.magic == 0xA016 && self.version == 1
    }
}

/// Operation codes; unrecognized wire values map to `Unknown`.
#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum OpCode {
    Get = 0,
    Exists = 1,
    Unknown = 99,
}

/// A 16-byte request frame with big-endian operation and key fields.
/// The Rust struct has 8-byte alignment; decoding copies from any byte alignment.
#[repr(C, align(8))]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone)]
pub struct Request {
    pub op: U32,       // automatically handles big-endian conversion for the operation code.
    pub _padding: u32, // explicit padding to ensure the `key` field is 8-byte aligned.
    pub key: U64,      // automatically handles big-endian conversion for the 64-bit key.
}

/// An 8-byte response header with big-endian status and payload length.
#[repr(C, align(8))]
#[derive(AsBytes, FromBytes, FromZeroes, Debug, Copy, Clone, Default)]
pub struct ResponseHeader {
    pub status: U32, // the status code of the response (e.g., ok, not found).
    pub length: U32, // the length of the data payload that follows the header.
}

impl Request {
    /// Copies a request from exactly 16 bytes, regardless of input alignment.
    /// Returns `None` if the slice length differs.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    /// Converts the raw numeric operation code into a typed `OpCode`.
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

        let decoded_req = Request::from_bytes(&bytes).expect("Failed to decode");

        assert_eq!(original_req.op.get(), decoded_req.op.get());
        assert_eq!(original_req.key.get(), decoded_req.key.get());
    }
}
