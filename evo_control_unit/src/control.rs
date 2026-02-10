//! Control engine root.
//!
//! Universal position control engine: PID + feedforward + DOB + filters.
//! Each component activated/deactivated by setting gain parameters to zero.

pub mod dob;
pub mod feedforward;
pub mod filters;
pub mod lag;
pub mod output;
pub mod pid;
