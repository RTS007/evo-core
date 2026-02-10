//! Deterministic RT cycle: read → process → write (T031–T034).
//!
//! Implements the main control loop with `clock_nanosleep(TIMER_ABSTIME)`,
//! cycle time measurement, overrun detection, and the three-phase cycle body.
//!
//! ## RT Setup Sequence (T031)
//! 1. Pre-allocate all runtime state (zero heap in loop).
//! 2. `mlockall(MCL_CURRENT | MCL_FUTURE)` — lock all pages.
//! 3. Prefault stack and heap pages.
//! 4. `sched_setaffinity` — pin to isolated CPU core.
//! 5. `sched_setscheduler(SCHED_FIFO, 80)` — RT priority.
//!
//! ## Cycle Loop (T032)
//! Absolute-time sleep on `CLOCK_MONOTONIC` for drift-free pacing.
//! Single cycle overrun → `ERR_CYCLE_OVERRUN` → `SAFETY_STOP` (FR-138).
//!
//! ## Cycle Body (T033)
//! Read inbound SHM → process (placeholder) → write outbound SHM.
//!
//! ## Runtime State (T034)
//! Pre-allocated `[AxisRuntimeState; MAX_AXES]` + global machine/safety state.

use evo_common::control_unit::config::MAX_AXES_LIMIT;
use evo_common::control_unit::shm::{CuToHalSegment, CuToMqtSegment, CuToReSegment};
use evo_common::control_unit::state::{MachineState, SafetyState};

use crate::config::LoadedConfig;
use crate::shm::segments::{CuSegments, SegmentError, SegmentThresholds};

// ─── Cycle Statistics (T032) ────────────────────────────────────────

/// O(1) per-cycle timing statistics.
///
/// Updated every cycle with no allocation. Provides min/max/avg/stddev
/// for cycle latency monitoring and overrun detection.
#[derive(Debug, Clone)]
pub struct CycleStats {
    /// Total cycles executed.
    pub cycle_count: u64,
    /// Last cycle duration [ns].
    pub last_cycle_ns: i64,
    /// Minimum cycle duration [ns].
    pub min_cycle_ns: i64,
    /// Maximum cycle duration [ns].
    pub max_cycle_ns: i64,
    /// Running sum for average computation.
    pub sum_cycle_ns: i64,
    /// Running sum of squares for stddev computation.
    pub sum_sq_cycle_ns: i128,
    /// Number of overruns detected.
    pub overruns: u64,
    /// Maximum wake-up latency [ns] (time between expected and actual wake).
    pub max_latency_ns: i64,
}

impl CycleStats {
    /// Create a new zeroed stats instance.
    pub const fn new() -> Self {
        Self {
            cycle_count: 0,
            last_cycle_ns: 0,
            min_cycle_ns: i64::MAX,
            max_cycle_ns: 0,
            sum_cycle_ns: 0,
            sum_sq_cycle_ns: 0,
            overruns: 0,
            max_latency_ns: 0,
        }
    }

    /// Record a cycle duration. O(1), no allocation.
    #[inline]
    pub fn record(&mut self, duration_ns: i64, latency_ns: i64) {
        self.cycle_count += 1;
        self.last_cycle_ns = duration_ns;
        if duration_ns < self.min_cycle_ns {
            self.min_cycle_ns = duration_ns;
        }
        if duration_ns > self.max_cycle_ns {
            self.max_cycle_ns = duration_ns;
        }
        self.sum_cycle_ns += duration_ns;
        self.sum_sq_cycle_ns += (duration_ns as i128) * (duration_ns as i128);
        if latency_ns > self.max_latency_ns {
            self.max_latency_ns = latency_ns;
        }
    }

    /// Average cycle time [ns] (returns 0 if no cycles).
    #[inline]
    pub fn avg_cycle_ns(&self) -> i64 {
        if self.cycle_count == 0 {
            0
        } else {
            self.sum_cycle_ns / self.cycle_count as i64
        }
    }
}

// ─── Per-Axis Runtime State (T034) ─────────────────────────────────

