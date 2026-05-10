use anyhow::{Context, Result};
use kv_store::{HEADER_SIZE, Header, IndexEntry};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use zerocopy::AsBytes; // we use this to turn structs into bytes safely

// we use anyhow now, so we can remove the manual BakerError enum and
// let anyhow handle the context of what went wrong

fn main() -> Result<()> {
    // we create our source data just like before
    let mut raw_data = vec![
        (100, b"First value content".to_vec()),
        (50, b"Second".to_vec()),
        (200, b"Third and longest value here".to_vec()),
    ];

    // we sort by key to ensure our binary search works later
    raw_data.sort_by_key(|item| item.0);

    let my_header = Header {
        magic: 0xA016,
        version: 1,
        count: raw_data.len() as u64,
        padding: 0,
    };

    // we do our jump math to find where the data starts
    let header_size = HEADER_SIZE as u64;
    let index_entry_size = std::mem::size_of::<IndexEntry>() as u64;
    let index_section_size = my_header.count * index_entry_size;
    let data_start_offset = header_size + index_section_size;

    let mut current_offset = data_start_offset;
    let mut index_entries = Vec::new();

    // we calculate the positions of all our values
    for (key, value) in &raw_data {
        let val_len = value.len() as u32;
        index_entries.push(IndexEntry {
            key: *key as u64,
            val_offset: current_offset,
            val_len,
            _padding: 0,
        });

        // we move the offset and align it to 8 bytes for cpu efficiency
        current_offset += val_len as u64;
        if current_offset % 8 != 0 {
            current_offset += 8 - (current_offset % 8);
        }
    }

    // we create the file and wrap it in a BufWriter to make disk access faster
    let file = File::create_new("storage.db")
        .context("storage.db already exists. delete it before re-baking")?;
    let mut writer = BufWriter::new(file);

    // why this is better: zero-copy "as_bytes()" replaces the unsafe pointer mess
    // it guarantees we don't accidentally read past the struct's memory
    writer
        .write_all(my_header.as_bytes())
        .context("failed to write header")?;

    // we write all index entries at once. zerocopy handles the slice conversion for us
    writer
        .write_all(index_entries.as_bytes())
        .context("failed to write index entries")?;

    // we write the data blobs and their padding
    for (_, value) in &raw_data {
        writer
            .write_all(value)
            .context("failed to write data value")?;

        let remainder = value.len() % 8;
        if remainder != 0 {
            let padding_needed = 8 - remainder;
            let padding = vec![0u8; padding_needed];
            writer
                .write_all(&padding)
                .context("failed to write alignment padding")?;
        }
    }

    // we flush to ensure everything is physically moved from memory to the disk platter
    writer.flush().context("failed to flush data to disk")?;

    println!(
        "successfully baked storage.db with {} entries",
        raw_data.len()
    );
    Ok(())
}
