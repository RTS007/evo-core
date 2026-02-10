//! State machine enums for the Control Unit (FR-001 through FR-070).
//!
//! All enums use `#[repr(u8)]` for compact memory layout and zero-copy
//! SHM transport. Includes global state (MachineState, SafetyState),
//! per-axis state (PowerState, MotionState, OperationalMode, CouplingState,
//! GearboxState, LoadingState), and supporting types.

use serde::{Deserialize, Serialize};

// ─── LEVEL 1: Global State ──────────────────────────────────────────

/// Global machine lifecycle state (FR-001).
///
/// Only one `MachineState` is active at any time (I-MS-1).
/// `SystemError` exits only via recovery reset → `Idle` or full-reset → `Stopped` (I-MS-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MachineState {
    /// Initial state after boot.
    Stopped = 0,
    /// System initialization — loading config, validating SHM.
    Starting = 1,
    /// Ready, no active motion.
    Idle = 2,
    /// Manual jog/positioning.
    Manual = 3,
    /// Recipe/program running.
    Active = 4,
    /// Service mode (authorized).
    Service = 5,
    /// Critical fault — requires reset.
    SystemError = 6,
}

impl MachineState {
    /// Convert from raw `u8`. Returns `None` for invalid values.
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Stopped),
            1 => Some(Self::Starting),
            2 => Some(Self::Idle),
            3 => Some(Self::Manual),
            4 => Some(Self::Active),
            5 => Some(Self::Service),
            6 => Some(Self::SystemError),
            _ => None,
        }
    }
}

impl Default for MachineState {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Global safety overlay state (FR-010).
///
/// Overrides MachineState behavior (FR-011). SafetyStop forces MachineState → SystemError (I-SS-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SafetyState {
    /// All safety conditions satisfied.
    Safe = 0,
    /// Hardware speed limitation active (FR-011).
    SafeReducedSpeed = 1,
    /// Emergency — per-axis safe stop executing.
    SafetyStop = 2,
}

impl SafetyState {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Safe),
            1 => Some(Self::SafeReducedSpeed),
            2 => Some(Self::SafetyStop),
            _ => None,
        }
    }
}

impl Default for SafetyState {
    fn default() -> Self {
        Self::Safe
    }
}

// ─── LEVEL 2: Per-Axis Safe Stop ────────────────────────────────────

/// Per-axis safe stop category (FR-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SafeStopCategory {
    /// Safe Torque Off — immediate power cut.
    STO = 0,
    /// Safe Stop 1 — controlled deceleration → power cut (DEFAULT).
    SS1 = 1,
    /// Safe Stop 2 — controlled deceleration → hold position.
    SS2 = 2,
}

impl SafeStopCategory {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::STO),
            1 => Some(Self::SS1),
            2 => Some(Self::SS2),
            _ => None,
        }
    }
}

impl Default for SafeStopCategory {
    fn default() -> Self {
        Self::SS1
    }
}

// ─── LEVEL 3: Per-Axis State ────────────────────────────────────────

/// Axis identifier — 1-based (1..=64), maps to array index `id - 1` (FR-143).
pub type AxisId = u8;

/// Maximum number of axes supported.
pub const MAX_AXES: usize = 64;

/// Per-axis power state (FR-020).
///
/// No motion output when `PowerState != Motion` (I-PW-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PowerState {
    /// Drive disabled, brake engaged.
    PowerOff = 0,
    /// Multi-step enable sequence in progress.
    PoweringOn = 1,
    /// Drive ready, no motion commanded.
    Standby = 2,
    /// Drive actively controlling.
    Motion = 3,
    /// Multi-step disable sequence in progress.
    PoweringOff = 4,
    /// Service: drive OFF, brake released (I-PW-3: Service mode only).
    NoBrake = 5,
    /// Unrecoverable drive fault.
    PowerError = 6,
}

impl PowerState {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::PowerOff),
            1 => Some(Self::PoweringOn),
            2 => Some(Self::Standby),
            3 => Some(Self::Motion),
            4 => Some(Self::PoweringOff),
            5 => Some(Self::NoBrake),
            6 => Some(Self::PowerError),
            _ => None,
        }
    }

    /// Returns true if this is an interruptible sequence state.
    #[inline]
    pub const fn is_sequence(&self) -> bool {
        matches!(self, Self::PoweringOn | Self::PoweringOff)
    }
}