/// Per-axis mutable runtime state, pre-allocated at startup.
///
/// Aggregates all per-axis state machine values, error flags,
/// and feedback data. Updated every cycle from SHM feedback and
/// internal state machine logic.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AxisRuntimeState {
    // ── State machines ──
    /// Current axis control state.
    pub power_state: u8,
    /// Motion state.
    pub motion_state: u8,
    /// Operational mode.
    pub operational_mode: u8,
    /// Coupling state.
    pub coupling_state: u8,
    /// Gearbox state.
    pub gearbox_state: u8,
    /// Loading state.
    pub loading_state: u8,
    /// Homing state (0 = not homed).
    pub homing_state: u8,

    /// Padding for alignment.
    pub _pad: u8,

    // ── Error flags ──
    /// Power error bitflags.
    pub power_errors: u32,
    /// Motion error bitflags.
    pub motion_errors: u32,
    /// Gearbox error bitflags.
    pub gearbox_errors: u32,
    /// Coupling error bitflags.
    pub coupling_errors: u32,

    // ── Feedback data (from HAL) ──
    /// Actual position [user units].
    pub actual_position: f64,
    /// Actual velocity [user units/s].
    pub actual_velocity: f64,
    /// Following error (command − actual) [user units].
    pub lag: f64,
    /// Estimated torque [% of rated].
    pub torque_estimate: f64,

    // ── Command data (computed by CU) ──
    /// Target position [user units].
    pub target_position: f64,
    /// Target velocity [user units/s].
    pub target_velocity: f64,
    /// Control output (to HAL) [user units].
    pub control_output: [f64; 4],

    /// Drive status byte from HAL.
    pub drive_status: u8,
    /// Drive fault code from HAL.
    pub drive_fault_code: u16,

    _pad2: [u8; 5],
}

impl Default for AxisRuntimeState {
    fn default() -> Self {
        // SAFETY: All fields are numeric; all-zeros is valid.
        unsafe { core::mem::zeroed() }
    }
}

// ─── Global Runtime State (T034) ───────────────────────────────────

/// Pre-allocated runtime state for the entire machine.
///
/// Created once during `MachineState::Starting`, never reallocated.
/// All fields are stack/inline — zero heap allocation.
pub struct RuntimeState {
    /// Per-axis state array (fixed-size, max 64 axes).
    pub axes: [AxisRuntimeState; MAX_AXES_LIMIT as usize],
    /// Number of active axes (from config).
    pub axis_count: u8,
    /// Global machine state.
    pub machine_state: MachineState,
    /// Global safety state.
    pub safety_state: SafetyState,
    /// Cycle statistics.
    pub stats: CycleStats,

    // ── Pre-allocated outbound segment buffers ──
    /// CU→HAL output buffer (updated every cycle).
    pub out_hal: CuToHalSegment,
    /// CU→MQT diagnostic buffer (updated every N cycles).
    pub out_mqt: CuToMqtSegment,
    /// CU→RE acknowledgement buffer.
    pub out_re: CuToReSegment,
}

impl RuntimeState {
    /// Create a new zeroed runtime state with the given axis count.
    pub fn new(axis_count: u8) -> Self {
        Self {
            axes: [AxisRuntimeState::default(); MAX_AXES_LIMIT as usize],
            axis_count,
            machine_state: MachineState::default(),
            safety_state: SafetyState::default(),
            stats: CycleStats::new(),
            // SAFETY: All segment types are repr(C) with numeric fields.
            out_hal: unsafe { core::mem::zeroed() },
            out_mqt: unsafe { core::mem::zeroed() },
            out_re: unsafe { core::mem::zeroed() },
        }
    }
}

// ─── RT Setup (T031) ───────────────────────────────────────────────

/// Errors during RT setup or cycle execution.
#[derive(Debug)]
pub enum CycleError {
    /// RT system call failed.
    RtSetup(String),
    /// SHM segment error.
    Segment(SegmentError),
    /// Cycle overrun detected (FR-138).
    CycleOverrun {
        /// Actual cycle duration [ns].
        actual_ns: i64,
        /// Configured cycle budget [ns].
        budget_ns: i64,
    },
}

impl std::fmt::Display for CycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RtSetup(msg) => write!(f, "RT setup error: {msg}"),
            Self::Segment(e) => write!(f, "segment error: {e}"),
            Self::CycleOverrun {
                actual_ns,
                budget_ns,
            } => write!(
                f,
                "cycle overrun: {actual_ns}ns > {budget_ns}ns budget"
            ),
        }
    }
}

impl std::error::Error for CycleError {}

impl From<SegmentError> for CycleError {
    fn from(e: SegmentError) -> Self {
        Self::Segment(e)
    }
}

