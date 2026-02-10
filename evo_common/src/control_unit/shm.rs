//! SHM segment payload structs for the Control Unit (FR-131–FR-136).
//!
//! All payloads use `#[repr(C, align(64))]` for zero-copy binary access.
//! Struct version hashes validated at startup via `struct_version_hash<T>()`.

use static_assertions::const_assert_eq;

use super::control::ControlOutputVector;
use super::state::OperationalMode;
use crate::shm::p2p::P2pSegmentHeader;

// ─── §1: evo_hal_cu — HAL → Control Unit (FR-131) ──────────────────

/// Real-time axis feedback and I/O state from HAL to CU.
///
/// Written by `evo_hal` every cycle (1 ms). Read by `evo_control_unit`.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct HalToCuSegment {
    /// P2P segment header (64 bytes).
    pub header: P2pSegmentHeader,
    /// Active axis count (1..=64).
    pub axis_count: u8,
    /// Padding to align axes array.
    pub _pad: [u8; 63],
    /// Per-axis feedback data.
    pub axes: [HalAxisFeedback; 64],
    /// Digital input bank: 1024 DI bits (128 bytes).
    pub di_bank: [u64; 16],
    /// Analog input values (64 channels).
    pub ai_values: [f64; 64],
}

/// Per-axis feedback from HAL (24 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HalAxisFeedback {
    /// Actual position from encoder [mm].
    pub actual_position: f64,
    /// Actual velocity from encoder [mm/s].
    pub actual_velocity: f64,
    /// Drive status bitfield: bit0=ready, bit1=fault, bit2=enabled, bit3=referenced, bit4=zerospeed.
    pub drive_status: u8,
    /// Drive-specific fault code.
    pub fault_code: u16,
    /// Padding to 24 bytes.
    pub _padding: [u8; 4],
}

const_assert_eq!(core::mem::size_of::<HalAxisFeedback>(), 24);

impl Default for HalAxisFeedback {
    fn default() -> Self {
        Self {
            actual_position: 0.0,
            actual_velocity: 0.0,
            drive_status: 0,
            fault_code: 0,
            _padding: [0u8; 4],
        }
    }
}

impl HalAxisFeedback {
    // drive_status bitfield accessors
    #[inline]
    pub const fn is_ready(&self) -> bool {
        (self.drive_status & 0x01) != 0
    }
    #[inline]
    pub const fn is_fault(&self) -> bool {
        (self.drive_status & 0x02) != 0
    }
    #[inline]
    pub const fn is_enabled(&self) -> bool {
        (self.drive_status & 0x04) != 0
    }
    #[inline]
    pub const fn is_referenced(&self) -> bool {
        (self.drive_status & 0x08) != 0
    }
    #[inline]
    pub const fn is_zerospeed(&self) -> bool {
        (self.drive_status & 0x10) != 0
    }
}

// ─── §2: evo_cu_hal — Control Unit → HAL (FR-132) ──────────────────

/// Control commands from CU to HAL drives.
///
/// Written by `evo_control_unit` every cycle (1 ms). Read by `evo_hal`.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToHalSegment {
    /// P2P segment header (64 bytes).
    pub header: P2pSegmentHeader,
    /// Active axis count.
    pub axis_count: u8,
    /// Padding to align axes array.
    pub _pad: [u8; 63],
    /// Per-axis control commands.
    pub axes: [CuAxisCommand; 64],
    /// Digital output bank: 1024 DO bits (128 bytes).
    pub do_bank: [u64; 16],
    /// Analog output values (64 channels).
    pub ao_values: [f64; 64],
}

/// Per-axis command to HAL (40 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CuAxisCommand {
    /// Control output vector (4 × f64 = 32 bytes).
    pub output: ControlOutputVector,
    /// 0 = disable, 1 = enable.
    pub enable: u8,
    /// Current operational mode (OperationalMode as u8).
    pub mode: u8,
    /// Padding to 40 bytes.
    pub _pad: [u8; 6],
}

const_assert_eq!(core::mem::size_of::<CuAxisCommand>(), 40);

