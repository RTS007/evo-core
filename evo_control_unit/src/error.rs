//! Error module root.
//!
//! Hierarchical error propagation: CRITICAL → SAFETY_STOP,
//! non-critical → axis-local only.

pub mod propagation;
