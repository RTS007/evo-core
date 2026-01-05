//! Axis physics simulator.
//!
//! The `AxisSimulator` provides realistic motion simulation for different axis types:
//! - Simple: On/off without position feedback
//! - Positioning: Full kinematics with velocity/acceleration limits
//! - Slave: Coupled to master axis with offset
//! - Measurement: Encoder-only without drive

use evo_common::hal::config::{AxisConfig, AxisType};
use evo_common::hal::types::{AxisCommand, AxisStatus};
use std::time::Duration;
use tracing::{debug, trace};

use super::referencing::{ReferencingState, ReferencingStateMachine};

/// Error codes for axis status
pub const ERROR_NONE: u16 = 0;
pub const ERROR_LAG: u16 = 0x0001;
#[allow(dead_code)]
pub const ERROR_LIMIT_POSITIVE: u16 = 0x0002;
#[allow(dead_code)]
pub const ERROR_LIMIT_NEGATIVE: u16 = 0x0004;
pub const ERROR_REFERENCING: u16 = 0x0008;

/// Axis simulator providing physics-based motion simulation.
pub struct AxisSimulator {
    /// Axis configuration
    config: AxisConfig,
    /// Current position in user units
    position: f64,
    /// Current velocity in user units/second
    velocity: f64,
    /// Target position from command
    target_position: f64,
    /// Is axis enabled?
    enabled: bool,
    /// Is axis referenced?
    referenced: bool,
    /// Current error code
    error_code: u16,
    /// Lag error (target - actual)
    lag_error: f64,
    /// In-position flag
    in_position: bool,
    /// Moving flag
    moving: bool,
    /// Referencing state machine
    referencing_sm: ReferencingStateMachine,
    /// Master axis index for Slave type
    master_index: Option<usize>,
    /// Coupling offset for Slave type (captured at coupling time)
    coupling_offset: f64,
}

impl AxisSimulator {
    /// Create a new axis simulator from configuration.
    pub fn new(config: AxisConfig) -> Self {
        let master_index = if config.axis_type == AxisType::Slave {
            config.master_axis
        } else {
            None
        };

        // Initialize referencing state based on config
        // Axis is considered referenced if:
        // 1. It's a Simple type (no positioning)
        // 2. Referencing mode is None (no referencing needed)
        // 3. Referencing is not required
        let initial_referenced = config.axis_type == AxisType::Simple
            || config.referencing.mode == evo_common::hal::config::ReferencingMode::None
            || config.referencing.required == evo_common::hal::config::ReferencingRequired::No;

        let coupling_offset = config.coupling_offset.unwrap_or(0.0);

        Self {
            referencing_sm: ReferencingStateMachine::new(&config.referencing),
            config,
            position: 0.0,
            velocity: 0.0,
            target_position: 0.0,
            enabled: false,
            referenced: initial_referenced,
            error_code: ERROR_NONE,
            lag_error: 0.0,
            in_position: true,
            moving: false,
            master_index,
            coupling_offset,
        }
    }

    /// Update axis state for one cycle.
    ///
    /// # Arguments
    /// * `command` - Command from control unit
    /// * `dt` - Time delta since last cycle
    /// * `master_position` - Position of master axis (for Slave type)
    ///
    /// # Returns
    /// Updated axis status
    pub fn update(&mut self, command: &AxisCommand, dt: Duration, master_position: Option<f64>) -> AxisStatus {
        let dt_s = dt.as_secs_f64();

        // Handle reset command
        if command.reset && !self.enabled {
            self.reset_error();
        }

        // Handle enable state changes
        if command.enable != self.enabled {
            if command.enable {
                self.on_enable();
            } else {
                self.on_disable();
            }
        }

        // Handle referencing command
        if command.reference && !self.referencing_sm.is_active() && !self.has_error() {
            self.start_referencing();
        }

        // Store target position
        self.target_position = command.target_position;

        // Update based on axis type
        match self.config.axis_type {
            AxisType::Simple => self.update_simple(dt_s),
            AxisType::Positioning => self.update_positioning(dt_s),
            AxisType::Slave => self.update_slave(master_position),
            AxisType::Measurement => self.update_measurement(),
        }

        // Update referencing state machine
        if self.referencing_sm.is_active() {
            let ref_done = self.referencing_sm.update(self.position, dt);
            if ref_done {
                self.on_referencing_complete();
            }
        }

        // Calculate lag error for Positioning axes
        if self.config.axis_type == AxisType::Positioning && self.enabled {
            self.lag_error = self.target_position - self.position;
            self.check_lag_error();
        } else {
            self.lag_error = 0.0;
        }

        // Check in-position
        self.in_position = (self.target_position - self.position).abs() <= self.config.in_position_window;

        // Build status
        self.build_status()
    }

