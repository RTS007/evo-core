//! ControlOutputVector assembly and HAL command writer (T044 / T065 / T068).
//!
//! Sums PID + FF + DOB, applies notch → lowpass → clamp(OutMax),
//! populates all 4 fields per FR-102/FR-105.
//!
//! Also defines `AxisControlState` which holds all per-axis controller state
//! (PID integrator, DOB filter, signal filters). Reset on axis disable or
//! mode change per invariants I-PW-4 / I-OM-4.

use evo_common::control_unit::control::{ControlOutputVector, UniversalControlParameters};
use evo_common::control_unit::shm::{CuAxisCommand, CuToHalSegment};
use evo_common::control_unit::state::PowerState;

use super::dob::{DobGains, DobState, dob_compute};
use super::feedforward::{FeedforwardGains, feedforward_compute, torque_offset_compute};
use super::filters::FilterChainState;
use super::pid::{PidGains, PidState, pid_compute};

// ─── AxisControlState ───────────────────────────────────────────────

/// Per-axis control engine state.
///
/// Holds all stateful controller components. Must be reset when:
/// - Axis is disabled (I-PW-4)
/// - Operational mode changes (I-OM-4)
/// - Safety stop is triggered
#[derive(Debug, Clone)]
pub struct AxisControlState {
    /// PID controller state.
    pub pid: PidState,
    /// Disturbance observer state.
    pub dob: DobState,
    /// Signal conditioning filter chain state.
    pub filters: FilterChainState,
    /// Previous cycle's applied torque (for DOB input).
    pub prev_applied_torque: f64,
    /// Whether the filter chain has been initialized with coefficients.
    filters_initialized: bool,
}

impl Default for AxisControlState {
    fn default() -> Self {
        Self {
            pid: PidState::default(),
            dob: DobState::default(),
            filters: FilterChainState::default(),
            prev_applied_torque: 0.0,
            filters_initialized: false,
        }
    }
}

impl AxisControlState {
    /// Reset all internal controller state to zero (T068).
    ///
    /// Called on axis disable, mode change, or safety stop.
    /// Preserves filter coefficients — only zeros the dynamic state.
    #[inline]
    pub fn reset(&mut self) {
        self.pid.reset();
        self.dob.reset();
        self.filters.reset();
        self.prev_applied_torque = 0.0;
    }

    /// Initialize filter coefficients from parameters.
    ///
    /// Call once at startup or when control parameters change.
    pub fn init_filters(&mut self, params: &UniversalControlParameters, sample_rate: f64) {
        self.filters
            .init(params.f_notch, params.bw_notch, params.flp, sample_rate);
        self.filters_initialized = true;
    }
}

// ─── Control Output Computation (FR-102) ────────────────────────────

/// Input data needed by the control engine for one axis in one cycle.
#[derive(Debug, Clone, Copy)]
pub struct ControlInput {
    /// Target position [mm].
    pub target_position: f64,
    /// Actual (encoder) position [mm].
    pub actual_position: f64,
    /// Target velocity [mm/s].
    pub target_velocity: f64,
    /// Actual velocity [mm/s].
    pub actual_velocity: f64,
    /// Target acceleration [mm/s²].
    pub target_acceleration: f64,
    /// Cycle period [s].
    pub dt: f64,
}

