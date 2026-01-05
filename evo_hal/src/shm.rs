//! Shared memory layout and access for HAL.
//!
//! This module defines the SHM structures for communication between
//! HAL and the Control Unit.

use evo_common::hal::consts::{MAX_AI, MAX_AO, MAX_AXES, MAX_DI, MAX_DO};
use std::sync::atomic::AtomicU64;

/// Magic number for SHM validation: "EVO_HAL\0"
pub const SHM_MAGIC: u64 = 0x45564F5F48414C00;

/// SHM header structure (64 bytes).
#[repr(C, align(64))]
pub struct HalShmHeader {
    /// Magic number for validation
    pub magic: u64,
    /// Version counter (atomic, odd = write in progress)
    pub version: AtomicU64,
    /// Configured axis count
    pub axis_count: u32,
    /// Configured DI count
    pub di_count: u32,
    /// Configured DO count
    pub do_count: u32,
    /// Configured AI count
    pub ai_count: u32,
    /// Configured AO count
    pub ao_count: u32,
    /// Cycle time in microseconds
    pub cycle_time_us: u32,
    /// Reserved for future use
    _reserved: [u8; 24],
}

/// Per-axis command structure (written by Control Unit).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AxisShmCommand {
    /// Target position in user units
    pub target_position: f64,
    /// Enable axis
    pub enable: bool,
    /// Reset error
    pub reset: bool,
    /// Start referencing
    pub reference: bool,
    /// Reserved command flags
    _reserved: [u8; 5],
}

/// Per-axis status structure (written by HAL).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AxisShmStatus {
    /// Actual position in user units
    pub actual_position: f64,
    /// Actual velocity in user units/sec
    pub actual_velocity: f64,
    /// Current lag error
    pub lag_error: f64,
    /// Axis ready for motion
    pub ready: bool,
    /// Axis in error state
    pub error: bool,
    /// Axis is referenced
    pub referenced: bool,
    /// Referencing in progress
    pub referencing: bool,
    /// Axis is moving
    pub moving: bool,
    /// At target position
    pub in_position: bool,
    /// Error code (0 = no error)
    pub error_code: u16,
}

/// Per-axis SHM data (256 bytes).
#[repr(C)]
pub struct AxisShmData {
    /// Command section
    pub command: AxisShmCommand,
    /// Status section
    pub status: AxisShmStatus,
    /// Padding for 256-byte alignment
    _padding: [u8; 192],
}

/// Analog I/O SHM data (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AnalogShmData {
    /// Normalized value (0.0 - 1.0)
    pub normalized: f64,
    /// Scaled value in engineering units
    pub scaled: f64,
}

/// Main SHM data structure (~48KB).
#[repr(C, align(64))]
pub struct HalShmData {
    /// Header with version and metadata
    pub header: HalShmHeader,
    /// Axis data array (fixed size)
    pub axes: [AxisShmData; MAX_AXES],
    /// Digital inputs (bitfield)
    pub digital_inputs: [u8; MAX_DI / 8],
    /// Digital outputs (bitfield)
    pub digital_outputs: [u8; MAX_DO / 8],
    /// Analog inputs (dual representation)
    pub analog_inputs: [AnalogShmData; MAX_AI],
    /// Analog outputs (dual representation)
    pub analog_outputs: [AnalogShmData; MAX_AO],
}

impl HalShmData {
    /// Calculate the total size in bytes
    pub const fn size() -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Get digital input/output state from bitfield.
#[inline]
pub fn get_digital(buffer: &[u8], index: usize) -> bool {
    let byte_idx = index / 8;
    let bit_idx = index % 8;
    if byte_idx < buffer.len() {
        (buffer[byte_idx] >> bit_idx) & 1 == 1
    } else {
        false
    }
}

/// Set digital input/output state in bitfield.
#[inline]
pub fn set_digital(buffer: &mut [u8], index: usize, value: bool) {
    let byte_idx = index / 8;
    let bit_idx = index % 8;
    if byte_idx < buffer.len() {
        if value {
            buffer[byte_idx] |= 1 << bit_idx;
        } else {
            buffer[byte_idx] &= !(1 << bit_idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digital_bitfield() {
        let mut buffer = [0u8; 4];

        // Set bit 0
        set_digital(&mut buffer, 0, true);
        assert!(get_digital(&buffer, 0));
        assert!(!get_digital(&buffer, 1));

        // Set bit 7
        set_digital(&mut buffer, 7, true);
        assert!(get_digital(&buffer, 7));

        // Set bit 8 (second byte)
        set_digital(&mut buffer, 8, true);
        assert!(get_digital(&buffer, 8));

        // Clear bit 0
        set_digital(&mut buffer, 0, false);
        assert!(!get_digital(&buffer, 0));
    }

    #[test]
    fn test_shm_size() {
        // Verify SHM size is approximately 48KB as documented
        let size = HalShmData::size();
        assert!(size > 40_000 && size < 60_000, "SHM size: {}", size);
    }
}
