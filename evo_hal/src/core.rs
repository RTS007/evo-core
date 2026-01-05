//! HAL Core struct and RT loop management.
//!
//! The `HalCore` struct is the main entry point for HAL operations.
//! It manages driver loading, SHM communication, and the real-time loop.

use evo_common::hal::config::{AxisConfig, MachineConfig};
use evo_common::hal::consts::HAL_SERVICE_NAME;
use evo_common::hal::driver::{HalDriver, HalError};
use evo_common::hal::types::{HalCommands, HalStatus};
use evo_shared_memory::SegmentDiscovery;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::driver_registry::create_driver;
use crate::drivers::register_all_drivers;
use crate::module_status::ModuleStatusPublisher;

/// HAL Core manages drivers and the real-time loop.
pub struct HalCore {
    /// Machine configuration
    config: MachineConfig,
    /// Loaded axis configurations
    axis_configs: Vec<AxisConfig>,
    /// Active driver instance
    driver: Option<Box<dyn HalDriver>>,
    /// Running flag for RT loop control
    running: Arc<AtomicBool>,
    /// Cycle time from config
    cycle_time: Duration,
    /// Timing statistics
    stats: TimingStats,
    /// Module status publisher for EVO supervisor integration
    module_status: ModuleStatusPublisher,
}

/// Timing statistics for RT loop monitoring.
#[derive(Debug, Default)]
struct TimingStats {
    /// Number of cycles executed
    cycle_count: u64,
    /// Number of timing violations (cycle exceeded target)
    timing_violations: u64,
    /// Maximum observed cycle time
    max_cycle_time_us: u64,
    /// Sum of cycle times for average calculation
    total_cycle_time_us: u64,
}

impl HalCore {
    /// Create a new HalCore instance with the given configuration.
    ///
    /// # Arguments
    /// * `config` - Machine configuration loaded from TOML
    ///
    /// # Errors
    /// Returns error if configuration validation fails.
    pub fn new(config: MachineConfig) -> Result<Self, HalError> {
        // Validate configuration
        config.validate()?;

        let cycle_time = Duration::from_micros(config.cycle_time_us as u64);
        
        // Create module status publisher with canonical service name (ignore TOML override)
        let module_status = ModuleStatusPublisher::new(HAL_SERVICE_NAME);
        
        info!(
            "HalCore created with {} axis config paths, cycle_time={}us",
            config.axes.len(),
            config.cycle_time_us
        );

        Ok(Self {
            config,
            axis_configs: Vec::new(),
            driver: None,
            running: Arc::new(AtomicBool::new(false)),
            cycle_time,
            stats: TimingStats::default(),
            module_status,
        })
    }

    /// Load machine configuration from a TOML file.
    ///
    /// # Arguments
    /// * `config_path` - Path to machine.toml file
    ///
    /// # Returns
    /// Loaded and validated MachineConfig
    pub fn load_config(config_path: &Path) -> Result<MachineConfig, HalError> {
        info!("Loading configuration from {:?}", config_path);

        let content = fs::read_to_string(config_path).map_err(|e| {
            HalError::ConfigError(format!("Failed to read config file {:?}: {}", config_path, e))
        })?;

        let config: MachineConfig = toml::from_str(&content).map_err(|e| {
            HalError::ConfigError(format!("Failed to parse config file {:?}: {}", config_path, e))
        })?;

        info!(
            "Loaded config: drivers={:?}, {} axis files",
            config.drivers,
            config.axes.len()
        );

        Ok(config)
    }

