use crate::EngineError;
use crate::{HEADER_SIZE, Header, IndexEntry};
use anyhow::Result;
use memmap2::{Advice, Mmap};
use zerocopy::FromBytes;

// i went with mmap because i want to treat the disk as if it were just
// a huge array in memory. it lets the kernel handle all the paging and
// caching, which is the secret sauce for our zero-copy lookups.
pub struct Storage {
    pub object: Mmap,
    pub header: Header,
    // i'm using a raw pointer for the index because i need this struct
    // to be Send/Sync so it can live in an Arc. rust's lifetime rules
    // make self-referential slices nearly impossible to move between
    // threads, so i'm handling the safety manually here.
    index_ptr: *const IndexEntry,
}

impl Storage {
    pub fn new(path: &str) -> Result<Self, EngineError> {
        let file = std::fs::File::open(path).map_err(EngineError::Io)?;
        let mmap = unsafe { Mmap::map(&file).map_err(|e| EngineError::MmapFailed(e.to_string()))? };

        // i added this advice to stop the kernel from being "helpful."
        // since i'm using binary search, i'll be jumping around. sequential
        // pre-fetching just wastes cache space and i/o bandwidth.
        let _ = mmap.advise(Advice::Random);

        if mmap.len() < HEADER_SIZE {
            return Err(EngineError::InvalidHeader);
        }

        let header = Header::read_from(&mmap[..HEADER_SIZE]).ok_or(EngineError::InvalidHeader)?;
        if !header.is_valid() {
            return Err(EngineError::MagicMismatch);
        }

        let index_ptr = unsafe { mmap.as_ptr().add(HEADER_SIZE) as *const IndexEntry };
        Ok(Storage {
            object: mmap,
            header,
            index_ptr,
        })
    }

    #[inline(always)]
    // i'm inlining this so the compiler just treats the index as a direct
    // array access, removing any function call overhead from the hot path.
    fn index(&self) -> &[IndexEntry] {
        unsafe { std::slice::from_raw_parts(self.index_ptr, self.header.count as usize) }
    }

    pub fn get(&self, key: u64) -> Option<&[u8]> {
        let index = self.index();
        // i can use binary search because i made sure the baker sorted
        // the entries. it's O(log n), so even with millions of keys,
        // it only takes a few hops.
        let pos = index.binary_search_by_key(&key, |e| e.key).ok()?;
        let entry = &index[pos];
        let start = entry.val_offset as usize;
        let end = start + entry.val_len as usize;
        if end > self.object.len() {
            return None;
        }
        Some(&self.object[start..end])
    }
}

// Explicitly implement Send and Sync. Since the mmap is read-only after creation
// and we are not using internal mutability, it is safe to share across threads.
unsafe impl Send for Storage {}
unsafe impl Sync for Storage {}

#[cfg(test)]
mod tests {
    use super::*;
    // we bring in `Header` and `IndexEntry` from `lib.rs` for testing purposes.
    use crate::{Header, IndexEntry};
    use std::fs;

    #[test]
    fn test_basic_retrieval() -> Result<(), Box<dyn std::error::Error>> {
        let path = "test_engine.db";
        let mut file_content = Vec::new();
        // we manually construct a header and an index entry to simulate a baked file.

        let header = Header {
            magic: 0xA016,
            version: 1,
            count: 1,
            padding: 0,
        };

        let entry = IndexEntry {
            key: 42,
            val_offset: 56, // Header (32) + 1 IndexEntry (24)
            val_len: 5,
            _padding: 0,
        };

        // we use unsafe blocks here to convert our structs into raw byte slices for writing to the file.
        unsafe {
            let h_ptr = &header as *const Header as *const u8;
            file_content.extend_from_slice(std::slice::from_raw_parts(
                h_ptr,
                std::mem::size_of::<Header>(),
            ));

            let e_ptr = &entry as *const IndexEntry as *const u8;
            file_content.extend_from_slice(std::slice::from_raw_parts(
                e_ptr,
                std::mem::size_of::<IndexEntry>(),
            ));
        }

        // we append the actual value data.
        file_content.extend_from_slice(b"hello");
        // we write the simulated database file to disk.
        fs::write(path, file_content)?;

        let storage = Storage::new(path)?;
        // we attempt to retrieve the key we just wrote.
        let result = storage.get(42);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), b"hello");

        fs::remove_file(path)?;
        Ok(())
    }
}
