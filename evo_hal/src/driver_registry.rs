//! Driver registry for HAL drivers.
//!
//! Provides a `DriverRegistry` struct for registering and retrieving HAL driver
//! factories. This uses constructor-injection rather than global state.

use evo_common::hal::driver::{DriverFactory, HalDriver, HalError};
use std::collections::HashMap;

/// Registry of available HAL drivers.
///
/// Constructed at startup, populated via `register()`, and passed to
/// `HalCore` by value. No global state — testable in isolation.
pub struct DriverRegistry {
    factories: HashMap<&'static str, DriverFactory>,
}

impl DriverRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a driver factory.
    ///
    /// # Panics
    /// Panics if a driver with the same name is already registered.
    pub fn register(&mut self, name: &'static str, factory: DriverFactory) {
        if self.factories.contains_key(name) {
            panic!("Driver '{name}' is already registered");
        }
        self.factories.insert(name, factory);
    }

    /// Get a driver factory by name.
    pub fn get_factory(&self, name: &str) -> Option<DriverFactory> {
        self.factories.get(name).copied()
    }

    /// Create a driver instance by name.
    ///
    /// # Errors
    /// Returns `HalError::DriverNotFound` if no driver with the given name is registered.
    pub fn create_driver(&self, name: &str) -> Result<Box<dyn HalDriver>, HalError> {
        let factory = self
            .get_factory(name)
            .ok_or_else(|| HalError::DriverNotFound(name.to_string()))?;
        Ok(factory())
    }

    /// List all registered driver names.
    pub fn list_drivers(&self) -> Vec<&'static str> {
        self.factories.keys().copied().collect()
    }
}

impl Default for DriverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Legacy compatibility shims ─────────────────────────────────────
//
// These free functions wrap a global `LazyLock` registry to support
// existing code that calls `register_driver()` / `get_driver_factory()`
// / `create_driver()`. They will be removed once all callers are
// migrated to use `DriverRegistry` directly.

use std::sync::{LazyLock, RwLock};

/// Global driver registry (legacy compatibility).
static GLOBAL_REGISTRY: LazyLock<RwLock<HashMap<&'static str, DriverFactory>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a driver factory globally (legacy).
pub fn register_driver(name: &'static str, factory: DriverFactory) {
    let mut reg = GLOBAL_REGISTRY.write().expect("Registry lock poisoned");
    if reg.contains_key(name) {
        panic!("Driver '{name}' is already registered");
    }
    reg.insert(name, factory);
}

/// Get a driver factory by name from the global registry (legacy).
pub fn get_driver_factory(name: &str) -> Option<DriverFactory> {
    let reg = GLOBAL_REGISTRY.read().expect("Registry lock poisoned");
    reg.get(name).copied()
}

/// Create a driver instance by name from the global registry (legacy).
pub fn create_driver(name: &str) -> Result<Box<dyn HalDriver>, HalError> {
    let factory =
        get_driver_factory(name).ok_or_else(|| HalError::DriverNotFound(name.to_string()))?;
    Ok(factory())
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::hal::config::MachineConfig;
    use evo_common::hal::types::{HalCommands, HalStatus};
    use std::time::Duration;

    struct TestDriver;

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

    fn create_test_driver() -> Box<dyn HalDriver> {
        Box::new(TestDriver)
    }

    #[test]
    fn registry_register_and_create() {
        let mut reg = DriverRegistry::new();
        reg.register("test_driver", create_test_driver);

        let driver = reg.create_driver("test_driver").expect("should create");
        assert_eq!(driver.name(), "test");
    }

    #[test]
    fn registry_driver_not_found() {
        let reg = DriverRegistry::new();
        let result = reg.create_driver("nonexistent");
        assert!(matches!(result, Err(HalError::DriverNotFound(_))));
    }

    #[test]
    fn registry_list_drivers() {
        let mut reg = DriverRegistry::new();
        reg.register("alpha", create_test_driver);
        reg.register("beta", create_test_driver);

        let mut names = reg.list_drivers();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn registry_duplicate_panics() {
        let mut reg = DriverRegistry::new();
        reg.register("dup", create_test_driver);
        reg.register("dup", create_test_driver);
    }
}
