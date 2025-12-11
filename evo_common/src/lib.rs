//! EVO Common Library
//!
//! This crate provides shared constants and configuration loading utilities
//! for all EVO workspace crates.
//!
//! # Module Structure
//!
//! - [`shm`] - Shared memory constants and configuration
//! - [`hal`] - Hardware abstraction layer constants and configuration
//! - [`config`] - Configuration loading traits and types
//! - [`prelude`] - Common re-exports for convenience
//!
//! # Usage
//!
//! Add to your `Cargo.toml` with alias for shorter imports:
//! ```toml
//! [dependencies]
//! evo = { package = "evo_common", path = "../evo_common" }
//! ```
//!
//! Then import:
//! ```rust
//! use evo::shm::consts::*;
//! use evo::config::{ConfigLoader, SharedConfig};
//! ```

pub mod config;
pub mod hal;
pub mod prelude;
pub mod shm;
