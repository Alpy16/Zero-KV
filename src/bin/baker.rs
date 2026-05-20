use anyhow::{Context, Result};
use kv_store::{DEFAULT_STORAGE_PATH, HEADER_SIZE, Header, IndexEntry};
use std::fs::File;
// we bring in `BufWriter` for buffered file I/O, which is more efficient for writing large amounts of data.
use std::env;
use std::fs;
use std::io::{BufWriter, Write};
use zerocopy::AsBytes; // we use this to turn structs into bytes safely

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        anyhow::bail!("Usage: baker <input_file>\nFormat: key,value (one per line)");
    }

    let input_path = &args[1];
    let content = fs::read_to_string(input_path)
        .with_context(|| format!("failed to read input file: {}", input_path))?;

    let mut raw_data = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((k_str, v_str)) = line.split_once(',') {
            let key = k_str
                .trim()
                .parse::<u64>()
                .with_context(|| format!("invalid key on line {}: {}", idx + 1, k_str))?;
            raw_data.push((key, v_str.trim().as_bytes().to_vec()));
        } else {
            eprintln!("Warning: Skipping malformed line {}: {}", idx + 1, line);
        }
    }

    if raw_data.is_empty() {
        anyhow::bail!("No valid entries found in input file.");
    }

    // sorting is non-negotiable; binary search needs order.
    raw_data.sort_by_key(|item| item.0);

    let my_header = Header {
        magic: 0xA016,
        version: 1,
        count: raw_data.len() as u64,
        padding: 0,
    };

    let header_size = HEADER_SIZE as u64;
    let index_entry_size = std::mem::size_of::<IndexEntry>() as u64;
    let index_section_size = my_header.count * index_entry_size;
    let data_start_offset = header_size + index_section_size;

    // `current_offset` tracks where the next value will be written in the file.
    let mut current_offset = data_start_offset;
    // `index_entries` will store all the `IndexEntry` structs that form our index.
    let mut index_entries = Vec::new();

    // we calculate the positions of all our values
    for (key, value) in &raw_data {
        let val_len = value.len() as u32;
        index_entries.push(IndexEntry {
            key: *key,
            val_offset: current_offset,
            val_len,
            _padding: 0,
        });

        // this is where mechanical sympathy comes in—i'm forcing every value
        // to start on an 8-byte boundary. it ensures the cpu can fetch data
        // without splitting a read across cache lines.
        current_offset += val_len as u64;
        if !current_offset.is_multiple_of(8) {
            current_offset += 8 - (current_offset % 8);
        }
    }

    let file = File::create_new(DEFAULT_STORAGE_PATH)
        .context("storage file already exists. delete it before re-baking")?;
    let mut writer = BufWriter::new(file);

    writer
        .write_all(my_header.as_bytes())
        .context("failed to write header")?;

    writer
        .write_all(index_entries.as_bytes())
        .context("failed to write index entries")?;

    for (_, value) in &raw_data {
        writer
            .write_all(value)
            .context("failed to write data value")?;

        // padding the actual file to maintain our 8-byte alignment.
        let remainder = value.len() % 8;
        if remainder != 0 {
            let padding_needed = 8 - remainder;
            let padding = vec![0u8; padding_needed];
            writer
                .write_all(&padding)
                .context("failed to write alignment padding")?;
        }
    }

    // we flush the `BufWriter` to ensure all buffered data is physically moved
    // from memory to the disk platter, making the database file persistent.
    writer.flush().context("failed to flush data to disk")?;

    println!(
        "successfully baked storage.db with {} entries",
        raw_data.len()
    );
    Ok(())
}
