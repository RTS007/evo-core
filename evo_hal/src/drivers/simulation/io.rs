//! I/O Simulator for digital and analog I/O simulation.
//!
//! The `IOSimulator` manages:
//! - Digital inputs and outputs with state tracking
//! - Linked DI reactions (DO triggers delayed DI changes)
//! - Analog inputs and outputs with polynomial scaling

use evo_common::hal::config::{AnalogIOConfig, DigitalIOConfig, LinkedDigitalInput};
use evo_common::io::config::AnalogCurve;
use evo_common::hal::types::AnalogValue;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tracing::{debug, trace};

/// Pending DI change queued by a linked reaction.
#[derive(Debug, Clone)]
struct PendingDiChange {
    /// When this change should be applied
    trigger_time: Instant,
    /// Index of DI to change
    di_index: usize,
    /// New state for the DI
    new_state: bool,
}

/// Analog I/O state with dual representation.
#[derive(Debug, Clone, Copy, Default)]
struct AnalogState {
    /// Normalized value (0.0 - 1.0)
    normalized: f64,
    /// Scaled value in engineering units
    scaled: f64,
}

/// I/O Simulator for digital and analog I/O.
pub struct IOSimulator {
    /// Digital input states
    di_states: Vec<bool>,
    /// Digital output states
    do_states: Vec<bool>,
    /// Linked reactions for each DO (indexed by DO index)
    linked_reactions: Vec<Vec<LinkedDigitalInput>>,
    /// Previous DO states for edge detection
    do_prev_states: Vec<bool>,
    /// Queue of pending DI changes
    pending_changes: VecDeque<PendingDiChange>,

    /// Analog input states
    ai_states: Vec<AnalogState>,
    /// Analog output states
    ao_states: Vec<AnalogState>,
    /// Analog input configurations (for scaling — used in test-only methods)
    #[cfg_attr(not(test), allow(dead_code))]
    ai_configs: Vec<AnalogScalingConfig>,
    /// Analog output configurations (for scaling)
    #[cfg_attr(not(test), allow(dead_code))]
    ao_configs: Vec<AnalogScalingConfig>,
}

/// Scaling configuration for an analog I/O point.
#[derive(Debug, Clone)]
struct AnalogScalingConfig {
    min_value: f64,
    max_value: f64,
    curve: AnalogCurve,
}

impl Default for AnalogScalingConfig {
    fn default() -> Self {
        Self {
            min_value: 0.0,
            max_value: 1.0,
            curve: AnalogCurve::LINEAR,
        }
    }
}

impl IOSimulator {
    /// Create a new IOSimulator with the given configuration.
    pub fn new(
        di_configs: &[DigitalIOConfig],
        do_configs: &[DigitalIOConfig],
        ai_configs: &[AnalogIOConfig],
        ao_configs: &[AnalogIOConfig],
    ) -> Self {
        // Initialize DI states from config
        let di_states: Vec<bool> = di_configs.iter().map(|c| c.initial_value).collect();

        // Initialize DO states to off
        let do_count = do_configs.len();
        let do_states = vec![false; do_count];
        let do_prev_states = vec![false; do_count];

        // Build linked reactions per DO
        let linked_reactions: Vec<Vec<LinkedDigitalInput>> = do_configs
            .iter()
            .map(|c| c.linked_inputs.clone())
            .collect();

        // Initialize AI states from config
        let ai_states: Vec<AnalogState> = ai_configs
            .iter()
            .map(|c| {
                let initial = c.initial_value.unwrap_or(c.min_value);
                let curve = c.curve;
                let normalized = curve.to_normalized(initial, c.min_value, c.max_value);
                AnalogState {
                    normalized,
                    scaled: initial,
                }
            })
            .collect();

        // Initialize AO states
        let ao_states = vec![AnalogState::default(); ao_configs.len()];

        // Build scaling configs
        let ai_scaling: Vec<AnalogScalingConfig> = ai_configs
            .iter()
            .map(|c| AnalogScalingConfig {
                min_value: c.min_value,
                max_value: c.max_value,
                curve: c.curve,
            })
            .collect();

        let ao_scaling: Vec<AnalogScalingConfig> = ao_configs
            .iter()
            .map(|c| AnalogScalingConfig {
                min_value: c.min_value,
                max_value: c.max_value,
                curve: c.curve,
            })
            .collect();

        debug!(
            "IOSimulator initialized: {} DI, {} DO, {} AI, {} AO",
            di_states.len(),
            do_states.len(),
            ai_states.len(),
            ao_states.len()
        );

        Self {
            di_states,
            do_states,
            linked_reactions,
            do_prev_states,
            pending_changes: VecDeque::new(),
            ai_states,
            ao_states,
            ai_configs: ai_scaling,
            ao_configs: ao_scaling,
        }
    }

