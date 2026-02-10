//! # EVO Control Unit
//!
//! Real-time deterministic control loop for industrial motion control.
//!
//! This binary will be implemented in T031–T034:
//! - RT thread setup (mlockall, sched_setaffinity, clock_nanosleep)
//! - SHM segment attachment
//! - Deterministic cycle body
//!
//! For now it loads & validates configuration, then exits.

use evo_control_unit::config::load_config;
use std::path::Path;
use std::process;

fn main() {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/cu.toml".to_string());

    eprintln!("evo_control_unit: loading config from {config_path}");

    match load_config(Path::new(&config_path)) {
        Ok(loaded) => {
            eprintln!(
                "Config OK: cycle_time={}µs, axes={}",
                loaded.cu_config.cycle_time_us,
                loaded.machine.axes.len(),
            );
        }
        Err(e) => {
            eprintln!("FATAL: {e}");
            process::exit(1);
        }
    }

    eprintln!("evo_control_unit: placeholder — RT loop not yet implemented (T031-T034)");
}
