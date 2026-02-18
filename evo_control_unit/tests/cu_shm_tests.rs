//! CU SHM integration tests (T057).
//!
//! Verifies:
//! 1. CU creates outbound segments and external reader can verify data.
//! 2. CU reads HAL feedback from evo_hal_cu segment.
//! 3. CU writes commands to evo_cu_hal segment.
//! 4. MQT error_flags is full-width u32 (not truncated).

use evo_common::shm::io_helpers::{get_di, set_do};
use evo_common::shm::p2p::{ModuleAbbrev, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::*;

// ─── Helpers ────────────────────────────────────────────────────────

/// Unique segment names to avoid collisions with parallel tests.
fn test_seg(base: &str, suffix: &str) -> String {
    format!("test_{base}_{suffix}")
}

// ─── Test 1: CU creates writer, external reader verifies data ──────

#[test]
fn test_cu_writer_creates_segment_and_reader_verifies() {
    let seg_name = test_seg("cu_hal", "t1");

    // CU creates a writer for CuToHalSegment.
    let mut writer =
        TypedP2pWriter::<CuToHalSegment>::create(&seg_name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("writer create failed");

    // Build a segment with known data.
    let mut payload: CuToHalSegment = unsafe { core::mem::zeroed() };
    payload.axis_count = 4;
    payload.axes[0].enable = 1;
    payload.axes[0].target_position = 100.5;
    payload.axes[1].enable = 1;
    payload.axes[1].target_velocity = 50.0;
    // Set a digital output.
    set_do(&mut payload.do_bank, 7, true);
    payload.ao_values[0] = 3.14;

    writer.commit(&payload).expect("commit failed");

    // External reader attaches and reads.
    let mut reader =
        TypedP2pReader::<CuToHalSegment>::attach(&seg_name, 10).expect("reader attach failed");

    let read_payload = reader.read().expect("read failed");
    assert_eq!(read_payload.axis_count, 4);
    assert_eq!(read_payload.axes[0].enable, 1);
    assert!((read_payload.axes[0].target_position - 100.5).abs() < f64::EPSILON);
    assert_eq!(read_payload.axes[1].enable, 1);
    assert!((read_payload.axes[1].target_velocity - 50.0).abs() < f64::EPSILON);
    assert!(get_di(&read_payload.do_bank, 7));
    assert!(!get_di(&read_payload.do_bank, 6));
    assert!((read_payload.ao_values[0] - 3.14).abs() < f64::EPSILON);
}

// ─── Test 2: CU reads HAL feedback from segment ────────────────────

#[test]
fn test_cu_reads_hal_feedback_from_segment() {
    let seg_name = test_seg("hal_cu", "t2");

    // Simulate HAL writing feedback.
    let mut hal_writer =
        TypedP2pWriter::<HalToCuSegment>::create(&seg_name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
            .expect("HAL writer create failed");

    let mut hal_payload: HalToCuSegment = unsafe { core::mem::zeroed() };
    hal_payload.axis_count = 2;
    hal_payload.axes[0].position = 42.0;
    hal_payload.axes[0].velocity = 10.0;
    hal_payload.axes[0].drive_ready = 1;
    hal_payload.axes[1].position = 84.0;
    hal_payload.axes[1].velocity = 20.0;
    hal_payload.axes[1].drive_fault = 5;
    // Set some DI bits.
    set_do(&mut hal_payload.di_bank, 0, true);
    set_do(&mut hal_payload.di_bank, 100, true);
    // Set an AI value.
    hal_payload.ai_values[3] = 2.718;

    hal_writer.commit(&hal_payload).expect("HAL commit failed");

    // CU reader attaches and reads.
    let mut cu_reader =
        TypedP2pReader::<HalToCuSegment>::attach(&seg_name, 10).expect("CU reader attach failed");

    let read = cu_reader.read().expect("CU read failed");
    assert_eq!(read.axis_count, 2);
    assert!((read.axes[0].position - 42.0).abs() < f64::EPSILON);
    assert!((read.axes[0].velocity - 10.0).abs() < f64::EPSILON);
    assert_eq!(read.axes[0].drive_ready, 1);
    assert!((read.axes[1].position - 84.0).abs() < f64::EPSILON);
    assert_eq!(read.axes[1].drive_fault, 5);
    // Verify DI bank.
    assert!(get_di(&read.di_bank, 0));
    assert!(get_di(&read.di_bank, 100));
    assert!(!get_di(&read.di_bank, 1));
    // Verify AI values.
    assert!((read.ai_values[3] - 2.718).abs() < f64::EPSILON);
}

// ─── Test 3: CU writes commands, external reader verifies ──────────

#[test]
fn test_cu_writes_commands_external_reader_verifies() {
    let seg_name = test_seg("cu_hal", "t3");

    let mut writer =
        TypedP2pWriter::<CuToHalSegment>::create(&seg_name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("writer create failed");

    let mut cmd: CuToHalSegment = unsafe { core::mem::zeroed() };
    cmd.axis_count = 3;
    for i in 0..3 {
        cmd.axes[i].enable = 1;
        cmd.axes[i].target_position = (i as f64 + 1.0) * 10.0;
        cmd.axes[i].target_velocity = (i as f64 + 1.0) * 5.0;
    }
    // Set some digital outputs.
    set_do(&mut cmd.do_bank, 0, true);
    set_do(&mut cmd.do_bank, 15, true);
    cmd.ao_values[0] = 1.23;
    cmd.ao_values[1] = 4.56;

    writer.commit(&cmd).expect("commit failed");

    let mut reader =
        TypedP2pReader::<CuToHalSegment>::attach(&seg_name, 10).expect("reader attach failed");
    let read = reader.read().expect("read failed");

    assert_eq!(read.axis_count, 3);
    for i in 0..3 {
        assert_eq!(read.axes[i].enable, 1);
        assert!((read.axes[i].target_position - (i as f64 + 1.0) * 10.0).abs() < f64::EPSILON);
        assert!((read.axes[i].target_velocity - (i as f64 + 1.0) * 5.0).abs() < f64::EPSILON);
    }
    assert!(get_di(&read.do_bank, 0));
    assert!(get_di(&read.do_bank, 15));
    assert!(!get_di(&read.do_bank, 1));
    assert!((read.ao_values[0] - 1.23).abs() < f64::EPSILON);
    assert!((read.ao_values[1] - 4.56).abs() < f64::EPSILON);
}

// ─── Test 4: MQT error_flags is full-width u32 ─────────────────────

#[test]
fn test_mqt_error_flags_full_width_u32() {
    let seg_name = test_seg("cu_mqt", "t4");

    let mut writer =
        TypedP2pWriter::<CuToMqtSegment>::create(&seg_name, ModuleAbbrev::Cu, ModuleAbbrev::Mqt)
            .expect("writer create failed");

    let mut mqt: CuToMqtSegment = unsafe { core::mem::zeroed() };
    mqt.machine_state = 3; // Active
    mqt.safety_state = 0; // Normal
    mqt.axis_count = 1;
    // Set error_flags with high bits (>= bit 16) to verify no truncation.
    mqt.error_flags = 0x8001_FFFF; // Bits 31, 16..0 all set.
    mqt.axis_status[0].axis_state = 2;
    mqt.axis_status[0].error_code = 0xABCD;

    writer.commit(&mqt).expect("commit failed");

    let mut reader =
        TypedP2pReader::<CuToMqtSegment>::attach(&seg_name, 10).expect("reader attach failed");
    let read = reader.read().expect("read failed");

    assert_eq!(read.machine_state, 3);
    assert_eq!(read.safety_state, 0);
    assert_eq!(read.axis_count, 1);
    // Verify error_flags is NOT truncated — all 32 bits preserved.
    assert_eq!(read.error_flags, 0x8001_FFFF);
    assert_eq!(read.axis_status[0].axis_state, 2);
    assert_eq!(read.axis_status[0].error_code, 0xABCD);
}

// ─── Test 5: DI bank round-trip through segments ────────────────────

#[test]
fn test_di_bank_roundtrip_through_segments() {
    let seg_name = test_seg("hal_cu", "t5");

    let mut writer =
        TypedP2pWriter::<HalToCuSegment>::create(&seg_name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
            .expect("writer create failed");

    let mut payload: HalToCuSegment = unsafe { core::mem::zeroed() };
    // Set specific DI pins in different words.
    set_do(&mut payload.di_bank, 0, true);
    set_do(&mut payload.di_bank, 63, true); // Last bit of word 0
    set_do(&mut payload.di_bank, 64, true); // First bit of word 1
    set_do(&mut payload.di_bank, 1023, true); // Last possible bit

    writer.commit(&payload).expect("commit failed");

    let mut reader =
        TypedP2pReader::<HalToCuSegment>::attach(&seg_name, 10).expect("reader attach failed");
    let read = reader.read().expect("read failed");

    assert!(get_di(&read.di_bank, 0));
    assert!(get_di(&read.di_bank, 63));
    assert!(get_di(&read.di_bank, 64));
    assert!(get_di(&read.di_bank, 1023));
    // Verify unset bits are clear.
    assert!(!get_di(&read.di_bank, 1));
    assert!(!get_di(&read.di_bank, 62));
    assert!(!get_di(&read.di_bank, 65));
    assert!(!get_di(&read.di_bank, 512));
}

// ─── Test 6: AI values round-trip ───────────────────────────────────

#[test]
fn test_ai_values_roundtrip_through_segments() {
    let seg_name = test_seg("hal_cu", "t6");

    let mut writer =
        TypedP2pWriter::<HalToCuSegment>::create(&seg_name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
            .expect("writer create failed");

    let mut payload: HalToCuSegment = unsafe { core::mem::zeroed() };
    // Set various AI values.
    payload.ai_values[0] = 0.0;
    payload.ai_values[1] = -1.5;
    payload.ai_values[63] = 999.999;
    payload.ai_values[100] = f64::MAX;

    writer.commit(&payload).expect("commit failed");

    let mut reader =
        TypedP2pReader::<HalToCuSegment>::attach(&seg_name, 10).expect("reader attach failed");
    let read = reader.read().expect("read failed");

    assert!((read.ai_values[0] - 0.0).abs() < f64::EPSILON);
    assert!((read.ai_values[1] - (-1.5)).abs() < f64::EPSILON);
    assert!((read.ai_values[63] - 999.999).abs() < f64::EPSILON);
    assert_eq!(read.ai_values[100], f64::MAX);
}

// ─── Test 7: CU→RE segment write/read ──────────────────────────────

#[test]
fn test_cu_to_re_segment_roundtrip() {
    let seg_name = test_seg("cu_re", "t7");

    let mut writer =
        TypedP2pWriter::<CuToReSegment>::create(&seg_name, ModuleAbbrev::Cu, ModuleAbbrev::Re)
            .expect("writer create failed");

    let mut payload: CuToReSegment = unsafe { core::mem::zeroed() };
    // CuToReSegment is reserved — write a pattern into the reserved bytes.
    payload._reserved[0] = 0xAA;
    payload._reserved[255] = 0xBB;

    writer.commit(&payload).expect("commit failed");

    let mut reader =
        TypedP2pReader::<CuToReSegment>::attach(&seg_name, 10).expect("reader attach failed");
    let read = reader.read().expect("read failed");

    assert_eq!(read._reserved[0], 0xAA);
    assert_eq!(read._reserved[255], 0xBB);
    assert_eq!(read._reserved[1], 0x00); // Untouched bytes are zero.
}