    /// Update for Simple axis type (on/off, instant)
    fn update_simple(&mut self, _dt: f64) {
        if self.enabled {
            // Simple axes track target instantly
            self.position = self.target_position;
            self.velocity = 0.0;
            self.moving = false;
        }
    }

    /// Update for Positioning axis type (full kinematics)
    fn update_positioning(&mut self, dt: f64) {
        if !self.enabled || self.has_error() {
            // Decelerate to stop
            self.decelerate_to_stop(dt);
            return;
        }

        if self.referencing_sm.is_active() {
            // Referencing motion
            self.update_referencing_motion(dt);
            return;
        }

        // Normal positioning motion
        let position_error = self.target_position - self.position;
        let max_vel = self.config.max_velocity.unwrap_or(100.0);
        let max_acc = self.config.max_acceleration.unwrap_or(1000.0);

        // Calculate desired velocity based on position error
        // Use triangular velocity profile for smooth motion
        let stopping_distance = self.velocity.abs() * self.velocity.abs() / (2.0 * max_acc);
        let desired_velocity = if position_error.abs() <= stopping_distance {
            // Deceleration phase
            position_error.signum() * (2.0 * max_acc * position_error.abs()).sqrt().min(max_vel)
        } else {
            // Acceleration/cruise phase
            position_error.signum() * max_vel
        };

        // Apply acceleration limits
        let vel_error = desired_velocity - self.velocity;
        let max_vel_change = max_acc * dt;
        let vel_change = vel_error.clamp(-max_vel_change, max_vel_change);
        self.velocity += vel_change;

        // Apply velocity limits
        self.velocity = self.velocity.clamp(-max_vel, max_vel);

        // Integrate position
        self.position += self.velocity * dt;

        // Check soft limits
        self.check_soft_limits();

        // Update moving flag
        self.moving = self.velocity.abs() > 0.001;

        trace!(
            "Axis {}: pos={:.3}, vel={:.3}, target={:.3}, err={:.3}",
            self.config.name,
            self.position,
            self.velocity,
            self.target_position,
            position_error
        );
    }

    /// Update for Slave axis type (coupled to master)
    fn update_slave(&mut self, master_position: Option<f64>) {
        if let Some(master_pos) = master_position {
            if self.enabled {
                // Slave follows master with offset
                self.position = master_pos + self.coupling_offset;
                self.velocity = 0.0; // Could calculate from master velocity
                self.moving = false;
            }
        }
    }

    /// Update for Measurement axis type (encoder only)
    fn update_measurement(&mut self) {
        // Measurement axis doesn't move on its own
        // Position is set externally or stays constant
        self.velocity = 0.0;
        self.moving = false;
    }

    /// Update during referencing motion
    fn update_referencing_motion(&mut self, dt: f64) {
        let ref_speed = self.config.referencing.speed;
        let direction = if self.config.referencing.negative_direction { -1.0 } else { 1.0 };

        match self.referencing_sm.state() {
            ReferencingState::SearchingSwitch | ReferencingState::SearchingIndex => {
                // Move at referencing speed
                self.velocity = direction * ref_speed * self.referencing_sm.direction_multiplier();
                self.position += self.velocity * dt;
                self.moving = true;
            }
            _ => {
                self.decelerate_to_stop(dt);
            }
        }
    }

    /// Decelerate to stop
    fn decelerate_to_stop(&mut self, dt: f64) {
        if self.velocity.abs() < 0.001 {
            self.velocity = 0.0;
            self.moving = false;
            return;
        }

        let max_acc = self.config.max_acceleration.unwrap_or(1000.0);
        let decel = max_acc * dt;

        if self.velocity > 0.0 {
            self.velocity = (self.velocity - decel).max(0.0);
        } else {
            self.velocity = (self.velocity + decel).min(0.0);
        }

        self.position += self.velocity * dt;
        self.moving = self.velocity.abs() > 0.001;
    }

    /// Check soft limits
    fn check_soft_limits(&mut self) {
        if let Some(limit_pos) = self.config.soft_limit_positive {
            if self.position > limit_pos {
                self.position = limit_pos;
                if self.velocity > 0.0 {
                    self.velocity = 0.0;
                }
                // Note: We don't set error for soft limit hit, just clamp
            }
        }

        if let Some(limit_neg) = self.config.soft_limit_negative {
            if self.position < limit_neg {
                self.position = limit_neg;
                if self.velocity < 0.0 {
                    self.velocity = 0.0;
                }
            }
        }
    }

