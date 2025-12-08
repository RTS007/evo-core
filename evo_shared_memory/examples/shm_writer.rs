//! Simple writer example demonstrating segment creation and writing

use evo::shm::consts::SHM_MIN_SIZE;
use evo_shared_memory::{SegmentWriter, ShmResult};
use std::io;

fn main() -> ShmResult<()> {
    println!("EVO Shared Memory Writer Example");
    println!("================================");

    // Create a shared memory segment
    let segment_name = "example_segment";

    println!(
        "Creating segment '{}' with size {} bytes...",
        segment_name, SHM_MIN_SIZE
    );

    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;

    println!("✓ Segment created successfully!");
    println!("  Writer PID: {}", writer.writer_pid());
    println!("  Data size: {} bytes", writer.data_size());
    println!("  Initial version: {}", writer.current_version());

    // Write some data
    let data = b"Hello, EVO Shared Memory!";
    println!("\nWriting data: {:?}", std::str::from_utf8(data).unwrap());

    writer.write(data)?;

    println!("✓ Data written successfully!");
    println!("  New version: {}", writer.current_version());

    // Write more data at a specific offset
    let more_data = b" This is additional data.";
    let offset = data.len();

    println!("\nWriting additional data at offset {}...", offset);
    writer.write_at(offset, more_data)?;

    println!("✓ Additional data written successfully!");
    println!("  Final version: {}", writer.current_version());

    // Flush to ensure all writes are committed
    writer.flush()?;

    println!("\nPress Enter to exit (this will clean up the segment)...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();

    println!("Cleaning up segment...");

    Ok(())
}
