//! Referencing state machine for axis homing.
//!
//! Implements the 6 referencing modes:
//! - None (0): No referencing needed
//! - SwitchThenIndex (1): Reference switch + K0 index pulse
//! - SwitchOnly (2): Reference switch only
//! - IndexOnly (3): K0 index pulse only
//! - LimitThenIndex (4): Limit switch + K0 index pulse
//! - LimitOnly (5): Limit switch only

use evo_common::hal::config::{ReferencingConfig, ReferencingMode};
use std::time::Duration;
use tracing::{debug, trace};

/// Referencing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferencingState {
    /// Axis is not referenced and not referencing
    Unreferenced,
    /// Searching for reference switch (moving toward switch)
    SearchingSwitch,
    /// Found switch, reversing to find edge
    ReversingFromSwitch,
    /// Searching for K0 index pulse
    SearchingIndex,
    /// Referencing complete, position is valid
    Referenced,
    /// Referencing failed
    Error,
}

/// Referencing state machine.
pub struct ReferencingStateMachine {
    /// Current state
    state: ReferencingState,
    /// Referencing mode from config
    mode: ReferencingMode,
    /// Reference switch position (simulation)
    switch_position: f64,
    /// Index pulse position (simulation)
    index_position: f64,
    /// Is switch normally closed?
    normally_closed: bool,
    /// Initial search direction (1 = positive, -1 = negative)
    initial_direction: f64,
    /// Was switch detected?
    switch_found: bool,
    /// Was index pulse detected?
    index_found: bool,
    /// Timeout counter
    timeout_cycles: u32,
    /// Max cycles before timeout
    max_cycles: u32,
}

impl ReferencingStateMachine {
    /// Create a new referencing state machine.
    pub fn new(config: &ReferencingConfig) -> Self {
        let initial_direction = if config.negative_direction { -1.0 } else { 1.0 };

        Self {
            state: ReferencingState::Unreferenced,
            mode: config.mode,
            switch_position: config.reference_switch_position,
            index_position: config.index_pulse_position,
            normally_closed: config.normally_closed,
            initial_direction,
            switch_found: false,
            index_found: false,
            timeout_cycles: 0,
            max_cycles: 100_000, // ~100 seconds at 1ms cycle
        }
    }

    /// Start the referencing sequence.
    pub fn start(&mut self) {
        if self.mode == ReferencingMode::None {
            self.state = ReferencingState::Referenced;
            return;
        }

        self.switch_found = false;
        self.index_found = false;
        self.timeout_cycles = 0;

        match self.mode {
            ReferencingMode::None => {
                self.state = ReferencingState::Referenced;
            }
            ReferencingMode::SwitchThenIndex | ReferencingMode::SwitchOnly => {
                self.state = ReferencingState::SearchingSwitch;
            }
            ReferencingMode::IndexOnly => {
                self.state = ReferencingState::SearchingIndex;
            }
            ReferencingMode::LimitThenIndex | ReferencingMode::LimitOnly => {
                // Limit switches work similar to reference switches
                self.state = ReferencingState::SearchingSwitch;
            }
        }

        debug!("Referencing started in mode {:?}", self.mode);
    }

    /// Update the state machine.
    ///
    /// # Arguments
    /// * `position` - Current axis position
    /// * `dt` - Time delta since last update
    ///
    /// # Returns
    /// `true` if referencing is complete (success or error)
    pub fn update(&mut self, position: f64, _dt: Duration) -> bool {
        if !self.is_active() {
            return false;
        }

        self.timeout_cycles += 1;
        if self.timeout_cycles > self.max_cycles {
            debug!("Referencing timeout");
            self.state = ReferencingState::Error;
            return true;
        }

        match self.state {
            ReferencingState::SearchingSwitch => {
                self.update_searching_switch(position)
            }
            ReferencingState::ReversingFromSwitch => {
                self.update_reversing_from_switch(position)
            }
            ReferencingState::SearchingIndex => {
                self.update_searching_index(position)
            }
            _ => false,
        }
    }

    /// Update when searching for switch.
    fn update_searching_switch(&mut self, position: f64) -> bool {
        // Check if we've reached the switch position
        let on_switch = self.is_on_switch(position);

        if on_switch {
            trace!("Switch found at position {:.3}", position);
            self.switch_found = true;

            match self.mode {
                ReferencingMode::SwitchOnly | ReferencingMode::LimitOnly => {
                    // Done - we found the switch
                    self.state = ReferencingState::Referenced;
                    debug!("Referencing complete (switch only)");
                    return true;
                }
                ReferencingMode::SwitchThenIndex | ReferencingMode::LimitThenIndex => {
                    // Need to find switch edge, then index
                    self.state = ReferencingState::ReversingFromSwitch;
                    // Note: We don't modify direction - state machine tracks this via state
                }
                _ => {}
            }
        }

        false
    }

    /// Update when reversing from switch to find edge.
    fn update_reversing_from_switch(&mut self, position: f64) -> bool {
        // When reversing, check if we've moved off the switch
        // Use a simple position comparison instead of direction-based detection
        let distance_from_switch = (position - self.switch_position).abs();
        let off_switch = distance_from_switch > 1.0; // More than 1 unit from switch

        if off_switch {
            // Found the switch edge, now search for index
            trace!("Switch edge found at position {:.3}", position);
            self.state = ReferencingState::SearchingIndex;
        }

        false
    }

    /// Update when searching for index pulse.
    fn update_searching_index(&mut self, position: f64) -> bool {
        // Check if we've crossed the index position
        let on_index = self.is_on_index(position);

        if on_index {
            trace!("Index pulse found at position {:.3}", position);
            self.index_found = true;
            self.state = ReferencingState::Referenced;
            debug!("Referencing complete (with index)");
            return true;
        }

        false
    }