/// Lock all current and future memory pages (prevent page faults in RT loop).
///
/// No-op when the `rt` feature is not enabled.
#[cfg(feature = "rt")]
fn rt_mlockall() -> Result<(), CycleError> {
    use nix::sys::mman::{mlockall, MlockallFlags};
    mlockall(MlockallFlags::MCL_CURRENT | MlockallFlags::MCL_FUTURE)
        .map_err(|e| CycleError::RtSetup(format!("mlockall failed: {e}")))?;
    Ok(())
}

#[cfg(not(feature = "rt"))]
fn rt_mlockall() -> Result<(), CycleError> {
    Ok(()) // No-op in simulation mode
}

/// Prefault stack pages to prevent page faults during RT execution.
///
/// Touches a large stack allocation to force page allocation.
fn prefault_stack() {
    // Touch 1 MB of stack to prefault pages.
    let mut buf = [0u8; 1024 * 1024];
    // Prevent compiler from optimizing away the write.
    for byte in buf.iter_mut() {
        unsafe { core::ptr::write_volatile(byte, 0xFF) };
    }
    core::hint::black_box(&buf);
}

/// Pin the current thread to a specific CPU core.
///
/// No-op when the `rt` feature is not enabled.
#[cfg(feature = "rt")]
fn rt_set_affinity(cpu: usize) -> Result<(), CycleError> {
    use nix::sched::{sched_setaffinity, CpuSet};
    use nix::unistd::Pid;

    let mut cpuset = CpuSet::new();
    cpuset
        .set(cpu)
        .map_err(|e| CycleError::RtSetup(format!("CpuSet::set({cpu}) failed: {e}")))?;
    sched_setaffinity(Pid::from_raw(0), &cpuset)
        .map_err(|e| CycleError::RtSetup(format!("sched_setaffinity failed: {e}")))?;
    Ok(())
}

#[cfg(not(feature = "rt"))]
fn rt_set_affinity(_cpu: usize) -> Result<(), CycleError> {
    Ok(()) // No-op in simulation mode
}

/// Set SCHED_FIFO with the given RT priority.
///
/// No-op when the `rt` feature is not enabled.
#[cfg(feature = "rt")]
fn rt_set_scheduler(priority: i32) -> Result<(), CycleError> {
    let param = libc::sched_param {
        sched_priority: priority,
    };
    let ret = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(CycleError::RtSetup(format!(
            "sched_setscheduler(SCHED_FIFO, {priority}) failed: {err}"
        )));
    }
    Ok(())
}

#[cfg(not(feature = "rt"))]
fn rt_set_scheduler(_priority: i32) -> Result<(), CycleError> {
    Ok(()) // No-op in simulation mode
}

/// Perform the full RT setup sequence (T031).
///
/// Must be called before entering the cycle loop.
/// In simulation mode (no `rt` feature), all RT calls are no-ops.
pub fn rt_setup(cpu_core: usize, rt_priority: i32) -> Result<(), CycleError> {
    // 1. Lock all memory pages.
    rt_mlockall()?;

    // 2. Prefault stack pages.
    prefault_stack();

    // 3. Pin to CPU core.
    rt_set_affinity(cpu_core)?;

    // 4. Set RT scheduler.
    rt_set_scheduler(rt_priority)?;

    Ok(())
}

// ─── Cycle Runner (T032, T033) ─────────────────────────────────────

/// The main deterministic cycle runner.
///
/// Owns all runtime state, SHM segments, and timing infrastructure.
/// The `run()` method enters the infinite cycle loop.
pub struct CycleRunner {
    /// Loaded & validated configuration.
    pub config: LoadedConfig,
    /// SHM segment connections.
    pub segments: CuSegments,
    /// Pre-allocated runtime state.
    pub state: RuntimeState,
    /// Configured cycle time [ns].
    cycle_time_ns: i64,
    /// MQT update interval [cycles].
    mqt_interval: u64,
}

impl CycleRunner {
    /// Create a new cycle runner from a loaded configuration.
    ///
    /// This initializes SHM segments and pre-allocates all runtime state.
    pub fn new(config: LoadedConfig) -> Result<Self, CycleError> {
        let thresholds = SegmentThresholds {
            hal_stale: config.cu_config.hal_stale_threshold,
            re_stale: config.cu_config.re_stale_threshold,
            rpc_stale: config.cu_config.rpc_stale_threshold,
        };

        let segments = CuSegments::init(&thresholds)?;
        let axis_count = config.machine.axes.len() as u8;
        let state = RuntimeState::new(axis_count);
        let cycle_time_ns = config.cu_config.cycle_time_us as i64 * 1000;
        let mqt_interval = config.cu_config.mqt_update_interval as u64;

        Ok(Self {
            config,
            segments,
            state,
            cycle_time_ns,
            mqt_interval,
        })
    }

