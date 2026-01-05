//! Simulation driver module.
//!
//! This module provides a software simulation driver for development and testing
//! without physical hardware.

mod driver;
mod io;
mod physics;
mod state;

pub use driver::SimulationDriver;
pub use io::IOSimulator;
pub use physics::{AxisSimulator, ReferencingState, ReferencingStateMachine};
pub use state::{PersistedAxisState, PersistedState, StatePersistence, needs_referencing};

use evo_common::hal::driver::HalDriver;

/// Factory function to create a simulation driver instance.
pub fn create_driver() -> Box<dyn HalDriver> {
    Box::new(SimulationDriver::new())
}
