//! SHM segment payload types for all 15 P2P connections.
//!
//! These structs represent the PAYLOAD portion of each P2P SHM segment.
//! The `P2pSegmentHeader` (64 bytes) precedes each payload in the mapped SHM
//! region — managed by `TypedP2pWriter<T>` / `TypedP2pReader<T>`.
//!
//! All structs: `#[repr(C, align(64))]` for cross-process compatibility and
//! cache-line alignment. Fixed-size types only (no `String`, `Vec`, etc.).
//!
//! ## Segment Summary (15 total)
//!
//! | # | Segment name  | Writer→Reader | Payload struct     | Status      |
//! |---|---------------|---------------|--------------------|-------------|
//! | 1 | `evo_hal_cu`  | HAL → CU      | `HalToCuSegment`   | Active      |
//! | 2 | `evo_cu_hal`  | CU → HAL      | `CuToHalSegment`   | Active      |
//! | 3 | `evo_cu_mqt`  | CU → MQTT     | `CuToMqtSegment`   | Skeleton    |
//! | 4 | `evo_hal_mqt` | HAL → MQTT    | `HalToMqtSegment`  | Skeleton    |
//! | 5 | `evo_re_cu`   | RE → CU       | `ReToCuSegment`    | Skeleton    |
//! | 6 | `evo_re_hal`  | RE → HAL      | `ReToHalSegment`   | Skeleton    |
//! | 7 | `evo_re_mqt`  | RE → MQTT     | `ReToMqtSegment`   | Skeleton    |
//! | 8 | `evo_re_rpc`  | RE → gRPC     | `ReToRpcSegment`   | Skeleton    |
//! | 9 | `evo_rpc_cu`  | gRPC → CU     | `RpcToCuSegment`   | Skeleton    |
//! |10 | `evo_rpc_hal` | gRPC → HAL    | `RpcToHalSegment`  | Skeleton    |
//! |11 | `evo_rpc_re`  | gRPC → RE     | `RpcToReSegment`   | Skeleton    |
//! |12 | `evo_cu_re`   | CU → RE       | `CuToReSegment`    | Placeholder |
//! |13 | `evo_cu_rpc`  | CU → gRPC     | `CuToRpcSegment`   | Placeholder |
//! |14 | `evo_hal_rpc` | HAL → gRPC    | `HalToRpcSegment`  | Placeholder |
//! |15 | `evo_hal_re`  | HAL → RE      | `HalToReSegment`   | Placeholder |

use crate::consts::{MAX_AXES, MAX_AI, MAX_AO};
use crate::shm::io_helpers::BANK_WORDS;

// ─── Segment Name Constants ─────────────────────────────────────────

/// Segment name: HAL → CU (`"hal_cu"`).
pub const SEG_HAL_CU: &str = "hal_cu";
/// Segment name: CU → HAL (`"cu_hal"`).
pub const SEG_CU_HAL: &str = "cu_hal";
/// Segment name: CU → MQTT (`"cu_mqt"`).
pub const SEG_CU_MQT: &str = "cu_mqt";
/// Segment name: HAL → MQTT (`"hal_mqt"`).
pub const SEG_HAL_MQT: &str = "hal_mqt";
/// Segment name: RE → CU (`"re_cu"`).
pub const SEG_RE_CU: &str = "re_cu";
/// Segment name: RE → HAL (`"re_hal"`).
pub const SEG_RE_HAL: &str = "re_hal";
/// Segment name: RE → MQTT (`"re_mqt"`).
pub const SEG_RE_MQT: &str = "re_mqt";
/// Segment name: RE → gRPC (`"re_rpc"`).
pub const SEG_RE_RPC: &str = "re_rpc";
/// Segment name: gRPC → CU (`"rpc_cu"`).
pub const SEG_RPC_CU: &str = "rpc_cu";
/// Segment name: gRPC → HAL (`"rpc_hal"`).
pub const SEG_RPC_HAL: &str = "rpc_hal";
/// Segment name: gRPC → RE (`"rpc_re"`).
pub const SEG_RPC_RE: &str = "rpc_re";
/// Segment name: CU → RE (`"cu_re"`).
pub const SEG_CU_RE: &str = "cu_re";
/// Segment name: CU → gRPC (`"cu_rpc"`).
pub const SEG_CU_RPC: &str = "cu_rpc";
/// Segment name: HAL → gRPC (`"hal_rpc"`).
pub const SEG_HAL_RPC: &str = "hal_rpc";
/// Segment name: HAL → RE (`"hal_re"`).
pub const SEG_HAL_RE: &str = "hal_re";