/// Compute the full control output for one axis (FR-102 / FR-105).
///
/// Pipeline:
/// 1. PID: `Kp * error + Ki * ∫error + Kd * d(error)/dt`
/// 2. Feedforward: `Kvff * vel + Kaff * accel + Friction * sign(vel)`
/// 3. DOB: disturbance rejection
/// 4. Sum: `raw = PID + FF + DOB`
/// 5. Notch filter → Low-pass filter → Clamp(±OutMax)
/// 6. Populate all 4 ControlOutputVector fields
pub fn compute_control_output(
    state: &mut AxisControlState,
    params: &UniversalControlParameters,
    input: &ControlInput,
) -> ControlOutputVector {
    let dt = input.dt;

    // Ensure filters are initialized
    if !state.filters_initialized {
        state.init_filters(params, if dt > 0.0 { 1.0 / dt } else { 1000.0 });
    }

    // ── 1. PID ──────────────────────────────────────────────
    let error = input.target_position - input.actual_position;
    let pid_gains = PidGains {
        kp: params.kp,
        ki: params.ki,
        kd: params.kd,
        tf: params.tf,
        tt: params.tt,
        out_max: params.out_max,
    };
    let pid_output = pid_compute(&mut state.pid, &pid_gains, error, dt);

    // ── 2. Feedforward ──────────────────────────────────────
    let ff_gains = FeedforwardGains {
        kvff: params.kvff,
        kaff: params.kaff,
        friction: params.friction,
    };
    let ff_output = feedforward_compute(&ff_gains, input.target_velocity, input.target_acceleration);

    // ── 3. DOB ──────────────────────────────────────────────
    let dob_gains = DobGains {
        jn: params.jn,
        bn: params.bn,
        gdob: params.gdob,
    };
    let dob_output = dob_compute(
        &mut state.dob,
        &dob_gains,
        input.actual_velocity,
        state.prev_applied_torque,
        dt,
    );

    // ── 4. Sum ──────────────────────────────────────────────
    let raw_output = pid_output + ff_output + dob_output;

    // ── 5. Filter chain: notch → low-pass → clamp ──────────
    let filtered = state.filters.apply(raw_output, params.flp, dt);
    let clamped = if params.out_max > 0.0 {
        filtered.clamp(-params.out_max, params.out_max)
    } else {
        filtered
    };

    // Store for next cycle's DOB
    state.prev_applied_torque = clamped;

    // ── 6. Populate ControlOutputVector (FR-105) ────────────
    // Torque offset = acceleration FF + DOB (for drives with FF injection)
    let torque_offset = torque_offset_compute(params.kaff, input.target_acceleration, dob_output);

    ControlOutputVector {
        calculated_torque: clamped,
        target_velocity: input.target_velocity,
        target_position: input.target_position,
        torque_offset,
    }
}

// ─── HAL Command Building ───────────────────────────────────────────

/// Populate a `CuAxisCommand` for a single axis.
///
/// # Arguments
/// - `power_state`: Current power state of the axis.
/// - `mode`: Current OperationalMode as u8.
/// - `output`: Computed ControlOutputVector (or zero if not in Motion).
///
/// # Returns
/// A `CuAxisCommand` with:
/// - `enable` = 1 if PowerState is Standby, Motion, PoweringOn; 0 otherwise.
/// - `mode` = current operational mode.
/// - `output` = the provided ControlOutputVector.
#[inline]
pub fn build_axis_command(
    power_state: PowerState,
    mode: u8,
    output: ControlOutputVector,
) -> CuAxisCommand {
    let enable = match power_state {
        PowerState::Standby | PowerState::Motion | PowerState::PoweringOn => 1u8,
        _ => 0u8,
    };

    CuAxisCommand {
        output,
        enable,
        mode,
        _pad: [0u8; 6],
    }
}

/// Fill the CU→HAL segment with commands for all active axes.
///
/// # Arguments
/// - `segment`: Mutable reference to the output segment buffer.
/// - `axis_count`: Number of active axes.
/// - `commands`: Per-axis `CuAxisCommand` slice (indexed by position).
pub fn fill_cu_to_hal(
    segment: &mut CuToHalSegment,
    axis_count: u8,
    commands: &[CuAxisCommand],
) {
    segment.axis_count = axis_count;
    let n = axis_count as usize;
    for i in 0..n {
        if let Some(cmd) = commands.get(i) {
            segment.axes[i] = *cmd;
        } else {
            segment.axes[i] = CuAxisCommand::default();
        }
    }
    // Zero out remaining axes.
    for i in n..64 {
        segment.axes[i] = CuAxisCommand::default();
    }
}

// ─── Approach-Speed Reduction (T071, FR-112) ────────────────────────

