use evo_shared_memory::{SegmentDiscovery, ShmResult};
use std::collections::HashSet;
use std::{thread, time::Duration};

fn main() -> ShmResult<()> {
    println!("EVO System Radar - Waiting for new devices...");

    // Set of known devices (names)
    let mut known_devices = HashSet::new();

    // Create discovery instance
    let discovery = SegmentDiscovery::new();

    // Initial scanning
    // Use list_segments() method
    for info in discovery.list_segments()? {
        known_devices.insert(info.name);
    }
    println!("Initial state: {} devices connected.", known_devices.len());

    // Monitoring loop
    loop {
        // Get current segment list
        let current_segments = discovery.list_segments()?;

        // Convert list of SegmentInfo structs to set of names (String)
        let current_set: HashSet<String> =
            current_segments.into_iter().map(|info| info.name).collect();

        // Check for new devices
        for device in current_set.difference(&known_devices) {
            println!(">>> NEW DEVICE DETECTED: [{}]", device);

            // We can get details of the new device
            if let Ok(Some(info)) = discovery.find_segment(device) {
                println!(
                    "    Details: Size={} bytes, PID={}",
                    info.size, info.writer_pid
                );
            }
        }

        // Check for disconnected devices
        for device in known_devices.difference(&current_set) {
            println!("<<< DEVICE LOST: [{}]", device);
        }

        known_devices = current_set;
        thread::sleep(Duration::from_millis(500));
    }
}
