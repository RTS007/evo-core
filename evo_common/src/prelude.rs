//! Prelude module for common re-exports.
//!
//! This module provides convenient re-exports of commonly used types.
//!
//! # Usage
//!
//! ```rust
//! use evo_common::prelude::*;
//! ```

use std::time::Duration;

pub use log::Level as LogLevel;
pub use log::LevelFilter as LogLevelFilter;

pub use crate::config::{ConfigError, ConfigLoader, SharedConfig};

/// Default system cycle time in microseconds (1ms = 1000us).
/// Used by all real-time components: HAL, Control Unit, etc.
pub const DEFAULT_CYCLE_TIME_US: u32 = 1000;

/// Default system cycle time as Duration.
pub const DEFAULT_CYCLE_TIME: Duration = Duration::from_micros(DEFAULT_CYCLE_TIME_US as u64);