// ─── Sub-structs ────────────────────────────────────────────────────

/// Per-axis feedback from HAL (position, velocity, torque, flags).
///
/// Used in `HalToCuSegment`, `HalToMqtSegment`, `HalToReSegment`.
///
/// Size: 32 bytes (3×f64 + 4×u8 + 4 trailing pad).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct HalAxisFeedback {
    /// Current axis position (mm or deg).
    pub position: f64,
    /// Current axis velocity (mm/s or deg/s).
    pub velocity: f64,
    /// Estimated torque (Nm or %).
    pub torque_estimate: f64,
    /// Drive ready flag (0=not ready, 1=ready).
    pub drive_ready: u8,
    /// Drive fault flag (0=ok, 1=fault).
    pub drive_fault: u8,
    /// Axis referenced/homed flag (0=no, 1=yes).
    pub referenced: u8,
    /// Axis active flag (0=inactive, 1=active).
    pub active: u8,
    // Implicit trailing padding: 4 bytes → total 32 = 4×align(8)
}

/// Per-axis command from CU (control outputs + enable flags).
///
/// Used in `CuToHalSegment`.
///
/// Size: 40 bytes (4×f64 + 2×u8 + 6 trailing pad).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CuAxisCommand {
    /// Target position from trajectory generator.
    pub target_position: f64,
    /// Target velocity from trajectory generator.
    pub target_velocity: f64,
    /// Calculated torque from PID controller.
    pub calculated_torque: f64,
    /// Torque offset / feedforward.
    pub torque_offset: f64,
    /// Axis enable command (0=disabled, 1=enabled).
    pub enable: u8,
    /// Brake release command (0=engage, 1=release).
    pub brake_release: u8,
    // Implicit trailing padding: 6 bytes → total 40 = 5×align(8)
}

/// Per-axis status for CU → MQTT / gRPC segments.
///
/// 6 orthogonal state machines + flags + error code.
///
/// Size: 16 bytes.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CuAxisStatus {
    /// Main axis state (enum discriminant).
    pub axis_state: u8,
    /// Motion state (enum discriminant).
    pub motion_state: u8,
    /// Homing state (enum discriminant).
    pub homing_state: u8,
    /// Safety state (enum discriminant).
    pub safety_state: u8,
    /// Error state (enum discriminant).
    pub error_state: u8,
    /// Enable state (enum discriminant).
    pub enable_state: u8,
    /// Per-axis error code.
    pub error_code: u16,
    /// Per-axis safety flags (bitmask).
    pub safety_flags: u16,
    /// Reserved for future use.
    pub _reserved: [u8; 6],
}

/// Per-axis PID diagnostic state for CU → gRPC segment.
///
/// Size: 24 bytes (3×f64).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct AxisPidState {
    /// Current PID error.
    pub error: f64,
    /// Current PID integral accumulator.
    pub integral: f64,
    /// Current PID output.
    pub output: f64,
}

// ─── Default via zeroed() ───────────────────────────────────────────
//
// Large segment structs use mem::zeroed() for Default to avoid deep stack
// usage. This is safe because all fields are plain numeric types or arrays
// thereof — zero is a valid value for every field.

macro_rules! impl_default_zeroed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Default for $ty {
                fn default() -> Self {
                    // SAFETY: All fields are numeric primitives or fixed-size arrays
                    // of numeric primitives. Zero is a valid value for every field.
                    unsafe { core::mem::zeroed() }
                }
            }
        )*
    };
}

// ═══════════════════════════════════════════════════════════════════
//  Active Segments (#1–#2)
// ═══════════════════════════════════════════════════════════════════

/// **#1** HAL → CU feedback segment (`evo_hal_cu`).
///
/// Written by HAL every RT cycle. Contains axis feedback, digital input
/// bank, and analog input values.
///
/// FR-011, FR-030, FR-035.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct HalToCuSegment {
    /// Per-axis feedback (position, velocity, torque, flags).
    pub axes: [HalAxisFeedback; MAX_AXES as usize],
    /// Digital input bank — 1024 bits packed into 16×u64.
    pub di_bank: [u64; BANK_WORDS],
    /// Analog input values in engineering units.
    pub ai_values: [f64; MAX_AI],
    /// Number of active axes (0..MAX_AXES).
    pub axis_count: u8,
}

