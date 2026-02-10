//! Control Unit shared types (FR-140).
//!
//! All types shared between the Control Unit and other EVO modules live here.
//! Organized by domain: state enums, error bitflags, safety types, control
//! parameters, command types, homing configuration, SHM segment payloads,
//! and configuration structures.

pub mod command;
pub mod config;
pub mod control;
pub mod error;
pub mod homing;
pub mod safety;
pub mod shm;
pub mod state;
