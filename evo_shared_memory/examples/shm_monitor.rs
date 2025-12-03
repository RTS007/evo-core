use evo_shared_memory::{SegmentDiscovery, ShmResult};
use std::collections::HashSet;
use std::{thread, time::Duration};

fn main() -> ShmResult<()> {
    println!("EVO System Radar - Waiting for new devices...");

    // Zbiór znanych urządzeń (nazw)
    let mut known_devices = HashSet::new();

    // Tworzymy instancję discovery
    let discovery = SegmentDiscovery::new();

    // Wstępne skanowanie
    // Używamy metody list_segments()
    for info in discovery.list_segments()? {
        known_devices.insert(info.name);
    }
    println!("Initial state: {} devices connected.", known_devices.len());

    // Pętla monitorująca
    loop {
        // Pobieramy aktualną listę segmentów
        let current_segments = discovery.list_segments()?;

        // Zamieniamy listę struktur SegmentInfo na zbiór nazw (String)
        let current_set: HashSet<String> =
            current_segments.into_iter().map(|info| info.name).collect();

        // Sprawdź nowe
        for device in current_set.difference(&known_devices) {
            println!(">>> NEW DEVICE DETECTED: [{}]", device);

            // Możemy pobrać szczegóły nowego urządzenia
            if let Ok(Some(info)) = discovery.find_segment(device) {
                println!(
                    "    Details: Size={} bytes, PID={}",
                    info.size, info.writer_pid
                );
            }
        }

        // Sprawdź odłączone
        for device in known_devices.difference(&current_set) {
            println!("<<< DEVICE LOST: [{}]", device);
        }

        known_devices = current_set;
        thread::sleep(Duration::from_millis(500));
    }
}
