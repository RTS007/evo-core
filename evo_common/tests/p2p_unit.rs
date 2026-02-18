//! Extended P2P unit tests — covers `has_changed`, `reset_stale`, heartbeat
//! increment on commit, and edge cases that complement the inline `mod tests`
//! block in `evo_common::shm::p2p`.

use evo_common::shm::p2p::{
    ModuleAbbrev, P2pSegmentHeader, ShmError, TypedP2pReader, TypedP2pWriter,
};

/// Helper segment for testing — 128 bytes, cache-line aligned.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
struct TestSeg {
    header: P2pSegmentHeader,
    value: u64,
    _pad: [u8; 56],
}

/// Test: `has_changed()` returns true after new write, false when unchanged.
#[test]
fn has_changed_tracks_heartbeat() {
    let name = format!("test_hc_{}", std::process::id());
    let mut writer =
        TypedP2pWriter::<TestSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("create");

    let mut payload: TestSeg = unsafe { core::mem::zeroed() };
    payload.value = 1;
    writer.commit(&payload).expect("commit 1");

    let mut reader = TypedP2pReader::<TestSeg>::attach(&name, 10).expect("attach");

    // Before any read, has_changed should be true (heartbeat > 0 vs last_heartbeat = 0).
    assert!(reader.has_changed(), "initial: heartbeat > 0 → changed");

    // After read, last_heartbeat is synced → has_changed is false.
    let _d = reader.read().expect("read 1");
    assert!(!reader.has_changed(), "after read, no new write → no change");

    // Writer commits again — heartbeat increments → has_changed is true.
    payload.value = 2;
    writer.commit(&payload).expect("commit 2");
    assert!(reader.has_changed(), "after second commit → changed");

    // Read to sync, then verify false again.
    let _d = reader.read().expect("read 2");
    assert!(!reader.has_changed(), "synced again → no change");
}

/// Test: `reset_stale()` clears the stale counter.
#[test]
fn reset_stale_clears_counter() {
    let name = format!("test_rs_{}", std::process::id());
    let mut writer =
        TypedP2pWriter::<TestSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("create");

    let payload: TestSeg = unsafe { core::mem::zeroed() };
    writer.commit(&payload).expect("commit");

    let mut reader = TypedP2pReader::<TestSeg>::attach(&name, 10).expect("attach");
    let _d = reader.read().expect("read initial");

    // Read without writer committing — stale count should increase.
    let _d = reader.read().expect("read 2");
    assert!(reader.stale_count() >= 1, "stale count should be ≥1");

    // Reset and verify.
    reader.reset_stale();
    assert_eq!(reader.stale_count(), 0, "stale count should be reset");
}

/// Test: heartbeat monotonically increments with each commit.
#[test]
fn heartbeat_increments_on_commit() {
    let name = format!("test_hbi_{}", std::process::id());
    let mut writer =
        TypedP2pWriter::<TestSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("create");

    let mut reader = TypedP2pReader::<TestSeg>::attach(&name, 10).expect("attach");

    let mut prev_hb = 0u64;
    for i in 0..10 {
        let mut payload: TestSeg = unsafe { core::mem::zeroed() };
        payload.value = i;
        writer.commit(&payload).expect("commit");

        let data = reader.read().expect("read");
        assert_eq!(data.value, i);
        let hb = reader.last_heartbeat();
        assert!(hb > prev_hb, "heartbeat should increment: {hb} > {prev_hb}");
        prev_hb = hb;
    }
}

/// Test: `name()` returns the segment name.
#[test]
fn reader_name_accessor() {
    let name = format!("test_rn_{}", std::process::id());
    let mut writer =
        TypedP2pWriter::<TestSeg>::create(&name, ModuleAbbrev::Re, ModuleAbbrev::Mqt)
            .expect("create");
    let payload: TestSeg = unsafe { core::mem::zeroed() };
    writer.commit(&payload).expect("commit");

    let reader = TypedP2pReader::<TestSeg>::attach(&name, 10).expect("attach");
    assert_eq!(reader.name(), name);
}

/// Test: `attach_validated()` with wrong dest module fails.
#[test]
fn attach_validated_wrong_dest_is_error() {
    let name = format!("test_avw_{}", std::process::id());
    let mut writer =
        TypedP2pWriter::<TestSeg>::create(&name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
            .expect("create");
    let payload: TestSeg = unsafe { core::mem::zeroed() };
    writer.commit(&payload).expect("commit");

    let result =
        TypedP2pReader::<TestSeg>::attach_validated(&name, 10, ModuleAbbrev::Re);
    assert!(
        matches!(result, Err(ShmError::DestinationMismatch { .. })),
        "expected DestinationMismatch"
    );

    // Correct dest should succeed.
    let _ok =
        TypedP2pReader::<TestSeg>::attach_validated(&name, 10, ModuleAbbrev::Cu)
            .expect("correct dest should succeed");
}

/// Test: large payload (4 pages) writes and reads correctly.
#[test]
fn large_payload_roundtrip() {
    let name = format!("test_lp_{}", std::process::id());

    #[derive(Debug, Clone, Copy)]
    #[repr(C, align(64))]
    struct BigSeg {
        header: P2pSegmentHeader,
        data: [u64; 512], // 4096 bytes of data
    }

    let mut writer =
        TypedP2pWriter::<BigSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("create");

    let mut payload: BigSeg = unsafe { core::mem::zeroed() };
    for (i, v) in payload.data.iter_mut().enumerate() {
        *v = i as u64 * 42;
    }
    writer.commit(&payload).expect("commit");

    let mut reader = TypedP2pReader::<BigSeg>::attach(&name, 10).expect("attach");
    let read_data = reader.read().expect("read");
    for (i, v) in read_data.data.iter().enumerate() {
        assert_eq!(*v, i as u64 * 42, "mismatch at index {i}");
    }
}

/// Test: creating a writer for a non-existent segment then attaching reader fails.
#[test]
fn reader_attach_no_segment() {
    let name = format!("test_noseg_{}", std::process::id());
    let result = TypedP2pReader::<TestSeg>::attach(&name, 10);
    assert!(
        matches!(result, Err(ShmError::SegmentNotFound { .. })),
        "expected SegmentNotFound"
    );
}

/// Test: writer can overwrite data multiple times, reader always gets latest.
#[test]
fn rapid_overwrite() {
    let name = format!("test_ow_{}", std::process::id());
    let mut writer =
        TypedP2pWriter::<TestSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("create");

    let mut reader = TypedP2pReader::<TestSeg>::attach(&name, 10).expect("attach");

    for i in 0..100u64 {
        let mut payload: TestSeg = unsafe { core::mem::zeroed() };
        payload.value = i;
        writer.commit(&payload).expect("commit");
    }

    // Reader should get the latest value.
    let data = reader.read().expect("read");
    assert_eq!(data.value, 99, "reader should see the last committed value");
}
