//! P2P SHM latency benchmarks.
//!
//! Measures single-writer / single-reader latency for various segment sizes.
//! Target: write ≤ 5µs, read ≤ 2µs for segments ≤ 8KB.

use criterion::{Criterion, criterion_group, criterion_main};
use evo_common::shm::p2p::{ModuleAbbrev, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::{HalToCuSegment, CuToHalSegment, CuToMqtSegment};
use std::hint::black_box;
use std::sync::atomic::{AtomicU32, Ordering};

/// Generate a unique segment name per benchmark iteration.
static BENCH_CTR: AtomicU32 = AtomicU32::new(0);

fn bench_seg(prefix: &str) -> String {
    let id = BENCH_CTR.fetch_add(1, Ordering::Relaxed);
    format!("bench_{prefix}_{id}")
}

fn bench_write_hal_to_cu(c: &mut Criterion) {
    let name = bench_seg("w_hal_cu");
    let mut writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name, ModuleAbbrev::Hal, ModuleAbbrev::Cu,
    ).expect("create writer");

    let payload = HalToCuSegment::default();

    c.bench_function("p2p_write_HalToCuSegment", |b| {
        b.iter(|| {
            writer.commit(black_box(&payload)).unwrap();
        });
    });
}

fn bench_read_hal_to_cu(c: &mut Criterion) {
    let name = bench_seg("r_hal_cu");
    let mut writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name, ModuleAbbrev::Hal, ModuleAbbrev::Cu,
    ).expect("create writer");

    // Write initial data so reader has something.
    let payload = HalToCuSegment::default();
    writer.commit(&payload).unwrap();

    let mut reader = TypedP2pReader::<HalToCuSegment>::attach(&name, 1000).expect("attach reader");

    c.bench_function("p2p_read_HalToCuSegment", |b| {
        b.iter(|| {
            let _data = black_box(reader.read());
        });
    });
}

fn bench_write_cu_to_hal(c: &mut Criterion) {
    let name = bench_seg("w_cu_hal");
    let mut writer = TypedP2pWriter::<CuToHalSegment>::create(
        &name, ModuleAbbrev::Cu, ModuleAbbrev::Hal,
    ).expect("create writer");

    let payload = CuToHalSegment::default();

    c.bench_function("p2p_write_CuToHalSegment", |b| {
        b.iter(|| {
            writer.commit(black_box(&payload)).unwrap();
        });
    });
}

fn bench_write_cu_to_mqt(c: &mut Criterion) {
    let name = bench_seg("w_cu_mqt");
    let mut writer = TypedP2pWriter::<CuToMqtSegment>::create(
        &name, ModuleAbbrev::Cu, ModuleAbbrev::Mqt,
    ).expect("create writer");

    let payload = CuToMqtSegment::default();

    c.bench_function("p2p_write_CuToMqtSegment", |b| {
        b.iter(|| {
            writer.commit(black_box(&payload)).unwrap();
        });
    });
}

fn bench_roundtrip_hal_cu(c: &mut Criterion) {
    let name = bench_seg("rt_hal_cu");
    let mut writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name, ModuleAbbrev::Hal, ModuleAbbrev::Cu,
    ).expect("create writer");

    let payload = HalToCuSegment::default();
    writer.commit(&payload).unwrap();

    let mut reader = TypedP2pReader::<HalToCuSegment>::attach(&name, 1000).expect("attach reader");

    c.bench_function("p2p_roundtrip_HalToCuSegment", |b| {
        b.iter(|| {
            writer.commit(black_box(&payload)).unwrap();
            let _data = black_box(reader.read());
        });
    });
}

criterion_group!(
    benches,
    bench_write_hal_to_cu,
    bench_read_hal_to_cu,
    bench_write_cu_to_hal,
    bench_write_cu_to_mqt,
    bench_roundtrip_hal_cu,
);
criterion_main!(benches);
