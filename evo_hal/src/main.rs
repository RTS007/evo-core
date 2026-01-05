//! # EVO HAL Core Binary
//!
//! Hardware Abstraction Layer with pluggable driver architecture for real-time
//! sensor data, actuator control, and I/O management.
//!
//! # Usage
//!
//! ```bash
//! # Run with simulation driver (exclusive mode)
//! evo_hal --config config/machine.toml --simulate
//!
//! # Run with specific driver(s)
//! evo_hal --config config/machine.toml --driver ethercat
//!
//! # Verbose logging
//! evo_hal -c config/machine.toml -s -v
//! ```

#![deny(warnings)]

use clap::Parser;
use evo_hal::core::HalCore;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tracing::{error, info, Level};
use tracing_subscriber::EnvFilter;

/// EVO HAL Core - Hardware Abstraction Layer with pluggable drivers
#[derive(Parser, Debug)]
#[command(name = "evo_hal")]
#[command(author = "RTS007")]
#[command(version)]
#[command(about = "Hardware Abstraction Layer Core with pluggable driver architecture")]
#[command(long_about = None)]
struct Args {
    /// Path to machine configuration file (machine.toml)
    #[arg(short, long, default_value = "/etc/evo/machine.toml")]
    config: PathBuf,

    /// Force simulation driver (exclusive - ignores all other drivers)
    #[arg(short = 's', long)]
    simulate: bool,

    /// Load specific driver (can be specified multiple times)
    #[arg(short, long = "driver", action = clap::ArgAction::Append)]
    drivers: Vec<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Output logs in JSON format
    #[arg(long)]
    json: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = run() {
        // Use tracing for errors so formatting matches INFO logs
        error!("HAL startup failed: {}", e);
        std::process::exit(1);
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize tracing
    setup_tracing(&args);

    info!("EVO HAL Core v{} starting...", env!("CARGO_PKG_VERSION"));

    // Determine driver to use
    let driver_name = if args.simulate {
        info!("Simulation mode enabled (exclusive)");
        "simulation".to_string()
    } else if !args.drivers.is_empty() {
        info!("Drivers from CLI: {:?}", args.drivers);
        // For now, use first driver (multi-driver support is future work)
        args.drivers[0].clone()
    } else {
        // Load from config file to determine drivers
        "simulation".to_string() // Default to simulation if nothing specified
    };

    // Load configuration
    let config = HalCore::load_config(&args.config)?;
    
    // Get config directory for resolving relative paths
    let config_dir = args.config.parent().unwrap_or(std::path::Path::new("."));

    // Create HalCore
    let mut hal_core = HalCore::new(config)?;

    // Load axis configurations
    hal_core.load_axis_configs(config_dir)?;

    // Setup signal handlers
    let running = hal_core.running_flag();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        running.store(false, Ordering::SeqCst);
    })?;

    // Initialize HAL Core with driver
    hal_core.init(&driver_name)?;

    // Run the RT loop
    if let Err(e) = hal_core.run() {
        error!("RT loop error: {}", e);
    }

    // Shutdown
    hal_core.shutdown()?;

    info!("EVO HAL Core shutdown complete");
    Ok(())
}

/// Setup tracing subscriber based on CLI arguments
fn setup_tracing(args: &Args) {
    let level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let filter = EnvFilter::from_default_env().add_directive(level.into());

    if args.json {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
}
