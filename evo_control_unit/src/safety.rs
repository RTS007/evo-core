//! Safety module root.
//!
//! Safety peripheral monitoring, flag evaluation, SAFETY_STOP execution,
//! and recovery sequence.

pub mod flags;
pub mod peripherals;
pub mod recovery;
pub mod stop;
