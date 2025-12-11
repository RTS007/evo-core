//! Prelude module for common re-exports.
//!
//! This module provides convenient re-exports of commonly used types.
//!
//! # Usage
//!
//! ```rust
//! use evo_common::prelude::*;
//! ```

pub use log::Level as LogLevel;
pub use log::LevelFilter as LogLevelFilter;

pub use crate::config::{ConfigError, ConfigLoader, SharedConfig};