impl Default for PowerState {
    fn default() -> Self {
        Self::PowerOff
    }
}

/// Per-axis motion state (FR-030).
///
/// Motion state is ONLY updated when `PowerState == Motion` (I-MO-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MotionState {
    /// No motion commanded.
    Standstill = 0,
    /// Velocity increasing.
    Accelerating = 1,
    /// At target velocity.
    ConstantVelocity = 2,
    /// Velocity decreasing.
    Decelerating = 3,
    /// Controlled stop (not emergency).
    Stopping = 4,
    /// Safety-triggered deceleration.
    EmergencyStop = 5,
    /// Homing procedure active.
    Homing = 6,
    /// Passive follower (gearbox oscillation).
    GearAssistMotion = 7,
    /// Motion fault active.
    MotionError = 8,
}

impl MotionState {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Standstill),
            1 => Some(Self::Accelerating),
            2 => Some(Self::ConstantVelocity),
            3 => Some(Self::Decelerating),
            4 => Some(Self::Stopping),
            5 => Some(Self::EmergencyStop),
            6 => Some(Self::Homing),
            7 => Some(Self::GearAssistMotion),
            8 => Some(Self::MotionError),
            _ => None,
        }
    }

    /// Returns true if axis is in any moving state.
    /// `any_moving` = {Accelerating, ConstantVelocity, Decelerating, GearAssistMotion}
    #[inline]
    pub const fn is_moving(&self) -> bool {
        matches!(
            self,
            Self::Accelerating | Self::ConstantVelocity | Self::Decelerating | Self::GearAssistMotion
        )
    }
}

impl Default for MotionState {
    fn default() -> Self {
        Self::Standstill
    }
}

/// Per-axis operational mode (FR-040).
///
/// Mode change only when `MotionState == Standstill` and `PowerState == Standby` (I-OM-1).
/// Coupled slaves cannot change mode independently (I-OM-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OperationalMode {
    /// Position control (PID on position).
    Position = 0,
    /// Velocity control (PID on velocity).
    Velocity = 1,
    /// Direct torque control.
    Torque = 2,
    /// Manual jog (limited velocity).
    Manual = 3,
    /// Service mode testing.
    Test = 4,
}

impl OperationalMode {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Position),
            1 => Some(Self::Velocity),
            2 => Some(Self::Torque),
            3 => Some(Self::Manual),
            4 => Some(Self::Test),
            _ => None,
        }
    }
}

impl Default for OperationalMode {
    fn default() -> Self {
        Self::Position
    }
}

/// Per-axis coupling state (FR-050).
///
/// Maximum 8 direct slaves per master (I-CP-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum CouplingState {
    /// Independent axis.
    Uncoupled = 0,
    /// Leading coupled group.
    Master = 1,
    /// Following master × ratio.
    SlaveCoupled = 2,
    /// Following master × ratio + offset.
    SlaveModulated = 3,
    /// Synchronizing to master.
    WaitingSync = 4,
    /// In-sync with master.
    Synchronized = 5,
    /// Lost synchronization.
    SyncLost = 6,
    /// Transition: engaging.
    Coupling = 7,
    /// Transition: disengaging.
    Decoupling = 8,
}

impl CouplingState {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Uncoupled),
            1 => Some(Self::Master),
            2 => Some(Self::SlaveCoupled),
            3 => Some(Self::SlaveModulated),
            4 => Some(Self::WaitingSync),
            5 => Some(Self::Synchronized),
            6 => Some(Self::SyncLost),
            7 => Some(Self::Coupling),
            8 => Some(Self::Decoupling),
            _ => None,
        }
    }

    /// Returns true if axis is in any coupled state.
    /// `any_coupled` = {Master, SlaveCoupled, SlaveModulated, WaitingSync, Synchronized}
    #[inline]
    pub const fn is_coupled(&self) -> bool {
        matches!(
            self,
            Self::Master
                | Self::SlaveCoupled
                | Self::SlaveModulated
                | Self::WaitingSync
                | Self::Synchronized
        )
    }

    /// Returns true if axis is a slave (coupled or modulated).
    #[inline]
    pub const fn is_slave(&self) -> bool {
        matches!(self, Self::SlaveCoupled | Self::SlaveModulated)
    }
}

impl Default for CouplingState {
    fn default() -> Self {
        Self::Uncoupled
    }
}