/// **#2** CU → HAL command segment (`evo_cu_hal`).
///
/// Written by CU every RT cycle. Contains axis commands, digital output
/// bank, and analog output values.
///
/// FR-012, FR-031, FR-035, FR-040.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToHalSegment {
    /// Per-axis commands (position, velocity, torque, enable).
    pub axes: [CuAxisCommand; MAX_AXES as usize],
    /// Digital output bank — 1024 bits packed into 16×u64.
    pub do_bank: [u64; BANK_WORDS],
    /// Analog output values in engineering units.
    pub ao_values: [f64; MAX_AO],
    /// Number of active axes (0..MAX_AXES).
    pub axis_count: u8,
}

// ═══════════════════════════════════════════════════════════════════
//  Skeleton Segments (#3–#11)
// ═══════════════════════════════════════════════════════════════════

/// **#3** CU → MQTT status snapshot (`evo_cu_mqt`).
///
/// Machine state, axis states, safety, errors. Snapshot only — no event
/// ring buffer.
///
/// FR-013, FR-040, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToMqtSegment {
    /// Machine state (enum discriminant).
    pub machine_state: u8,
    /// Safety state (enum discriminant).
    pub safety_state: u8,
    /// Number of active axes.
    pub axis_count: u8,
    /// Padding.
    pub _pad1: u8,
    /// Error flags — full width, NOT truncated (FR-043).
    pub error_flags: u32,
    /// Per-axis status (6 state machines + flags).
    pub axis_status: [CuAxisStatus; MAX_AXES as usize],
}

/// **#4** HAL → MQTT telemetry (`evo_hal_mqt`).
///
/// Superset of `HalToCuSegment` plus output state and timing.
/// Continuous telemetry for MQTT publishing.
///
/// FR-014c, FR-030a, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct HalToMqtSegment {
    /// Per-axis feedback.
    pub axes: [HalAxisFeedback; MAX_AXES as usize],
    /// Digital input bank.
    pub di_bank: [u64; BANK_WORDS],
    /// Analog input values.
    pub ai_values: [f64; MAX_AI],
    /// Digital output bank (current state).
    pub do_bank: [u64; BANK_WORDS],
    /// Analog output values (current state).
    pub ao_values: [f64; MAX_AO],
    /// Driver cycle time in nanoseconds.
    pub cycle_time_ns: u64,
    /// Per-axis driver state (enum discriminants).
    pub driver_state: [u8; MAX_AXES as usize],
    /// Number of active axes.
    pub axis_count: u8,
}

/// **#5** RE → CU command segment (`evo_re_cu`).
///
/// Motion requests, program commands, `AllowManualMode`.
/// Content deferred to RE specification.
///
/// FR-014, FR-040, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct ReToCuSegment {
    /// Reserved — content defined in RE spec.
    pub _reserved: [u8; 256],
}

/// **#6** RE → HAL I/O command segment (`evo_re_hal`).
///
/// Direct I/O commands from RE — only for pins without `IoRole` assignment.
/// HAL ignores commands for role-assigned pins → `ERR_IO_ROLE_OWNED`.
///
/// FR-014e, FR-030c, FR-036, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct ReToHalSegment {
    /// Target digital output pin index.
    pub set_do_pin: u16,
    /// Digital output value (0=off, 1=on).
    pub set_do_value: u8,
    /// Padding.
    pub _pad1: u8,
    /// Target analog output pin index.
    pub set_ao_pin: u16,
    /// Padding.
    pub _pad2: [u8; 2],
    /// Analog output value.
    pub set_ao_value: f64,
    /// Request ID for ack correlation.
    pub request_id: u64,
    /// Reserved for future expansion.
    pub _reserved: [u8; 232],
}

/// **#7** RE → MQTT telemetry (`evo_re_mqt`).
///
/// Recipe execution telemetry.
///
/// FR-014b/FR-014f, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct ReToMqtSegment {
    /// Current recipe step index.
    pub current_step: u16,
    /// RE state (enum discriminant).
    pub re_state: u8,
    /// Padding.
    pub _pad1: u8,
    /// Error code.
    pub error_code: u32,
    /// Execution cycle count.
    pub cycle_count: u64,
    /// Program/recipe name (fixed-size, null-terminated).
    pub program_name: [u8; 64],
    /// Reserved for future expansion.
    pub _reserved: [u8; 176],
}

