//! HAL driver implementations.
//!
//! This module contains all HAL driver implementations:
//!
//! - [`simulation`] - Software simulation driver for development and testing
//!
//! # Adding New Drivers
//!
//! 1. Create a new submodule under `drivers/`
//! 2. Implement the `HalDriver` trait from `evo_common::hal::driver`
//! 3. Register the driver in this module using `register_driver()`
//! 4. Add export and documentation

pub mod simulation;

use crate::driver_registry::register_driver;

/// Initialize and register all built-in drivers.
///
/// This function should be called once at startup before any drivers are requested.
pub fn register_all_drivers() {
    // Register simulation driver
    register_driver("simulation", simulation::create_driver);

    // Future drivers will be registered here:
    // register_driver("ethercat", ethercat::create_driver);
    // register_driver("canopen", canopen::create_driver);
}
