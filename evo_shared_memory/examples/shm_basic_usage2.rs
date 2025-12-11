//! Basic usage example for EVO Shared Memory
//!
//! Demonstrates simple producer-consumer pattern with error handling
//! and basic performance monitoring.

use evo::shm::consts::SHM_MIN_SIZE;
use evo_shared_memory::{SegmentReader, SegmentWriter, ShmError, ShmResult};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::{Duration, Instant};

/// Example sensor data structure
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SensorData {
    temperature: f32,
    pressure: f32,
    flow_rate: f32,
    timestamp: u64,
    status_flags: u32,
}

impl SensorData {
    fn new(temp: f32, pressure: f32, flow: f32) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        Self {
            temperature: temp,
            pressure,
            flow_rate: flow,
            timestamp,
            status_flags: 0x1, // Status OK
        }
    }

    fn is_valid(&self) -> bool {
        self.temperature > -273.15 && // Above absolute zero
        self.pressure >= 0.0 &&
        self.flow_rate >= 0.0 &&
        self.status_flags & 0x1 != 0 // Status OK flag
    }
}

fn serialize_data<T: Serialize>(data: &T) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(data)?)
}

fn deserialize_data<'a, T: Deserialize<'a>>(
    data: &'a [u8],
) -> Result<T, Box<dyn std::error::Error>> {
    // CHANGE: We use iterator that takes the first valid object and ignores the rest
    let mut stream = serde_json::Deserializer::from_slice(data).into_iter::<T>();
    match stream.next() {
        Some(result) => Ok(result?),
        None => Err("No JSON object found".into()),
    }
}

fn main() -> ShmResult<()> {
    // Initialize tracing for debugging
    evo_shared_memory::init_tracing();

    println!("EVO Shared Memory - Basic Usage Example");
    println!("======================================");

    // Start producer thread
    let producer_handle = thread::spawn(|| {
        producer_thread().unwrap_or_else(|e| {
            eprintln!("Producer error: {}", e);
        });
    });

    // Start consumer thread
    let consumer_handle = thread::spawn(|| {
        consumer_thread().unwrap_or_else(|e| {
            eprintln!("Consumer error: {}", e);
        });
    });

    // Let example run for 5 seconds
    thread::sleep(Duration::from_secs(5));

    // Wait for threads to complete
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();

    println!("Basic usage example completed successfully!");
    Ok(())
}

/// Producer thread that generates sensor data
fn producer_thread() -> ShmResult<()> {
    println!("Starting producer...");

    // Create a shared memory segment for sensor data
    let mut writer = SegmentWriter::create("sensor_readings", SHM_MIN_SIZE)?;

    let mut counter = 0;
    let start_time = Instant::now();

    loop {
        // Simulate sensor readings
        let temp = 20.0 + (counter as f32 * 0.1).sin() * 5.0; // 20°C ± 5°C
        let pressure = 1013.25 + (counter as f32 * 0.05).cos() * 50.0; // 1013.25 ± 50 hPa
        let flow = 100.0 + (counter as f32 * 0.02).sin() * 10.0; // 100 ± 10 L/min

        let sensor_data = SensorData::new(temp, pressure, flow);

        // Write sensor data to shared memory
        let data_bytes = serialize_data(&sensor_data).unwrap();
        match writer.write(&data_bytes) {
            Ok(()) => {
                if counter % 10 == 0 {
                    println!(
                        "Producer: Wrote data #{}: temp={:.1}°C",
                        counter, sensor_data.temperature
                    );
                }
            }
            Err(e) => {
                eprintln!("Producer: Write error: {}", e);
                break;
            }
        }

        counter += 1;

        // Stop after 5 seconds or 1000 writes
        if start_time.elapsed() > Duration::from_secs(5) || counter >= 1000 {
            break;
        }

        // 100 Hz update rate
        thread::sleep(Duration::from_millis(10));
    }

    println!("Producer: Completed {} writes", counter);
    Ok(())
}

/// Consumer thread that reads sensor data
fn consumer_thread() -> ShmResult<()> {
    println!("Starting consumer...");
    // Wait a bit for producer to create the segment
    thread::sleep(Duration::from_millis(100));

    let start_time = Instant::now();

    // Open the shared memory segment for reading with retry limit
    let mut reader = {
        let max_retries = 50; // maksymalna liczba prób
        let mut attempts = 0;

        loop {
            match SegmentReader::attach("sensor_readings") {
                Ok(r) => break r,
                Err(ShmError::NotFound { .. }) => {
                    attempts += 1;
                    if attempts >= max_retries {
                        return Err(ShmError::NotFound {
                            name: "sensor_readings".to_string(),
                        });
                    }
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    };

    let mut read_count = 0;
    let mut valid_readings = 0;

    loop {
        if reader.has_changed() {
            match reader.read() {
                Ok(bytes) => {
                    let sensor_data: SensorData = deserialize_data(bytes).unwrap();
                    let version = reader.version();

                    read_count += 1;

                    // Validate the data
                    if sensor_data.is_valid() {
                        valid_readings += 1;

                        if read_count % 100 == 0 {
                            println!(
                                "Consumer: Read #{} (version {}): temp={:.1}°C, pressure={:.1}hPa",
                                read_count, version, sensor_data.temperature, sensor_data.pressure
                            );
                        }
                    } else {
                        eprintln!("Consumer: Invalid sensor data received!");
                    }
                }
                Err(e) => {
                    eprintln!("Consumer: Read error: {}", e);
                    thread::sleep(Duration::from_millis(10));
                }
            }
        } else {
            // No new data, continue monitoring
            thread::sleep(Duration::from_millis(1));
        }

        // Check if we should stop (in a real app, we'd have a signal)
        // For this example, we'll just rely on the main thread killing us or running forever
        // But since we're in a loop, let's check if we've done enough work
        if start_time.elapsed() > Duration::from_secs(5) || read_count >= 1000 {
            break;
        }
    }

    println!(
        "Consumer: Completed {} reads ({} valid)",
        read_count, valid_readings
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_data_validation() {
        let valid_data = SensorData::new(25.0, 1000.0, 50.0);
        assert!(valid_data.is_valid());

        let invalid_temp = SensorData {
            temperature: -300.0, // Below absolute zero
            pressure: 1000.0,
            flow_rate: 50.0,
            timestamp: 0,
            status_flags: 0x1,
        };
        assert!(!invalid_temp.is_valid());

        let invalid_status = SensorData {
            temperature: 25.0,
            pressure: 1000.0,
            flow_rate: 50.0,
            timestamp: 0,
            status_flags: 0x0, // Status not OK
        };
        assert!(!invalid_status.is_valid());
    }

    #[test]
    fn test_basic_write_read() -> ShmResult<()> {
        let segment_name = "test_basic_rw";

        // Create writer and write data
        let mut writer = SegmentWriter::<SensorData>::create(segment_name, 10)?;
        let test_data = SensorData::new(22.5, 1013.25, 75.0);
        let version = writer.write(test_data.clone())?;

        // Create reader and read data
        let reader = SegmentReader::<SensorData>::open(segment_name)?;
        let (read_data, read_version) = reader.read()?;

        assert_eq!(read_data.temperature, test_data.temperature);
        assert_eq!(read_data.pressure, test_data.pressure);
        assert_eq!(read_data.flow_rate, test_data.flow_rate);
        assert_eq!(read_version, version);
        assert!(read_data.is_valid());

        Ok(())
    }
}
