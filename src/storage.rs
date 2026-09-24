use crate::{EngineError, HEADER_SIZE, Header, IndexEntry};
use anyhow::Result;
use memmap2::{Advice, Mmap};
use zerocopy::{AsBytes, FromBytes};

/// Read-only mapped storage with borrowed value slices.
pub struct Storage {
    pub object: Mmap,
    pub header: Header,
    /// Points into `object`, avoiding a self-referential slice field.
    /// Validity requires keeping the mapping and index count unchanged.
    index_ptr: *const IndexEntry,
}

impl Storage {
    pub fn new(path: &str) -> Result<Self, EngineError> {
        // Open the immutable dataset read-only.
        let file = std::fs::File::open(path).map_err(EngineError::Io)?;

        // SAFETY: The caller must ensure the file is not modified or truncated while mapped.
        let mmap = unsafe { Mmap::map(&file).map_err(|e| EngineError::MmapFailed(e.to_string()))? };

        // Hint that binary searches access the mapping non-sequentially.
        let _ = mmap.advise(Advice::Random);

        // Ensure the file is at least large enough to contain the fixed-size header.
        if mmap.len() < HEADER_SIZE {
            return Err(EngineError::InvalidHeader);
        }

        // Extract the header from the start of the mmap.
        let header = Header::read_from(&mmap[..HEADER_SIZE]).ok_or(EngineError::InvalidHeader)?;
        if header.magic != 0xA016 {
            return Err(EngineError::MagicMismatch);
        }
        if header.version != 1 {
            return Err(EngineError::VersionMismatch(header.version));
        }

        // Verify CRC32 with the header checksum field set to zero.
        let mut header_copy = header;
        let stored_checksum = header_copy.header_checksum;
        header_copy.header_checksum = 0;
        let calculated_checksum = crc32fast::hash(header_copy.as_bytes());

        if stored_checksum != calculated_checksum {
            return Err(EngineError::InvalidHeader);
        }

        // Calculate the expected size of the index section based on the count stored in the header.
        let total_index_size = header.count as usize * std::mem::size_of::<IndexEntry>();
        if mmap.len() < HEADER_SIZE + total_index_size {
            return Err(EngineError::IndexSizeMismatch);
        }

        // SAFETY: HEADER_SIZE is within the mapping after the header length check.
        let index_ptr = unsafe { mmap.as_ptr().add(HEADER_SIZE) as *const IndexEntry };
        Ok(Storage {
            object: mmap,
            header,
            index_ptr,
        })
    }

    #[inline(always)]
    /// Provides a safe slice view of the sorted index.
    fn index(&self) -> &[IndexEntry] {
        // SAFETY: Requires an aligned index within the owned mapping, with the mapping
        // and header count unchanged since construction and no external file mutation.
        unsafe { std::slice::from_raw_parts(self.index_ptr, self.header.count as usize) }
    }

    /// Finds a key by binary search and verifies the value CRC32 before borrowing it.
    /// Cost is O(log n + value length) for a hit.
    pub fn get(&self, key: u64) -> Result<Option<&[u8]>, EngineError> {
        let index = self.index();
        // Binary search is O(log N) and takes advantage of the sorted index section.
        let pos = match index.binary_search_by_key(&key, |e| e.key) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let entry = &index[pos];
        let start = entry.val_offset as usize;
        let end = start + entry.val_len as usize;

        // Defensive bounds check to protect against corrupted index offsets.
        if end > self.object.len() {
            return Err(EngineError::IndexSizeMismatch);
        }

        let data = &self.object[start..end];
        // Verify the payload CRC32 before returning the borrowed bytes.
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
    use crate::{Header, IndexEntry};
    use std::fs;
    use zerocopy::AsBytes;

    #[test]
    fn test_basic_retrieval() -> Result<(), Box<dyn std::error::Error>> {
        let path = "test_engine.db";
        let mut file_content = Vec::new();
        // Construct a one-entry storage file.

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

        file_content.extend_from_slice(b"hello");
        fs::write(path, file_content)?;

        let storage = Storage::new(path)?;
        let result = storage.get(42)?;

        assert!(result.is_some());
        assert_eq!(result.unwrap(), b"hello");

        fs::remove_file(path)?;
        Ok(())
    }
}