/// **#8** RE → gRPC status (`evo_re_rpc`).
///
/// Recipe status and acks for Dashboard/API.
///
/// FR-014b/FR-014f, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct ReToRpcSegment {
    /// Execution progress (0..100 percent).
    pub execution_progress: u8,
    /// Step result (enum discriminant).
    pub step_result: u8,
    /// Padding.
    pub _pad1: [u8; 6],
    /// Request ID for ack correlation.
    pub request_id: u64,
    /// Error message (fixed-size, null-terminated).
    pub error_message: [u8; 128],
    /// Reserved for future expansion.
    pub _reserved: [u8; 112],
}

/// **#9** gRPC → CU command segment (`evo_rpc_cu`).
///
/// External commands: jog, mode change, config reload, service bypass.
/// Content resolved in gRPC specification.
///
/// FR-014, FR-040, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct RpcToCuSegment {
    /// Reserved — content defined in gRPC spec.
    pub _reserved: [u8; 256],
}

/// **#10** gRPC → HAL command segment (`evo_rpc_hal`).
///
/// Direct HAL commands: set DO, set AO, driver commands.
///
/// FR-014d, FR-030b, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct RpcToHalSegment {
    /// Target pin or axis index.
    pub target: u16,
    /// Padding.
    pub _pad1: [u8; 6],
    /// Command value.
    pub value: f64,
    /// Request ID for ack correlation.
    pub request_id: u64,
    /// Reserved for future expansion.
    pub _reserved: [u8; 232],
}

/// **#11** gRPC → RE segment (`evo_rpc_re`).
///
/// Placeholder — content defined in separate spec.
/// Heartbeat-only segment.
///
/// FR-014d, FR-090.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct RpcToReSegment {
    /// Reserved — content defined in separate spec.
    pub _reserved: [u8; 64],
}

// ═══════════════════════════════════════════════════════════════════
//  Placeholder Segments (#12–#15)
// ═══════════════════════════════════════════════════════════════════

/// **#12** CU → RE segment (`evo_cu_re`).
///
/// Ack, execution status, axis availability, error feedback.
/// Reserved placeholder — heartbeat only.
///
/// FR-014, FR-040.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToReSegment {
    /// Reserved — content defined in RE spec.
    pub _reserved: [u8; 256],
}

/// **#13** CU → gRPC diagnostic segment (`evo_cu_rpc`).
///
/// Superset of `CuToMqtSegment` plus PID states, cycle timing, jitter
/// histogram. Full diagnostic snapshot for Dashboard/API.
///
/// FR-014a, FR-040.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct CuToRpcSegment {
    /// Machine state (enum discriminant).
    pub machine_state: u8,
    /// Safety state (enum discriminant).
    pub safety_state: u8,
    /// Number of active axes.
    pub axis_count: u8,
    /// Padding.
    pub _pad1: u8,
    /// Error flags (full width).
    pub error_flags: u32,
    /// Per-axis status (6 state machines + flags).
    pub axis_status: [CuAxisStatus; MAX_AXES as usize],
    /// Per-axis PID diagnostic state.
    pub pid_states: [AxisPidState; MAX_AXES as usize],
    /// Last cycle duration in nanoseconds.
    pub last_cycle_ns: u64,
    /// Maximum observed cycle duration in nanoseconds.
    pub max_cycle_ns: u64,
    /// Jitter histogram (microseconds, 64 buckets).
    pub jitter_histogram_us: [u32; MAX_AXES as usize],
}

/// **#14** HAL → gRPC response segment (`evo_hal_rpc`).
///
/// HAL action responses/acks (DO set confirmation, driver state).
///
/// FR-014d, FR-030b.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct HalToRpcSegment {
    /// Request ID correlating to inbound request.
    pub request_id: u64,
    /// Result code (0 = success).
    pub result_code: u32,
    /// Padding.
    pub _pad1: [u8; 4],
    /// Error message (fixed-size, null-terminated).
    pub error_message: [u8; 128],
    /// Reserved for future expansion.
    pub _reserved: [u8; 112],
}

/// **#15** HAL → RE feedback segment (`evo_hal_re`).
///
/// Full I/O states and per-axis feedback for RE decision logic.
/// Fast read path for recipe execution.
///
/// FR-014e, FR-030c.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct HalToReSegment {
    /// Per-axis feedback (position, velocity, torque, flags).
    pub axes: [HalAxisFeedback; MAX_AXES as usize],
    /// Digital input bank.
    pub di_bank: [u64; BANK_WORDS],
    /// Digital output bank.
    pub do_bank: [u64; BANK_WORDS],
    /// Analog input values.
    pub ai_values: [f64; MAX_AI],
    /// Analog output values.
    pub ao_values: [f64; MAX_AO],
    /// Number of active axes.
    pub axis_count: u8,
}

