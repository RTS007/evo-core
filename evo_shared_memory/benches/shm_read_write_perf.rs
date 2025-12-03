//! Read/write performance benchmarks

use criterion::{Criterion, criterion_group, criterion_main};
use evo_shared_memory::SHM_MIN_SIZE;
use evo_shared_memory::reader::SegmentReader;
use evo_shared_memory::writer::SegmentWriter;
use std::hint::black_box;

/// Benchmark write operations for different sizes
fn bench_write_operations(c: &mut Criterion) {
    let segment_name = "bench_write";
    let mut writer = SegmentWriter::create(segment_name, 65536).unwrap();

    let data_64 = vec![0xAAu8; 64];
    let data_1k = vec![0xAAu8; 1024];
    let data_4k = vec![0xAAu8; 4096];

    c.bench_function("write_64_bytes", |b| {
        b.iter(|| {
            black_box(writer.write(&data_64).unwrap());
        });
    });

    c.bench_function("write_1k_bytes", |b| {
        b.iter(|| {
            black_box(writer.write(&data_1k).unwrap());
        });
    });

    c.bench_function("write_4k_bytes", |b| {
        b.iter(|| {
            black_box(writer.write(&data_4k).unwrap());
        });
    });
}

/// Benchmark read operations for different sizes
fn bench_read_operations(c: &mut Criterion) {
    let segment_name = "bench_read";
    let mut writer = SegmentWriter::create(segment_name, 65536).unwrap();
    let mut reader = SegmentReader::attach(segment_name).unwrap();

    // Write test data
    let data_64 = vec![0xAAu8; 64];
    let data_1k = vec![0xAAu8; 1024];
    let data_4k = vec![0xAAu8; 4096];

    writer.write(&data_64).unwrap();
    c.bench_function("read_64_bytes", |b| {
        b.iter(|| {
            let read_data = black_box(reader.read().unwrap());
            black_box(read_data.len());
        });
    });

    writer.write(&data_1k).unwrap();
    c.bench_function("read_1k_bytes", |b| {
        b.iter(|| {
            let read_data = black_box(reader.read().unwrap());
            black_box(read_data.len());
        });
    });

    writer.write(&data_4k).unwrap();
    c.bench_function("read_4k_bytes", |b| {
        b.iter(|| {
            let read_data = black_box(reader.read().unwrap());
            black_box(read_data.len());
        });
    });
}

/// Benchmark write-read round trip for sub-microsecond validation
fn bench_write_read_roundtrip(c: &mut Criterion) {
    let segment_name = "bench_roundtrip";
    let mut writer = SegmentWriter::create(segment_name, 65536).unwrap();
    let mut reader = SegmentReader::attach(segment_name).unwrap();
    let data = vec![0xAAu8; 64];

    c.bench_function("roundtrip_64_bytes", |b| {
        b.iter(|| {
            black_box(writer.write(&data).unwrap());
            let read_data = black_box(reader.read().unwrap());
            black_box(read_data.len());
        });
    });
}

/// Benchmark version counter performance
fn bench_version_operations(c: &mut Criterion) {
    let segment_name = "bench_version";
    let _writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE).unwrap();
    let reader = SegmentReader::attach(segment_name).unwrap();

    c.bench_function("version_check", |b| {
        b.iter(|| {
            let version = black_box(reader.version());
            black_box(version);
        });
    });

    c.bench_function("has_changed_check", |b| {
        b.iter(|| {
            let changed = black_box(reader.has_changed());
            black_box(changed);
        });
    });
}

criterion_group!(
    benches,
    bench_write_operations,
    bench_read_operations,
    bench_write_read_roundtrip,
    bench_version_operations
);
criterion_main!(benches);
