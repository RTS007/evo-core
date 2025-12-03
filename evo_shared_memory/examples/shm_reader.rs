//! Simple reader example demonstrating multi-reader access

use evo_shared_memory::{SegmentReader, ShmResult};
use std::io;
use std::time::Duration;

fn main() -> ShmResult<()> {
    println!("EVO Shared Memory Reader Example");
    println!("=================================");

    let segment_name = "example_segment";

    println!("Attempting to attach to segment '{}'...", segment_name);

    let mut reader = match SegmentReader::attach(segment_name) {
        Ok(r) => r,
        Err(e) => {
            println!("❌ Failed to attach to segment: {}", e);
            println!("\nMake sure to run the writer example first:");
            println!("  cargo run --example simple_writer");
            return Err(e);
        }
    };

    println!("✓ Successfully attached to segment!");
    println!("  Reader PID: {}", reader.reader_pid());
    println!("  Data size: {} bytes", reader.data_size());
    println!("  Current version: {}", reader.version());
    println!("  Reader count: {}", reader.reader_count());

    println!("\nReading initial data...");

    let data = reader.read()?;
    let text = String::from_utf8_lossy(&data);
    println!("Data: {:?}", text.trim_end_matches('\0'));

    println!("\nMonitoring for changes (press Enter to exit)...");

    // Monitor for changes in a separate thread
    let segment_name = segment_name.to_string();
    std::thread::spawn(move || {
        let mut monitor_reader = SegmentReader::attach(&segment_name).unwrap();

        loop {
            std::thread::sleep(Duration::from_millis(100));

            if monitor_reader.has_changed() {
                println!("\n📢 Data changed!");
                match monitor_reader.read() {
                    Ok(data) => {
                        let text = String::from_utf8_lossy(&data).to_string();
                        println!("New version: {}", monitor_reader.version());
                        println!("New data: {:?}", text.trim_end_matches('\0'));
                    }
                    Err(e) => println!("Error reading data: {}", e),
                }
            }
        }
    });

    // Wait for user input
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();

    println!("\nFinal reader count: {}", reader.reader_count());
    println!("Reader exiting...");

    Ok(())
}
