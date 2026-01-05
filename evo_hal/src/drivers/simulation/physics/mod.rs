//! Physics simulation module.
//!
//! This module provides physics-based simulation for axis motion,
//! including kinematics, referencing, and error detection.

mod axis;
mod referencing;

pub use axis::AxisSimulator;
pub use referencing::{ReferencingState, ReferencingStateMachine};
