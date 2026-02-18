//! HAL Core struct and RT loop management.
//!
//! The `HalCore` struct is the main entry point for HAL operations.
//! It manages driver loading, P2P SHM communication, and the real-time loop.

use evo_common::config::FullConfig;
use evo_common::consts::CYCLE_TIME_US;
use evo_common::hal::config::{AxisConfig, MachineConfig};
use evo_common::hal::consts::HAL_SERVICE_NAME;
use evo_common::hal::driver::{HalDriver, HalError};
use evo_common::hal::types::{HalCommands, HalStatus};
use evo_common::io::registry::IoRegistry;
use evo_common::shm::conversions::{hal_status_to_segment, segment_to_hal_commands};
use evo_common::shm::p2p::{ModuleAbbrev, ShmError, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::driver_registry::create_driver;
use crate::drivers::register_all_drivers;
use crate::module_status::ModuleStatusPublisher;

/// Default stale threshold (heartbeats) for P2P readers.
/// Readers detect staleness if writer heartbeat hasn't advanced in N reads.
const READER_STALE_THRESHOLD: u32 = 100;

/// HAL Core manages drivers and the real-time loop.
pub struct HalCore {
    /// Machine configuration (legacy path)
    config: MachineConfig,
    /// Loaded axis configurations (legacy path)
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
    /// Number of active axes (set from config)
    axis_count: u8,

    // ── P2P SHM writers (HAL is the writer/producer) ──
    /// Writer: HAL → CU (`evo_hal_cu`)
    writer_hal_cu: Option<TypedP2pWriter<HalToCuSegment>>,
    /// Writer: HAL → MQTT (`evo_hal_mqt`)
    writer_hal_mqt: Option<TypedP2pWriter<HalToMqtSegment>>,
    /// Writer: HAL → gRPC (`evo_hal_rpc`)
    writer_hal_rpc: Option<TypedP2pWriter<HalToRpcSegment>>,
    /// Writer: HAL → RE (`evo_hal_re`)
    writer_hal_re: Option<TypedP2pWriter<HalToReSegment>>,

    // ── P2P SHM readers (HAL is the reader/consumer) ──
    /// Reader: CU → HAL (`evo_cu_hal`)
    reader_cu_hal: Option<TypedP2pReader<CuToHalSegment>>,
    /// Reader: gRPC → HAL (`evo_rpc_hal`)
    reader_rpc_hal: Option<TypedP2pReader<RpcToHalSegment>>,
    /// Reader: RE → HAL (`evo_re_hal`)
    reader_re_hal: Option<TypedP2pReader<ReToHalSegment>>,

    /// I/O Registry for role-based ownership enforcement (FR-036).
    io_registry: Option<IoRegistry>,
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
    /// Create a new HalCore instance with the given configuration (legacy path).
    ///
    /// # Arguments
    /// * `config` - Machine configuration loaded from TOML
    ///
    /// # Errors
    /// Returns error if configuration validation fails.
    pub fn new(config: MachineConfig) -> Result<Self, HalError> {
        config.validate()?;

        let cycle_time = Duration::from_micros(config.cycle_time_us as u64);
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
            axis_count: 0,
            writer_hal_cu: None,
            writer_hal_mqt: None,
            writer_hal_rpc: None,
            writer_hal_re: None,
            reader_cu_hal: None,
            reader_rpc_hal: None,
            reader_re_hal: None,
            io_registry: None,
        })
    }

    /// Create a new HalCore instance from unified config (new path via --config-dir).
    ///
    /// Uses `FullConfig` from `load_config_dir()` and optional `IoRegistry`.
    pub fn from_full_config(
        full: FullConfig,
        io_registry: Option<IoRegistry>,
    ) -> Result<Self, HalError> {
        let cycle_time_us = CYCLE_TIME_US;
        let cycle_time = Duration::from_micros(cycle_time_us);
        let axis_count = full.axes.len().min(64) as u8;
        let module_status = ModuleStatusPublisher::new(HAL_SERVICE_NAME);

        // Build a legacy MachineConfig from the new format for driver compatibility.
        let mut config = MachineConfig::default();
        config.cycle_time_us = cycle_time_us as u32;
        config.drivers = vec!["simulation".to_string()];

        info!(
            "HalCore created from unified config: {} axes, cycle_time={}us",
            axis_count, cycle_time_us
        );

        Ok(Self {
            config,
            axis_configs: Vec::new(),
            driver: None,
            running: Arc::new(AtomicBool::new(false)),
            cycle_time,
            stats: TimingStats::default(),
            module_status,
            axis_count,
            writer_hal_cu: None,
            writer_hal_mqt: None,
            writer_hal_rpc: None,
            writer_hal_re: None,
            reader_cu_hal: None,
            reader_rpc_hal: None,
            reader_re_hal: None,
            io_registry,
        })
    }

    /// Load machine configuration from a TOML file (legacy path).
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

    /// Load axis configurations from files (legacy path).
    pub fn load_axis_configs(&mut self, config_dir: &Path) -> Result<(), HalError> {
        info!("Loading {} axis configuration files", self.config.axes.len());

        let mut axis_configs = Vec::with_capacity(self.config.axes.len());

        for (idx, axis_path) in self.config.axes.iter().enumerate() {
            let full_path = resolve_path(config_dir, axis_path);
            let axis_config = load_axis_config(&full_path)?;
            axis_config.validate(idx, &axis_configs)?;
            info!("  Loaded axis {}: {} ({:?})", idx, axis_config.name, axis_config.axis_type);
            axis_configs.push(axis_config);
        }

        // Check for duplicate axis names.
        let mut names = std::collections::HashSet::new();
        for axis in &axis_configs {
            if !names.insert(&axis.name) {
                return Err(HalError::ConfigError(format!(
                    "Duplicate axis name: {}",
                    axis.name
                )));
            }
        }

        self.axis_count = axis_configs.len().min(64) as u8;
        self.axis_configs = axis_configs;
        info!("Loaded {} axis configurations", self.axis_configs.len());
        Ok(())
    }

    /// Initialize the HAL Core — load driver, create P2P segments.
    pub fn init(&mut self, driver_name: &str) -> Result<(), HalError> {
        info!("Initializing HalCore with driver '{}'...", driver_name);

        // Register all built-in drivers.
        register_all_drivers();

        // Create driver instance.
        let mut driver = create_driver(driver_name)?;
        info!(
            "Created driver: {} v{}",
            driver.name(),
            driver.version()
        );

        // Initialize driver with config.
        driver.init(&self.config)?;

        // Provide loaded axis configurations to driver.
        driver.set_axis_configs(&self.axis_configs);

        self.driver = Some(driver);

        // ── P2P SHM Setup (T042/T043) ──
        self.init_p2p_writers();
        self.init_p2p_readers();

        // Initialize module status publisher for EVO supervisor integration.
        let hal_segment_name = HAL_SERVICE_NAME.to_string();
        if let Err(e) = self.module_status.init(&hal_segment_name) {
            warn!("Failed to initialize module status publisher: {:?}", e);
        }

        info!("HalCore initialized successfully");
        Ok(())
    }

    /// Create P2P writers for HAL outbound segments (T042).
    fn init_p2p_writers(&mut self) {
        // HAL → CU (active — critical for RT loop).
        match TypedP2pWriter::<HalToCuSegment>::create(
            SEG_HAL_CU,
            ModuleAbbrev::Hal,
            ModuleAbbrev::Cu,
        ) {
            Ok(w) => {
                info!("P2P writer created: evo_{}", SEG_HAL_CU);
                self.writer_hal_cu = Some(w);
            }
            Err(e) => error!("Failed to create evo_{}: {}", SEG_HAL_CU, e),
        }

        // HAL → MQTT (skeleton).
        match TypedP2pWriter::<HalToMqtSegment>::create(
            SEG_HAL_MQT,
            ModuleAbbrev::Hal,
            ModuleAbbrev::Mqt,
        ) {
            Ok(w) => {
                info!("P2P writer created: evo_{}", SEG_HAL_MQT);
                self.writer_hal_mqt = Some(w);
            }
            Err(e) => warn!("Failed to create evo_{}: {} (non-critical)", SEG_HAL_MQT, e),
        }

        // HAL → gRPC (placeholder).
        match TypedP2pWriter::<HalToRpcSegment>::create(
            SEG_HAL_RPC,
            ModuleAbbrev::Hal,
            ModuleAbbrev::Rpc,
        ) {
            Ok(w) => {
                info!("P2P writer created: evo_{}", SEG_HAL_RPC);
                self.writer_hal_rpc = Some(w);
            }
            Err(e) => warn!("Failed to create evo_{}: {} (non-critical)", SEG_HAL_RPC, e),
        }

        // HAL → RE (placeholder).
        match TypedP2pWriter::<HalToReSegment>::create(
            SEG_HAL_RE,
            ModuleAbbrev::Hal,
            ModuleAbbrev::Re,
        ) {
            Ok(w) => {
                info!("P2P writer created: evo_{}", SEG_HAL_RE);
                self.writer_hal_re = Some(w);
            }
            Err(e) => warn!("Failed to create evo_{}: {} (non-critical)", SEG_HAL_RE, e),
        }
    }

    /// Attempt to attach P2P readers for HAL inbound segments (T043).
    ///
    /// Non-blocking — segments may not exist yet (CU/RE/gRPC not started).
    fn init_p2p_readers(&mut self) {
        // CU → HAL (active).
        match TypedP2pReader::<CuToHalSegment>::attach(SEG_CU_HAL, READER_STALE_THRESHOLD) {
            Ok(r) => {
                info!("P2P reader attached: evo_{}", SEG_CU_HAL);
                self.reader_cu_hal = Some(r);
            }
            Err(ShmError::SegmentNotFound { .. }) => {
                info!("evo_{} not found yet — CU not started. Will retry.", SEG_CU_HAL);
            }
            Err(e) => warn!("Failed to attach evo_{}: {}", SEG_CU_HAL, e),
        }

        // gRPC → HAL (skeleton).
        match TypedP2pReader::<RpcToHalSegment>::attach(SEG_RPC_HAL, READER_STALE_THRESHOLD) {
            Ok(r) => {
                info!("P2P reader attached: evo_{}", SEG_RPC_HAL);
                self.reader_rpc_hal = Some(r);
            }
            Err(ShmError::SegmentNotFound { .. }) => {
                debug!("evo_{} not found (gRPC not started).", SEG_RPC_HAL);
            }
            Err(e) => warn!("Failed to attach evo_{}: {}", SEG_RPC_HAL, e),
        }

        // RE → HAL (skeleton).
        match TypedP2pReader::<ReToHalSegment>::attach(SEG_RE_HAL, READER_STALE_THRESHOLD) {
            Ok(r) => {
                info!("P2P reader attached: evo_{}", SEG_RE_HAL);
                self.reader_re_hal = Some(r);
            }
            Err(ShmError::SegmentNotFound { .. }) => {
                debug!("evo_{} not found (RE not started).", SEG_RE_HAL);
            }
            Err(e) => warn!("Failed to attach evo_{}: {}", SEG_RE_HAL, e),
        }
    }

    /// Periodically retry attaching P2P readers that weren't available at startup.
    fn retry_p2p_readers(&mut self) {
        if self.reader_cu_hal.is_none() {
            if let Ok(r) = TypedP2pReader::<CuToHalSegment>::attach(
                SEG_CU_HAL,
                READER_STALE_THRESHOLD,
            ) {
                info!("Late attach: evo_{}", SEG_CU_HAL);
                self.reader_cu_hal = Some(r);
            }
        }
        if self.reader_rpc_hal.is_none() {
            if let Ok(r) = TypedP2pReader::<RpcToHalSegment>::attach(
                SEG_RPC_HAL,
                READER_STALE_THRESHOLD,
            ) {
                info!("Late attach: evo_{}", SEG_RPC_HAL);
                self.reader_rpc_hal = Some(r);
            }
        }
        if self.reader_re_hal.is_none() {
            if let Ok(r) = TypedP2pReader::<ReToHalSegment>::attach(
                SEG_RE_HAL,
                READER_STALE_THRESHOLD,
            ) {
                info!("Late attach: evo_{}", SEG_RE_HAL);
                self.reader_re_hal = Some(r);
            }
        }
    }

    /// Run the real-time loop.
    ///
    /// This method blocks until shutdown is requested via signal or error.
    pub fn run(&mut self) -> Result<(), HalError> {
        let mut driver = self.driver.take().ok_or_else(|| {
            HalError::InitFailed("Driver not initialized".to_string())
        })?;

        info!(
            "Starting HalCore RT loop (cycle_time={}us, axes={})...",
            self.cycle_time.as_micros(),
            self.axis_count,
        );
        self.running.store(true, Ordering::SeqCst);

        let is_rt = detect_rt_mode();
        if is_rt {
            info!("Running in real-time mode");
        } else {
            info!("Running in standard (non-RT) mode");
        }

        let mut last_cycle = Instant::now();
        let mut commands = HalCommands::default();

        while self.running.load(Ordering::SeqCst) {
            let cycle_start = Instant::now();
            let dt = cycle_start.duration_since(last_cycle);
            last_cycle = cycle_start;

            // ── Read commands from SHM (T045) ──
            if let Some(ref mut reader) = self.reader_cu_hal {
                match reader.read() {
                    Ok(seg) => {
                        commands = segment_to_hal_commands(seg);
                    }
                    Err(ShmError::HeartbeatStale { .. }) => {
                        // CU heartbeat stale — zero out commands for safety.
                        commands = HalCommands::default();
                        if self.stats.cycle_count % 1000 == 0 {
                            warn!("CU heartbeat stale — using default zero commands");
                        }
                    }
                    Err(ShmError::ReadContention { .. }) => {
                        // Writer mid-write — use previous commands (acceptable).
                    }
                    Err(e) => {
                        debug!("evo_{} read error: {}", SEG_CU_HAL, e);
                    }
                }
            }

            // ── Apply I/O role ownership enforcement for RE commands (T048/FR-036) ──
            if let Some(ref mut reader) = self.reader_re_hal {
                match reader.read() {
                    Ok(re_seg) => {
                        use evo_common::io::role::IoPointType;
                        // Only apply if request_id > 0 (non-zero = valid command).
                        if re_seg.request_id > 0 {
                            let do_pin = re_seg.set_do_pin;
                            let ao_pin = re_seg.set_ao_pin;
                            let owned_do = self.io_registry.as_ref()
                                .is_some_and(|reg| reg.pin_is_role_owned(IoPointType::Do, do_pin));
                            let owned_ao = self.io_registry.as_ref()
                                .is_some_and(|reg| reg.pin_is_role_owned(IoPointType::Ao, ao_pin));

                            if owned_do {
                                debug!(
                                    "RE DO command rejected: pin {} is role-owned (req_id={})",
                                    do_pin, re_seg.request_id
                                );
                            } else if re_seg.set_do_value != 0 {
                                // Apply DO command (non-role pin).
                                commands.digital_outputs[do_pin as usize] = true;
                            }

                            if owned_ao {
                                debug!(
                                    "RE AO command rejected: pin {} is role-owned (req_id={})",
                                    ao_pin, re_seg.request_id
                                );
                            } else {
                                // Apply AO command (non-role pin).
                                commands.analog_outputs[ao_pin as usize] = re_seg.set_ao_value;
                            }
                        }
                    }
                    Err(ShmError::HeartbeatStale { .. }) | Err(ShmError::ReadContention { .. }) => {
                        // RE stale or contention — ignore.
                    }
                    Err(e) => {
                        debug!("evo_{} RE read error: {}", SEG_RE_HAL, e);
                    }
                }
            }

            // ── Execute driver cycle ──
            let status: HalStatus = driver.cycle(&commands, dt);

            // ── Write status to SHM (T044, T046, T047) ──
            if let Some(ref mut writer) = self.writer_hal_cu {
                let seg = hal_status_to_segment(&status, self.axis_count);
                if let Err(e) = writer.commit(&seg) {
                    debug!("evo_{} write error: {}", SEG_HAL_CU, e);
                }
            }

            // Write to HAL → MQT segment (superset of hal_cu plus outputs and timing).
            if let Some(ref mut writer) = self.writer_hal_mqt {
                let seg = build_hal_mqt_segment(&status, &commands, self.axis_count, dt);
                if let Err(e) = writer.commit(&seg) {
                    debug!("evo_{} write error: {}", SEG_HAL_MQT, e);
                }
            }

            // Update timing stats.
            let cycle_time_us = cycle_start.elapsed().as_micros() as u64;
            self.stats.cycle_count += 1;
            self.stats.total_cycle_time_us += cycle_time_us;
            if cycle_time_us > self.stats.max_cycle_time_us {
                self.stats.max_cycle_time_us = cycle_time_us;
            }

            // Check for timing violation.
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

            // Sleep for remaining cycle time.
            let elapsed = cycle_start.elapsed();
            if elapsed < self.cycle_time {
                std::thread::sleep(self.cycle_time - elapsed);
            }

            // Periodic tasks — once per second at 1kHz.
            let cycles_per_second = 1_000_000u64 / self.cycle_time.as_micros().max(1) as u64;
            if self.stats.cycle_count % cycles_per_second.max(1) == 0 {
                self.retry_p2p_readers();
            }

            // Update module status for EVO supervisor (every 100 cycles ≈ 100ms at 1kHz).
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

            // Debug log every 1000 cycles.
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

        // Put driver back for shutdown.
        self.driver = Some(driver);

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

        // Shutdown module status publisher.
        if let Err(e) = self.module_status.shutdown() {
            warn!("Failed to shutdown module status publisher: {:?}", e);
        }

        // Shutdown driver if present.
        if let Some(driver) = self.driver.as_mut() {
            driver.shutdown()?;
        }

        // P2P writers are dropped automatically — shm_unlink on Drop.
        // P2P readers are dropped automatically — munmap only.
        info!("HAL P2P segments will be cleaned up on drop");

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

// ─── HAL → MQT segment builder (T044, T046, T047) ──────────────────

/// Build the `HalToMqtSegment` — superset of `HalToCuSegment` plus output
/// state and timing telemetry.
fn build_hal_mqt_segment(
    status: &HalStatus,
    commands: &HalCommands,
    axis_count: u8,
    cycle_dt: Duration,
) -> HalToMqtSegment {
    use evo_common::consts::MAX_AI;
    use evo_common::shm::io_helpers::pack_bools;

    let mut seg = HalToMqtSegment::default();
    seg.axis_count = axis_count;
    seg.cycle_time_ns = cycle_dt.as_nanos() as u64;

    // Axis feedback (same as HalToCuSegment).
    let count = (axis_count as usize).min(64);
    for i in 0..count {
        let src = &status.axes[i];
        seg.axes[i] = HalAxisFeedback {
            position: src.actual_position,
            velocity: src.actual_velocity,
            torque_estimate: src.lag_error,
            drive_ready: src.ready as u8,
            drive_fault: src.error as u8,
            referenced: src.referenced as u8,
            active: (src.ready || src.moving || src.referencing) as u8,
        };
    }

    // DI bank.
    pack_bools(&status.digital_inputs, &mut seg.di_bank);

    // AI values.
    for i in 0..MAX_AI {
        seg.ai_values[i] = status.analog_inputs[i].scaled;
    }

    // DO bank (from commands — represents current output state).
    pack_bools(&commands.digital_outputs, &mut seg.do_bank);

    // AO values.
    let ao_count = commands.analog_outputs.len().min(seg.ao_values.len());
    seg.ao_values[..ao_count].copy_from_slice(&commands.analog_outputs[..ao_count]);

    seg
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
