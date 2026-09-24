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
        // We open the file in read-only mode as the engine is designed for immutable datasets.
        let file = std::fs::File::open(path).map_err(EngineError::Io)?;

        // We map the entire file into the process's address space.
        // SAFETY: The file is not modified while mapped, satisfying Mmap requirements.
        let mmap = unsafe { Mmap::map(&file).map_err(|e| EngineError::MmapFailed(e.to_string()))? };

        // We disable sequential pre-fetching hints because binary search patterns
        // trigger random access, making standard read-ahead logic counterproductive.
        let _ = mmap.advise(Advice::Random);

        // Ensure the file is at least large enough to contain our fixed-size header.
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

        // We verify the header checksum using the "Zero-Field" technique.
        // We calculate the hash of the header struct while its own checksum field is set to zero.
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

        // We derive a raw pointer to the start of the index section (immediately following the header).
        // This allows us to create slices on-demand in the hot path without ownership
        // complexity inside the Arc-wrapped Storage object.
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
        // SAFETY: The pointer validity was established at creation by checking the Mmap length.
        // Since the Mmap is read-only and immutable for the lifetime of the Storage struct,
        // this pointer remains valid for as long as 'self' exists.
        // The slice is backed by the Mmap object which is owned by this Storage instance.
        unsafe { std::slice::from_raw_parts(self.index_ptr, self.header.count as usize) }
    }

    /// Performs a high-speed lookup of a key.
    /// This involves a binary search over the mmap index, followed by a direct
    /// reference to the value data in the mmap.
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
        // We verify the payload checksum on the read path to detect storage bit-rot.
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
