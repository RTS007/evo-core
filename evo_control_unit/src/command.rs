//! Command processing root.
//!
//! Command arbitration (RE vs RPC), source locking, and homing supervision.

pub mod arbitration;
pub mod homing;
pub mod source_lock;
