//! # EVO Control Unit Library
//!
//! Real-time axis control brain for the EVO industrial motion control system.
//! Provides a deterministic <1ms cycle that reads sensor feedback, executes
//! hierarchical state machines, runs a universal position control engine,
//! and produces per-axis control output via P2P shared memory segments.
//!
//! ## Architecture Levels
//!
//! 1. **MachineState** — Global machine lifecycle
//! 2. **SafetyState** — Global safety overlay
//! 3. **AxisState** — 6 orthogonal per-axis state machines
//! 4. **AxisSafetyState** — Per-axis safety flags
//! 5. **AxisErrorState** — Per-axis error bitflags
//!
//! ## Zero-Allocation RT Loop
//!
//! All runtime state is pre-allocated in fixed-size arrays during startup.
//! The RT cycle performs zero heap allocations. SHM segments are memory-mapped
//! and accessed via zero-copy binary reads/writes.

#![deny(clippy::disallowed_types)]

pub mod command;
pub mod config;
pub mod control;
pub mod cycle;
pub mod error;
pub mod safety;
pub mod shm;
pub mod state;