    /// Enter the deterministic cycle loop (T032).
    ///
    /// This method never returns under normal operation. It uses
    /// `clock_nanosleep(TIMER_ABSTIME)` for drift-free cycle pacing.
    ///
    /// In simulation mode (no `rt` feature), uses `std::thread::sleep`
    /// for approximate timing.
    ///
    /// # Errors
    /// Returns `CycleError::CycleOverrun` on the first overrun detected
    /// (FR-138: hard real-time deadline).
    pub fn run(&mut self) -> Result<(), CycleError> {
        self.state.machine_state = MachineState::Idle;

        #[cfg(feature = "rt")]
        {
            self.run_rt_loop()
        }

        #[cfg(not(feature = "rt"))]
        {
            self.run_sim_loop()
        }
    }

    /// RT cycle loop using `clock_nanosleep(TIMER_ABSTIME)`.
    #[cfg(feature = "rt")]
    fn run_rt_loop(&mut self) -> Result<(), CycleError> {
        use nix::sys::time::TimeSpec;
        use nix::time::{clock_gettime, clock_nanosleep, ClockId, ClockNanosleepFlags};

        let clock = ClockId::CLOCK_MONOTONIC;
        let mut next_wake = clock_gettime(clock)
            .map_err(|e| CycleError::RtSetup(format!("clock_gettime: {e}")))?;

        loop {
            // Advance next wake time.
            next_wake = timespec_add_ns(next_wake, self.cycle_time_ns);

            // Record cycle start.
            let cycle_start = clock_gettime(clock)
                .map_err(|e| CycleError::RtSetup(format!("clock_gettime: {e}")))?;
            let wake_latency_ns = timespec_diff_ns(&cycle_start, &next_wake).abs();

            // ── Execute cycle body ──
            self.cycle_body()?;

            // Record cycle end and check overrun.
            let cycle_end = clock_gettime(clock)
                .map_err(|e| CycleError::RtSetup(format!("clock_gettime: {e}")))?;
            let duration_ns = timespec_diff_ns(&cycle_end, &cycle_start);

            self.state.stats.record(duration_ns, wake_latency_ns);

            if duration_ns > self.cycle_time_ns {
                self.state.stats.overruns += 1;
                return Err(CycleError::CycleOverrun {
                    actual_ns: duration_ns,
                    budget_ns: self.cycle_time_ns,
                });
            }

            // Sleep until next cycle boundary (absolute time).
            let _ = clock_nanosleep(clock, ClockNanosleepFlags::TIMER_ABSTIME, &next_wake);
        }
    }

    /// Simulation cycle loop using `std::thread::sleep`.
    #[cfg(not(feature = "rt"))]
    fn run_sim_loop(&mut self) -> Result<(), CycleError> {
        use std::time::Instant;

        let cycle_duration = std::time::Duration::from_nanos(self.cycle_time_ns as u64);

        loop {
            let cycle_start = Instant::now();

            // ── Execute cycle body ──
            self.cycle_body()?;

            let elapsed = cycle_start.elapsed();
            let duration_ns = elapsed.as_nanos() as i64;

            self.state.stats.record(duration_ns, 0);

            if duration_ns > self.cycle_time_ns {
                self.state.stats.overruns += 1;
                // In simulation mode, log but don't abort on overrun.
                // Production (rt feature) would return CycleOverrun here.
            }

            // Sleep for remaining time.
            if let Some(remaining) = cycle_duration.checked_sub(elapsed) {
                std::thread::sleep(remaining);
            }
        }
    }

