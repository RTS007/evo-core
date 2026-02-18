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