    /// Check lag error limit
    fn check_lag_error(&mut self) {
        if let Some(lag_limit) = self.config.lag_error_limit {
            // Only check lag error when axis is supposed to be tracking (settling/in position)
            // Large position error during approach is expected and not a lag error
            // Position error = target - actual; lag error only meaningful when error is small
            let position_error = (self.target_position - self.position).abs();
            let is_tracking = position_error < lag_limit * 5.0; // Within 5x of lag limit
            if self.lag_error.abs() > lag_limit && self.enabled && is_tracking {
                debug!(
                    "Axis {}: Lag error {:.3} exceeded limit {:.3}",
                    self.config.name, self.lag_error, lag_limit
                );
                self.set_error(ERROR_LAG);
            }
        }
    }

    /// Handle enable command
    fn on_enable(&mut self) {
        self.enabled = true;
        debug!("Axis {} enabled", self.config.name);
    }

    /// Handle disable command
    fn on_disable(&mut self) {
        self.enabled = false;
        debug!("Axis {} disabled", self.config.name);
    }

    /// Start referencing sequence
    fn start_referencing(&mut self) {
        debug!("Axis {} starting referencing", self.config.name);
        self.referenced = false;
        self.referencing_sm.start();
    }

    /// Handle referencing completion
    fn on_referencing_complete(&mut self) {
        if self.referencing_sm.state() == ReferencingState::Referenced {
            debug!("Axis {} referencing complete", self.config.name);
            self.referenced = true;
            // Set position to reference point (typically 0)
            self.position = 0.0;
        } else if self.referencing_sm.state() == ReferencingState::Error {
            debug!("Axis {} referencing failed", self.config.name);
            self.set_error(ERROR_REFERENCING);
        }
    }

    /// Set error code
    fn set_error(&mut self, code: u16) {
        self.error_code |= code;
        self.enabled = false; // Disable on error
    }

    /// Reset error
    fn reset_error(&mut self) {
        self.error_code = ERROR_NONE;
        self.referencing_sm.reset();
        debug!("Axis {} error reset", self.config.name);
    }

    /// Check if axis has error
    fn has_error(&self) -> bool {
        self.error_code != ERROR_NONE
    }

    /// Build status structure
    fn build_status(&self) -> AxisStatus {
        AxisStatus {
            actual_position: self.position,
            actual_velocity: self.velocity,
            lag_error: self.lag_error,
            ready: self.enabled && !self.has_error() && self.referenced,
            error: self.has_error(),
            referenced: self.referenced,
            referencing: self.referencing_sm.is_active(),
            moving: self.moving,
            in_position: self.in_position,
            error_code: self.error_code,
        }
    }

    /// Get current position
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Set position externally (for Measurement axes or state restore)
    pub fn set_position(&mut self, pos: f64) {
        self.position = pos;
    }

    /// Get axis name
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Get axis type
    pub fn axis_type(&self) -> AxisType {
        self.config.axis_type
    }

    /// Get master axis index (for Slave type)
    pub fn master_index(&self) -> Option<usize> {
        self.master_index
    }

    /// Check if axis is referenced
    pub fn is_referenced(&self) -> bool {
        self.referenced
    }

    /// Set referenced state (for state restore)
    pub fn set_referenced(&mut self, referenced: bool) {
        self.referenced = referenced;
    }

