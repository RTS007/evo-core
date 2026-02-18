//! Shared memory subsystem.
//!
//! This module contains:
//! - `p2p`: The P2P lock-free segment writer/reader (sole SHM transport).
//! - `consts`: SHM size limits and cache line constants.
//! - `io_helpers`: Bit-packed digital I/O bank helpers.
//!
//! Future submodules (added when implementing US7):
//! - `segments`: All 15 typed SHM segment structs.
//! - `conversions`: HAL↔SHM data type conversions.

pub mod consts;
pub mod conversions;
pub mod io_helpers;
pub mod p2p;
pub mod segments;
