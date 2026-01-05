//! Simulation driver implementation.
//!
//! The `SimulationDriver` implements the `HalDriver` trait to provide
//! software-emulated motion control, referencing, and I/O for development
//! and testing without physical hardware.

use super::io::IOSimulator;
use super::physics::AxisSimulator;
use super::state::{PersistedAxisState, PersistedState, StatePersistence, needs_referencing};
use evo_common::hal::config::{AxisConfig, MachineConfig};
use evo_common::hal::driver::{HalDriver, HalError};
use evo_common::hal::types::{HalCommands, HalStatus};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Simulation driver implementing the HalDriver trait.
pub struct SimulationDriver {
    /// Driver name
    name: &'static str,
    /// Driver version
    version: &'static str,
    /// Initialized flag
    initialized: bool,
    /// I/O simulator
    io_sim: Option<IOSimulator>,
    /// Axis simulators (one per configured axis)
    axis_sims: Vec<AxisSimulator>,
    /// State persistence manager
    state_persistence: Option<StatePersistence>,
    /// Persisted state (loaded on init)
    persisted_state: Option<PersistedState>,
    /// Simulation start time (for timestamping)
    start_time: Option<Instant>,
}

impl SimulationDriver {
    /// Create a new simulation driver instance.
    pub fn new() -> Self {
        Self {
            name: "simulation",
            version: env!("CARGO_PKG_VERSION"),
            initialized: false,
            io_sim: None,
            axis_sims: Vec::new(),
            state_persistence: None,
            persisted_state: None,
            start_time: None,
        }
    }

    /// Restore axis state from persisted data.
    fn restore_axis_state(&mut self, configs: &[AxisConfig]) {
        let Some(persisted) = &self.persisted_state else {
            debug!("No persisted state to restore");
            return;
        };

        for (idx, axis_sim) in self.axis_sims.iter_mut().enumerate() {
            if idx >= configs.len() {
                continue;
            }
            let config = &configs[idx];

            // Find persisted state for this axis by name
            if let Some(axis_state) = persisted.axes.iter().find(|a| a.name == config.name) {
                // Determine if we should restore position based on referencing_required
                let should_restore_position = !needs_referencing(
                    config.referencing.required,
                    Some(axis_state.referenced),
                );

                if should_restore_position && axis_state.referenced {
                    axis_sim.set_position(axis_state.position);
                    axis_sim.set_referenced(true);
                    info!(
                        "Restored axis {} position: {:.3}, referenced: true",
                        config.name, axis_state.position
                    );
                } else {
                    debug!(
                        "Axis {} requires referencing (required={:?}, persisted_referenced={})",
                        config.name, config.referencing.required, axis_state.referenced
                    );
                }
            } else {
                debug!("No persisted state for axis {}", config.name);
            }
        }
    }

    /// Build persisted state from current axis states.
    fn build_persisted_state(&self) -> PersistedState {
        let axes = self
            .axis_sims
            .iter()
            .map(|axis| PersistedAxisState {
                name: axis.name().to_string(),
                position: axis.position(),
                referenced: axis.is_referenced(),
                error_code: 0, // Don't persist error state
            })
            .collect();

        PersistedState {
            version: PersistedState::CURRENT_VERSION,
            axes,
            saved_at: 0, // Will be set by StatePersistence::save
        }
    }
}

impl Default for SimulationDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl HalDriver for SimulationDriver {
    fn name(&self) -> &'static str {
        self.name
    }

    fn version(&self) -> &'static str {
        self.version
    }

    fn init(&mut self, config: &MachineConfig) -> Result<(), HalError> {
        info!(
            "Initializing simulation driver with {} axes, {} DI, {} DO, {} AI, {} AO",
            config.axes.len(),
            config.digital_inputs.len(),
            config.digital_outputs.len(),
            config.analog_inputs.len(),
            config.analog_outputs.len()
        );

        // Initialize I/O simulator
        self.io_sim = Some(IOSimulator::new(
            &config.digital_inputs,
            &config.digital_outputs,
            &config.analog_inputs,
            &config.analog_outputs,
        ));

        // Initialize state persistence if configured
        if let Some(ref state_file) = config.state_file {
            let persistence = StatePersistence::new(state_file);
            // Try to load persisted state
            match persistence.load() {
                Ok(Some(state)) => {
                    info!("Loaded persisted state for {} axes", state.axes.len());
                    self.persisted_state = Some(state);
                }
                Ok(None) => {
                    debug!("No persisted state found");
                }
                Err(e) => {
                    warn!("Failed to load persisted state: {}", e);
                }
            }
            self.state_persistence = Some(persistence);
        }

        self.start_time = Some(Instant::now());
        self.initialized = true;

        info!("Simulation driver initialized (axis simulators will be set via set_axis_configs)");
        Ok(())
    }

    fn set_axis_configs(&mut self, configs: &[AxisConfig]) {
        // Create axis simulators from configs
        self.axis_sims = configs
            .iter()
            .map(|config| AxisSimulator::new(config.clone()))
            .collect();

        // Restore state from persisted data
        self.restore_axis_state(configs);

        info!(
            "Initialized {} axis simulators",
            self.axis_sims.len()
        );
    }

    fn cycle(&mut self, commands: &HalCommands, dt: Duration) -> HalStatus {
        debug!("Simulation driver cycle, dt={:?}", dt);

        let now = self.start_time.map(|_| Instant::now()).unwrap_or_else(Instant::now);
        let mut status = HalStatus::default();

        // Process I/O
        if let Some(io_sim) = &mut self.io_sim {
            let (di_states, ai_values) = io_sim.cycle(
                &commands.digital_outputs,
                &commands.analog_outputs,
                now,
            );

            // Copy DI states to status
            for (idx, &state) in di_states.iter().enumerate() {
                if idx < status.digital_inputs.len() {
                    status.digital_inputs[idx] = state;
                }
            }

            // Copy AI values to status
            for (idx, value) in ai_values.iter().enumerate() {
                if idx < status.analog_inputs.len() {
                    status.analog_inputs[idx] = *value;
                }
            }
        }

        // Update axis simulators
        // First, collect master positions for slave axes
        let master_positions: Vec<Option<f64>> = self.axis_sims
            .iter()
            .map(|axis| {
                if let Some(master_idx) = axis.master_index() {
                    if master_idx < self.axis_sims.len() {
                        Some(self.axis_sims[master_idx].position())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        // Update each axis
        for (idx, axis_sim) in self.axis_sims.iter_mut().enumerate() {
            if idx < commands.axes.len() && idx < status.axes.len() {
                let axis_status = axis_sim.update(&commands.axes[idx], dt, master_positions[idx]);
                status.axes[idx] = axis_status;
            }
        }

        status
    }

    fn shutdown(&mut self) -> Result<(), HalError> {
        info!("Shutting down simulation driver");

        // Persist state to file
        if let Some(ref persistence) = self.state_persistence {
            let state = self.build_persisted_state();
            if let Err(e) = persistence.save(&state) {
                warn!("Failed to save state: {}", e);
            }
        }

        self.axis_sims.clear();
        self.io_sim = None;
        self.initialized = false;
        Ok(())
    }

    fn supports_hot_swap(&self) -> bool {
        false
    }
}
