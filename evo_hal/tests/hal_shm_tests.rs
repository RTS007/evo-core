//! HAL SHM integration tests (T051).
//!
//! Tests that HAL P2P writers create segments and external readers
//! can verify data. Also tests reading known values from a segment,
//! DI packing round-trip, AI scaling verification.

use evo_common::consts::MAX_AI;
use evo_common::hal::types::{AnalogValue, HalCommands, HalStatus};
use evo_common::shm::conversions::{
    hal_commands_to_segment, hal_status_to_segment, segment_to_hal_commands,
    segment_to_hal_status,
};
use evo_common::shm::io_helpers::{get_di, pack_bools, set_do, unpack_bools, BANK_WORDS};
use evo_common::shm::p2p::{ModuleAbbrev, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::*;

/// Helper: cleanup SHM segment if it exists.
fn cleanup_segment(name: &str) {
    let data_path = format!("/evo_{}", name);
    let lock_path = format!("/evo_{}.lock", name);
    let _ = nix::sys::mman::shm_unlink(data_path.as_str());
    let _ = nix::sys::mman::shm_unlink(lock_path.as_str());
}

#[test]
fn test_hal_writer_creates_segment_and_reader_verifies() {
    let seg_name = "test_hal_cu_t051a";
    cleanup_segment(seg_name);

    // HAL creates writer.
    let mut writer =
        TypedP2pWriter::<HalToCuSegment>::create(seg_name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
            .expect("create writer");

    // Write known data.
    let mut payload = HalToCuSegment::default();
    payload.axis_count = 2;
    payload.axes[0].position = 42.5;
    payload.axes[0].velocity = 10.0;
    payload.axes[0].drive_ready = 1;
    payload.axes[1].position = -100.0;
    payload.axes[1].referenced = 1;
    // Set DI pin 7.
    payload.di_bank[0] |= 1u64 << 7;
    // Set AI value.
    payload.ai_values[0] = 3.14;

    writer.commit(&payload).expect("commit");

    // External reader attaches and verifies.
    let mut reader =
        TypedP2pReader::<HalToCuSegment>::attach(seg_name, 100).expect("attach reader");

    let read_data = reader.read().expect("read");
    assert_eq!(read_data.axis_count, 2);
    assert!((read_data.axes[0].position - 42.5).abs() < f64::EPSILON);
    assert!((read_data.axes[0].velocity - 10.0).abs() < f64::EPSILON);
    assert_eq!(read_data.axes[0].drive_ready, 1);
    assert!((read_data.axes[1].position - (-100.0)).abs() < f64::EPSILON);
    assert_eq!(read_data.axes[1].referenced, 1);
    assert!(get_di(&read_data.di_bank, 7));
    assert!(!get_di(&read_data.di_bank, 6));
    assert!((read_data.ai_values[0] - 3.14).abs() < f64::EPSILON);

    drop(reader);
    drop(writer);
    cleanup_segment(seg_name);
}

#[test]
fn test_hal_reads_commands_from_segment() {
    let seg_name = "test_cu_hal_t051b";
    cleanup_segment(seg_name);

    // CU creates writer (simulating CU side).
    let mut writer =
        TypedP2pWriter::<CuToHalSegment>::create(seg_name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("create CU writer");

    // Write known commands.
    let mut payload = CuToHalSegment::default();
    payload.axis_count = 3;
    payload.axes[0].target_position = 100.0;
    payload.axes[0].target_velocity = 50.0;
    payload.axes[0].enable = 1;
    payload.axes[1].target_position = -200.0;
    payload.axes[1].brake_release = 1;
    // Set DO pin 42.
    set_do(&mut payload.do_bank, 42, true);
    // Set AO value.
    payload.ao_values[5] = 2.718;

    writer.commit(&payload).expect("commit");

    // HAL attaches reader and reads.
    let mut reader =
        TypedP2pReader::<CuToHalSegment>::attach(seg_name, 100).expect("attach HAL reader");

    let read_data = reader.read().expect("read");
    assert_eq!(read_data.axis_count, 3);
    assert!((read_data.axes[0].target_position - 100.0).abs() < f64::EPSILON);
    assert!((read_data.axes[0].target_velocity - 50.0).abs() < f64::EPSILON);
    assert_eq!(read_data.axes[0].enable, 1);
    assert!((read_data.axes[1].target_position - (-200.0)).abs() < f64::EPSILON);
    assert_eq!(read_data.axes[1].brake_release, 1);
    assert!(get_di(&read_data.do_bank, 42)); // DO uses same bank layout.
    assert!((read_data.ao_values[5] - 2.718).abs() < f64::EPSILON);

    // Convert to HalCommands.
    let cmds = segment_to_hal_commands(read_data);
    assert!(cmds.axes[0].enable);
    assert!((cmds.axes[0].target_position - 100.0).abs() < f64::EPSILON);
    assert!(cmds.digital_outputs[42]);
    assert!(!cmds.digital_outputs[41]);

    drop(reader);
    drop(writer);
    cleanup_segment(seg_name);
}

#[test]
fn test_di_packing_roundtrip() {
    // Pack bools → u64 bank → unpack bools. Verify match.
    let mut input = [false; 1024];
    input[0] = true;
    input[7] = true;
    input[63] = true;
    input[64] = true;
    input[511] = true;
    input[1023] = true;

    let mut bank = [0u64; BANK_WORDS];
    pack_bools(&input, &mut bank);

    // Verify specific bits.
    assert!(get_di(&bank, 0));
    assert!(get_di(&bank, 7));
    assert!(get_di(&bank, 63));
    assert!(get_di(&bank, 64));
    assert!(get_di(&bank, 511));
    assert!(get_di(&bank, 1023));
    assert!(!get_di(&bank, 1));
    assert!(!get_di(&bank, 512));

    // Round-trip back.
    let mut output = [false; 1024];
    unpack_bools(&bank, &mut output);

    for i in 0..1024 {
        assert_eq!(input[i], output[i], "mismatch at pin {i}");
    }
}

#[test]
fn test_ai_scaling_via_conversion() {
    // Verify that AnalogValue.scaled is extracted correctly via conversion.
    let mut status = HalStatus::default();
    status.analog_inputs[0] = AnalogValue {
        normalized: 0.5,
        scaled: 123.456,
    };
    status.analog_inputs[3] = AnalogValue {
        normalized: 1.0,
        scaled: -99.99,
    };
    status.analog_inputs[MAX_AI - 1] = AnalogValue {
        normalized: 0.0,
        scaled: 0.001,
    };

    let seg = hal_status_to_segment(&status, 4);

    assert!((seg.ai_values[0] - 123.456).abs() < f64::EPSILON);
    assert!((seg.ai_values[3] - (-99.99)).abs() < f64::EPSILON);
    assert!((seg.ai_values[MAX_AI - 1] - 0.001).abs() < f64::EPSILON);
}

#[test]
fn test_conversion_roundtrip_hal_status() {
    // HalStatus → HalToCuSegment → HalStatus: verify data preserved.
    let mut status = HalStatus::default();
    status.axes[0].actual_position = 42.5;
    status.axes[0].actual_velocity = 10.0;
    status.axes[0].ready = true;
    status.axes[0].referenced = true;
    status.axes[1].actual_position = -200.0;
    status.axes[1].error = true;
    status.axes[1].error_code = 0x1234;
    status.digital_inputs[7] = true;
    status.digital_inputs[100] = true;
    status.analog_inputs[0].scaled = 3.14;

    let seg = hal_status_to_segment(&status, 2);
    let roundtrip = segment_to_hal_status(&seg);

    assert!((roundtrip.axes[0].actual_position - 42.5).abs() < f64::EPSILON);
    assert!((roundtrip.axes[0].actual_velocity - 10.0).abs() < f64::EPSILON);
    assert!(roundtrip.axes[0].ready);
    assert!(roundtrip.axes[0].referenced);
    assert!((roundtrip.axes[1].actual_position - (-200.0)).abs() < f64::EPSILON);
    assert!(roundtrip.axes[1].error);
    assert!(roundtrip.digital_inputs[7]);
    assert!(roundtrip.digital_inputs[100]);
    assert!(!roundtrip.digital_inputs[8]);
    assert!((roundtrip.analog_inputs[0].scaled - 3.14).abs() < f64::EPSILON);
}

#[test]
fn test_conversion_roundtrip_hal_commands() {
    // HalCommands → CuToHalSegment → HalCommands: verify data preserved.
    let mut cmds = HalCommands::default();
    cmds.axes[0].target_position = 100.0;
    cmds.axes[0].enable = true;
    cmds.axes[2].target_position = -500.0;
    cmds.digital_outputs[42] = true;
    cmds.digital_outputs[0] = true;
    cmds.analog_outputs[3] = 7.77;

    let seg = hal_commands_to_segment(&cmds, 3);
    let roundtrip = segment_to_hal_commands(&seg);

    assert!((roundtrip.axes[0].target_position - 100.0).abs() < f64::EPSILON);
    assert!(roundtrip.axes[0].enable);
    assert!((roundtrip.axes[2].target_position - (-500.0)).abs() < f64::EPSILON);
    assert!(roundtrip.digital_outputs[42]);
    assert!(roundtrip.digital_outputs[0]);
    assert!(!roundtrip.digital_outputs[1]);
    assert!((roundtrip.analog_outputs[3] - 7.77).abs() < f64::EPSILON);
}

#[test]
fn test_hal_mqt_segment_has_both_input_and_output() {
    let seg_name = "test_hal_mqt_t051c";
    cleanup_segment(seg_name);

    // Create HAL → MQT writer.
    let mut writer =
        TypedP2pWriter::<HalToMqtSegment>::create(seg_name, ModuleAbbrev::Hal, ModuleAbbrev::Mqt)
            .expect("create MQT writer");

    let mut payload = HalToMqtSegment::default();
    payload.axis_count = 1;
    payload.axes[0].position = 50.0;
    payload.cycle_time_ns = 1_000_000; // 1ms
    // DI.
    payload.di_bank[0] = 0xFF;
    // DO.
    payload.do_bank[0] = 0x01;
    // AI.
    payload.ai_values[0] = 1.23;
    // AO.
    payload.ao_values[0] = 4.56;

    writer.commit(&payload).expect("commit");

    // Reader verifies superset.
    let mut reader =
        TypedP2pReader::<HalToMqtSegment>::attach(seg_name, 100).expect("attach reader");

    let read = reader.read().expect("read");
    assert_eq!(read.axis_count, 1);
    assert!((read.axes[0].position - 50.0).abs() < f64::EPSILON);
    assert_eq!(read.cycle_time_ns, 1_000_000);
    assert_eq!(read.di_bank[0], 0xFF);
    assert_eq!(read.do_bank[0], 0x01);
    assert!((read.ai_values[0] - 1.23).abs() < f64::EPSILON);
    assert!((read.ao_values[0] - 4.56).abs() < f64::EPSILON);

    drop(reader);
    drop(writer);
    cleanup_segment(seg_name);
}
