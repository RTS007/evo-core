//! I/O Configuration — Role-Based I/O Abstraction (FR-148–FR-155).
//!
//! Shared between HAL and CU. Both parse the same `io.toml` at startup.
//! Runtime access via [`IoRegistry`] role-based lookup — O(1) HashMap,
//! no heap allocation after startup.

pub mod config;
pub mod registry;
pub mod role;