impl Default for CuAxisCommand {
    fn default() -> Self {
        Self {
            output: ControlOutputVector::default(),
            enable: 0,
            mode: OperationalMode::Position as u8,
            _pad: [0u8; 6],
        }
    }
}

// ─── §3: evo_re_cu — Recipe Executor → Control Unit (FR-133) ───────

/// Recipe commands and motion targets from RE to CU.
///
/// Written by `evo_recipe_executor` asynchronously. Read by `evo_control_unit`.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct ReToCuSegment {
    /// P2P segment header (64 bytes).
    pub header: P2pSegmentHeader,
    /// Current command.
    pub command: ReCommand,
}

/// Recipe command.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ReCommand {
    /// Command type.
    pub command_type: u8,
    /// Padding for alignment.
    pub _pad0: [u8; 7],
    /// Bit mask: bit N = axis (N+1) is targeted.
    pub axis_mask: u64,
    /// Per-axis motion targets.
    pub targets: [ReAxisTarget; 64],
    /// Monotonic sequence ID for ack tracking.
    pub sequence_id: u32,
    /// Padding.
    pub _pad1: [u8; 4],
}

impl Default for ReCommand {
    fn default() -> Self {
        Self {
            command_type: ReCommandType::Nop as u8,
            _pad0: [0u8; 7],
            axis_mask: 0,
            targets: [ReAxisTarget::default(); 64],
            sequence_id: 0,
            _pad1: [0u8; 4],
        }
    }
}

/// RE command types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReCommandType {
    /// No active command.
    Nop = 0,
    MoveAbsolute = 1,
    MoveRelative = 2,
    MoveVelocity = 3,
    Home = 4,
    Stop = 5,
    EmergencyStop = 6,
    EnableAxis = 7,
    DisableAxis = 8,
    SetMode = 9,
    Couple = 10,
    Decouple = 11,
    GearChange = 12,
    /// Authorize manual mode for axes in axis_mask (FR-004).
    AllowManualMode = 13,
}

impl ReCommandType {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Nop),
            1 => Some(Self::MoveAbsolute),
            2 => Some(Self::MoveRelative),
            3 => Some(Self::MoveVelocity),
            4 => Some(Self::Home),
            5 => Some(Self::Stop),
            6 => Some(Self::EmergencyStop),
            7 => Some(Self::EnableAxis),
            8 => Some(Self::DisableAxis),
            9 => Some(Self::SetMode),
            10 => Some(Self::Couple),
            11 => Some(Self::Decouple),
            12 => Some(Self::GearChange),
            13 => Some(Self::AllowManualMode),
            _ => None,
        }
    }
}

/// Per-axis motion target from RE (40 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ReAxisTarget {
    /// Target position [mm].
    pub target_position: f64,
    /// Maximum velocity [mm/s].
    pub target_velocity: f64,
    /// Acceleration [mm/s²].
    pub acceleration: f64,
    /// Deceleration [mm/s²].
    pub deceleration: f64,
    /// Operational mode.
    pub mode: u8,
    /// Padding.
    pub _pad: [u8; 7],
}

const_assert_eq!(core::mem::size_of::<ReAxisTarget>(), 40);

impl Default for ReAxisTarget {
    fn default() -> Self {
        Self {
            target_position: 0.0,
            target_velocity: 0.0,
            acceleration: 0.0,
            deceleration: 0.0,
            mode: OperationalMode::Position as u8,
            _pad: [0u8; 7],
        }
    }
}

// ─── §4: evo_cu_mqt — Control Unit → MQTT Bridge (FR-134) ──────────

/// Diagnostic state snapshot for monitoring, logging, and dashboard.
///
/// Written by `evo_control_unit` every N cycles (default: 10 = 10ms).
/// Read by `evo_mqtt`.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToMqtSegment {
    /// P2P segment header (64 bytes).
    pub header: P2pSegmentHeader,
    /// Current MachineState (as u8).
    pub machine_state: u8,
    /// Current SafetyState (as u8).
    pub safety_state: u8,
    /// Active axis count.
    pub axis_count: u8,
    /// Padding.
    pub _pad: [u8; 61],
    /// Per-axis state snapshots.
    pub axes: [AxisStateSnapshot; 64],
}

