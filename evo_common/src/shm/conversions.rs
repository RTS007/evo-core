//! HAL ↔ SHM segment conversion functions.
//!
//! Converts between internal HAL types (`HalStatus`, `HalCommands`) and
//! their SHM segment representations (`HalToCuSegment`, `CuToHalSegment`).
//!
//! ## Conversion Direction
//!
//! - **HAL writes**: `HalStatus` → `HalToCuSegment` (pack feedback for CU).
//! - **HAL reads**: `CuToHalSegment` → `HalCommands` (unpack commands from CU).
//!
//! FR-035.

use crate::consts::{MAX_AI, MAX_AO, MAX_AXES};
use crate::hal::types::{
    AnalogValue, AxisCommand, AxisStatus, HalCommands, HalStatus,
};
use crate::shm::io_helpers::{pack_bools, unpack_bools};
use crate::shm::segments::{
    CuAxisCommand, CuToHalSegment, HalAxisFeedback, HalToCuSegment,
};

// ─── HalStatus → HalToCuSegment ────────────────────────────────────

/// Convert `HalStatus` into `HalToCuSegment` for SHM write.
///
/// - Axis feedback: position, velocity, torque (lag_error maps to torque
///   estimate), boolean flags packed as `u8`.
/// - DI bank: `[bool; 1024]` → `[u64; 16]` bit-packed.
/// - AI values: extracts `.scaled` from each `AnalogValue`.
///
/// # Arguments
///
/// - `status`: Current HAL status from driver cycle.
/// - `axis_count`: Number of active axes.
pub fn hal_status_to_segment(status: &HalStatus, axis_count: u8) -> HalToCuSegment {
    let mut seg = HalToCuSegment::default();
    seg.axis_count = axis_count;

    // Per-axis feedback.
    let count = (axis_count as usize).min(MAX_AXES);
    for i in 0..count {
        let src = &status.axes[i];
        seg.axes[i] = HalAxisFeedback {
            position: src.actual_position,
            velocity: src.actual_velocity,
            torque_estimate: src.lag_error, // Best available torque proxy.
            drive_ready: src.ready as u8,
            drive_fault: src.error as u8,
            referenced: src.referenced as u8,
            active: (src.ready || src.moving || src.referencing) as u8,
        };
    }

    // DI bank: bool[] → bit-packed u64[].
    pack_bools(&status.digital_inputs, &mut seg.di_bank);

    // AI values: extract .scaled field.
    for i in 0..MAX_AI {
        seg.ai_values[i] = status.analog_inputs[i].scaled;
    }

    seg
}

// ─── CuToHalSegment → HalCommands ──────────────────────────────────

/// Convert `CuToHalSegment` from SHM read into `HalCommands`.
///
/// - Axis commands: position, enable, brake (mapped to reset/reference defaults).
/// - DO bank: `[u64; 16]` → `[bool; 1024]`.
/// - AO values: direct copy.
///
/// # Arguments
///
/// - `seg`: Segment data read from SHM.
pub fn segment_to_hal_commands(seg: &CuToHalSegment) -> HalCommands {
    let mut cmds = HalCommands::default();

    // Per-axis commands.
    let count = (seg.axis_count as usize).min(MAX_AXES);
    for i in 0..count {
        let src = &seg.axes[i];
        cmds.axes[i] = AxisCommand {
            target_position: src.target_position,
            enable: src.enable != 0,
            reset: false,     // Not transmitted via SHM — CU uses state machine.
            reference: false, // Not transmitted via SHM — CU uses state machine.
        };
    }

    // DO bank: bit-packed u64[] → bool[].
    unpack_bools(&seg.do_bank, &mut cmds.digital_outputs);

    // AO values: direct copy.
    cmds.analog_outputs[..MAX_AO].copy_from_slice(&seg.ao_values[..MAX_AO]);

    cmds
}

// ─── HalCommands → CuToHalSegment (reverse — for tests) ────────────

/// Convert `HalCommands` into `CuToHalSegment` (utility for testing).
///
/// # Arguments
///
/// - `cmds`: HAL commands to pack.
/// - `axis_count`: Number of active axes.
pub fn hal_commands_to_segment(cmds: &HalCommands, axis_count: u8) -> CuToHalSegment {
    let mut seg = CuToHalSegment::default();
    seg.axis_count = axis_count;

    let count = (axis_count as usize).min(MAX_AXES);
    for i in 0..count {
        let src = &cmds.axes[i];
        seg.axes[i] = CuAxisCommand {
            target_position: src.target_position,
            target_velocity: 0.0, // Not in legacy AxisCommand.
            calculated_torque: 0.0,
            torque_offset: 0.0,
            enable: src.enable as u8,
            brake_release: 0,
        };
    }

    pack_bools(&cmds.digital_outputs, &mut seg.do_bank);
    seg.ao_values[..MAX_AO].copy_from_slice(&cmds.analog_outputs[..MAX_AO]);

    seg
}

