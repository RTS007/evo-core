//! # EVO HAL Library
//!
//! Hardware Abstraction Layer Core with pluggable driver architecture.
//!
//! This crate provides the HAL Core binary and driver modules for hardware abstraction.
//! Drivers implement the `HalDriver` trait defined in `evo_common::hal::driver`.
//!
//! # Module Structure
//!
//! - [`core`] - HalCore struct, RT loop management
//! - [`driver_registry`] - Driver factory registration
//! - [`drivers`] - HAL driver implementations
//! - [`module_status`] - Module status publishing (stub)
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                     evo_hal (single crate)                       │
//! │  ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐  │
//! │  │   P2P SHM   │◄──►│  HalCore     │◄──►│  Driver Registry    │  │
//! │  │  (evo_common)│    │  (RT Loop)   │    │                     │  │
//! │  └─────────────┘    └──────┬───────┘    └─────────────────────┘  │
//! │                            │                                     │
//! │                            ▼                                     │
//! │                   ┌────────────────┐                             │
//! │                   │  HalDriver     │ (trait object)              │
//! │                   │  trait         │                             │
//! │                   └────────────────┘                             │
//! └──────────────────────────────────────────────────────────────────┘
//! ```

#![deny(warnings)]
#![deny(missing_docs)]

pub mod core;
pub mod driver_registry;
pub mod drivers;
pub mod module_status;

// Re-export key types for convenience
pub use crate::core::HalCore;
pub use crate::driver_registry::{get_driver_factory, register_driver, DriverRegistry};
pub use crate::module_status::ModuleStatusPublisher;