/// Per-axis diagnostic snapshot (56 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AxisStateSnapshot {
    /// Axis ID (1-based).
    pub axis_id: u8,
    /// PowerState.
    pub power: u8,
    /// MotionState.
    pub motion: u8,
    /// OperationalMode.
    pub operational: u8,
    /// CouplingState.
    pub coupling: u8,
    /// GearboxState.
    pub gearbox: u8,
    /// LoadingState.
    pub loading: u8,
    /// CommandSource (who holds the lock).
    pub locked_by: u8,
    /// Packed AxisSafetyState (8 booleans).
    pub safety_flags: u8,
    /// PowerError bitflags.
    pub error_power: u16,
    /// MotionError bitflags.
    pub error_motion: u16,
    /// CommandError bitflags.
    pub error_command: u8,
    /// GearboxError bitflags.
    pub error_gearbox: u8,
    /// CouplingError bitflags.
    pub error_coupling: u8,
    /// Padding to align f64 fields.
    pub _pad: [u8; 5],
    /// Actual position [mm].
    pub position: f64,
    /// Actual velocity [mm/s].
    pub velocity: f64,
    /// Lag error [mm] (|target - actual|).
    pub lag: f64,
    /// Current torque output [Nm].
    pub torque: f64,
}

const_assert_eq!(core::mem::size_of::<AxisStateSnapshot>(), 56);

impl Default for AxisStateSnapshot {
    fn default() -> Self {
        Self {
            axis_id: 0,
            power: 0,
            motion: 0,
            operational: 0,
            coupling: 0,
            gearbox: 0,
            loading: 0,
            locked_by: 0,
            safety_flags: 0xFF, // all OK by default
            error_power: 0,
            error_motion: 0,
            error_command: 0,
            error_gearbox: 0,
            error_coupling: 0,
            _pad: [0u8; 5],
            position: 0.0,
            velocity: 0.0,
            lag: 0.0,
            torque: 0.0,
        }
    }
}

// ─── §5: evo_rpc_cu — gRPC API → Control Unit (FR-132c) ────────────

/// Manual commands from dashboard/API.
///
/// Written by `evo_grpc` asynchronously. Read by `evo_control_unit`.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct RpcToCuSegment {
    /// P2P segment header (64 bytes).
    pub header: P2pSegmentHeader,
    /// Current command.
    pub command: RpcCommand,
}

/// RPC command.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RpcCommand {
    /// Command type.
    pub command_type: u8,
    /// Target axis ID (0 = global command).
    pub axis_id: u8,
    /// Padding.
    pub _pad: [u8; 6],
    /// Command-specific float parameter.
    pub param_f64: f64,
    /// Command-specific integer parameter.
    pub param_u32: u32,
    /// Monotonic sequence ID for ack tracking.
    pub sequence_id: u32,
}

impl Default for RpcCommand {
    fn default() -> Self {
        Self {
            command_type: RpcCommandType::Nop as u8,
            axis_id: 0,
            _pad: [0u8; 6],
            param_f64: 0.0,
            param_u32: 0,
            sequence_id: 0,
        }
    }
}

/// RPC command types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RpcCommandType {
    Nop = 0,
    JogPositive = 1,
    JogNegative = 2,
    JogStop = 3,
    MoveAbsolute = 4,
    EnableAxis = 5,
    DisableAxis = 6,
    HomeAxis = 7,
    ResetError = 8,
    /// param_u32 = target MachineState.
    SetMachineState = 9,
    SetMode = 10,
    GearChange = 11,
    /// Request source lock.
    AcquireLock = 12,
    ReleaseLock = 13,
    /// Authorize manual mode for axis_id (FR-004).
    AllowManualMode = 14,
    /// Hot-reload config during SAFETY_STOP (FR-145).
    ReloadConfig = 15,
}

