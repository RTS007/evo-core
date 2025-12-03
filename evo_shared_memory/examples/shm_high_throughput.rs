//! High-throughput data streaming example
//!
//! Demonstrates high-frequency data broadcasting with multiple consumers,
//! NUMA optimizations, and performance monitoring for data-intensive applications.

use evo_shared_memory::{SegmentReader, SegmentWriter, ShmError, ShmResult};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

/// High-frequency market data structure
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct MarketData {
    symbol_id: u32,
    price: f64,
    volume: u64,
    bid: f64,
    ask: f64,
    timestamp: u64,
    sequence: u64,
}

/// Performance metrics for throughput analysis
#[derive(Debug)]
struct ThroughputMetrics {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    start_time: Instant,
}

impl ThroughputMetrics {
    fn new() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    fn record_sent(&self, bytes: u64) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    fn record_received(&self, bytes: u64) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    fn print_statistics(&self) {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let sent = self.messages_sent.load(Ordering::Relaxed);
        let received = self.messages_received.load(Ordering::Relaxed);
        let bytes_sent = self.bytes_sent.load(Ordering::Relaxed);
        let bytes_received = self.bytes_received.load(Ordering::Relaxed);

        println!("Throughput Statistics ({}s):", elapsed as u32);
        println!("  Messages sent: {} ({:.0}/s)", sent, sent as f64 / elapsed);
        println!(
            "  Messages received: {} ({:.0}/s)",
            received,
            received as f64 / elapsed
        );
        println!(
            "  Bandwidth sent: {:.2} MB/s",
            (bytes_sent as f64) / elapsed / 1_000_000.0
        );
        println!(
            "  Bandwidth received: {:.2} MB/s",
            (bytes_received as f64) / elapsed / 1_000_000.0
        );
    }
}

fn main() -> ShmResult<()> {
    println!("EVO Shared Memory - High-Throughput Streaming Example");
    println!("====================================================");

    let metrics = Arc::new(ThroughputMetrics::new());
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Start data producer
    let producer_metrics = metrics.clone();
    let producer_stop = stop_flag.clone();
    let producer_handle = thread::spawn(move || {
        high_frequency_producer(producer_metrics, producer_stop).unwrap_or_else(|e| {
            eprintln!("Producer error: {}", e);
        });
    });

    // Start multiple consumers
    let num_consumers = 4;
    let mut consumer_handles = Vec::new();

    for consumer_id in 0..num_consumers {
        let consumer_metrics = metrics.clone();
        let consumer_stop = stop_flag.clone();

        let handle = thread::spawn(move || {
            high_speed_consumer(consumer_id, consumer_metrics, consumer_stop).unwrap_or_else(|e| {
                eprintln!("Consumer {} error: {}", consumer_id, e);
            });
        });

        consumer_handles.push(handle);
    }

    // Run test for 10 seconds
    thread::sleep(Duration::from_secs(10));

    // Signal stop
    stop_flag.store(true, Ordering::Relaxed);

    // Wait for threads
    producer_handle.join().unwrap();
    for handle in consumer_handles {
        handle.join().unwrap();
    }

    // Print final statistics
    metrics.print_statistics();

    println!("High-throughput streaming example completed!");
    Ok(())
}

/// High-frequency data producer (simulating market data feed)
fn high_frequency_producer(
    metrics: Arc<ThroughputMetrics>,
    stop_flag: Arc<AtomicBool>,
) -> ShmResult<()> {
    println!("Starting high-frequency producer...");

    // Create segment optimized for high throughput
    // Note: Options are simplified for this example as the current API supports basic creation
    let mut writer = SegmentWriter::create(
        "market_feed",
        12288, // Size in bytes, adjusted for serialized data
    )?;

    let mut sequence = 0u64;
    let symbols = ["AAPL", "GOOGL", "MSFT", "TSLA", "AMZN"];
    let mut prices = [150.0, 2800.0, 300.0, 900.0, 3200.0];

    // Target: 100kHz update rate
    let target_interval = Duration::from_nanos(10_000); // 10μs = 100kHz

    while !stop_flag.load(Ordering::Relaxed) {
        let cycle_start = Instant::now();

        for (i, _) in symbols.iter().enumerate() {
            // Simulate price movement
            let price_change = (sequence as f64 * 0.001).sin() * 0.1;
            prices[i] += price_change;

            let market_data = MarketData {
                symbol_id: i as u32,
                price: prices[i],
                volume: 1000 + (sequence % 5000),
                bid: prices[i] - 0.01,
                ask: prices[i] + 0.01,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
                sequence,
            };

            let data_bytes = serde_json::to_vec(&market_data).map_err(|e| ShmError::Io {
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            })?;

            match writer.write(&data_bytes) {
                Ok(_) => {
                    metrics.record_sent(data_bytes.len() as u64);
                    sequence += 1;
                }
                Err(e) => {
                    eprintln!("Producer: Write failed: {}", e);
                }
            }
        }

        // Maintain target frequency
        let cycle_time = cycle_start.elapsed();
        if cycle_time < target_interval {
            thread::sleep(target_interval - cycle_time);
        }
    }

    println!("High-frequency producer completed {} updates", sequence);
    Ok(())
}

