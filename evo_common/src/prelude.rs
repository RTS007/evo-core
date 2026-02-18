//! Prelude module for common re-exports.
//!
//! This module provides convenient re-exports of commonly used types
//! so that consumers can do `use evo_common::prelude::*;` and get
//! the most important types without listing individual paths.
//!
//! # Usage
//!
//! ```rust
//! use evo_common::prelude::*;
//! ```

use std::time::Duration;

// ─── Logging ────────────────────────────────────────────────────────
pub use crate::config::LogLevel;

// ─── Configuration ──────────────────────────────────────────────────
pub use crate::config::{ConfigError, ConfigLoader, SharedConfig};

// ─── System Constants ───────────────────────────────────────────────
pub use crate::consts::{CYCLE_TIME_US, MAX_AXES};

// ─── I/O ────────────────────────────────────────────────────────────
pub use crate::io::registry::IoRegistry;
pub use crate::io::role::IoRole;

// ─── P2P Shared Memory ─────────────────────────────────────────────
pub use crate::shm::p2p::{
    ModuleAbbrev, P2pSegmentHeader, ShmError, TypedP2pReader, TypedP2pWriter,
};

/// Default system cycle time in microseconds (1ms = 1000us).
/// Used by all real-time components: HAL, Control Unit, etc.
pub const DEFAULT_CYCLE_TIME_US: u32 = 1000;

/// Default system cycle time as Duration.
pub const DEFAULT_CYCLE_TIME: Duration = Duration::from_micros(DEFAULT_CYCLE_TIME_US as u64);