impl RpcCommandType {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Nop),
            1 => Some(Self::JogPositive),
            2 => Some(Self::JogNegative),
            3 => Some(Self::JogStop),
            4 => Some(Self::MoveAbsolute),
            5 => Some(Self::EnableAxis),
            6 => Some(Self::DisableAxis),
            7 => Some(Self::HomeAxis),
            8 => Some(Self::ResetError),
            9 => Some(Self::SetMachineState),
            10 => Some(Self::SetMode),
            11 => Some(Self::GearChange),
            12 => Some(Self::AcquireLock),
            13 => Some(Self::ReleaseLock),
            14 => Some(Self::AllowManualMode),
            15 => Some(Self::ReloadConfig),
            _ => None,
        }
    }
}

// ─── §6: evo_cu_re — Control Unit → Recipe Executor (FR-134a) ──────

/// Command acknowledgments and axis status for recipe progression.
///
/// DRAFT / PLACEHOLDER — finalized when Recipe Executor is implemented.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToReSegment {
    /// P2P segment header (64 bytes).
    pub header: P2pSegmentHeader,
    /// Last completed sequence_id.
    pub last_ack_seq_id: u32,
    /// 0=ok, 1=rejected, 2=error.
    pub ack_status: u8,
    /// Padding.
    pub _pad: [u8; 3],
    /// Bit per axis: axis is in position (bit 0 = axis 1).
    pub axes_in_position: u64,
    /// Bit per axis: axis has error (bit 0 = axis 1).
    pub axes_in_error: u64,
}

