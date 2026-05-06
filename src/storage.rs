// 1 - The Mapping Tool
use memmap2::Mmap;
use memmap2::MmapOptions;

use crate::EngineError;
// 2 - Our Blueprints (from my own lib.rs)
use crate::{HEADER_SIZE, Header, INDEX_ENTRY_SIZE, IndexEntry};

use std::env::consts;
// 3 - Standard Library Tools
use std::fs::File;
use std::slice; // Used for the "unsafe" casting of bytes to structs

struct Storage {
    object: Mmap, // this is our memory-mapped file, it will let us treat the file like a big array of bytes
    header: Header, // we will read the header once and keep it in memory for easy access
    index: *const IndexEntry, // this will point to the start of our index section in the memory-mapped file
}

impl Storage {
    fn new(file_path: &str) -> Result<Self, EngineError> {
        // open the file handle
        let file = File::open(file_path)?;

        // map the file into memory
        let mmap = unsafe { MmapOptions::new().map(&file)? };

        // ensure the file isn't too small to even contain our 32-byte header
        if mmap.len() < HEADER_SIZE {
            return Err(EngineError::InvalidFormat);
        }

        // overlay our 'Header' blueprint onto the first 32 bytes
        let header_ptr = mmap.as_ptr() as *const Header;

        // copy the header data into a local variable so we can use it safely
        let header = unsafe { *header_ptr };

        // security Check: Make sure this is actually OUR database file format
        if header.magic != 0xA016 {
            return Err(EngineError::InvalidFormat);
        }

        // calculate the "Starting Line" of the Index (Header address + 32 bytes)
        // we do math as u8 (1 byte) then cast to IndexEntry (24 bytes)
        let index_ptr = unsafe { mmap.as_ptr().add(HEADER_SIZE) as *const IndexEntry };

        // return the completed engine
        Ok(Self {
            object: mmap,
            header,
            index: index_ptr,
        })
    }

    fn get(&self, key: u64) -> Option<&[u8]> {
        // we use a raw pointer and the count from the header to create a slice
        // this tells rust to treat that memory address as an array of index entries
        // it is unsafe because rust cannot guarantee the file still exists on disk
        let index_slice =
            unsafe { std::slice::from_raw_parts(self.index, self.header.count as usize) };

        // we perform a binary search which is o log n efficiency
        // we use a closure to tell rust to compare the key against the key field in each entry
        // ok converts the result to an option so the question mark can return none if not found
        let pos = index_slice.binary_search_by_key(&key, |e| e.key).ok()?;

        // we grab a reference to the specific index entry found at that position
        // this entry contains the location and size of our actual data
        let entry = &index_slice[pos];

        // we cast the u64 values from our file format into usize for memory indexing
        // start is the exact byte where our value begins in the memory map
        let start = entry.val_offset as usize;

        // end is calculated by adding the length to the start offset
        let end = start + entry.val_len as usize;

        // we perform a final boundary check to prevent an out of bounds panic
        // this ensures a corrupted index cannot force the program to read invalid memory
        if end > self.object.len() {
            return None;
        }

        // we return a slice of the mmap object which points directly to the data
        // this is zero-copy because the data stays in the os page cache and is not moved
        Some(&self.object[start..end])
    }
}
// safety: the mmap is read-only and the pointer address never changes
unsafe impl Send for Storage {}
unsafe impl Sync for Storage {}

#[cfg(test)]
#[cfg(test)]
#[cfg(test)] // tells the compiler to only run this code when we execute cargo test
mod tests {
    // we reach into the parent module to grab the storage engine
    use crate::storage::Storage;
    // we reach into the crate root for our binary layout specifications
    use crate::{Header, IndexEntry};
    // used for creating and deleting the temporary test database on disk
    use std::fs;

    #[test] // marks this function as a test case for the rust test runner
    fn test_basic_retrieval() -> Result<(), Box<dyn std::error::Error>> {
        // the filename for our temporary testing database
        let path = "test_engine.db";

        // --- step 1: bake the test data ---
        // we use a byte vector to build our file content in memory before writing to disk
        let mut file_content = Vec::new();

        // we manually construct a header that matches our engine requirements
        let header = Header {
            magic: 0xA016,
            version: 1,
            count: 1,
            padding: 0,
        };

        // we construct an index entry pointing to a value 56 bytes into the file
        // 56 is the sum of the 32 byte header and the 24 byte index entry
        let entry = IndexEntry {
            key: 42,
            val_offset: 56,
            val_len: 5,
            _padding: 0,
        };

        // we use unsafe blocks to interpret our structs as raw byte slices
        // this allows us to push the exact memory layout of the structs into our file buffer
        unsafe {
            // we calculate the size of the header and copy its raw bytes into the vector
            let h_ptr = &header as *const Header as *const u8;
            file_content.extend_from_slice(std::slice::from_raw_parts(
                h_ptr,
                std::mem::size_of::<Header>(),
            ));

            // we do the same for the index entry following the header
            let e_ptr = &entry as *const IndexEntry as *const u8;
            file_content.extend_from_slice(std::slice::from_raw_parts(
                e_ptr,
                std::mem::size_of::<IndexEntry>(),
            ));
        }

        // we add the string hello as raw bytes at the end of the file
        file_content.extend_from_slice(b"hello");

        // we write the completed byte vector to the local filesystem
        fs::write(path, file_content)?;

        // --- step 2: test the engine ---
        // we initialize your storage engine with the file we just created
        // the question mark will fail the test if the file cannot be opened or mapped
        let storage = Storage::new(path)?;

        // we attempt to retrieve the data for key 42
        let result = storage.get(42);

        // assert checks if the condition is true and stops the test if it is false
        // we verify that the key exists in our index
        assert!(result.is_some(), "key 42 should exist");

        // assert_eq compares two values and prints them if they do not match
        // we verify the data returned from memory matches the string we wrote to disk
        assert_eq!(result.unwrap(), b"hello");

        // we verify that the engine correctly handles a missing key
        assert!(storage.get(99).is_none(), "key 99 should not exist");

        // --- step 3: cleanup ---
        // we remove the test file so we do not leave artifacts in the project directory
        fs::remove_file(path)?;

        // returning ok signals to the test runner that everything passed successfully
        Ok(())
    }
}
