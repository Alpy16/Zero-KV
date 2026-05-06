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