    /// Load axis configurations from files.
    ///
    /// # Arguments
    /// * `config_dir` - Base directory for resolving relative paths
    ///
    /// # Errors
    /// Returns error if any axis config file cannot be loaded.
    pub fn load_axis_configs(&mut self, config_dir: &Path) -> Result<(), HalError> {
        info!("Loading {} axis configuration files", self.config.axes.len());

        let mut axis_configs = Vec::with_capacity(self.config.axes.len());

        for (idx, axis_path) in self.config.axes.iter().enumerate() {
            let full_path = resolve_path(config_dir, axis_path);
            let axis_config = load_axis_config(&full_path)?;

            // Validate axis config
            axis_config.validate(idx, &axis_configs)?;

            info!("  Loaded axis {}: {} ({:?})", idx, axis_config.name, axis_config.axis_type);
            axis_configs.push(axis_config);
        }

        // Check for duplicate axis names
        let mut names = std::collections::HashSet::new();
        for axis in &axis_configs {
            if !names.insert(&axis.name) {
                return Err(HalError::ConfigError(format!(
                    "Duplicate axis name: {}",
                    axis.name
                )));
            }
        }

        self.axis_configs = axis_configs;
        info!("Loaded {} axis configurations", self.axis_configs.len());
        Ok(())
    }

    /// Initialize the HAL Core - load driver and initialize SHM.
    ///
    /// # Arguments
    /// * `driver_name` - Name of driver to load (e.g., "simulation")
    ///
    /// # Errors
    /// Returns error if driver initialization or SHM setup fails.
    pub fn init(&mut self, driver_name: &str) -> Result<(), HalError> {
        info!("Initializing HalCore with driver '{}'...", driver_name);

        // Register all built-in drivers
        register_all_drivers();

        // Create driver instance
        let mut driver = create_driver(driver_name)?;
        info!(
            "Created driver: {} v{}",
            driver.name(),
            driver.version()
        );

        // Initialize driver with config
        driver.init(&self.config)?;

        // Provide loaded axis configurations to driver
        driver.set_axis_configs(&self.axis_configs);

        self.driver = Some(driver);
        
        // Prevent double-start: check for existing HAL/module SHM segments
        let module_segment = format!("module_{}", HAL_SERVICE_NAME);
        let hal_segment = format!("{}", HAL_SERVICE_NAME);
        let discovery = SegmentDiscovery::new();
        let existing = discovery
            .list_segments()
            .map_err(|e| HalError::ShmError(e.to_string()))?;
        if let Some(seg) = existing
            .iter()
            .find(|s| s.name == module_segment || s.name == hal_segment)
        {
            return Err(HalError::InitFailed(format!(
                "HAL already running (segment {} by pid {})",
                seg.name, seg.writer_pid
            )));
        }

        // Initialize module status publisher for EVO supervisor integration
        // Base name only; SegmentWriter will add evo_ prefix and PID
        let hal_segment_name = hal_segment;
        if let Err(e) = self.module_status.init(&hal_segment_name) {
            warn!("Failed to initialize module status publisher: {:?}", e);
            // Non-fatal - HAL can still run without supervisor integration
        }
        
        info!("HalCore initialized successfully");
        Ok(())
    }