/// High-speed consumer with latency tracking
fn high_speed_consumer(
    consumer_id: usize,
    metrics: Arc<ThroughputMetrics>,
    stop_flag: Arc<AtomicBool>,
) -> ShmResult<()> {
    println!("Starting consumer {}...", consumer_id);

    // Wait for producer to create segment
    thread::sleep(Duration::from_millis(100));

    let mut reader = SegmentReader::attach("market_feed")?;
    let mut last_sequence = 0u64;
    let mut gap_count = 0u64;
    let mut latencies = Vec::new();

    while !stop_flag.load(Ordering::Relaxed) {
        if reader.has_changed() {
            match reader.read() {
                Ok(data_bytes) => {
                    // OLD CODE (causes error):
                    /*
                    let market_data: MarketData = serde_json::from_slice(data_bytes)
                        .map_err(|e| ShmError::Io { source: std::io::Error::new(std::io::ErrorKind::InvalidData, e) })?;
                    */

                    // NEW CODE (ignores garbage):
                    let mut stream =
                        serde_json::Deserializer::from_slice(data_bytes).into_iter::<MarketData>();
                    let market_data = match stream.next() {
                        Some(Ok(data)) => data,
                        Some(Err(_)) => {
                            continue;
                        }
                        None => continue,
                    };

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64;

                    // Calculate latency
                    if market_data.timestamp > 0 {
                        let latency_ns = now.saturating_sub(market_data.timestamp);
                        latencies.push(latency_ns);

                        // Keep only recent latencies for analysis
                        if latencies.len() > 10000 {
                            latencies.drain(..5000);
                        }
                    }

                    // Check for sequence gaps
                    if market_data.sequence > last_sequence + 1 {
                        gap_count += market_data.sequence - last_sequence - 1;
                    }
                    last_sequence = market_data.sequence;

                    metrics.record_received(std::mem::size_of::<MarketData>() as u64);

                    // Log every 10,000 messages
                    if market_data.sequence % 10000 == 0 && consumer_id == 0 {
                        if let Some(&latest_latency) = latencies.last() {
                            println!(
                                "Consumer {}: Seq={}, Price={:.2}, Latency={}μs",
                                consumer_id,
                                market_data.sequence,
                                market_data.price,
                                latest_latency / 1000
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Consumer {}: Read error: {}", consumer_id, e);
                    thread::sleep(Duration::from_micros(10));
                }
            }
        } else {
            // No new data, continue polling
        }

        // High-frequency polling
        thread::yield_now();
    }

    // Calculate and print latency statistics
    if !latencies.is_empty() {
        latencies.sort_unstable();
        let len = latencies.len();
        let p50 = latencies[len / 2];
        let p95 = latencies[len * 95 / 100];
        let p99 = latencies[len * 99 / 100];
        let max = latencies[len - 1];

        println!("Consumer {} Latency Statistics:", consumer_id);
        println!("  Messages: {}", len);
        println!("  P50: {}μs", p50 / 1000);
        println!("  P95: {}μs", p95 / 1000);
        println!("  P99: {}μs", p99 / 1000);
        println!("  Max: {}μs", max / 1000);
        println!("  Sequence gaps: {}", gap_count);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_throughput_metrics() {
        let metrics = ThroughputMetrics::new();

        metrics.record_sent(100);
        metrics.record_received(50);

        assert_eq!(metrics.messages_sent.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.messages_received.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.bytes_sent.load(Ordering::Relaxed), 100);
        assert_eq!(metrics.bytes_received.load(Ordering::Relaxed), 50);
    }

    #[test]
    fn test_market_data_creation() {
        let data = MarketData {
            symbol_id: 0,
            price: 150.0,
            volume: 1000,
            bid: 149.99,
            ask: 150.01,
            timestamp: 1234567890,
            sequence: 42,
        };

        assert_eq!(data.symbol_id, 0);
        assert_eq!(data.price, 150.0);
        assert_eq!(data.sequence, 42);
    }

    #[test]
    fn test_high_frequency_write_read() -> ShmResult<()> {
        let segment_name = "test_hf_stream";

        // Create writer
        let mut writer = SegmentWriter::create(segment_name, 1000)?;

        // Create reader
        let mut reader = SegmentReader::attach(segment_name)?;

        // Test rapid write-read cycle
        for i in 0..100 {
            let data = MarketData {
                symbol_id: i % 5,
                price: 100.0 + i as f64,
                volume: 1000,
                bid: 99.99 + i as f64,
                ask: 100.01 + i as f64,
                timestamp: i as u64,
                sequence: i as u64,
            };

            let data_bytes = serde_json::to_vec(&data).map_err(|e| ShmError::Io {
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            })?;

            writer.write(&data_bytes)?;

            // In a real scenario, we might need to wait or retry, but here we assume it's fast enough
            // or we just check if we can read something.
            // Since writer and reader are in the same thread (which is not typical for SHM but possible for test),
            // we can read immediately.

            let read_bytes = reader.read()?;
            let read_data: MarketData =
                serde_json::from_slice(read_bytes).map_err(|e| ShmError::Io {
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                })?;

            // Latest write should be readable
            assert!(read_data.sequence >= data.sequence);
        }

        Ok(())
    }
}
