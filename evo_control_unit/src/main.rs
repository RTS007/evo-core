//! # EVO Control Unit
//!
//! Real-time deterministic control loop for industrial motion control.
//!
//! Supports two configuration modes:
//! - **Unified** (`--config-dir`): Loads config.toml, machine.toml, io.toml,
//!   and per-axis files via `evo_common::config::load_config_dir()`.
//! - **Legacy** (positional arg): Loads a single CU TOML via
//!   `evo_control_unit::config::load_config()`.
//!
//! The CU creates outbound P2P SHM segments (CU→HAL, CU→MQT, CU→RE),
//! attaches inbound segments (HAL→CU, optionally RE→CU and RPC→CU),
//! performs RT setup, and enters the deterministic cycle loop.

use clap::Parser;
use evo_common::config::load_config_dir;
use evo_common::io::config::IoConfig;
use evo_common::io::registry::IoRegistry;
use evo_control_unit::config::{load_config, LoadedConfig};
use evo_control_unit::cycle::{rt_setup, CycleRunner};
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn, Level};
use tracing_subscriber::EnvFilter;

/// EVO Control Unit — Real-time axis control loop
#[derive(Parser, Debug)]
#[command(name = "evo_control_unit")]
#[command(author = "RTS007")]
#[command(version)]
#[command(about = "Deterministic RT control loop for industrial motion control")]
struct Args {
    /// Path to unified config directory (config.toml + machine.toml + io.toml + axis_NN_*.toml).
    /// Preferred over positional config path.
    #[arg(long, value_name = "DIR")]
    config_dir: Option<PathBuf>,

    /// Path to legacy CU configuration TOML (cu.toml).
    /// Use --config-dir for the new unified layout.
    #[arg(default_value = "config/cu.toml")]
    config: PathBuf,

    /// CPU core to pin the RT thread to (default: 1).
    #[arg(long, default_value_t = 1)]
    cpu_core: usize,

    /// SCHED_FIFO priority (default: 80).
    #[arg(long, default_value_t = 80)]
    rt_priority: i32,

    /// Enable verbose logging (DEBUG level).
    #[arg(short, long)]
    verbose: bool,

    /// Output logs in JSON format.
    #[arg(long)]
    json: bool,
}

fn main() {
    let args = Args::parse();
    setup_tracing(&args);

    info!("EVO Control Unit v{} starting...", env!("CARGO_PKG_VERSION"));

    if let Err(e) = run(&args) {
        error!("FATAL: {e}");
        process::exit(1);
    }

    info!("EVO Control Unit shutdown complete");
}

fn run(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let loaded = if let Some(ref config_dir) = args.config_dir {
        // ── Unified config-dir path ──
        info!("Loading unified config from {:?}", config_dir);
        let full = load_config_dir(config_dir)?;
        info!(
            "Loaded {} axes from {}",
            full.axes.len(),
            config_dir.display()
        );

        // Build IoRegistry from io.toml (optional).
        let io_registry = load_io_registry(config_dir);

        // Adapt FullConfig → CU LoadedConfig.
        adapt_full_config(&full, io_registry)?
    } else {
        // ── Legacy single-file path ──
        warn!(
            "Using legacy config path '{}'. Prefer --config-dir for unified config.",
            args.config.display()
        );
        load_config(&args.config).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
    };

    info!(
        "Config OK: cycle_time={}µs, axes={}",
        loaded.cu_config.cycle_time_us,
        loaded.machine.axes.len(),
    );

    // RT setup (mlockall, affinity, scheduler).
    rt_setup(args.cpu_core, args.rt_priority)?;
    info!(
        "RT setup complete (cpu_core={}, priority={})",
        args.cpu_core, args.rt_priority
    );

    // Create CycleRunner (initializes SHM segments + runtime state).
    let mut runner = CycleRunner::new(loaded)?;
    info!("CycleRunner initialized, entering RT loop");

    // Setup signal handler for graceful shutdown.
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        r.store(false, Ordering::SeqCst);
    })?;

    // Enter the deterministic cycle loop.
    // NOTE: run() currently loops forever (or returns on overrun).
    // A future task will integrate the `running` flag into the loop.
    if let Err(e) = runner.run() {
        error!("RT loop error: {e}");
        return Err(Box::new(e) as Box<dyn std::error::Error>);
    }

    Ok(())
}

/// Adapt a `FullConfig` (from `load_config_dir`) to the CU's `LoadedConfig`.
///
/// Maps the unified config structs into the CU-specific types. If `IoRegistry`
/// is `None`, creates a default empty registry.
fn adapt_full_config(
    full: &evo_common::config::FullConfig,
    io_registry: Option<IoRegistry>,
) -> Result<LoadedConfig, Box<dyn std::error::Error>> {
    use evo_common::control_unit::config::{
        ControlUnitConfig, CuAxisConfig, CuMachineConfig, HAL_STALE_THRESHOLD_DEFAULT,
        MANUAL_TIMEOUT_DEFAULT, MQT_UPDATE_INTERVAL_DEFAULT, NON_RT_STALE_THRESHOLD_DEFAULT,
    };
    use evo_common::config::DEFAULT_CYCLE_TIME_US;

    // Build ControlUnitConfig from defaults.
    let cu_config = ControlUnitConfig {
        cycle_time_us: DEFAULT_CYCLE_TIME_US,
        max_axes: full.axes.len() as u8,
        machine_config_path: String::new(), // Not used in unified mode
        io_config_path: String::new(),      // Not used in unified mode
        manual_timeout: MANUAL_TIMEOUT_DEFAULT,
        hal_stale_threshold: HAL_STALE_THRESHOLD_DEFAULT,
        re_stale_threshold: NON_RT_STALE_THRESHOLD_DEFAULT,
        rpc_stale_threshold: NON_RT_STALE_THRESHOLD_DEFAULT,
        mqt_update_interval: MQT_UPDATE_INTERVAL_DEFAULT,
    };

    // Map axes: NewAxisConfig → CuAxisConfig.
    let axes: Vec<CuAxisConfig> = full
        .axes
        .iter()
        .map(|ax| CuAxisConfig::from_new_axis_config(ax))
        .collect();

    let machine = CuMachineConfig {
        axes,
        ..Default::default()
    };

    let registry = io_registry.unwrap_or_default();

    Ok(LoadedConfig {
        cu_config,
        machine,
        io_registry: registry,
    })
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
                        registry.di_count, registry.do_count, registry.ai_count, registry.ao_count,
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
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .compact()
            .init();
    }
}
