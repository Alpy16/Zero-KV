use crate::{EngineError, HEADER_SIZE, Header, IndexEntry};
use anyhow::Result;
use memmap2::{Advice, Mmap};
use zerocopy::{AsBytes, FromBytes};

/// We leverage Memory-Mapped I/O (Mmap) to treat the database file as an addressable
/// byte array in memory. This delegates page cache management to the kernel,
/// enabling zero-copy lookups and maximizing I/O throughput.
pub struct Storage {
    pub object: Mmap,
    pub header: Header,
    /// We utilize a raw pointer for the index section to satisfy Send/Sync requirements
    /// within an Arc. The safety of this pointer is guaranteed by the lifetime of the
    /// owned Mmap object.
    index_ptr: *const IndexEntry,
}

impl Storage {
    pub fn new(path: &str) -> Result<Self, EngineError> {
        let file = std::fs::File::open(path).map_err(EngineError::Io)?;
        let mmap = unsafe { Mmap::map(&file).map_err(|e| EngineError::MmapFailed(e.to_string()))? };

        // We disable sequential pre-fetching hints because binary search patterns
        // trigger random access, making standard read-ahead logic counterproductive.
        let _ = mmap.advise(Advice::Random);

        if mmap.len() < HEADER_SIZE {
            return Err(EngineError::InvalidHeader);
        }

        let header = Header::read_from(&mmap[..HEADER_SIZE]).ok_or(EngineError::InvalidHeader)?;
        if header.magic != 0xA016 {
            return Err(EngineError::MagicMismatch);
        }
        if header.version != 1 {
            return Err(EngineError::VersionMismatch(header.version));
        }

        // Verify Header Integrity
        let mut header_copy = header;
        let stored_checksum = header_copy.header_checksum;
        header_copy.header_checksum = 0;
        let calculated_checksum = crc32fast::hash(header_copy.as_bytes());
        if stored_checksum != calculated_checksum {
            return Err(EngineError::InvalidHeader);
        }

        let total_index_size = header.count as usize * std::mem::size_of::<IndexEntry>();
        if mmap.len() < HEADER_SIZE + total_index_size {
            return Err(EngineError::IndexSizeMismatch);
        }

        let index_ptr = unsafe { mmap.as_ptr().add(HEADER_SIZE) as *const IndexEntry };
        Ok(Storage {
            object: mmap,
            header,
            index_ptr,
        })
    }

    #[inline(always)]
    fn index(&self) -> &[IndexEntry] {
        unsafe { std::slice::from_raw_parts(self.index_ptr, self.header.count as usize) }
    }

    pub fn get(&self, key: u64) -> Result<Option<&[u8]>, EngineError> {
        let index = self.index();
        let pos = match index.binary_search_by_key(&key, |e| e.key) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let entry = &index[pos];
        let start = entry.val_offset as usize;
        let end = start + entry.val_len as usize;
        if end > self.object.len() {
            return Err(EngineError::IndexSizeMismatch);
        }

        let data = &self.object[start..end];
        if crc32fast::hash(data) != entry.val_checksum {
            return Err(EngineError::ChecksumMismatch);
        }
        Ok(Some(data))
    }
}

unsafe impl Send for Storage {}
unsafe impl Sync for Storage {}

#[cfg(test)]
mod tests {
    use super::*;
    // we bring in `Header` and `IndexEntry` from `lib.rs` for testing purposes.
    use crate::{Header, IndexEntry};
    use std::fs;
    use zerocopy::AsBytes;

    #[test]
    fn test_basic_retrieval() -> Result<(), Box<dyn std::error::Error>> {
        let path = "test_engine.db";
        let mut file_content = Vec::new();
        // we manually construct a header and an index entry to simulate a baked file.

        let mut header = Header {
            magic: 0xA016,
            version: 1,
            count: 1,
            header_checksum: 0,
            _padding: 0,
        };
        header.header_checksum = crc32fast::hash(header.as_bytes());

        let entry = IndexEntry {
            key: 42,
            val_offset: 56, // Header (32) + 1 IndexEntry (24)
            val_len: 5,
            val_checksum: crc32fast::hash(b"hello"),
        };

        file_content.extend_from_slice(header.as_bytes());
        file_content.extend_from_slice(entry.as_bytes());

        // we append the actual value data.
        file_content.extend_from_slice(b"hello");
        // we write the simulated database file to disk.
        fs::write(path, file_content)?;

        let storage = Storage::new(path)?;
        // we attempt to retrieve the key we just wrote.
        let result = storage.get(42)?;

        assert!(result.is_some());
        assert_eq!(result.unwrap(), b"hello");

        fs::remove_file(path)?;
        Ok(())
    }
}
