//! Concurrent access performance benchmarks

use criterion::{Criterion, criterion_group, criterion_main};
use evo_shared_memory::reader::SegmentReader;
use evo_shared_memory::writer::SegmentWriter;
use std::hint::black_box;
use std::sync::{Arc, Barrier};
use std::thread;

/// Benchmark multiple concurrent readers
fn bench_concurrent_readers(c: &mut Criterion) {
    let segment_name = "bench_concurrent";
    let mut writer = SegmentWriter::create(segment_name, 65536).unwrap();
    let data = vec![0xAAu8; 1024];
    writer.write(&data).unwrap();

    c.bench_function("concurrent_10_readers", |b| {
        b.iter(|| {
            let barrier = Arc::new(Barrier::new(11)); // 10 readers + 1 main thread
            let mut handles = Vec::new();

            // Spawn 10 concurrent readers
            for _ in 0..10 {
                let barrier_clone = barrier.clone();
                let handle = thread::spawn(move || {
                    let mut reader = SegmentReader::attach(segment_name).unwrap();
                    barrier_clone.wait(); // Synchronize start

                    // Read multiple times
                    for _ in 0..100 {
                        let read_data = black_box(reader.read().unwrap());
                        black_box(read_data.len());
                    }
                });
                handles.push(handle);
            }

            barrier.wait(); // Start all threads simultaneously

            // Wait for all readers to complete
            for handle in handles {
                handle.join().unwrap();
            }
        });
    });
}

/// Benchmark reader throughput under write pressure
fn bench_reader_write_contention(c: &mut Criterion) {
    c.bench_function("reader_under_write_pressure", |b| {
        b.iter(|| {
            // We use two barriers:
            // 1. "Ready to start" - Writer created segment
            // 2. "Go" - Everyone ready to work
            let barrier_created = Arc::new(Barrier::new(2));
            let barrier_start = Arc::new(Barrier::new(2));

            let bc_writer = barrier_created.clone();
            let bs_writer = barrier_start.clone();

            let bc_reader = barrier_created.clone();
            let bs_reader = barrier_start.clone();

            // Spawn writer thread
            let writer_handle = thread::spawn(move || {
                let segment_name = "bench_contention_w";
                let full_path = "/dev/shm/evo_bench_contention_w";

                // FIX: Retry loop.
                // Operating system may have delay in releasing the file.
                // We try until success.
                let mut local_writer = loop {
                    // 1. Try to remove (ignore error if file doesn't exist)
                    let _ = std::fs::remove_file(full_path);

                    // 2. Try to create
                    match SegmentWriter::create(segment_name, 65536) {
                        Ok(w) => break w, // Success! Exit the loop.
                        Err(_) => {
                            // File still exists or is locked.
                            // Wait 10 microseconds and try again.
                            thread::sleep(std::time::Duration::from_micros(10));
                        }
                    }
                };

                let local_data = vec![0xAAu8; 512];

                // 2. Signal: "Created!"
                bc_writer.wait();

                // 3. Wait for test start
                bs_writer.wait();

                // Write continuously
                for _ in 0..50 {
                    black_box(local_writer.write(&local_data).unwrap());
                    thread::yield_now();
                }
            });

            // Reader thread
            let reader_handle = thread::spawn(move || {
                // 1. Wait until Writer creates segment
                bc_reader.wait();

                // 2. Now safely connect
                // Add small retry loop, as filesystem may have microsecond delay
                let mut reader = loop {
                    match SegmentReader::attach("bench_contention_w") {
                        Ok(r) => break r,
                        Err(_) => thread::yield_now(),
                    }
                };

                // 3. Czekamy na start testu
                bs_reader.wait();

                // Read continuously
                for _ in 0..100 {
                    if let Ok(read_data) = reader.read() {
                        black_box(read_data.len());
                    }
                    thread::yield_now();
                }
            });

            writer_handle.join().unwrap();
            reader_handle.join().unwrap();
        });
    });
}

/// Benchmark version conflict detection under high contention
fn bench_version_conflicts(c: &mut Criterion) {
    let segment_name = "bench_version_conflicts";
    let mut writer = SegmentWriter::create(segment_name, 65536).unwrap();

    c.bench_function("version_conflict_detection", |b| {
        b.iter(|| {
            let barrier = Arc::new(Barrier::new(6)); // 5 readers + 1 main
            let mut handles = Vec::new();

            // Spawn 5 readers checking for version changes
            for i in 0..5 {
                let barrier_clone = barrier.clone();
                let handle = thread::spawn(move || {
                    let reader = SegmentReader::attach(segment_name).unwrap();
                    barrier_clone.wait();

                    // Check version changes rapidly
                    for _ in 0..200 {
                        let _changed = black_box(reader.has_changed());
                        let _version = black_box(reader.version());
                        if i % 2 == 0 {
                            thread::yield_now();
                        }
                    }
                });
                handles.push(handle);
            }

            barrier.wait();

            // Write frequently to create version conflicts
            for i in 0..20 {
                let test_data = vec![i as u8; 256];
                black_box(writer.write(&test_data).unwrap());
                thread::yield_now();
            }

            for handle in handles {
                handle.join().unwrap();
            }
        });
    });
}

criterion_group!(
    benches,
    bench_concurrent_readers,
    bench_reader_write_contention,
    bench_version_conflicts
);
criterion_main!(benches);