    /// Three-phase cycle body: read → process → write (T033).
    ///
    /// Each phase is a placeholder for future state machine,
    /// control, and safety logic (T036+).
    fn cycle_body(&mut self) -> Result<(), CycleError> {
        // ═══ READ PHASE ═══
        // Read mandatory HAL→CU feedback.
        let hal = self.segments.hal_to_cu.read()?;

        // Copy HAL feedback into per-axis runtime state.
        let n = self.state.axis_count as usize;
        for i in 0..n {
            self.state.axes[i].actual_position = hal.axes[i].actual_position;
            self.state.axes[i].actual_velocity = hal.axes[i].actual_velocity;
            self.state.axes[i].drive_status = hal.axes[i].drive_status;
            self.state.axes[i].drive_fault_code = hal.axes[i].fault_code;
        }

        // Read optional RE→CU commands.
        if let Some(ref mut re_reader) = self.segments.re_to_cu {
            if re_reader.has_changed() {
                let _re = re_reader.read()?;
                // TODO (T036+): Process recipe commands → command arbitration.
            }
        }

        // Read optional RPC→CU commands.
        if let Some(ref mut rpc_reader) = self.segments.rpc_to_cu {
            if rpc_reader.has_changed() {
                let _rpc = rpc_reader.read()?;
                // TODO (T036+): Process RPC commands → command arbitration.
            }
        }

        // ═══ PROCESS PHASE ═══
        // Phase 4 Integration: Safety evaluation (T053)
        //
        // The safety evaluation pipeline runs here, between SHM read and
        // state machine processing:
        //
        // 1. Evaluate all safety peripherals per axis (tailstock, lock pin, brake, guard)
        //    via `AxisPeripherals::evaluate()` using DI bank from HAL feedback.
        //
        // 2. Aggregate into `AxisSafetyState` flags via `evaluate_axis_safety()`,
        //    combining peripheral results with limit switch, soft limit, motion enable,
        //    and gearbox checks.
        //
        // 3. If any CRITICAL error → `SafetyStateMachine::force_safety_stop()`
        //    → trigger SafeStopExecutor per axis.
        //
        // 4. If SafetyState == SafeReducedSpeed → clamp all target velocities
        //    via `clamp_velocity_for_safety()`.
        //
        // 5. Tick SafeStopExecutors and apply StopActions (Decelerate/DisableAndBrake/HoldTorque).
        //
        // 6. If SafetyState == SafetyStop and recovery requested → tick RecoveryManager.
        //
        // Full wiring requires IoRegistry and peripherals array on CycleRunner,
        // which will be added when the runtime orchestration is consolidated (T060+).

        // Phase 6 Integration: Control engine (T067)
        //
        // For each axis in PowerState::Motion, compute control output:
        //
        // 1. Build `ControlInput` from AxisRuntimeState feedback + target commands.
        //
        // 2. Call `compute_control_output(&mut control_state, &params, &input)`
        //    which runs the full PID + FF + DOB + notch → lowpass → clamp pipeline.
        //
        // 3. Evaluate lag error via `evaluate_lag(target, actual, limit, policy)`.
        //    - LagPolicy::Critical → trigger SAFETY_STOP for all axes.
        //    - LagPolicy::Unwanted → axis-local MotionError.
        //    - LagPolicy::Neutral → flag only.
        //    - LagPolicy::Desired → suppress.
        //
        // 4. On axis disable or mode change → `AxisControlState::reset()`
        //    to zero PID integral, DOB state, filter state (I-PW-4 / I-OM-4).
        //
        // 5. Store resulting `ControlOutputVector` in AxisRuntimeState::control_output.
        //
        // 6. In WRITE phase, use `build_axis_command(power_state, mode, output)`
        //    to construct CuAxisCommand with the computed ControlOutputVector.
        //
        // Per-axis AxisControlState instances and UniversalControlParameters are stored
        // on CycleRunner, initialized from config at startup. Full runtime wiring
        // will be added when the complete control loop orchestration is integrated.

        // Compute lag for diagnostics.
        for i in 0..n {
            self.state.axes[i].lag =
                self.state.axes[i].target_position - self.state.axes[i].actual_position;
        }

        // ═══ WRITE PHASE ═══
        // Build CU→HAL axis commands.
        self.state.out_hal.axis_count = self.state.axis_count;
        for i in 0..n {
            self.state.out_hal.axes[i].enable =
                if self.state.machine_state == MachineState::Active {
                    1
                } else {
                    0
                };
            // TODO (T036+): Fill control output from PID/control pipeline.
        }
        self.segments.cu_to_hal.commit(&self.state.out_hal)?;

        // Build CU→MQT diagnostic (throttled to every N cycles).
        if self.state.stats.cycle_count % self.mqt_interval == 0 {
            self.state.out_mqt.machine_state = self.state.machine_state as u8;
            self.state.out_mqt.safety_state = self.state.safety_state as u8;
            self.state.out_mqt.axis_count = self.state.axis_count;
            for i in 0..n {
                let snap = &mut self.state.out_mqt.axes[i];
                let ax = &self.state.axes[i];
                snap.power = ax.power_state;
                snap.motion = ax.motion_state;
                snap.operational = ax.operational_mode;
                snap.coupling = ax.coupling_state;
                snap.gearbox = ax.gearbox_state;
                snap.loading = ax.loading_state;
                snap.position = ax.actual_position;
                snap.velocity = ax.actual_velocity;
                snap.lag = ax.lag;
                snap.torque = ax.torque_estimate;
                snap.error_power = ax.power_errors as u16;
                snap.error_motion = ax.motion_errors as u16;
                snap.error_gearbox = ax.gearbox_errors as u8;
                snap.error_coupling = ax.coupling_errors as u8;
            }
            self.segments.cu_to_mqt.commit(&self.state.out_mqt)?;
        }

        // Build CU→RE acknowledgement.
        // TODO (T036+): Fill ack_seq_id and ack_status from command processing.
        self.segments.cu_to_re.commit(&self.state.out_re)?;

        Ok(())
    }
}