impl Default for CuToReSegment {
    fn default() -> Self {
        Self {
            header: P2pSegmentHeader::new(
                crate::shm::p2p::ModuleAbbrev::Cu,
                crate::shm::p2p::ModuleAbbrev::Re,
                0,
                0,
            ),
            last_ack_seq_id: 0,
            ack_status: 0,
            _pad: [0u8; 3],
            axes_in_position: 0,
            axes_in_error: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hal_axis_feedback_size() {
        assert_eq!(core::mem::size_of::<HalAxisFeedback>(), 24);
    }

    #[test]
    fn cu_axis_command_size() {
        assert_eq!(core::mem::size_of::<CuAxisCommand>(), 40);
    }

    #[test]
    fn axis_state_snapshot_size() {
        assert_eq!(core::mem::size_of::<AxisStateSnapshot>(), 56);
    }

    #[test]
    fn control_output_vector_size() {
        assert_eq!(core::mem::size_of::<ControlOutputVector>(), 32);
    }

    #[test]
    fn re_axis_target_size() {
        assert_eq!(core::mem::size_of::<ReAxisTarget>(), 40);
    }

    #[test]
    fn re_command_type_roundtrip() {
        for v in 0..=13u8 {
            let cmd = ReCommandType::from_u8(v).unwrap();
            assert_eq!(cmd as u8, v);
        }
        assert!(ReCommandType::from_u8(14).is_none());
    }

    #[test]
    fn rpc_command_type_roundtrip() {
        for v in 0..=15u8 {
            let cmd = RpcCommandType::from_u8(v).unwrap();
            assert_eq!(cmd as u8, v);
        }
        assert!(RpcCommandType::from_u8(16).is_none());
    }

    #[test]
    fn drive_status_bitfield() {
        let f = HalAxisFeedback {
            drive_status: 0b10111,
            ..Default::default()
        };
        assert!(f.is_ready());
        assert!(f.is_fault());
        assert!(f.is_enabled());
        assert!(!f.is_referenced());
        assert!(f.is_zerospeed());
    }

    #[test]
    fn p2p_header_size() {
        assert_eq!(core::mem::size_of::<P2pSegmentHeader>(), 64);
        assert_eq!(core::mem::align_of::<P2pSegmentHeader>(), 64);
    }

    #[test]
    fn version_hash_deterministic() {
        let h1 = crate::shm::p2p::struct_version_hash::<HalToCuSegment>();
        let h2 = crate::shm::p2p::struct_version_hash::<HalToCuSegment>();
        assert_eq!(h1, h2);

        // Different types should have different hashes
        let h3 = crate::shm::p2p::struct_version_hash::<CuToHalSegment>();
        assert_ne!(h1, h3);
    }

    #[test]
    fn segment_alignment() {
        assert_eq!(core::mem::align_of::<HalToCuSegment>(), 64);
        assert_eq!(core::mem::align_of::<CuToHalSegment>(), 64);
        assert_eq!(core::mem::align_of::<ReToCuSegment>(), 64);
        assert_eq!(core::mem::align_of::<CuToMqtSegment>(), 64);
        assert_eq!(core::mem::align_of::<RpcToCuSegment>(), 64);
        assert_eq!(core::mem::align_of::<CuToReSegment>(), 64);
    }

    // ── T035b: Top-level segment size assertions ──

    #[test]
    fn top_level_segment_sizes() {
        // HalToCuSegment: header(64) + axis_count(1) + _pad(63) + axes(24*64) + di_bank(128) + ai_values(512) = 2304
        assert_eq!(core::mem::size_of::<HalToCuSegment>(), 2304);
        // CuToHalSegment: header(64) + axis_count(1) + _pad(63) + axes(40*64) + do_bank(128) + ao_values(512) = 3328
        assert_eq!(core::mem::size_of::<CuToHalSegment>(), 3328);
        // CuToMqtSegment: header(64) + 3 u8s + _pad(61) + axes(56*64) = 3712
        assert_eq!(core::mem::size_of::<CuToMqtSegment>(), 3712);
        // RpcToCuSegment: header(64) + RpcCommand(24) = 88
        assert_eq!(core::mem::size_of::<RpcToCuSegment>(), 128);
        // CuToReSegment: header(64) + fields(24) = 88
        assert_eq!(core::mem::size_of::<CuToReSegment>(), 128);
    }

    #[test]
    fn inner_struct_sizes() {
        assert_eq!(core::mem::size_of::<ReCommand>(), 2584);
        assert_eq!(core::mem::size_of::<RpcCommand>(), 24);
    }

    #[test]
    fn segment_sizes_are_cache_line_multiples() {
        // All top-level segments should be multiples of 64 (cache line).
        for size in [
            core::mem::size_of::<HalToCuSegment>(),
            core::mem::size_of::<CuToHalSegment>(),
            core::mem::size_of::<ReToCuSegment>(),
            core::mem::size_of::<CuToMqtSegment>(),
            core::mem::size_of::<RpcToCuSegment>(),
            core::mem::size_of::<CuToReSegment>(),
        ] {
            assert_eq!(size % 64, 0, "segment size {size} is not a cache-line multiple");
        }
    }

    #[test]
    fn version_hash_distinct_segments() {
        // Distinct-sized segments must produce distinct hashes.
        use crate::shm::p2p::struct_version_hash;
        let hashes = [
            struct_version_hash::<HalToCuSegment>(),
            struct_version_hash::<CuToHalSegment>(),
            struct_version_hash::<ReToCuSegment>(),
            struct_version_hash::<CuToMqtSegment>(),
            // RpcToCuSegment and CuToReSegment may collide (same size)
            // — disambiguated by source/dest module fields.
        ];
        for (i, h1) in hashes.iter().enumerate() {
            for (j, h2) in hashes.iter().enumerate() {
                if i != j {
                    assert_ne!(h1, h2, "hash collision between segment {i} and {j}");
                }
            }
        }
    }

    #[test]
    fn default_initialisation_zeroed() {
        let snap = AxisStateSnapshot::default();
        assert_eq!(snap.axis_id, 0);
        assert_eq!(snap.position, 0.0);
        assert_eq!(snap.velocity, 0.0);
        assert_eq!(snap.lag, 0.0);
        assert_eq!(snap.torque, 0.0);

        let cmd = CuAxisCommand::default();
        assert_eq!(cmd.enable, 0);

        let fb = HalAxisFeedback::default();
        assert_eq!(fb.actual_position, 0.0);
        assert_eq!(fb.drive_status, 0);
    }
}
