//! Configuration loading traits and types.
//!
//! This module provides a standardized way to load TOML configuration files
//! across all EVO applications.
//!
//! # Usage
//!
//! ```rust,no_run
//! use evo_common::config::{ConfigLoader, SharedConfig, ConfigError};
//! use serde::Deserialize;
//! use std::path::Path;
//!
//! #[derive(Debug, Deserialize)]
//! struct MyAppConfig {
//!     shared: SharedConfig,
//!     port: u16,
//! }
//!
//! fn main() -> Result<(), ConfigError> {
//!     let config = MyAppConfig::load(Path::new("config.toml"))?;
//!     println!("Service: {}", config.shared.service_name);
//!     Ok(())
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Error type for configuration loading operations.
///
/// This enum represents all possible errors that can occur when loading
/// configuration files.
#[derive(Debug, Clone, Error)]
pub enum ConfigError {
    /// Configuration file not found at specified path.
    #[error("Configuration file not found")]
    FileNotFound,

    /// TOML parsing failed.
    #[error("Failed to parse configuration: {0}")]
    ParseError(String),

    /// Semantic validation failed.
    #[error("Configuration validation failed: {0}")]
    ValidationError(String),
}

/// Log level for application logging.
///
/// Represents the verbosity level of logging output.
/// Uses lowercase serde values for TOML compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Most verbose, detailed tracing information.
    Trace,
    /// Debug information useful during development.
    Debug,
    /// General information about application operation.
    #[default]
    Info,
    /// Warning messages for potentially problematic situations.
    Warn,
    /// Error messages for serious problems.
    Error,
}

/// Common configuration fields shared across all EVO applications.
///
/// This struct should be embedded in application-specific configuration
/// structs to provide consistent base configuration.
///
/// # TOML Example
///
/// ```toml
/// [shared]
/// log_level = "debug"
/// service_name = "evo-hal-sim-01"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedConfig {
    /// Logging verbosity level.
    #[serde(default)]
    pub log_level: LogLevel,

    /// Application instance identifier.
    pub service_name: String,
}

impl SharedConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::ValidationError` if:
    /// - `service_name` is empty
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.service_name.is_empty() {
            return Err(ConfigError::ValidationError(
                "service_name cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Trait for loading configuration from TOML files.
///
/// This trait provides a default implementation that works with any type
/// implementing `serde::de::DeserializeOwned`.
///
/// # Contract
///
/// - Returns `ConfigError::FileNotFound` if the file does not exist
/// - Returns `ConfigError::ParseError` if TOML syntax is invalid
/// - Returns `ConfigError::ValidationError` if semantic validation fails
///
/// # Example
///
/// ```rust,no_run
/// use evo_common::config::{ConfigLoader, SharedConfig, ConfigError};
/// use serde::Deserialize;
/// use std::path::Path;
///
/// #[derive(Debug, Deserialize)]
/// struct AppConfig {
///     shared: SharedConfig,
/// }
///
/// fn main() -> Result<(), ConfigError> {
///     let config = AppConfig::load(Path::new("config.toml"))?;
///     Ok(())
/// }
/// ```
pub trait ConfigLoader: Sized + serde::de::DeserializeOwned {
    /// Load configuration from a TOML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Returns
    ///
    /// * `Ok(Self)` - Successfully loaded and parsed configuration
    /// * `Err(ConfigError)` - Loading or parsing failed
    fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::FileNotFound
            } else {
                ConfigError::ParseError(e.to_string())
            }
        })?;

        toml::from_str(&content).map_err(|e| ConfigError::ParseError(e.to_string()))
    }
}

// Blanket implementation for all types that implement DeserializeOwned.
// This allows any serde-deserializable struct to use ConfigLoader.
impl<T: serde::de::DeserializeOwned> ConfigLoader for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_log_level_default() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
    }

    #[test]
    fn test_log_level_serialization() {
        // Test serialization within a struct (TOML requires a table)
        #[derive(Serialize)]
        struct TestWrapper {
            level: LogLevel,
        }

        let wrapper = TestWrapper {
            level: LogLevel::Trace,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("trace"));

        let wrapper = TestWrapper {
            level: LogLevel::Debug,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("debug"));

        let wrapper = TestWrapper {
            level: LogLevel::Info,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("info"));

        let wrapper = TestWrapper {
            level: LogLevel::Warn,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("warn"));

        let wrapper = TestWrapper {
            level: LogLevel::Error,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("error"));
    }

    #[test]
    fn test_log_level_deserialization() {
        // Test deserialization within a struct (TOML requires a table)
        #[derive(Debug, Deserialize, PartialEq)]
        struct TestWrapper {
            level: LogLevel,
        }

        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"trace\"")
                .unwrap()
                .level,
            LogLevel::Trace
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"debug\"")
                .unwrap()
                .level,
            LogLevel::Debug
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"info\"")
                .unwrap()
                .level,
            LogLevel::Info
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"warn\"")
                .unwrap()
                .level,
            LogLevel::Warn
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"error\"")
                .unwrap()
                .level,
            LogLevel::Error
        );
    }

    #[test]
    fn test_shared_config_validation_success() {
        let config = SharedConfig {
            log_level: LogLevel::Info,
            service_name: "test-service".to_string(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_shared_config_validation_empty_service_name() {
        let config = SharedConfig {
            log_level: LogLevel::Info,
            service_name: "".to_string(),
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn test_config_loader_file_not_found() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            value: String,
        }

        let result = TestConfig::load(Path::new("/nonexistent/path/config.toml"));
        assert!(matches!(result, Err(ConfigError::FileNotFound)));
    }

    #[test]
    fn test_config_loader_parse_error() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            value: String,
        }

        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "invalid toml {{{{").unwrap();

        let result = TestConfig::load(file.path());
        assert!(matches!(result, Err(ConfigError::ParseError(_))));
    }

    #[test]
    fn test_config_loader_success() {
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            shared: SharedConfig,
            port: u16,
        }

        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"port = 8080

[shared]
log_level = "debug"
service_name = "test-service"
"#
        )
        .unwrap();
        file.flush().unwrap();

        let config = TestConfig::load(file.path()).unwrap();
        assert_eq!(config.shared.log_level, LogLevel::Debug);
        assert_eq!(config.shared.service_name, "test-service");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_shared_config_default_log_level() {
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            shared: SharedConfig,
        }

        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"[shared]
service_name = "test-service"
"#
        )
        .unwrap();
        file.flush().unwrap();

        let config = TestConfig::load(file.path()).unwrap();
        assert_eq!(config.shared.log_level, LogLevel::Info); // Default
    }
}
