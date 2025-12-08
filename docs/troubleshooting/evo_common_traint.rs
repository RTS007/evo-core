//! ConfigLoader trait contract
//! 
//! This file defines the contract for configuration loading in evo_common.
//! It is a design artifact, not production code.

use std::path::Path;
use serde::de::DeserializeOwned;

/// Error type for configuration loading operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    /// Configuration file not found at specified path
    #[error("Configuration file not found")]
    FileNotFound,
    
    /// TOML parsing failed
    #[error("Failed to parse configuration: {0}")]
    ParseError(String),
    
    /// Semantic validation failed
    #[error("Configuration validation failed: {0}")]
    ValidationError(String),
}

/// Trait for loading configuration from TOML files
/// 
/// # Contract
/// 
/// Implementors must:
/// - Return `ConfigError::FileNotFound` if the file does not exist
/// - Return `ConfigError::ParseError` if TOML syntax is invalid
/// - Return `ConfigError::ValidationError` if semantic validation fails
/// 
/// # Example
/// 
/// ```rust,ignore
/// use evo::config::{ConfigLoader, SharedConfig, ConfigError};
/// use evo::shm::consts::*;
/// use std::path::Path;
/// 
/// #[derive(Debug, Deserialize)]
/// struct MyAppConfig {
///     shared: SharedConfig,
///     port: u16,
/// }
/// 
/// fn main() -> Result<(), ConfigError> {
///     let config = MyAppConfig::load(Path::new("config.toml"))?;
///     println!("Service: {}", config.shared.service_name);
///     println!("Max SHM size: {}", SHM_MAX_SIZE);
///     Ok(())
/// }
/// ```
pub trait ConfigLoader: Sized + DeserializeOwned {
    /// Load configuration from a TOML file
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
        let content = std::fs::read_to_string(path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ConfigError::FileNotFound
                } else {
                    ConfigError::ParseError(e.to_string())
                }
            })?;
        
        toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(e.to_string()))
    }
}

// Blanket implementation for all types that implement DeserializeOwned
// This allows any serde-deserializable struct to use ConfigLoader
impl<T: DeserializeOwned> ConfigLoader for T {}