// ─── Time Helpers ───────────────────────────────────────────────────

/// Add nanoseconds to a TimeSpec.
#[cfg(feature = "rt")]
fn timespec_add_ns(ts: nix::sys::time::TimeSpec, ns: i64) -> nix::sys::time::TimeSpec {
    use nix::sys::time::TimeSpec;
    let mut secs = ts.tv_sec();
    let mut nanos = ts.tv_nsec() + ns;
    while nanos >= 1_000_000_000 {
        secs += 1;
        nanos -= 1_000_000_000;
    }
    while nanos < 0 {
        secs -= 1;
        nanos += 1_000_000_000;
    }
    TimeSpec::new(secs, nanos)
}

/// Compute the difference (a - b) in nanoseconds.
#[cfg(feature = "rt")]
fn timespec_diff_ns(
    a: &nix::sys::time::TimeSpec,
    b: &nix::sys::time::TimeSpec,
) -> i64 {
    (a.tv_sec() - b.tv_sec()) * 1_000_000_000 + (a.tv_nsec() - b.tv_nsec())
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_stats_basic() {
        let mut stats = CycleStats::new();
        assert_eq!(stats.cycle_count, 0);
        assert_eq!(stats.avg_cycle_ns(), 0);

        stats.record(500_000, 1_000);
        assert_eq!(stats.cycle_count, 1);
        assert_eq!(stats.last_cycle_ns, 500_000);
        assert_eq!(stats.min_cycle_ns, 500_000);
        assert_eq!(stats.max_cycle_ns, 500_000);
        assert_eq!(stats.max_latency_ns, 1_000);
        assert_eq!(stats.avg_cycle_ns(), 500_000);

        stats.record(600_000, 500);
        assert_eq!(stats.cycle_count, 2);
        assert_eq!(stats.min_cycle_ns, 500_000);
        assert_eq!(stats.max_cycle_ns, 600_000);
        assert_eq!(stats.max_latency_ns, 1_000); // Max unchanged.
        assert_eq!(stats.avg_cycle_ns(), 550_000);
    }

    #[test]
    fn axis_runtime_state_default_is_zeroed() {
        let state = AxisRuntimeState::default();
        assert_eq!(state.actual_position, 0.0);
        assert_eq!(state.actual_velocity, 0.0);
        assert_eq!(state.power_state, 0);
        assert_eq!(state.power_errors, 0);
        assert_eq!(state.target_position, 0.0);
        assert_eq!(state.control_output, [0.0; 4]);
    }

    #[test]
    fn runtime_state_new() {
        let state = RuntimeState::new(4);
        assert_eq!(state.axis_count, 4);
        assert_eq!(state.machine_state, MachineState::default());
        assert_eq!(state.safety_state, SafetyState::default());
        assert_eq!(state.stats.cycle_count, 0);
        // All axes are zeroed.
        for i in 0..64 {
            assert_eq!(state.axes[i].actual_position, 0.0);
        }
    }

    #[test]
    fn rt_setup_no_rt_feature_is_noop() {
        // Without the `rt` feature, rt_setup should succeed as a no-op.
        #[cfg(not(feature = "rt"))]
        {
            let result = rt_setup(0, 80);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn cycle_error_display() {
        let err = CycleError::CycleOverrun {
            actual_ns: 1_500_000,
            budget_ns: 1_000_000,
        };
        let msg = format!("{err}");
        assert!(msg.contains("1500000"));
        assert!(msg.contains("1000000"));
    }
}