    /// Run the real-time loop.
    ///
    /// This method blocks until shutdown is requested via signal or error.
    ///
    /// # Errors
    /// Returns error if the RT loop encounters an unrecoverable error.
    pub fn run(&mut self) -> Result<(), HalError> {
        let driver = self.driver.as_mut().ok_or_else(|| {
            HalError::InitFailed("Driver not initialized".to_string())
        })?;

        info!(
            "Starting HalCore RT loop (cycle_time={}us)...",
            self.cycle_time.as_micros()
        );
        self.running.store(true, Ordering::SeqCst);

        // Detect RT mode
        let is_rt = detect_rt_mode();
        if is_rt {
            info!("Running in real-time mode");
        } else {
            info!("Running in standard (non-RT) mode");
        }

        let mut last_cycle = Instant::now();
        let commands = HalCommands::default();

        while self.running.load(Ordering::SeqCst) {
            let cycle_start = Instant::now();
            let dt = cycle_start.duration_since(last_cycle);
            last_cycle = cycle_start;

            // Execute driver cycle
            let _status: HalStatus = driver.cycle(&commands, dt);

            // TODO: Read commands from SHM
            // TODO: Write status to SHM

            // Update timing stats
            let cycle_time_us = cycle_start.elapsed().as_micros() as u64;
            self.stats.cycle_count += 1;
            self.stats.total_cycle_time_us += cycle_time_us;
            if cycle_time_us > self.stats.max_cycle_time_us {
                self.stats.max_cycle_time_us = cycle_time_us;
            }

            // Check for timing violation
            if cycle_time_us > self.config.cycle_time_us as u64 {
                self.stats.timing_violations += 1;
                if self.stats.timing_violations <= 10 || self.stats.timing_violations % 1000 == 0 {
                    warn!(
                        "Timing violation #{}: cycle took {}us (target {}us)",
                        self.stats.timing_violations,
                        cycle_time_us,
                        self.config.cycle_time_us
                    );
                }
            }

            // Sleep for remaining cycle time
            let elapsed = cycle_start.elapsed();
            if elapsed < self.cycle_time {
                std::thread::sleep(self.cycle_time - elapsed);
            }

            // Update module status for EVO supervisor (every 100 cycles = ~100ms at 1kHz)
            if self.stats.cycle_count % 100 == 0 {
                let avg_cycle = if self.stats.cycle_count > 0 {
                    self.stats.total_cycle_time_us / self.stats.cycle_count
                } else {
                    0
                };
                self.module_status.update_timing_metrics(
                    self.stats.cycle_count,
                    avg_cycle,
                    self.stats.max_cycle_time_us,
                    self.stats.timing_violations,
                );
                if let Err(e) = self.module_status.update() {
                    debug!("Failed to update module status: {:?}", e);
                }
            }

            // Debug log every 1000 cycles
            if self.stats.cycle_count % 1000 == 0 {
                debug!(
                    "RT loop: {} cycles, avg={}us, max={}us, violations={}",
                    self.stats.cycle_count,
                    self.stats.total_cycle_time_us / self.stats.cycle_count,
                    self.stats.max_cycle_time_us,
                    self.stats.timing_violations
                );
            }
        }

        info!(
            "HalCore RT loop stopped after {} cycles (violations: {})",
            self.stats.cycle_count, self.stats.timing_violations
        );
        Ok(())
    }

    /// Request shutdown of the RT loop.
    pub fn shutdown(&mut self) -> Result<(), HalError> {
        info!("Shutdown requested");
        self.running.store(false, Ordering::SeqCst);

        // Shutdown module status publisher
        if let Err(e) = self.module_status.shutdown() {
            warn!("Failed to shutdown module status publisher: {:?}", e);
        }

        // Shutdown driver if present
        if let Some(driver) = self.driver.as_mut() {
            driver.shutdown()?;
        }

        Ok(())
    }

    /// Get the running flag for signal handlers.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.running)
    }

    /// Get the loaded axis configurations.
    pub fn axis_configs(&self) -> &[AxisConfig] {
        &self.axis_configs
    }

    /// Get timing statistics.
    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.stats.cycle_count,
            self.stats.timing_violations,
            self.stats.max_cycle_time_us,
        )
    }
}

/// Resolve a possibly relative path against a base directory.
fn resolve_path(base: &Path, path: &PathBuf) -> PathBuf {
    if path.is_absolute() {
        path.clone()
    } else {
        base.join(path)
    }
}

/// Load a single axis configuration from a TOML file.
fn load_axis_config(path: &Path) -> Result<AxisConfig, HalError> {
    let content = fs::read_to_string(path).map_err(|e| {
        HalError::ConfigError(format!("Failed to read axis config {:?}: {}", path, e))
    })?;

    toml::from_str(&content).map_err(|e| {
        HalError::ConfigError(format!("Failed to parse axis config {:?}: {}", path, e))
    })
}

/// Detect if running in real-time mode by checking scheduler policy.
fn detect_rt_mode() -> bool {
    #[cfg(target_os = "linux")]
    {
        use libc::{sched_getscheduler, SCHED_FIFO, SCHED_RR};
        unsafe {
            let policy = sched_getscheduler(0);
            policy == SCHED_FIFO || policy == SCHED_RR
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}