/// Per-axis gearbox state (FR-060).
///
/// Gear change ONLY when `MotionState == Standstill` (I-GB-1).
/// `NO_GEARSTEP` is CRITICAL → SAFETY_STOP (I-GB-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum GearboxState {
    /// Axis has no gearbox.
    NoGearbox = 0,
    /// Currently in gear 1.
    Gear1 = 1,
    /// Currently in gear 2.
    Gear2 = 2,
    /// Currently in gear 3.
    Gear3 = 3,
    /// Currently in gear 4.
    Gear4 = 4,
    // Gears 5-249 supported but not explicitly enumerated.
    /// No gear engaged.
    Neutral = 250,
    /// Gear change in progress.
    Shifting = 251,
    /// Sensor conflict or timeout.
    GearboxError = 252,
    /// Initial state before detection.
    Unknown = 253,
}

impl GearboxState {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NoGearbox),
            1 => Some(Self::Gear1),
            2 => Some(Self::Gear2),
            3 => Some(Self::Gear3),
            4 => Some(Self::Gear4),
            // Values 5-249 are valid gear numbers — represented as raw u8
            250 => Some(Self::Neutral),
            251 => Some(Self::Shifting),
            252 => Some(Self::GearboxError),
            253 => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Create a GearboxState for a specific gear number (1-4).
    ///
    /// Industrial gearboxes support up to 4 gears. Returns `None`
    /// for gear numbers outside `1..=4`.
    #[inline]
    pub const fn from_gear_number(gear: u8) -> Option<Self> {
        match gear {
            1 => Some(Self::Gear1),
            2 => Some(Self::Gear2),
            3 => Some(Self::Gear3),
            4 => Some(Self::Gear4),
            _ => None,
        }
    }

    /// Returns true if a valid gear is engaged.
    #[inline]
    pub const fn is_gear_engaged(&self) -> bool {
        let v = *self as u8;
        v >= 1 && v <= 4
    }
}

impl Default for GearboxState {
    fn default() -> Self {
        Self::NoGearbox
    }
}

/// Per-axis loading state (FR-070).
///
/// Loading behavior is config-determined per axis (I-LD-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum LoadingState {
    /// Normal production mode.
    Production = 0,
    /// Axis ready for workpiece loading.
    ReadyForLoading = 1,
    /// Loading not possible (config).
    LoadingBlocked = 2,
    /// Manual loading only (reduced speed).
    LoadingManualAllowed = 3,
}

impl LoadingState {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Production),
            1 => Some(Self::ReadyForLoading),
            2 => Some(Self::LoadingBlocked),
            3 => Some(Self::LoadingManualAllowed),
            _ => None,
        }
    }
}

impl Default for LoadingState {
    fn default() -> Self {
        Self::Production
    }
}

/// Lag error policy — behavior when lag error exceeds `lag_error_limit` (FR-103).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum LagPolicy {
    /// Global SAFETY_STOP for ALL axes (e.g., spindle, coupled axes).
    Critical = 0,
    /// Axis-local stop only, axis → MotionError (DEFAULT).
    Unwanted = 1,
    /// Operator info only — set ERR_LAG_EXCEED flag, no stop.
    Neutral = 2,
    /// Expected behavior (e.g., friction axis) — suppress error flag entirely.
    Desired = 3,
}

impl LagPolicy {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Critical),
            1 => Some(Self::Unwanted),
            2 => Some(Self::Neutral),
            3 => Some(Self::Desired),
            _ => None,
        }
    }
}

impl Default for LagPolicy {
    fn default() -> Self {
        Self::Unwanted
    }
}

// ─── Power Sequence State ───────────────────────────────────────────

/// Tracks progress through the POWERING_ON or POWERING_OFF multi-step sequence.
#[derive(Debug, Clone, Copy, Default)]
pub struct PowerSequenceState {
    /// Current step in the sequence (0-based).
    pub step: u8,
    /// Time spent in the current step [s].
    pub step_timer: f64,
    /// For gravity-affected axes: time for position stability check [s].
    pub holding_timer: f64,
}

// ─── Axis Control State ─────────────────────────────────────────────