    /// Update I/O state for one cycle.
    ///
    /// # Arguments
    /// * `do_commands` - Digital output commands from control unit
    /// * `ao_commands` - Analog output commands (normalized 0.0-1.0) from control unit
    /// * `now` - Current instant for processing delayed reactions
    ///
    /// # Returns
    /// Tuple of (digital_inputs, analog_inputs)
    pub fn cycle(
        &mut self,
        do_commands: &[bool],
        ao_commands: &[f64],
        now: Instant,
    ) -> (Vec<bool>, Vec<AnalogValue>) {
        // Update DO states and detect edges
        self.update_do_states(do_commands, now);

        // Process pending DI changes
        self.process_pending_changes(now);

        // Update AO states
        self.update_ao_states(ao_commands);

        // Return current DI and AI states
        let di_result = self.di_states.clone();
        let ai_result: Vec<AnalogValue> = self
            .ai_states
            .iter()
            .map(|s| AnalogValue {
                normalized: s.normalized,
                scaled: s.scaled,
            })
            .collect();

        (di_result, ai_result)
    }

    /// Update digital output states and queue linked reactions on edges.
    fn update_do_states(&mut self, do_commands: &[bool], now: Instant) {
        for (idx, &cmd) in do_commands.iter().enumerate() {
            if idx >= self.do_states.len() {
                break;
            }

            let prev = self.do_prev_states[idx];
            self.do_states[idx] = cmd;

            // Check for edge (state change)
            if cmd != prev {
                self.handle_do_edge(idx, cmd, now);
            }

            self.do_prev_states[idx] = cmd;
        }
    }

    /// Handle DO edge by queueing linked reactions.
    fn handle_do_edge(&mut self, do_idx: usize, new_state: bool, now: Instant) {
        if do_idx >= self.linked_reactions.len() {
            return;
        }

        for reaction in &self.linked_reactions[do_idx] {
            // Check if this reaction triggers on this edge
            if reaction.trigger == new_state {
                let trigger_time = now + Duration::from_secs_f64(reaction.delay_s);
                let pending = PendingDiChange {
                    trigger_time,
                    di_index: reaction.di_index,
                    new_state: reaction.result,
                };

                trace!(
                    "DO[{}] {} -> queued DI[{}] = {} in {:.3}s",
                    do_idx,
                    if new_state { "ON" } else { "OFF" },
                    reaction.di_index,
                    if reaction.result { "ON" } else { "OFF" },
                    reaction.delay_s
                );

                self.pending_changes.push_back(pending);
            }
        }
    }

    /// Process pending DI changes whose time has arrived.
    fn process_pending_changes(&mut self, now: Instant) {
        // Process all pending changes that should trigger now or earlier
        while let Some(front) = self.pending_changes.front() {
            if front.trigger_time <= now {
                let change = self.pending_changes.pop_front().unwrap();
                if change.di_index < self.di_states.len() {
                    let old = self.di_states[change.di_index];
                    self.di_states[change.di_index] = change.new_state;
                    if old != change.new_state {
                        debug!(
                            "DI[{}] changed: {} -> {}",
                            change.di_index,
                            if old { "ON" } else { "OFF" },
                            if change.new_state { "ON" } else { "OFF" }
                        );
                    }
                }
            } else {
                // Queue is time-ordered (mostly), so we can break
                break;
            }
        }
    }

    /// Update analog output states with scaling.
    fn update_ao_states(&mut self, ao_commands: &[f64]) {
        for (idx, &normalized) in ao_commands.iter().enumerate() {
            if idx >= self.ao_states.len() {
                break;
            }

            let config = &self.ao_configs[idx];
            let scaled = config
                .curve
                .to_scaled(normalized, config.min_value, config.max_value);

            self.ao_states[idx] = AnalogState { normalized, scaled };
        }
    }

    /// Get current digital input state.
    #[cfg(test)]
    pub(crate) fn get_di(&self, index: usize) -> Option<bool> {
        self.di_states.get(index).copied()
    }