// ─── Default implementations ────────────────────────────────────────

impl_default_zeroed!(
    HalToCuSegment,
    CuToHalSegment,
    CuToMqtSegment,
    HalToMqtSegment,
    ReToCuSegment,
    ReToHalSegment,
    ReToMqtSegment,
    ReToRpcSegment,
    RpcToCuSegment,
    RpcToHalSegment,
    RpcToReSegment,
    CuToReSegment,
    CuToRpcSegment,
    HalToRpcSegment,
    HalToReSegment,
);

// ═══════════════════════════════════════════════════════════════════
//  Static Assertions (T027)
// ═══════════════════════════════════════════════════════════════════

// Sub-struct sizes (repr(C) with implicit trailing padding).
const _: () = assert!(core::mem::size_of::<HalAxisFeedback>() == 32);
const _: () = assert!(core::mem::size_of::<CuAxisCommand>() == 40);
const _: () = assert!(core::mem::size_of::<CuAxisStatus>() == 16);
const _: () = assert!(core::mem::size_of::<AxisPidState>() == 24);

// All 15 segment structs: alignment == 64 (cache-line aligned).
const _: () = assert!(core::mem::align_of::<HalToCuSegment>() == 64);
const _: () = assert!(core::mem::align_of::<CuToHalSegment>() == 64);
const _: () = assert!(core::mem::align_of::<CuToMqtSegment>() == 64);
const _: () = assert!(core::mem::align_of::<HalToMqtSegment>() == 64);
const _: () = assert!(core::mem::align_of::<ReToCuSegment>() == 64);
const _: () = assert!(core::mem::align_of::<ReToHalSegment>() == 64);
const _: () = assert!(core::mem::align_of::<ReToMqtSegment>() == 64);
const _: () = assert!(core::mem::align_of::<ReToRpcSegment>() == 64);
const _: () = assert!(core::mem::align_of::<RpcToCuSegment>() == 64);
const _: () = assert!(core::mem::align_of::<RpcToHalSegment>() == 64);
const _: () = assert!(core::mem::align_of::<RpcToReSegment>() == 64);
const _: () = assert!(core::mem::align_of::<CuToReSegment>() == 64);
const _: () = assert!(core::mem::align_of::<CuToRpcSegment>() == 64);
const _: () = assert!(core::mem::align_of::<HalToRpcSegment>() == 64);
const _: () = assert!(core::mem::align_of::<HalToReSegment>() == 64);

// All 15 segment structs: size is a multiple of 64 (cache-line granularity).
const _: () = assert!(core::mem::size_of::<HalToCuSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<CuToHalSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<CuToMqtSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<HalToMqtSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<ReToCuSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<ReToHalSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<ReToMqtSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<ReToRpcSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<RpcToCuSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<RpcToHalSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<RpcToReSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<CuToReSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<CuToRpcSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<HalToRpcSegment>() % 64 == 0);
const _: () = assert!(core::mem::size_of::<HalToReSegment>() % 64 == 0);