/// Per-axis control engine state — PID integral, DOB, and filter state.
///
/// 80 bytes per axis. Zeroed at startup. Reset on axis disable (I-PW-4)
/// and mode change (I-OM-4).
#[derive(Debug, Clone, Copy)]
pub struct AxisControlState {
    // PID state
    /// Ki integration sum.
    pub integral_accumulator: f64,
    /// Previous error for derivative calculation.
    pub prev_error: f64,
    /// Filtered derivative term (Tf).
    pub derivative_filtered: f64,

    // DOB state
    /// Previous velocity measurement.
    pub dob_prev_velocity: f64,
    /// Previous disturbance estimate.
    pub dob_prev_disturbance: f64,
    /// Previous acceleration estimate.
    pub dob_prev_accel_est: f64,

    // Filter state
    /// Biquad notch filter state variable 1.
    pub notch_w1: f64,
    /// Biquad notch filter state variable 2.
    pub notch_w2: f64,
    /// Low-pass filter previous output.
    pub lp_prev_output: f64,

    // Lag monitoring
    /// Current lag error: |target - actual| [mm].
    pub current_lag: f64,
}

impl Default for AxisControlState {
    fn default() -> Self {
        Self {
            integral_accumulator: 0.0,
            prev_error: 0.0,
            derivative_filtered: 0.0,
            dob_prev_velocity: 0.0,
            dob_prev_disturbance: 0.0,
            dob_prev_accel_est: 0.0,
            notch_w1: 0.0,
            notch_w2: 0.0,
            lp_prev_output: 0.0,
            current_lag: 0.0,
        }
    }
}

impl AxisControlState {
    /// Reset all state to zero (I-PW-4, I-OM-4).
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ─── Coupling Configuration ─────────────────────────────────────────

/// Master-slave coupling configuration (FR-050).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouplingConfig {
    /// Master axis ID. `None` if this axis is UNCOUPLED or is the MASTER.
    pub master_axis: Option<AxisId>,
    /// Direct slave axis IDs (MASTER only). Max 8 (I-CP-3).
    pub slave_axes: heapless::Vec<AxisId, 8>,
    /// Slave position ratio: `target = master_pos × ratio`.
    #[serde(default = "default_coupling_ratio")]
    pub coupling_ratio: f64,
    /// Additional offset for SLAVE_MODULATED: `target = master_pos × ratio + offset`.
    #[serde(default)]
    pub modulation_offset: f64,
    /// Maximum time to wait in WAITING_SYNC before timeout [s].
    #[serde(default = "default_sync_timeout")]
    pub sync_timeout: f64,
    /// Maximum master-slave lag difference [mm]. Exceeding → ERR_LAG_DIFF_EXCEED.
    #[serde(default = "default_max_lag_diff")]
    pub max_lag_diff: f64,
}

fn default_coupling_ratio() -> f64 {
    1.0
}
fn default_sync_timeout() -> f64 {
    5.0
}
fn default_max_lag_diff() -> f64 {
    1.0
}

