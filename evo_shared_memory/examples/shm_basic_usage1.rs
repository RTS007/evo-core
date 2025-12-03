//! Basic usage example for EVO Shared Memory
//! Demonstrates simple producer-consumer pattern

use evo_shared_memory::{SHM_MIN_SIZE, SegmentReader, SegmentWriter, ShmResult};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SensorData {
    temperature: f32,
    humidity: f32,
    timestamp: u64,
}

impl SensorData {
    fn new(temp: f32, hum: f32) -> Self {
        Self {
            temperature: temp,
            humidity: hum,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }
}

fn serialize_data<T: Serialize>(data: &T) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(data)?)
}

fn deserialize_data<T: for<'a> Deserialize<'a>>(
    bytes: &[u8],
) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(bytes)?)
}

fn producer() -> ShmResult<()> {
    println!("Producer: Starting...");

    let mut writer = SegmentWriter::create("sensor_data", SHM_MIN_SIZE)?;

    for i in 0..100 {
        let sensor = SensorData::new(20.0 + i as f32 * 0.1, 50.0 + i as f32 * 0.2);
        let data = serialize_data(&sensor).unwrap();

        writer.write(&data)?;

        if i % 10 == 0 {
            println!("Producer: Wrote #{}: temp={:.1}°C", i, sensor.temperature);
        }

        thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}

fn consumer() -> ShmResult<()> {
    println!("Consumer: Starting...");

    // Wait a bit for producer to start
    thread::sleep(Duration::from_millis(500));

    let mut reader = SegmentReader::attach("sensor_data")?;

    for i in 0..50 {
        match reader.read() {
            Ok(data) => {
                if let Ok(sensor) = deserialize_data::<SensorData>(&data) {
                    if i % 10 == 0 {
                        println!("Consumer: Read #{}: temp={:.1}°C", i, sensor.temperature);
                    }
                }
            }
            Err(e) => {
                println!("Consumer: Read error: {}", e);
            }
        }

        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
}

fn main() -> ShmResult<()> {
    println!("EVO Shared Memory - Basic Usage Example");
    println!("======================================");

    let producer_handle = thread::spawn(|| {
        producer().unwrap_or_else(|e| {
            eprintln!("Producer error: {}", e);
        });
    });

    let consumer_handle = thread::spawn(|| {
        consumer().unwrap_or_else(|e| {
            eprintln!("Consumer error: {}", e);
        });
    });

    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();

    println!("Basic usage example completed!");
    Ok(())
}