    /// Set analog input value (normalized).
    #[cfg(test)]
    pub(crate) fn set_ai_normalized(&mut self, index: usize, normalized: f64) {
        if index < self.ai_states.len() {
            let config = &self.ai_configs[index];
            let scaled = config
                .curve
                .to_scaled(normalized, config.min_value, config.max_value);
            self.ai_states[index] = AnalogState { normalized, scaled };
        }
    }

    /// Set analog input value (scaled/engineering units).
    #[cfg(test)]
    pub(crate) fn set_ai_scaled(&mut self, index: usize, scaled: f64) {
        if index < self.ai_states.len() {
            let config = &self.ai_configs[index];
            let normalized = config
                .curve
                .to_normalized(scaled, config.min_value, config.max_value);
            self.ai_states[index] = AnalogState { normalized, scaled };
        }
    }

    /// Get analog input value.
    #[cfg(test)]
    pub(crate) fn get_ai(&self, index: usize) -> Option<AnalogValue> {
        self.ai_states.get(index).map(|s| AnalogValue {
            normalized: s.normalized,
            scaled: s.scaled,
        })
    }

    /// Get analog output value.
    #[cfg(test)]
    pub(crate) fn get_ao(&self, index: usize) -> Option<AnalogValue> {
        self.ao_states.get(index).map(|s| AnalogValue {
            normalized: s.normalized,
            scaled: s.scaled,
        })
    }

    /// Get count of digital inputs.
    #[cfg(test)]
    pub(crate) fn di_count(&self) -> usize {
        self.di_states.len()
    }

    /// Get count of digital outputs.
    #[cfg(test)]
    pub(crate) fn do_count(&self) -> usize {
        self.do_states.len()
    }

    /// Get count of analog inputs.
    #[cfg(test)]
    pub(crate) fn ai_count(&self) -> usize {
        self.ai_states.len()
    }

