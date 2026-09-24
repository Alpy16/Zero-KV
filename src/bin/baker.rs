use anyhow::{Context, Result};
use kv_store::{DEFAULT_STORAGE_PATH, HEADER_SIZE, Header, IndexEntry};
use std::env;
use std::fs;
use std::fs::File;
/// We use `BufWriter` to batch our disk I/O operations, significantly reducing
/// the number of system calls required to flush the baked database to storage.
/// We utilize `zerocopy::AsBytes` to safely serialize our index directly from memory.
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

    // We perform an in-place sort by key to ensure the storage engine can
    // utilize O(log n) binary search lookups against the memory-mapped file.
    raw_data.sort_by_key(|item| item.0);

    let mut my_header = Header {
        magic: 0xA016,
        version: 1,
        count: raw_data.len() as u64,
        header_checksum: 0,
        _padding: 0,
    };

    // We calculate the header checksum using the "Zero-Field" pattern.
    // The `header_checksum` is calculated while its own field is treated as zero.
    my_header.header_checksum = crc32fast::hash(my_header.as_bytes());

    let header_size = HEADER_SIZE as u64;
    let index_entry_size = std::mem::size_of::<IndexEntry>() as u64;
    let index_section_size = my_header.count * index_entry_size;
    let data_start_offset = header_size + index_section_size;

    let mut current_offset = data_start_offset;
    let mut index_entries = Vec::new();

    for (key, value) in &raw_data {
        let val_len = value.len() as u32;
        // We calculate the CRC32C hash for the value payload to protect against bit-rot.
        index_entries.push(IndexEntry {
            key: *key,
            val_offset: current_offset,
            val_len: value.len() as u32,
            val_checksum: crc32fast::hash(value),
        });

        // We enforce an 8-byte boundary alignment for all data values.
        // This ensures that the CPU can retrieve data in a single memory fetch
        // without crossing cache line boundaries.
        // Aligning offsets during the bake phase is a one-time cost that
        // eliminates the need for expensive unaligned-load handling
        // or data-copying during the server's runtime.
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

        let remainder = value.len() % 8;
        if remainder != 0 {
            let padding_needed = 8 - remainder;
            let padding = vec![0u8; padding_needed];
            writer
                .write_all(&padding)
                .context("failed to write alignment padding")?;
        }
    }

    // We flush the buffer to ensure the persistence of the atomic database file.
    writer.flush().context("failed to flush data to disk")?;

    println!(
        "successfully baked storage.db with {} entries",
        raw_data.len()
    );
    Ok(())
}