impl Default for CouplingConfig {
    fn default() -> Self {
        Self {
            master_axis: None,
            slave_axes: heapless::Vec::new(),
            coupling_ratio: 1.0,
            modulation_offset: 0.0,
            sync_timeout: 5.0,
            max_lag_diff: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // T035a: Round-trip u8 → enum → u8 tests

    #[test]
    fn machine_state_roundtrip() {
        for v in 0..=6u8 {
            let state = MachineState::from_u8(v).unwrap();
            assert_eq!(state as u8, v);
        }
        assert!(MachineState::from_u8(7).is_none());
        assert!(MachineState::from_u8(255).is_none());
    }

    #[test]
    fn safety_state_roundtrip() {
        for v in 0..=2u8 {
            let state = SafetyState::from_u8(v).unwrap();
            assert_eq!(state as u8, v);
        }
        assert!(SafetyState::from_u8(3).is_none());
    }

    #[test]
    fn safe_stop_category_roundtrip() {
        for v in 0..=2u8 {
            let cat = SafeStopCategory::from_u8(v).unwrap();
            assert_eq!(cat as u8, v);
        }
        assert!(SafeStopCategory::from_u8(3).is_none());
    }

    #[test]
    fn power_state_roundtrip() {
        for v in 0..=6u8 {
            let state = PowerState::from_u8(v).unwrap();
            assert_eq!(state as u8, v);
        }
        assert!(PowerState::from_u8(7).is_none());
    }

    #[test]
    fn motion_state_roundtrip() {
        for v in 0..=8u8 {
            let state = MotionState::from_u8(v).unwrap();
            assert_eq!(state as u8, v);
        }
        assert!(MotionState::from_u8(9).is_none());
    }

    #[test]
    fn motion_state_is_moving() {
        assert!(!MotionState::Standstill.is_moving());
        assert!(MotionState::Accelerating.is_moving());
        assert!(MotionState::ConstantVelocity.is_moving());
        assert!(MotionState::Decelerating.is_moving());
        assert!(!MotionState::Stopping.is_moving());
        assert!(!MotionState::EmergencyStop.is_moving());
        assert!(!MotionState::Homing.is_moving());
        assert!(MotionState::GearAssistMotion.is_moving());
        assert!(!MotionState::MotionError.is_moving());
    }

    #[test]
    fn operational_mode_roundtrip() {
        for v in 0..=4u8 {
            let mode = OperationalMode::from_u8(v).unwrap();
            assert_eq!(mode as u8, v);
        }
        assert!(OperationalMode::from_u8(5).is_none());
    }

    #[test]
    fn coupling_state_roundtrip() {
        for v in 0..=8u8 {
            let state = CouplingState::from_u8(v).unwrap();
            assert_eq!(state as u8, v);
        }
        assert!(CouplingState::from_u8(9).is_none());
    }

    #[test]
    fn coupling_state_is_coupled() {
        assert!(!CouplingState::Uncoupled.is_coupled());
        assert!(CouplingState::Master.is_coupled());
        assert!(CouplingState::SlaveCoupled.is_coupled());
        assert!(CouplingState::SlaveModulated.is_coupled());
        assert!(CouplingState::WaitingSync.is_coupled());
        assert!(CouplingState::Synchronized.is_coupled());
        assert!(!CouplingState::SyncLost.is_coupled());
        assert!(!CouplingState::Coupling.is_coupled());
        assert!(!CouplingState::Decoupling.is_coupled());
    }

    #[test]
    fn gearbox_state_roundtrip() {
        // Named variants
        assert_eq!(GearboxState::from_u8(0).unwrap() as u8, 0);
        assert_eq!(GearboxState::from_u8(1).unwrap() as u8, 1);
        assert_eq!(GearboxState::from_u8(250).unwrap() as u8, 250);
        assert_eq!(GearboxState::from_u8(251).unwrap() as u8, 251);
        assert_eq!(GearboxState::from_u8(252).unwrap() as u8, 252);
        assert_eq!(GearboxState::from_u8(253).unwrap() as u8, 253);
        assert!(GearboxState::from_u8(254).is_none());
    }

    #[test]
    fn gearbox_from_gear_number() {
        let g1 = GearboxState::from_gear_number(1).unwrap();
        assert!(g1.is_gear_engaged());
        assert_eq!(g1 as u8, 1);

        let g4 = GearboxState::from_gear_number(4).unwrap();
        assert!(g4.is_gear_engaged());
        assert_eq!(g4 as u8, 4);

        assert!(GearboxState::from_gear_number(0).is_none());
        assert!(GearboxState::from_gear_number(5).is_none());
        assert!(GearboxState::from_gear_number(250).is_none());
    }

    #[test]
    fn loading_state_roundtrip() {
        for v in 0..=3u8 {
            let state = LoadingState::from_u8(v).unwrap();
            assert_eq!(state as u8, v);
        }
        assert!(LoadingState::from_u8(4).is_none());
    }

    #[test]
    fn lag_policy_roundtrip() {
        for v in 0..=3u8 {
            let policy = LagPolicy::from_u8(v).unwrap();
            assert_eq!(policy as u8, v);
        }
        assert!(LagPolicy::from_u8(4).is_none());
    }

    #[test]
    fn axis_control_state_reset() {
        let mut state = AxisControlState {
            integral_accumulator: 42.0,
            prev_error: 1.5,
            derivative_filtered: -3.0,
            dob_prev_velocity: 100.0,
            dob_prev_disturbance: 0.5,
            dob_prev_accel_est: 2.0,
            notch_w1: 0.1,
            notch_w2: 0.2,
            lp_prev_output: 5.0,
            current_lag: 0.3,
        };
        state.reset();
        assert_eq!(state.integral_accumulator, 0.0);
        assert_eq!(state.prev_error, 0.0);
        assert_eq!(state.derivative_filtered, 0.0);
        assert_eq!(state.current_lag, 0.0);
    }
}
