//! # EVO HAL Core Binary
//!
//! Hardware Abstraction Layer with pluggable driver architecture for real-time
//! sensor data, actuator control, and I/O management.
//!
//! # Usage
//!
//! ```bash
//! # Run with simulation driver (new config layout)
//! evo_hal --config-dir config/ --simulate
//!
//! # Run with specific driver(s)
//! evo_hal --config-dir config/ --driver ethercat
//!
//! # Verbose logging
//! evo_hal --config-dir config/ -s -v
//!
//! # Legacy mode (single machine.toml)
//! evo_hal --config config/machine.toml -s
//! ```

#![deny(warnings)]

use clap::Parser;
use evo_common::config::load_config_dir;
use evo_common::io::config::IoConfig;
use evo_common::io::registry::IoRegistry;
use evo_hal::core::HalCore;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tracing::{error, info, warn, Level};
use tracing_subscriber::EnvFilter;

/// EVO HAL Core - Hardware Abstraction Layer with pluggable drivers
#[derive(Parser, Debug)]
#[command(name = "evo_hal")]
#[command(author = "RTS007")]
#[command(version)]
#[command(about = "Hardware Abstraction Layer Core with pluggable driver architecture")]
#[command(long_about = None)]
struct Args {
    /// Path to unified config directory (config.toml + machine.toml + io.toml + axis_NN_*.toml).
    /// Preferred over --config.
    #[arg(long, value_name = "DIR")]
    config_dir: Option<PathBuf>,

    /// Path to legacy machine configuration file (machine.toml).
    /// Use --config-dir for the new unified layout.
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
        args.drivers[0].clone()
    } else {
        "simulation".to_string()
    };

    // --config-dir takes precedence over legacy --config
    if let Some(ref config_dir) = args.config_dir {
        info!("Loading unified config from {:?}", config_dir);
        let full_config = load_config_dir(config_dir)?;
        info!(
            "Loaded {} axes from {}",
            full_config.axes.len(),
            config_dir.display()
        );

        // Load I/O config (io.toml) and build IoRegistry.
        let io_registry = load_io_registry(config_dir);

        // Create HalCore from unified config.
        let mut hal_core = HalCore::from_full_config(full_config, io_registry)?;

        // Setup signal handler.
        let running = hal_core.running_flag();
        ctrlc::set_handler(move || {
            info!("Received shutdown signal");
            running.store(false, Ordering::SeqCst);
        })?;

        // Initialize driver + P2P segments.
        hal_core.init(&driver_name)?;

        // Run the RT loop.
        if let Err(e) = hal_core.run() {
            error!("RT loop error: {}", e);
        }

        // Shutdown (P2P writers dropped automatically).
        hal_core.shutdown()?;
    } else {
        // Legacy path (single machine.toml).
        warn!("Using legacy --config path. Prefer --config-dir for unified config.");
        let config = HalCore::load_config(&args.config)?;
        let config_dir_legacy = args.config.parent().unwrap_or(std::path::Path::new("."));

        let mut hal_core = HalCore::new(config)?;
        hal_core.load_axis_configs(config_dir_legacy)?;

        let running = hal_core.running_flag();
        ctrlc::set_handler(move || {
            info!("Received shutdown signal");
            running.store(false, Ordering::SeqCst);
        })?;

        hal_core.init(&driver_name)?;
        if let Err(e) = hal_core.run() {
            error!("RT loop error: {}", e);
        }
        hal_core.shutdown()?;
    }

    info!("EVO HAL Core shutdown complete");
    Ok(())
}

/// Load io.toml and build IoRegistry. Returns None if io.toml is missing.
fn load_io_registry(config_dir: &std::path::Path) -> Option<IoRegistry> {
    let io_path = config_dir.join("io.toml");
    match std::fs::read_to_string(&io_path) {
        Ok(content) => match IoConfig::from_toml(&content) {
            Ok(io_config) => match IoRegistry::from_config(&io_config) {
                Ok(registry) => {
                    info!(
                        "IoRegistry built: {} DI, {} DO, {} AI, {} AO",
                        registry.di_count, registry.do_count,
                        registry.ai_count, registry.ao_count,
                    );
                    Some(registry)
                }
                Err(e) => {
                    warn!("IoRegistry validation failed: {e}. Continuing without I/O roles.");
                    None
                }
            },
            Err(e) => {
                warn!("Failed to parse io.toml: {e}. Continuing without I/O roles.");
                None
            }
        },
        Err(e) => {
            info!("No io.toml found ({e}). Continuing without I/O roles.");
            None
        }
    }
}

/// Setup tracing subscriber based on CLI arguments.
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
