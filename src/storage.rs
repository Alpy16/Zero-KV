// 1 - the mapping tools
use memmap2::Mmap;

use crate::EngineError;
// 2 - our blueprints from lib.rs
use crate::{HEADER_SIZE, Header, IndexEntry};

// we use this struct to hold our memory-mapped file and its index
pub struct Storage {
    // the raw memory map object from the os
    pub object: Mmap,
    // a local copy of the file header for quick access
    pub header: Header,
    // we store the raw pointer to the index here instead of a reference
    // this avoids the "self-referential struct" nightmare in rust
    // we can move this struct into different threads/tasks without the borrow checker complaining
    index_ptr: *const IndexEntry,
}

impl Storage {
    // we initialize our engine by mapping the file to virtual memory
    pub fn new(path: &str) -> Result<Self, EngineError> {
        // we open the file handle from the disk
        let file = std::fs::File::open(path)?;
        // we map the file into our process address space
        // we use unsafe because we are trusting the file format to be valid
        let mmap = unsafe { Mmap::map(&file)? };

        // we read the header by casting the very first byte of the mmap
        let header_ptr = mmap.as_ptr() as *const Header;
        let header = unsafe { *header_ptr };

        // we calculate exactly where the index starts (right after the header)
        // we store this address once so we don't have to calculate it on every get()
        let index_ptr = unsafe { mmap.as_ptr().add(HEADER_SIZE) as *const IndexEntry };

        // we return our hardened storage object
        Ok(Storage {
            object: mmap,
            header,
            index_ptr,
        })
    }

    // we use this private helper to turn our raw pointer into a safe rust slice
    // this is "zero-cost" because it doesn't actually copy any data
    // it just gives us a high-level view of the memory for the duration of a function call
    fn index(&self) -> &[IndexEntry] {
        unsafe { std::slice::from_raw_parts(self.index_ptr, self.header.count as usize) }
    }

    // we perform our lookups using the helper we just built
    pub fn get(&self, key: u64) -> Option<&[u8]> {
        // we use the safe slice from our helper to run the binary search
        // this is much cleaner than the old version because the unsafe logic is hidden
        let pos = self.index().binary_search_by_key(&key, |e| e.key).ok()?;

        // we extract the entry metadata at the found position
        let entry = &self.index()[pos];
        let start = entry.val_offset as usize;
        let end = start + entry.val_len as usize;

        // we perform a final boundary check to keep things safe
        if end > self.object.len() {
            return None;
        }

        // we return the slice directly from the mmap
        // this is the "zero-copy" path that makes the engine so fast
        Some(&self.object[start..end])
    }
}

// we tell rust that it is safe to send this across threads
// because our mmap is read-only and our pointers are stable
unsafe impl Send for Storage {}
unsafe impl Sync for Storage {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, IndexEntry};
    use std::fs;

    #[test]
    fn test_basic_retrieval() -> Result<(), Box<dyn std::error::Error>> {
        // we use a temporary path for the test database
        let path = "test_engine.db";

        // we build the file content in a vector
        let mut file_content = Vec::new();

        // we manually create a valid header
        let header = Header {
            magic: 0xA016,
            version: 1,
            count: 1,
            padding: 0,
        };

        // we create an index entry pointing to the data after the index
        let entry = IndexEntry {
            key: 42,
            val_offset: 56, // header (32) + 1 index entry (24)
            val_len: 5,
            _padding: 0,
        };

        // we push the raw bytes into our vector
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

        // we add the value "hello" at the end
        file_content.extend_from_slice(b"hello");
        fs::write(path, file_content)?;

        // we initialize our hardened engine
        let storage = Storage::new(path)?;

        // we verify that our get() works with the new pointer-based architecture
        let result = storage.get(42);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), b"hello");

        // we cleanup the file
        fs::remove_file(path)?;
        Ok(())
    }
}

/*
why this is better than before:
no self-referential errors: i swapped the &'static slice for a raw pointer + helper.

this means the compiler won't yell at us when we try to move Storage into an Arc for our server.

encapsulated unsafe: i pulled the unsafe code out of the get() function and hid it in a tiny helper.

now our core lookup logic is 100% safe rust code.

thread safety: i explicitly implemented Send and Sync,

which acts as a green light for tokio to distribute our engine across as many cpu cores as we want.

*/
