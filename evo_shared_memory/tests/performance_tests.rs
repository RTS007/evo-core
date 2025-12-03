//! Performance and latency tests for EVO Shared Memory

use evo_shared_memory::{SHM_MIN_SIZE, SegmentReader, SegmentWriter, ShmResult};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn test_write_latency() -> ShmResult<()> {
    let segment_name = "test_write_latency";
    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    let test_data = vec![0xAA; 256];

    let iterations = 1000;
    let mut latencies = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        writer.write(&test_data)?;
        let latency = start.elapsed();
        latencies.push(latency.as_nanos() as u64);
    }

    // Calculate statistics
    latencies.sort_unstable();
    let min = latencies[0];
    let max = latencies[latencies.len() - 1];
    let avg = latencies.iter().sum::<u64>() / latencies.len() as u64;
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
    let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

    println!("Write Latency Stats (ns):");
    println!("  Min: {}", min);
    println!("  Max: {}", max);
    println!("  Avg: {}", avg);
    println!("  P95: {}", p95);
    println!("  P99: {}", p99);

    // Basic performance requirements - adjusted for test environment
    assert!(avg < 20_000, "Average write latency too high: {} ns", avg);
    assert!(p99 < 200_000, "P99 write latency too high: {} ns", p99);

    Ok(())
}

#[test]
fn test_read_latency() -> ShmResult<()> {
    let mut writer = SegmentWriter::create("perf_read_test", SHM_MIN_SIZE)?;
    let mut reader = SegmentReader::attach("perf_read_test")?;

    let test_data = b"Read latency test";
    writer.write(test_data)?;

    let start = Instant::now();
    for _ in 0..1000 {
        let data = reader.read()?;
        assert_eq!(&data[..test_data.len()], test_data);
    }
    let elapsed = start.elapsed();

    // Each read should be under 15 microseconds on average (realistic for test environment)
    let avg_latency = elapsed.as_nanos() / 1000;
    println!("Average read latency: {} ns", avg_latency);
    assert!(avg_latency < 15_000); // 15 μs

    Ok(())
}
#[test]
fn test_throughput() -> ShmResult<()> {
    let segment_name = "test_throughput";
    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    let test_data = vec![0xCC; 128];

    let operations = 10_000;
    let start = Instant::now();

    for _ in 0..operations {
        writer.write(&test_data)?;
    }

    let elapsed = start.elapsed();
    let throughput = operations as f64 / elapsed.as_secs_f64();

    println!("Throughput: {:.0} ops/sec", throughput);

    // Should achieve at least 100k ops/sec for small data
    assert!(
        throughput > 100_000.0,
        "Throughput too low: {:.0} ops/sec",
        throughput
    );

    Ok(())
}

#[test]
fn test_concurrent_access_performance() -> ShmResult<()> {
    let segment_name = "test_concurrent_performance";
    let thread_count = 4;
    let operations_per_thread = 1000;

    // Create writer and initial data
    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    writer.write(b"Initial data")?;

    let barrier = Arc::new(Barrier::new(thread_count + 1));
    let mut handles = Vec::new();

    // Spawn reader threads
    for _thread_id in 0..thread_count {
        let segment_name = segment_name.to_string();
        let barrier = barrier.clone();

        let handle = thread::spawn(move || -> Duration {
            let mut reader = SegmentReader::attach(&segment_name).unwrap();

            // Wait for all threads to be ready
            barrier.wait();

            let start = Instant::now();
            for _ in 0..operations_per_thread {
                let _data = reader.read().unwrap();
            }
            start.elapsed()
        });

        handles.push(handle);
    }

    // Start all threads
    barrier.wait();

    // Collect results
    let mut total_time = Duration::from_secs(0);
    for handle in handles {
        let thread_time = handle.join().unwrap();
        total_time = total_time.max(thread_time);
    }

    let total_operations = thread_count * operations_per_thread;
    let throughput = total_operations as f64 / total_time.as_secs_f64();

    println!("Concurrent read throughput: {:.0} ops/sec", throughput);

    // Should maintain good performance under concurrent load
    assert!(
        throughput > 50_000.0,
        "Concurrent throughput too low: {:.0} ops/sec",
        throughput
    );

    Ok(())
}

#[test]
fn test_memory_scaling() -> ShmResult<()> {
    let base_name = "test_memory_scaling";
    let sizes = vec![SHM_MIN_SIZE, 8192, 16384, 32768]; // Different segment sizes
    let data_size = 1024; // 1KB data

    for &size in &sizes {
        let segment_name = format!("{}_{}", base_name, size);
        let mut writer = SegmentWriter::create(&segment_name, size)?;
        let test_data = vec![0xDD; data_size];

        // Measure write performance for different segment sizes
        let iterations = 1000;
        let start = Instant::now();

        for _ in 0..iterations {
            writer.write(&test_data)?;
        }

        let elapsed = start.elapsed();
        let throughput = iterations as f64 / elapsed.as_secs_f64();

        println!("Segment size {}: {:.0} ops/sec", size, throughput);

        // Performance should not degrade significantly with larger segments
        assert!(
            throughput > 10_000.0,
            "Performance degraded for size {}: {:.0} ops/sec",
            size,
            throughput
        );
    }

    Ok(())
}

#[test]
fn test_data_size_scaling() -> ShmResult<()> {
    let segment_name = "test_data_scaling";
    let mut writer = SegmentWriter::create(segment_name, 32768)?; // 32KB segment

    let data_sizes = vec![64, 256, 1024, SHM_MIN_SIZE]; // Different data sizes

    for &size in &data_sizes {
        let test_data = vec![0xEE; size];
        let iterations = 1000;

        let start = Instant::now();
        for _ in 0..iterations {
            writer.write(&test_data)?;
        }
        let elapsed = start.elapsed();

        let throughput_ops = iterations as f64 / elapsed.as_secs_f64();
        let throughput_bytes = (iterations * size) as f64 / elapsed.as_secs_f64();

        println!(
            "Data size {}: {:.0} ops/sec, {:.0} bytes/sec",
            size, throughput_ops, throughput_bytes
        );

        // Should handle different data sizes efficiently
        assert!(
            throughput_ops > 1_000.0,
            "Low throughput for size {}: {:.0} ops/sec",
            size,
            throughput_ops
        );
    }

    Ok(())
}