/// Calculate a reduced velocity command when approaching a position boundary.
///
/// Uses constant-deceleration kinematics: `v_safe = sqrt(2 * decel * distance)`.
/// When within the deceleration zone, the velocity is clamped to guarantee
/// the axis can stop before the limit.
///
/// # Arguments
/// - `position`: Current actual position [mm].
/// - `velocity_cmd`: Commanded velocity [mm/s] (signed).
/// - `min_pos`: Software minimum position [mm].
/// - `max_pos`: Software maximum position [mm].
/// - `max_decel`: Maximum deceleration capability [mm/s²].
///
/// # Returns
/// Adjusted velocity command with magnitude reduced if within deceleration zone.
/// Direction (sign) is preserved. Returns 0.0 if at or beyond the limit.
#[inline]
pub fn approach_speed_limit(
    position: f64,
    velocity_cmd: f64,
    min_pos: f64,
    max_pos: f64,
    max_decel: f64,
) -> f64 {
    if max_decel <= 0.0 || velocity_cmd == 0.0 {
        return velocity_cmd;
    }

    let sign = velocity_cmd.signum();
    let speed = velocity_cmd.abs();

    // Distance to the limit in the direction of travel
    let distance_to_limit = if sign > 0.0 {
        max_pos - position
    } else {
        position - min_pos
    };

    if distance_to_limit <= 0.0 {
        // At or beyond the limit — stop entirely
        return 0.0;
    }

    // v_safe = sqrt(2 * a * d)
    let v_safe = (2.0 * max_decel * distance_to_limit).sqrt();

    if speed <= v_safe {
        velocity_cmd // Within safe envelope
    } else {
        sign * v_safe // Reduce to safe speed
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_output() -> ControlOutputVector {
        ControlOutputVector::default()
    }

    #[test]
    fn enable_when_standby() {
        let cmd = build_axis_command(PowerState::Standby, 0, zero_output());
        assert_eq!(cmd.enable, 1);
    }

    #[test]
    fn enable_when_motion() {
        let cmd = build_axis_command(PowerState::Motion, 1, zero_output());
        assert_eq!(cmd.enable, 1);
        assert_eq!(cmd.mode, 1);
    }

    #[test]
    fn disable_when_power_off() {
        let cmd = build_axis_command(PowerState::PowerOff, 0, zero_output());
        assert_eq!(cmd.enable, 0);
    }

    #[test]
    fn disable_when_power_error() {
        let cmd = build_axis_command(PowerState::PowerError, 0, zero_output());
        assert_eq!(cmd.enable, 0);
    }

    #[test]
    fn fill_cu_to_hal_basic() {
        let mut seg = unsafe { core::mem::zeroed::<CuToHalSegment>() };
        let cmds = [
            build_axis_command(PowerState::Standby, 0, zero_output()),
            build_axis_command(PowerState::Motion, 1, zero_output()),
            build_axis_command(PowerState::PowerOff, 0, zero_output()),
        ];
        fill_cu_to_hal(&mut seg, 3, &cmds);
        assert_eq!(seg.axis_count, 3);
        assert_eq!(seg.axes[0].enable, 1);
        assert_eq!(seg.axes[1].enable, 1);
        assert_eq!(seg.axes[2].enable, 0);
        assert_eq!(seg.axes[3].enable, 0);
    }

    #[test]
    fn compute_pure_p_control() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters {
            kp: 10.0,
            out_max: 100.0,
            ..Default::default()
        };
        let input = ControlInput {
            target_position: 100.0,
            actual_position: 99.0,
            target_velocity: 0.0,
            actual_velocity: 0.0,
            target_acceleration: 0.0,
            dt: 0.001,
        };
        let out = compute_control_output(&mut state, &params, &input);
        // P = 10 * 1.0 = 10.0
        assert!((out.calculated_torque - 10.0).abs() < 1.0);
        assert_eq!(out.target_position, 100.0);
        assert_eq!(out.target_velocity, 0.0);
    }

    #[test]
    fn compute_clamps_to_out_max() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters {
            kp: 1000.0,
            out_max: 5.0,
            ..Default::default()
        };
        let input = ControlInput {
            target_position: 100.0,
            actual_position: 0.0,
            target_velocity: 0.0,
            actual_velocity: 0.0,
            target_acceleration: 0.0,
            dt: 0.001,
        };
        let out = compute_control_output(&mut state, &params, &input);
        assert!((out.calculated_torque - 5.0).abs() < 1e-10);
    }

    #[test]
    fn compute_negative_clamp() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters {
            kp: 1000.0,
            out_max: 5.0,
            ..Default::default()
        };
        let input = ControlInput {
            target_position: 0.0,
            actual_position: 100.0,
            target_velocity: 0.0,
            actual_velocity: 0.0,
            target_acceleration: 0.0,
            dt: 0.001,
        };
        let out = compute_control_output(&mut state, &params, &input);
        assert!((out.calculated_torque - (-5.0)).abs() < 1e-10);
    }

    #[test]
    fn compute_with_feedforward() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters {
            kp: 0.0,
            kvff: 0.1,
            out_max: 100.0,
            ..Default::default()
        };
        let input = ControlInput {
            target_position: 100.0,
            actual_position: 100.0,
            target_velocity: 50.0,
            actual_velocity: 50.0,
            target_acceleration: 0.0,
            dt: 0.001,
        };
        let out = compute_control_output(&mut state, &params, &input);
        // FF = 0.1 * 50 = 5.0, PID = 0 (no error)
        assert!((out.calculated_torque - 5.0).abs() < 1.0);
    }

    #[test]
    fn all_four_fields_populated() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters {
            kp: 1.0,
            kvff: 0.1,
            out_max: 100.0,
            ..Default::default()
        };
        let input = ControlInput {
            target_position: 50.0,
            actual_position: 49.0,
            target_velocity: 20.0,
            actual_velocity: 20.0,
            target_acceleration: 5.0,
            dt: 0.001,
        };
        let out = compute_control_output(&mut state, &params, &input);
        assert!(out.is_finite());
        assert_eq!(out.target_position, 50.0);
        assert_eq!(out.target_velocity, 20.0);
        // calculated_torque and torque_offset are non-NaN
        assert!(out.calculated_torque.is_finite());
        assert!(out.torque_offset.is_finite());
    }

    #[test]
    fn axis_control_state_reset() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters {
            kp: 10.0,
            ki: 100.0,
            out_max: 100.0,
            ..Default::default()
        };
        let input = ControlInput {
            target_position: 100.0,
            actual_position: 90.0,
            target_velocity: 0.0,
            actual_velocity: 0.0,
            target_acceleration: 0.0,
            dt: 0.001,
        };
        // Run several cycles to accumulate state
        for _ in 0..100 {
            compute_control_output(&mut state, &params, &input);
        }
        // Verify state has accumulated
        assert!(state.prev_applied_torque != 0.0);

        // Reset
        state.reset();
        assert_eq!(state.prev_applied_torque, 0.0);

        // After reset, first cycle should be clean
        let out = compute_control_output(&mut state, &params, &input);
        // Should be a clean P+I first step, not carrying old state
        assert!(out.is_finite());
    }

    #[test]
    fn zero_params_produce_zero_torque() {
        let mut state = AxisControlState::default();
        let params = UniversalControlParameters::default(); // all zeros except out_max=100, lag defaults
        let input = ControlInput {
            target_position: 100.0,
            actual_position: 50.0,
            target_velocity: 200.0,
            actual_velocity: 100.0,
            target_acceleration: 50.0,
            dt: 0.001,
        };
        let out = compute_control_output(&mut state, &params, &input);
        // All gains are zero → calculated_torque should be 0
        assert!((out.calculated_torque).abs() < 1e-12);
        // target_position and target_velocity are always passed through
        assert_eq!(out.target_position, 100.0);
        assert_eq!(out.target_velocity, 200.0);
    }

    // ── Approach speed reduction tests (T071) ──

    #[test]
    fn approach_speed_far_from_limit() {
        // Position 50, limit at 100, moving positive at 200 mm/s, decel=1000
        // v_safe = sqrt(2*1000*50) = sqrt(100000) ≈ 316 > 200 → no change
        let v = approach_speed_limit(50.0, 200.0, 0.0, 100.0, 1000.0);
        assert!((v - 200.0).abs() < 1e-10);
    }

    #[test]
    fn approach_speed_near_limit_reduced() {
        // Position 99, limit at 100, moving positive at 200 mm/s, decel=1000
        // distance = 1.0, v_safe = sqrt(2*1000*1) ≈ 44.7 < 200 → reduced
        let v = approach_speed_limit(99.0, 200.0, 0.0, 100.0, 1000.0);
        let expected = (2.0 * 1000.0 * 1.0_f64).sqrt();
        assert!((v - expected).abs() < 0.01);
    }

    #[test]
    fn approach_speed_at_limit_zero() {
        let v = approach_speed_limit(100.0, 200.0, 0.0, 100.0, 1000.0);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn approach_speed_negative_direction() {
        // Moving negative toward min_pos=0, position=1
        // distance=1, v_safe = sqrt(2*1000*1) ≈ 44.7
        let v = approach_speed_limit(1.0, -200.0, 0.0, 100.0, 1000.0);
        let expected = -(2.0 * 1000.0 * 1.0_f64).sqrt();
        assert!((v - expected).abs() < 0.01);
    }

    #[test]
    fn approach_speed_zero_velocity_unchanged() {
        let v = approach_speed_limit(99.0, 0.0, 0.0, 100.0, 1000.0);
        assert_eq!(v, 0.0);
    }
}