    /// Check if position is on the reference switch.
    fn is_on_switch(&self, position: f64) -> bool {
        // Simulate switch with hysteresis
        let hysteresis = 0.5; // User units
        let detected = if self.initial_direction > 0.0 {
            position >= self.switch_position - hysteresis
        } else {
            position <= self.switch_position + hysteresis
        };

        if self.normally_closed {
            !detected
        } else {
            detected
        }
    }

    /// Check if position is on the index pulse.
    fn is_on_index(&self, position: f64) -> bool {
        // Index pulse is a narrow window
        let window = 0.1; // User units
        (position - self.index_position).abs() < window
    }

    /// Get current state.
    pub fn state(&self) -> ReferencingState {
        self.state
    }

    /// Check if referencing is currently active.
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            ReferencingState::SearchingSwitch
                | ReferencingState::ReversingFromSwitch
                | ReferencingState::SearchingIndex
        )
    }

    /// Get direction multiplier for motion (-1 or 1).
    pub fn direction_multiplier(&self) -> f64 {
        match self.state {
            ReferencingState::ReversingFromSwitch => -self.initial_direction,
            _ => self.initial_direction,
        }
    }

    /// Reset state machine.
    pub fn reset(&mut self) {
        self.state = ReferencingState::Unreferenced;
        self.switch_found = false;
        self.index_found = false;
        self.timeout_cycles = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(mode: ReferencingMode) -> ReferencingConfig {
        ReferencingConfig {
            required: evo_common::hal::config::ReferencingRequired::Yes,
            mode,
            reference_switch: Some(0),
            normally_closed: false,
            negative_direction: true,
            speed: 10.0,
            show_k0_distance_error: false,
            reference_switch_position: 0.0,
            index_pulse_position: 0.0,
        }
    }

    #[test]
    fn test_mode_none_immediate_complete() {
        let config = make_config(ReferencingMode::None);
        let mut sm = ReferencingStateMachine::new(&config);

        sm.start();
        assert_eq!(sm.state(), ReferencingState::Referenced);
        assert!(!sm.is_active());
    }

    #[test]
    fn test_switch_only_mode() {
        let mut config = make_config(ReferencingMode::SwitchOnly);
        config.reference_switch_position = -100.0;
        config.negative_direction = true;

        let mut sm = ReferencingStateMachine::new(&config);
        sm.start();

        assert_eq!(sm.state(), ReferencingState::SearchingSwitch);
        assert!(sm.is_active());

        // Simulate moving toward switch
        let dt = Duration::from_millis(1);

        // Not at switch yet
        let done = sm.update(-50.0, dt);
        assert!(!done);
        assert_eq!(sm.state(), ReferencingState::SearchingSwitch);

        // At switch position
        let done = sm.update(-100.0, dt);
        assert!(done);
        assert_eq!(sm.state(), ReferencingState::Referenced);
    }

    #[test]
    fn test_switch_then_index_mode() {
        let mut config = make_config(ReferencingMode::SwitchThenIndex);
        config.reference_switch_position = -100.0;
        config.index_pulse_position = -95.0;
        config.negative_direction = true;

        let mut sm = ReferencingStateMachine::new(&config);
        sm.start();

        let dt = Duration::from_millis(1);

        // Move to switch
        sm.update(-100.0, dt);
        assert_eq!(sm.state(), ReferencingState::ReversingFromSwitch);

        // Move off switch (reversing)
        sm.update(-90.0, dt);
        assert_eq!(sm.state(), ReferencingState::SearchingIndex);

        // Find index
        let done = sm.update(-95.0, dt);
        assert!(done);
        assert_eq!(sm.state(), ReferencingState::Referenced);
    }

    #[test]
    fn test_index_only_mode() {
        let mut config = make_config(ReferencingMode::IndexOnly);
        config.index_pulse_position = 0.0;

        let mut sm = ReferencingStateMachine::new(&config);
        sm.start();

        assert_eq!(sm.state(), ReferencingState::SearchingIndex);

        let dt = Duration::from_millis(1);

        // Not at index
        let done = sm.update(-50.0, dt);
        assert!(!done);

        // At index
        let done = sm.update(0.0, dt);
        assert!(done);
        assert_eq!(sm.state(), ReferencingState::Referenced);
    }

    #[test]
    fn test_timeout() {
        let config = make_config(ReferencingMode::SwitchOnly);
        let mut sm = ReferencingStateMachine::new(&config);
        sm.max_cycles = 10; // Short timeout for test

        sm.start();

        let dt = Duration::from_millis(1);

        // Run past timeout without finding switch
        for _ in 0..20 {
            sm.update(100.0, dt); // Far from switch
        }

        assert_eq!(sm.state(), ReferencingState::Error);
    }

    #[test]
    fn test_reset() {
        let config = make_config(ReferencingMode::SwitchOnly);
        let mut sm = ReferencingStateMachine::new(&config);

        sm.start();
        assert!(sm.is_active());

        sm.reset();
        assert!(!sm.is_active());
        assert_eq!(sm.state(), ReferencingState::Unreferenced);
    }

    #[test]
    fn test_direction_multiplier() {
        let mut config = make_config(ReferencingMode::SwitchThenIndex);
        config.negative_direction = true;
        config.reference_switch_position = -100.0;

        let mut sm = ReferencingStateMachine::new(&config);
        sm.start();

        // Initial direction should be negative
        assert_eq!(sm.direction_multiplier(), -1.0);

        // After finding switch and reversing
        let dt = Duration::from_millis(1);
        sm.update(-100.0, dt);

        // Direction should now be positive (reversed from negative)
        assert_eq!(sm.direction_multiplier(), 1.0);
    }
}
