//! Driver registry for HAL drivers.
//!
//! This module provides functions to register and retrieve HAL driver factories.
//! Drivers register themselves at compile time through the registry.

use evo_common::hal::driver::{DriverFactory, HalDriver, HalError};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Global driver registry
static DRIVER_REGISTRY: LazyLock<RwLock<HashMap<&'static str, DriverFactory>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a driver factory with the given name.
///
/// # Arguments
/// * `name` - Unique driver name (e.g., "simulation", "ethercat")
/// * `factory` - Factory function that creates driver instances
///
/// # Panics
/// Panics if a driver with the same name is already registered.
pub fn register_driver(name: &'static str, factory: DriverFactory) {
    let mut registry = DRIVER_REGISTRY.write().expect("Registry lock poisoned");
    if registry.contains_key(name) {
        panic!("Driver '{}' is already registered", name);
    }
    registry.insert(name, factory);
}

/// Get a driver factory by name.
///
/// # Arguments
/// * `name` - Driver name to look up
///
/// # Returns
/// The factory function if found, or None if not registered.
pub fn get_driver_factory(name: &str) -> Option<DriverFactory> {
    let registry = DRIVER_REGISTRY.read().expect("Registry lock poisoned");
    registry.get(name).copied()
}

/// Create a driver instance by name.
///
/// # Arguments
/// * `name` - Driver name to instantiate
///
/// # Errors
/// Returns `HalError::DriverNotFound` if no driver with the given name is registered.
pub fn create_driver(name: &str) -> Result<Box<dyn HalDriver>, HalError> {
    let factory = get_driver_factory(name)
        .ok_or_else(|| HalError::DriverNotFound(name.to_string()))?;
    Ok(factory())
}

/// List all registered driver names.
pub fn list_drivers() -> Vec<&'static str> {
    let registry = DRIVER_REGISTRY.read().expect("Registry lock poisoned");
    registry.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use evo_common::hal::config::MachineConfig;
    #[allow(unused_imports)]
    use evo_common::hal::driver::HalDriver;
    #[allow(unused_imports)]
    use evo_common::hal::types::{HalCommands, HalStatus};
    #[allow(unused_imports)]
    use std::time::Duration;

    #[allow(dead_code)]
    struct TestDriver;

    #[allow(dead_code)]
    impl HalDriver for TestDriver {
        fn name(&self) -> &'static str {
            "test"
        }

        fn version(&self) -> &'static str {
            "0.1.0"
        }

        fn init(&mut self, _config: &MachineConfig) -> Result<(), HalError> {
            Ok(())
        }

        fn cycle(&mut self, _commands: &HalCommands, _dt: Duration) -> HalStatus {
            HalStatus::default()
        }

        fn shutdown(&mut self) -> Result<(), HalError> {
            Ok(())
        }
    }

    #[allow(dead_code)]
    fn _create_test_driver() -> Box<dyn HalDriver> {
        Box::new(TestDriver)
    }

    // Note: These tests would need to be run in isolation due to global state
    // In practice, driver registration happens at startup
}