    /// Get count of analog outputs.
    #[cfg(test)]
    pub(crate) fn ao_count(&self) -> usize {
        self.ao_states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_di_config(initial: bool) -> DigitalIOConfig {
        DigitalIOConfig {
            name: "test_di".to_string(),
            description: None,
            initial_value: initial,
            linked_inputs: vec![],
        }
    }

    fn make_do_config_with_link(reactions: Vec<LinkedDigitalInput>) -> DigitalIOConfig {
        DigitalIOConfig {
            name: "test_do".to_string(),
            description: None,
            initial_value: false,
            linked_inputs: reactions,
        }
    }

    fn make_ai_config(min: f64, max: f64) -> AnalogIOConfig {
        AnalogIOConfig {
            name: "test_ai".to_string(),
            min_value: min,
            max_value: max,
            curve: AnalogCurve::LINEAR,
            unit: None,
            initial_value: None,
        }
    }

    fn make_ao_config(min: f64, max: f64) -> AnalogIOConfig {
        AnalogIOConfig {
            name: "test_ao".to_string(),
            min_value: min,
            max_value: max,
            curve: AnalogCurve::LINEAR,
            unit: None,
            initial_value: None,
        }
    }

    #[test]
    fn test_io_simulator_new() {
        let di_configs = vec![make_di_config(false), make_di_config(true)];
        let do_configs = vec![
            DigitalIOConfig {
                name: "do0".to_string(),
                description: None,
                initial_value: false,
                linked_inputs: vec![],
            },
            DigitalIOConfig {
                name: "do1".to_string(),
                description: None,
                initial_value: false,
                linked_inputs: vec![],
            },
        ];
        let ai_configs = vec![make_ai_config(0.0, 100.0)];
        let ao_configs = vec![make_ao_config(0.0, 10.0)];

        let sim = IOSimulator::new(&di_configs, &do_configs, &ai_configs, &ao_configs);

        assert_eq!(sim.di_count(), 2);
        assert_eq!(sim.do_count(), 2);
        assert_eq!(sim.ai_count(), 1);
        assert_eq!(sim.ao_count(), 1);

        // Check initial DI states
        assert_eq!(sim.get_di(0), Some(false));
        assert_eq!(sim.get_di(1), Some(true));
    }

    #[test]
    fn test_digital_output_passthrough() {
        let di_configs = vec![make_di_config(false)];
        let do_configs = vec![DigitalIOConfig {
            name: "do0".to_string(),
            description: None,
            initial_value: false,
            linked_inputs: vec![],
        }];
        let ai_configs = vec![];
        let ao_configs = vec![];

        let mut sim = IOSimulator::new(&di_configs, &do_configs, &ai_configs, &ao_configs);
        let now = Instant::now();

        // Initial state
        let (di, _ai) = sim.cycle(&[false], &[], now);
        assert_eq!(di[0], false);

        // DO doesn't directly affect DI without linked_inputs
        let (di, _ai) = sim.cycle(&[true], &[], now);
        assert_eq!(di[0], false);
    }

    #[test]
    fn test_linked_di_reaction() {
        // Setup: DO[0] ON -> after 0.1s -> DI[0] = ON
        let di_configs = vec![make_di_config(false)];
        let do_configs = vec![make_do_config_with_link(vec![LinkedDigitalInput {
            trigger: true,
            delay_s: 0.1,
            di_index: 0,
            result: true,
        }])];
        let ai_configs = vec![];
        let ao_configs = vec![];

        let mut sim = IOSimulator::new(&di_configs, &do_configs, &ai_configs, &ao_configs);
        let start = Instant::now();

        // Initial: DI[0] = false
        let (di, _) = sim.cycle(&[false], &[], start);
        assert_eq!(di[0], false);

        // Turn DO[0] ON - queues reaction
        let (di, _) = sim.cycle(&[true], &[], start);
        assert_eq!(di[0], false); // Not yet

        // After 50ms - still not triggered
        let t1 = start + Duration::from_millis(50);
        let (di, _) = sim.cycle(&[true], &[], t1);
        assert_eq!(di[0], false);

        // After 150ms - should trigger
        let t2 = start + Duration::from_millis(150);
        let (di, _) = sim.cycle(&[true], &[], t2);
        assert_eq!(di[0], true);
    }

    #[test]
    fn test_analog_scaling_linear() {
        let di_configs = vec![];
        let do_configs = vec![];
        let ai_configs = vec![make_ai_config(0.0, 100.0)];
        let ao_configs = vec![make_ao_config(0.0, 10.0)];

        let mut sim = IOSimulator::new(&di_configs, &do_configs, &ai_configs, &ao_configs);
        let now = Instant::now();

        // Set AO normalized = 0.5, should scale to 5.0 (0-10 range)
        let (_di, _ai) = sim.cycle(&[], &[0.5], now);
        let ao = sim.get_ao(0).unwrap();
        assert!((ao.normalized - 0.5).abs() < 0.001);
        assert!((ao.scaled - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_analog_input_initial_value() {
        let ai_config = AnalogIOConfig {
            name: "pressure".to_string(),
            min_value: 0.0,
            max_value: 10.0,
            curve: AnalogCurve::LINEAR,
            unit: Some("bar".to_string()),
            initial_value: Some(5.0),
        };

        let sim = IOSimulator::new(&[], &[], &[ai_config], &[]);
        let ai = sim.get_ai(0).unwrap();

        // Initial value = 5.0 bar (scaled), normalized = 0.5
        assert!((ai.scaled - 5.0).abs() < 0.001);
        assert!((ai.normalized - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_set_ai_normalized() {
        let ai_configs = vec![make_ai_config(0.0, 100.0)];
        let mut sim = IOSimulator::new(&[], &[], &ai_configs, &[]);

        sim.set_ai_normalized(0, 0.75);
        let ai = sim.get_ai(0).unwrap();

        assert!((ai.normalized - 0.75).abs() < 0.001);
        assert!((ai.scaled - 75.0).abs() < 0.001);
    }

    #[test]
    fn test_set_ai_scaled() {
        let ai_configs = vec![make_ai_config(0.0, 100.0)];
        let mut sim = IOSimulator::new(&[], &[], &ai_configs, &[]);

        sim.set_ai_scaled(0, 25.0);
        let ai = sim.get_ai(0).unwrap();

        assert!((ai.scaled - 25.0).abs() < 0.001);
        assert!((ai.normalized - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_analog_quadratic_curve() {
        let ai_config = AnalogIOConfig {
            name: "sensor".to_string(),
            min_value: 0.0,
            max_value: 100.0,
            curve: AnalogCurve::QUADRATIC,
            unit: None,
            initial_value: None,
        };

        let mut sim = IOSimulator::new(&[], &[], &[ai_config], &[]);

        // Set normalized = 0.5, quadratic: f(0.5) = 0.25
        // scaled = 0 + 0.25 * 100 = 25
        sim.set_ai_normalized(0, 0.5);
        let ai = sim.get_ai(0).unwrap();

        assert!((ai.normalized - 0.5).abs() < 0.001);
        assert!((ai.scaled - 25.0).abs() < 0.001);
    }
}