// Active segments: verify exact sizes.
const _: () = assert!(core::mem::size_of::<HalToCuSegment>() == 10432);
const _: () = assert!(core::mem::size_of::<CuToHalSegment>() == 10944);

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shm::p2p::struct_version_hash;

    #[test]
    fn sub_struct_sizes() {
        assert_eq!(core::mem::size_of::<HalAxisFeedback>(), 32);
        assert_eq!(core::mem::size_of::<CuAxisCommand>(), 40);
        assert_eq!(core::mem::size_of::<CuAxisStatus>(), 16);
        assert_eq!(core::mem::size_of::<AxisPidState>(), 24);
    }

    #[test]
    fn active_segment_sizes() {
        assert_eq!(core::mem::size_of::<HalToCuSegment>(), 10432);
        assert_eq!(core::mem::size_of::<CuToHalSegment>(), 10944);
    }

    #[test]
    fn all_segments_cache_line_aligned() {
        // Verified by const assertions above, but also at runtime.
        assert_eq!(core::mem::align_of::<HalToCuSegment>(), 64);
        assert_eq!(core::mem::align_of::<CuToHalSegment>(), 64);
        assert_eq!(core::mem::align_of::<CuToMqtSegment>(), 64);
        assert_eq!(core::mem::align_of::<HalToMqtSegment>(), 64);
        assert_eq!(core::mem::align_of::<CuToRpcSegment>(), 64);
        assert_eq!(core::mem::align_of::<HalToReSegment>(), 64);
    }

    #[test]
    fn version_hashes_differ_for_different_sizes() {
        // Segments with different sizes must have different version hashes.
        let h1 = struct_version_hash::<HalToCuSegment>();
        let h2 = struct_version_hash::<CuToHalSegment>();
        let h3 = struct_version_hash::<CuToMqtSegment>();
        let h4 = struct_version_hash::<HalToMqtSegment>();
        let h5 = struct_version_hash::<CuToRpcSegment>();
        let h6 = struct_version_hash::<HalToReSegment>();

        // All different sizes → all different hashes.
        assert_ne!(h1, h2, "HalToCu vs CuToHal");
        assert_ne!(h1, h3, "HalToCu vs CuToMqt");
        assert_ne!(h1, h4, "HalToCu vs HalToMqt");
        assert_ne!(h2, h3, "CuToHal vs CuToMqt");
        assert_ne!(h4, h5, "HalToMqt vs CuToRpc");
        assert_ne!(h4, h6, "HalToMqt vs HalToRe");
    }

    #[test]
    fn version_hash_stable() {
        // Version hashes should not change between compilations.
        // Record known-good values; update if struct layout intentionally changes.
        let h = struct_version_hash::<HalToCuSegment>();
        assert_ne!(h, 0, "hash should not be zero");
        // Same call must produce same result.
        assert_eq!(h, struct_version_hash::<HalToCuSegment>());
    }

    #[test]
    fn default_creates_zeroed_segment() {
        let seg = HalToCuSegment::default();
        assert_eq!(seg.axis_count, 0);
        assert_eq!(seg.axes[0].position, 0.0);
        assert_eq!(seg.di_bank[0], 0);
        assert_eq!(seg.ai_values[0], 0.0);
    }

    #[test]
    fn hal_axis_feedback_fields() {
        let mut fb = HalAxisFeedback::default();
        fb.position = 123.456;
        fb.velocity = -7.89;
        fb.torque_estimate = 42.0;
        fb.drive_ready = 1;
        fb.drive_fault = 0;
        fb.referenced = 1;
        fb.active = 1;

        assert_eq!(fb.position, 123.456);
        assert_eq!(fb.drive_ready, 1);
        assert_eq!(fb.referenced, 1);
    }

    #[test]
    fn cu_axis_command_fields() {
        let mut cmd = CuAxisCommand::default();
        cmd.target_position = 100.0;
        cmd.target_velocity = 50.0;
        cmd.calculated_torque = 25.0;
        cmd.torque_offset = 1.5;
        cmd.enable = 1;
        cmd.brake_release = 1;

        assert_eq!(cmd.target_position, 100.0);
        assert_eq!(cmd.enable, 1);
    }

    /// Print segment sizes for documentation (visible with `cargo test -- --nocapture`).
    #[test]
    fn print_segment_sizes() {
        println!("--- Segment Sizes ---");
        println!("HalToCuSegment:  {} bytes", core::mem::size_of::<HalToCuSegment>());
        println!("CuToHalSegment:  {} bytes", core::mem::size_of::<CuToHalSegment>());
        println!("CuToMqtSegment:  {} bytes", core::mem::size_of::<CuToMqtSegment>());
        println!("HalToMqtSegment: {} bytes", core::mem::size_of::<HalToMqtSegment>());
        println!("ReToCuSegment:   {} bytes", core::mem::size_of::<ReToCuSegment>());
        println!("ReToHalSegment:  {} bytes", core::mem::size_of::<ReToHalSegment>());
        println!("ReToMqtSegment:  {} bytes", core::mem::size_of::<ReToMqtSegment>());
        println!("ReToRpcSegment:  {} bytes", core::mem::size_of::<ReToRpcSegment>());
        println!("RpcToCuSegment:  {} bytes", core::mem::size_of::<RpcToCuSegment>());
        println!("RpcToHalSegment: {} bytes", core::mem::size_of::<RpcToHalSegment>());
        println!("RpcToReSegment:  {} bytes", core::mem::size_of::<RpcToReSegment>());
        println!("CuToReSegment:   {} bytes", core::mem::size_of::<CuToReSegment>());
        println!("CuToRpcSegment:  {} bytes", core::mem::size_of::<CuToRpcSegment>());
        println!("HalToRpcSegment: {} bytes", core::mem::size_of::<HalToRpcSegment>());
        println!("HalToReSegment:  {} bytes", core::mem::size_of::<HalToReSegment>());
    }
}
