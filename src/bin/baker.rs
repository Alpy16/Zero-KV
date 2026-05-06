use kv_store::{HEADER_SIZE, Header, IndexEntry};
use std::fs::File;
use std::io::{self, Write};
use thiserror::Error; // Custom error derivation

// We define every way our "Baker" stage could fail
#[derive(Error, Debug)]
pub enum BakerError {
    #[error("I/O error occurred: {0}")]
    Io(#[from] io::Error), // Automatically wraps standard filesystem errors

    #[error("Storage file already exists: delete 'storage.db' before re-baking")]
    FileCollision,

    #[error("System memory error during binary serialization")]
    SerializationError,
}

// Changing main to return a Result allows us to use the '?' operator
fn main() -> Result<(), BakerError> {
    // we create our source data. It's a list of tuples containing:
    // a "Key" (ID) and a "Value" (the actual data converted into bytes)
    let mut raw_data = vec![
        (100, b"First value content".to_vec()),
        (50, b"Second".to_vec()),
        (200, b"Third and longest value here".to_vec()),
    ];
    // we sort our data by the key (ID) so that our index entries will be in order, which makes lookups faster
    raw_data.sort_by_key(|item| item.0);
    let my_header = Header {
        magic: 0xA016, // just a random number to identify our file type, you can choose any unique value
        version: 1,    // version number for our file format, useful for future updates
        count: raw_data.len() as u64, // 'raw_data' is our sorted Vec
        padding: 0,
    };

    // We pull these sizes directly from the types defined in lib.rs
    let header_size = HEADER_SIZE as u64;
    let index_entry_size = std::mem::size_of::<IndexEntry>() as u64;

    // we do our jump math:

    // this is how big our index section will be, it's the number of entries times the size of each entry
    let index_section_size = my_header.count * index_entry_size;

    // this is where our data blob will start, right after the header and index section
    let data_start_offset = header_size + index_section_size;

    // this will keep track of where we are in the data blob as we add values
    let mut current_offset = data_start_offset;

    // we will loop over our sorted 'raw_data' and create our index entries, calculating the offsets as we go

    // create an empty list to hold our index (coordinate sheet) entries
    let mut index_entries = Vec::new();
    for (key, value) in &raw_data {
        let val_len = value.len() as u32;
        index_entries.push(IndexEntry {
            key: *key as u64,           // convert key to u64 for our index entry
            val_offset: current_offset, // this is the absolute offset where the value will be stored in the file
            val_len,                    // length of the value in bytes
            _padding: 0, // just padding to make the struct 24 bytes, we won't use it for anything
        });
        current_offset += val_len as u64; // move the offset for the next entry
        if current_offset % 8 != 0 {
            // if the division by 8 leaves a remainder, we need to add padding to align to the next 8-byte boundary
            current_offset += 8 - (current_offset % 8); // align to 8 bytes 
        }
    }

    // create a new file called 'storage.db' to store our data, as a bonus this will fail if the file already exists, which is good for testing
    // We use .map_err to turn a generic IO error into our specific 'FileCollision' error
    let mut file = File::create_new("storage.db").map_err(|_| BakerError::FileCollision)?;

    let header_bytes = unsafe {
        // we take our header struct and convert it into a byte slice so we can write it to the file
        // literally says take my my_header struct as a simple row of bytes so I can send it to the file
        std::slice::from_raw_parts(
            (&my_header as *const Header) as *const u8,
            std::mem::size_of::<Header>(),
        )
    };

    // The '?' operator replaces .unwrap(). If write fails, it returns the error immediately.
    file.write_all(header_bytes)?; // physically push those 32 bytes onto the disk.

    let index_bytes = unsafe {
        // even though index_entries is a list (Vec), the entries are packed together in memory
        // we point to the start of that list and tell Rust to treat the whole thing as a giant row of bytes
        std::slice::from_raw_parts(
            index_entries.as_ptr() as *const u8, // get the memory address of the first entry
            index_entries.len() * std::mem::size_of::<IndexEntry>(), // calculate total bytes (entries * 24)
        )
    };

    file.write_all(index_bytes)?; // save our "Table of Contents" to the file right after the header

    // now we loop one last time to write the actual data blobs (the strings)
    for (_, value) in &raw_data {
        // 1. write the actual string bytes to the file
        file.write_all(value)?;

        // 2. we must write physical "zeros" to the file to fill the gaps we calculated earlier
        // if we skip this, our file will be shorter than our IndexEntry offsets expect!
        let remainder = value.len() % 8;
        if remainder != 0 {
            let padding_needed = 8 - remainder;
            let padding = vec![0u8; padding_needed]; // create a temporary "bridge" of zero-bytes
            file.write_all(&padding)?; // push the zeros to the disk so the next value starts on an 8-byte boundary
        }
    }

    println!("Successfully baked storage.db");
    Ok(())
}