// ─── HalToCuSegment → HalStatus (reverse — for tests) ──────────────

/// Convert `HalToCuSegment` back into `HalStatus` (utility for testing).
///
/// # Arguments
///
/// - `seg`: Segment data to unpack.
pub fn segment_to_hal_status(seg: &HalToCuSegment) -> HalStatus {
    let mut status = HalStatus::default();

    let count = (seg.axis_count as usize).min(MAX_AXES);
    for i in 0..count {
        let src = &seg.axes[i];
        status.axes[i] = AxisStatus {
            actual_position: src.position,
            actual_velocity: src.velocity,
            lag_error: src.torque_estimate,
            ready: src.drive_ready != 0,
            error: src.drive_fault != 0,
            referenced: src.referenced != 0,
            referencing: false,
            moving: false,
            in_position: false,
            error_code: 0,
        };
    }

    unpack_bools(&seg.di_bank, &mut status.digital_inputs);

    for i in 0..MAX_AI {
        status.analog_inputs[i] = AnalogValue {
            normalized: seg.ai_values[i],
            scaled: seg.ai_values[i],
        };
    }

    status
}

// ═══════════════════════════════════════════════════════════════════
//  Tests (T028 — conversion round-trips)
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shm::io_helpers::{get_di, set_do, BANK_WORDS};

    /// Build a test HalStatus with known values.
    fn make_test_status(axis_count: usize) -> HalStatus {
        let mut status = HalStatus::default();
        for i in 0..axis_count.min(MAX_AXES) {
            status.axes[i] = AxisStatus {
                actual_position: 100.0 + i as f64,
                actual_velocity: 10.0 + i as f64,
                lag_error: 0.01 * (i + 1) as f64,
                ready: i % 2 == 0,
                error: false,
                referenced: true,
                referencing: false,
                moving: i % 3 == 0,
                in_position: i % 2 == 1,
                error_code: 0,
            };
        }
        // Set some DIs.
        status.digital_inputs[0] = true;
        status.digital_inputs[7] = true;
        status.digital_inputs[64] = true;
        status.digital_inputs[1023] = true;

        // Set some AIs.
        status.analog_inputs[0] = AnalogValue {
            normalized: 0.5,
            scaled: 2.5,
        };
        status.analog_inputs[99] = AnalogValue {
            normalized: 0.8,
            scaled: 4.0,
        };

        status
    }

    #[test]
    fn hal_status_roundtrip() {
        let status = make_test_status(8);
        let seg = hal_status_to_segment(&status, 8);

        assert_eq!(seg.axis_count, 8);

        // Verify axis 0.
        assert_eq!(seg.axes[0].position, 100.0);
        assert_eq!(seg.axes[0].velocity, 10.0);
        assert!((seg.axes[0].torque_estimate - 0.01).abs() < 1e-10);
        assert_eq!(seg.axes[0].drive_ready, 1); // i=0, even → ready
        assert_eq!(seg.axes[0].drive_fault, 0);
        assert_eq!(seg.axes[0].referenced, 1);
        assert_eq!(seg.axes[0].active, 1); // ready=true

        // Verify axis 1.
        assert_eq!(seg.axes[1].position, 101.0);
        assert_eq!(seg.axes[1].drive_ready, 0); // i=1, odd → not ready

        // Verify DI bank.
        assert!(get_di(&seg.di_bank, 0));
        assert!(get_di(&seg.di_bank, 7));
        assert!(!get_di(&seg.di_bank, 8));
        assert!(get_di(&seg.di_bank, 64));
        assert!(get_di(&seg.di_bank, 1023));

        // Verify AI values.
        assert_eq!(seg.ai_values[0], 2.5); // .scaled
        assert_eq!(seg.ai_values[99], 4.0);
        assert_eq!(seg.ai_values[1], 0.0);

        // Round-trip back.
        let status2 = segment_to_hal_status(&seg);
        assert_eq!(status2.axes[0].actual_position, 100.0);
        assert_eq!(status2.axes[0].ready, true);
        assert_eq!(status2.digital_inputs[0], true);
        assert_eq!(status2.digital_inputs[7], true);
        assert_eq!(status2.digital_inputs[8], false);
        assert_eq!(status2.analog_inputs[0].scaled, 2.5);
    }

    #[test]
    fn hal_commands_roundtrip() {
        let mut cmds = HalCommands::default();
        cmds.axes[0].target_position = 42.0;
        cmds.axes[0].enable = true;
        cmds.axes[1].target_position = 99.0;
        cmds.axes[1].enable = false;
        cmds.digital_outputs[0] = true;
        cmds.digital_outputs[100] = true;
        cmds.digital_outputs[1023] = true;
        cmds.analog_outputs[0] = 3.14;
        cmds.analog_outputs[99] = 2.71;

        let seg = hal_commands_to_segment(&cmds, 4);
        assert_eq!(seg.axis_count, 4);
        assert_eq!(seg.axes[0].target_position, 42.0);
        assert_eq!(seg.axes[0].enable, 1);
        assert_eq!(seg.axes[1].target_position, 99.0);
        assert_eq!(seg.axes[1].enable, 0);

        // DO bank.
        assert!(get_di(&seg.do_bank, 0)); // reuse get_di for reading bits
        assert!(get_di(&seg.do_bank, 100));
        assert!(get_di(&seg.do_bank, 1023));
        assert!(!get_di(&seg.do_bank, 1));

        // AO values.
        assert_eq!(seg.ao_values[0], 3.14);
        assert_eq!(seg.ao_values[99], 2.71);

        // Round-trip back.
        let cmds2 = segment_to_hal_commands(&seg);
        assert_eq!(cmds2.axes[0].target_position, 42.0);
        assert_eq!(cmds2.axes[0].enable, true);
        assert_eq!(cmds2.axes[1].enable, false);
        assert_eq!(cmds2.digital_outputs[0], true);
        assert_eq!(cmds2.digital_outputs[100], true);
        assert_eq!(cmds2.digital_outputs[1023], true);
        assert_eq!(cmds2.digital_outputs[1], false);
        assert_eq!(cmds2.analog_outputs[0], 3.14);
        assert_eq!(cmds2.analog_outputs[99], 2.71);
    }

    #[test]
    fn empty_status_converts_cleanly() {
        let status = HalStatus::default();
        let seg = hal_status_to_segment(&status, 0);
        assert_eq!(seg.axis_count, 0);
        assert_eq!(seg.axes[0].position, 0.0);
        assert_eq!(seg.di_bank, [0u64; BANK_WORDS]);
        assert_eq!(seg.ai_values[0], 0.0);
    }

    #[test]
    fn empty_commands_converts_cleanly() {
        let seg = CuToHalSegment::default();
        let cmds = segment_to_hal_commands(&seg);
        assert_eq!(cmds.axes[0].target_position, 0.0);
        assert_eq!(cmds.axes[0].enable, false);
        assert_eq!(cmds.digital_outputs[0], false);
        assert_eq!(cmds.analog_outputs[0], 0.0);
    }

    #[test]
    fn di_packing_preserves_all_1024_bits() {
        let mut status = HalStatus::default();
        // Set every other bit.
        for i in (0..1024).step_by(2) {
            status.digital_inputs[i] = true;
        }

        let seg = hal_status_to_segment(&status, 1);

        for i in 0..1024 {
            assert_eq!(
                get_di(&seg.di_bank, i),
                i % 2 == 0,
                "DI pin {i} mismatch"
            );
        }
    }

    #[test]
    fn do_unpacking_preserves_all_1024_bits() {
        let mut seg = CuToHalSegment::default();
        // Set every third bit.
        for i in (0..1024).step_by(3) {
            set_do(&mut seg.do_bank, i, true);
        }
        seg.axis_count = 0;

        let cmds = segment_to_hal_commands(&seg);
        for i in 0..1024 {
            assert_eq!(
                cmds.digital_outputs[i],
                i % 3 == 0,
                "DO pin {i} mismatch"
            );
        }
    }

    #[test]
    fn max_axis_count_clamped() {
        let status = HalStatus::default();
        // axis_count > MAX_AXES should be clamped.
        let seg = hal_status_to_segment(&status, 255);
        assert_eq!(seg.axis_count, 255);
        // The conversion loop is bounded by MAX_AXES, so no panic.
    }
}