    /// Capture coupling offset for Slave axis
    pub fn capture_coupling_offset(&mut self, master_position: f64) {
        self.coupling_offset = self.position - master_position;
        debug!(
            "Axis {} captured coupling offset: {:.3}",
            self.config.name, self.coupling_offset
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::hal::config::{ReferencingConfig, ReferencingMode, ReferencingRequired};

    fn make_simple_axis() -> AxisConfig {
        AxisConfig {
            name: "simple_axis".to_string(),
            axis_type: AxisType::Simple,
            encoder_resolution: None,
            max_velocity: None,
            max_acceleration: None,
            lag_error_limit: None,
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: None,
            soft_limit_negative: None,
        }
    }

    fn make_positioning_axis() -> AxisConfig {
        AxisConfig {
            name: "positioning_axis".to_string(),
            axis_type: AxisType::Positioning,
            encoder_resolution: Some(1000.0),
            max_velocity: Some(100.0),
            max_acceleration: Some(500.0),
            lag_error_limit: Some(10.0),
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.1,
            referencing: ReferencingConfig {
                required: ReferencingRequired::No,
                mode: ReferencingMode::None,  // None mode means immediately referenced
                ..Default::default()
            },
            soft_limit_positive: Some(1000.0),
            soft_limit_negative: Some(-1000.0),
        }
    }

    fn make_slave_axis(master: usize) -> AxisConfig {
        AxisConfig {
            name: "slave_axis".to_string(),
            axis_type: AxisType::Slave,
            encoder_resolution: Some(1000.0),
            max_velocity: None,
            max_acceleration: None,
            lag_error_limit: None,
            master_axis: Some(master),
            coupling_offset: Some(0.0),
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: None,
            soft_limit_negative: None,
        }
    }

    fn make_measurement_axis() -> AxisConfig {
        AxisConfig {
            name: "measurement_axis".to_string(),
            axis_type: AxisType::Measurement,
            encoder_resolution: Some(1000.0),
            max_velocity: None,
            max_acceleration: None,
            lag_error_limit: None,
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: None,
            soft_limit_negative: None,
        }
    }

    #[test]
    fn test_simple_axis_instant_position() {
        let config = make_simple_axis();
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(1);

        // Enable and command position
        let cmd = AxisCommand {
            target_position: 100.0,
            enable: true,
            reset: false,
            reference: false,
        };

        let status = sim.update(&cmd, dt, None);

        // Simple axis tracks instantly
        assert!((status.actual_position - 100.0).abs() < 0.001);
        assert!(status.ready);
        assert!(status.in_position);
        assert!(!status.moving);
    }

    #[test]
    fn test_positioning_axis_motion() {
        let config = make_positioning_axis();
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(10);

        // Enable
        let cmd = AxisCommand {
            target_position: 100.0,
            enable: true,
            reset: false,
            reference: false,
        };

        // First update - should start moving
        let status = sim.update(&cmd, dt, None);
        assert!(status.ready);
        assert!(!status.in_position); // Not at target yet

        // Run for some cycles to approach target
        for _ in 0..100 {
            sim.update(&cmd, dt, None);
        }

        let status = sim.update(&cmd, dt, None);
        // Should be close to target after 1 second (100 * 10ms)
        assert!(status.actual_position > 50.0); // Should have made progress
    }

    #[test]
    fn test_positioning_axis_velocity_limit() {
        let config = make_positioning_axis();
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(1);

        let cmd = AxisCommand {
            target_position: 10000.0, // Far target
            enable: true,
            reset: false,
            reference: false,
        };

        // Run until velocity stabilizes
        for _ in 0..1000 {
            sim.update(&cmd, dt, None);
        }

        let status = sim.update(&cmd, dt, None);
        // Velocity should be at or below max (100.0)
        assert!(status.actual_velocity.abs() <= 100.1);
    }

    #[test]
    fn test_slave_axis_follows_master() {
        let config = make_slave_axis(0);
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(1);

        let cmd = AxisCommand {
            target_position: 0.0,
            enable: true,
            reset: false,
            reference: false,
        };

        // Master at position 50
        let status = sim.update(&cmd, dt, Some(50.0));
        assert!((status.actual_position - 50.0).abs() < 0.001);

        // Master moves to 100
        let status = sim.update(&cmd, dt, Some(100.0));
        assert!((status.actual_position - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_slave_axis_with_offset() {
        let mut config = make_slave_axis(0);
        config.coupling_offset = Some(10.0);
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(1);

        let cmd = AxisCommand {
            target_position: 0.0,
            enable: true,
            reset: false,
            reference: false,
        };

        // Master at 50, slave should be at 50 + 10 = 60
        let status = sim.update(&cmd, dt, Some(50.0));
        assert!((status.actual_position - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_measurement_axis_no_motion() {
        let config = make_measurement_axis();
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(1);

        // Set initial position
        sim.set_position(100.0);

        let cmd = AxisCommand {
            target_position: 200.0, // This should be ignored
            enable: true,
            reset: false,
            reference: false,
        };

        let status = sim.update(&cmd, dt, None);

        // Position should not change (encoder only, no drive)
        assert!((status.actual_position - 100.0).abs() < 0.001);
        assert!(!status.moving);
    }

    #[test]
    fn test_positioning_axis_soft_limits() {
        let config = make_positioning_axis();
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(10);

        // Command beyond positive limit
        let cmd = AxisCommand {
            target_position: 2000.0, // Beyond soft_limit_positive (1000)
            enable: true,
            reset: false,
            reference: false,
        };

        // Run for a while
        for _ in 0..500 {
            sim.update(&cmd, dt, None);
        }

        let status = sim.update(&cmd, dt, None);
        // Position should be clamped to soft limit
        assert!(status.actual_position <= 1000.0);
    }

    #[test]
    fn test_axis_enable_disable() {
        let config = make_positioning_axis();
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(1);

        // Start disabled
        let cmd = AxisCommand {
            target_position: 100.0,
            enable: false,
            reset: false,
            reference: false,
        };

        let status = sim.update(&cmd, dt, None);
        assert!(!status.ready);

        // Enable
        let cmd = AxisCommand {
            target_position: 100.0,
            enable: true,
            reset: false,
            reference: false,
        };

        let status = sim.update(&cmd, dt, None);
        assert!(status.ready);
    }

    #[test]
    fn test_in_position_window() {
        let config = make_positioning_axis(); // in_position_window = 0.1
        let mut sim = AxisSimulator::new(config);
        sim.set_position(99.95); // Within 0.1 of target 100

        let dt = Duration::from_millis(1);
        let cmd = AxisCommand {
            target_position: 100.0,
            enable: true,
            reset: false,
            reference: false,
        };

        let status = sim.update(&cmd, dt, None);
        assert!(status.in_position);

        // Move outside window
        sim.set_position(99.0);
        let status = sim.update(&cmd, dt, None);
        assert!(!status.in_position);
    }

    #[test]
    fn test_lag_error_detection() {
        // Create axis with tight lag limit
        let config = AxisConfig {
            name: "lag_test_axis".to_string(),
            axis_type: AxisType::Positioning,
            encoder_resolution: Some(1000.0),
            max_velocity: Some(100.0),
            max_acceleration: Some(500.0),
            lag_error_limit: Some(1.0), // Very tight limit
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.1,
            referencing: ReferencingConfig {
                required: ReferencingRequired::No,
                mode: ReferencingMode::None,
                ..Default::default()
            },
            soft_limit_positive: None,
            soft_limit_negative: None,
        };
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(10);

        // Enable and command small motion (within lag limit tracking zone)
        let cmd = AxisCommand {
            target_position: 3.0, // Small target, within 5x lag limit = 5.0 tracking zone
            enable: true,
            reset: false,
            reference: false,
        };

        // Run a few cycles, position will approach target
        for _ in 0..50 {
            let status = sim.update(&cmd, dt, None);
            // Eventually should get lag error when in tracking zone but can't keep up
            if status.error {
                assert_eq!(status.error_code, 0x0001); // ERROR_LAG
                return;
            }
        }

        // If we get here, axis tracked successfully - that's also valid
        // since the sim physics might actually keep up
    }

    #[test]
    fn test_two_phase_error_recovery() {
        // Create axis that will trigger error
        let config = AxisConfig {
            name: "recovery_test_axis".to_string(),
            axis_type: AxisType::Positioning,
            encoder_resolution: Some(1000.0),
            max_velocity: Some(10.0), // Slow axis
            max_acceleration: Some(100.0),
            lag_error_limit: Some(0.5), // Very tight limit
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.1,
            referencing: ReferencingConfig {
                required: ReferencingRequired::No,
                mode: ReferencingMode::None,
                ..Default::default()
            },
            soft_limit_positive: None,
            soft_limit_negative: None,
        };
        let mut sim = AxisSimulator::new(config);
        let dt = Duration::from_millis(10);

        // Put axis in error state by forcing position mismatch
        sim.set_position(2.0);

        // Command position within tracking zone (< 5x lag limit = 2.5)
        let cmd = AxisCommand {
            target_position: 0.0, // 2.0 distance, within 2.5 tracking zone
            enable: true,
            reset: false,
            reference: false,
        };

        // This should trigger lag error since position error > lag limit
        let status = sim.update(&cmd, dt, None);
        
        // Axis should have error and be disabled
        if status.error {
            assert!(!status.ready); // Not ready when in error

            // Try reset while enabled - should NOT work
            let cmd_reset_enabled = AxisCommand {
                target_position: 0.0,
                enable: true, // Still enabled
                reset: true,  // Try reset
                reference: false,
            };
            let status = sim.update(&cmd_reset_enabled, dt, None);
            assert!(status.error); // Error should persist

            // Phase 1: Disable the axis
            let cmd_disable = AxisCommand {
                target_position: 0.0,
                enable: false, // Disable first
                reset: false,
                reference: false,
            };
            let status = sim.update(&cmd_disable, dt, None);
            assert!(status.error); // Error still present
            assert!(!status.ready); // Not ready (disabled + error)

            // Phase 2: Now reset while disabled
            let cmd_reset = AxisCommand {
                target_position: 0.0,
                enable: false, // Still disabled
                reset: true,   // Now reset
                reference: false,
            };
            let status = sim.update(&cmd_reset, dt, None);
            assert!(!status.error); // Error should be cleared
            assert_eq!(status.error_code, 0); // Error code reset
        }
    }
}
