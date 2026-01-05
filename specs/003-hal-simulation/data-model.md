# Data Model: HAL Core + Simulation Driver

**Feature**: 003-hal-simulation | **Date**: 2025-12-10

---

## HAL Driver Trait (in `evo_common::hal::driver`)

The core abstraction that all HAL drivers must implement. Drivers are located in `evo_hal/src/drivers/`.

```rust
/// Trait defining the interface for HAL drivers.
/// 
/// HAL Core manages drivers through this trait, enabling pluggable
/// hardware backends (simulation, EtherCAT, CANopen, etc.).
pub trait HalDriver: Send + Sync {
    /// Driver identification
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    
    /// Initialize the driver with machine configuration.
    /// Called once before the RT loop starts.
    /// 
    /// # Errors
    /// Returns error if hardware initialization fails.
    fn init(&mut self, config: &MachineConfig) -> Result<(), HalError>;
    
    /// Execute one cycle of the driver.
    /// Called every cycle_time_us from HAL Core's RT loop.
    /// 
    /// # Arguments
    /// * `commands` - Commands from Control Unit (read from SHM by HAL Core)
    /// * `dt` - Actual time since last cycle (for physics calculations)
    /// 
    /// # Returns
    /// Updated status to be written to SHM by HAL Core.
    fn cycle(&mut self, commands: &HalCommands, dt: Duration) -> HalStatus;
    
    /// Graceful shutdown of the driver.
    /// Called when HAL Core is stopping.
    fn shutdown(&mut self) -> Result<(), HalError>;
    
    /// Check if driver supports hot-swap (runtime replacement).
    fn supports_hot_swap(&self) -> bool { false }
    
    /// Get driver-specific diagnostics (optional).
    fn diagnostics(&self) -> Option<DriverDiagnostics> { None }
}

/// Factory function type for creating driver instances.
pub type DriverFactory = fn() -> Box<dyn HalDriver>;
```

---

## HAL Core Types (in `evo_common::hal::types`)

### HalCommands

Commands read from SHM, passed to driver.

```rust
#[derive(Debug, Clone, Default)]
pub struct HalCommands {
    /// Per-axis commands
    pub axes: [AxisCommand; MAX_AXES],
    
    /// Digital output states (from Control Unit)
    pub digital_outputs: [bool; MAX_DO],
    
    /// Analog output values (normalized 0.0-1.0)
    pub analog_outputs: [f64; MAX_AO],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AxisCommand {
    /// Target position in user units
    pub target_position: f64,
    /// Enable axis
    pub enable: bool,
    /// Reset error
    pub reset: bool,
    /// Start referencing
    pub reference: bool,
}
```

### HalStatus

Status returned by driver, written to SHM.

```rust
#[derive(Debug, Clone, Default)]
pub struct HalStatus {
    /// Per-axis status
    pub axes: [AxisStatus; MAX_AXES],
    
    /// Digital input states (from hardware/simulation)
    pub digital_inputs: [bool; MAX_DI],
    
    /// Analog input values (normalized 0.0-1.0, scaled)
    pub analog_inputs: [AnalogValue; MAX_AI],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AxisStatus {
    /// Actual position in user units
    pub actual_position: f64,
    /// Actual velocity in user units/sec
    pub actual_velocity: f64,
    /// Current lag error
    pub lag_error: f64,
    /// Axis ready for motion
    pub ready: bool,
    /// Axis in error state
    pub error: bool,
    /// Axis is referenced
    pub referenced: bool,
    /// Referencing in progress
    pub referencing: bool,
    /// Axis is moving
    pub moving: bool,
    /// At target position (within in_position_window)
    /// True when |actual_position - target_position| <= in_position_window
    pub in_position: bool,
    /// Error code (0 = no error)
    pub error_code: u16,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AnalogValue {
    /// Normalized value (0.0 - 1.0)
    pub normalized: f64,
    /// Scaled value in engineering units
    pub scaled: f64,
}
```

### HalError

```rust
#[derive(Debug, Clone, Error)]
pub enum HalError {
    #[error("Driver initialization failed: {0}")]
    InitFailed(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Hardware communication error: {0}")]
    CommunicationError(String),
    
    #[error("Driver not found: {0}")]
    DriverNotFound(String),
    
    #[error("State persistence error: {0}")]
    PersistenceError(String),
}
```

---

## Configuration Entities (in `evo_common::hal::config`)

### MachineConfig

Main configuration loaded from `machine.toml`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfig {
    /// Shared configuration (service name, log level)
    pub shared: SharedConfig,
    
    /// System cycle time in microseconds
    /// Defaults to evo_common::prelude::DEFAULT_CYCLE_TIME_US (1000μs) if omitted
    #[serde(default = "default_cycle_time_us")]
    pub cycle_time_us: u32,
    
    /// Path to state persistence file (relative to config dir)
    /// Used by all drivers to persist axis positions across restarts
    pub state_file: Option<PathBuf>,
    
    /// List of HAL drivers to load (e.g., ["ethercat", "canopen"])
    /// Note: "simulation" cannot be mixed with other drivers
    #[serde(default)]
    pub drivers: Vec<String>,
    
    /// Per-driver configuration sections
    /// Key = driver name, Value = driver-specific TOML table
    #[serde(default)]
    pub driver_config: HashMap<String, toml::Value>,
    
    /// Paths to axis configuration files (relative to config dir)
    #[serde(default)]
    pub axes: Vec<PathBuf>,
    
    /// Digital input configuration
    #[serde(default)]
    pub digital_inputs: Vec<DigitalIOConfig>,
    
    /// Digital output configuration
    #[serde(default)]
    pub digital_outputs: Vec<DigitalIOConfig>,
    
    /// Analog input configuration
    #[serde(default)]
    pub analog_inputs: Vec<AnalogIOConfig>,
    
    /// Analog output configuration
    #[serde(default)]
    pub analog_outputs: Vec<AnalogIOConfig>,
}
```

### AxisConfig

Per-axis configuration loaded from individual `axis_XX.toml` files.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisConfig {
    /// Axis name (unique identifier)
    pub name: String,
    
    /// Axis type
    pub axis_type: AxisType,
    
    /// Encoder resolution (increments per user unit) - required for types 1,2,3
    pub encoder_resolution: Option<f64>,
    
    /// Maximum velocity in user units per second - required for type 1
    pub max_velocity: Option<f64>,
    
    /// Maximum acceleration in user units per second² - required for type 1
    pub max_acceleration: Option<f64>,
    
    /// Lag error limit in user units - required for type 1
    pub lag_error_limit: Option<f64>,
    
    /// Master axis index (0-based) - required for type 2 (Slave)
    pub master_axis: Option<usize>,

    /// Coupling offset for Slave axes - captured at coupling time
    /// slave_position = master_position + coupling_offset
    #[serde(default)]
    pub coupling_offset: Option<f64>,
    
    /// In-position window in user units (e.g., 0.1 mm)
    /// Axis is "in position" when |actual - target| <= in_position_window
    /// Default: 0.01 user units
    #[serde(default = "default_in_position_window")]
    pub in_position_window: f64,
    
    /// Referencing configuration
    #[serde(default)]
    pub referencing: ReferencingConfig,
    
    /// Software limits
    pub soft_limit_positive: Option<f64>,
    pub soft_limit_negative: Option<f64>,
}
```

### AxisType

Enum defining the 4 supported axis types.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum AxisType {
    /// On/off axis without position feedback
    Simple = 0,
    /// Full servo axis with encoder and kinematics
    Positioning = 1,
    /// Axis coupled to master axis
    Slave = 2,
    /// Encoder-only axis without drive
    Measurement = 3,
}
```

### ReferencingConfig

Configuration for axis referencing behavior.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReferencingConfig {
    /// Whether referencing is required: "yes", "perhaps", "no"
    #[serde(default)]
    pub required: ReferencingRequired,
    
    /// Referencing mode (0-5)
    #[serde(default)]
    pub mode: ReferencingMode,
    
    /// Digital input index for reference switch
    pub reference_switch: Option<usize>,
    
    /// True if reference switch is normally closed
    #[serde(default)]
    pub normally_closed: bool,
    
    /// True if referencing moves in negative direction first
    #[serde(default = "default_true")]
    pub negative_direction: bool,
    
    /// Referencing speed in user units per second
    #[serde(default = "default_ref_speed")]
    pub speed: f64,
    
    /// Show error if K0 distance is too small
    #[serde(default)]
    pub show_k0_distance_error: bool,
    
    //=== Simulation-specific fields ===
    
    /// Position where virtual reference switch activates (simulation only)
    /// Default: 0.0 user units
    #[serde(default)]
    pub reference_switch_position: f64,
    
    /// Position where virtual K0 index pulse triggers (simulation only)
    /// Default: 0.0 user units
    #[serde(default)]
    pub index_pulse_position: f64,
}

fn default_true() -> bool { true }
fn default_ref_speed() -> f64 { 5.0 }
fn default_in_position_window() -> f64 { 0.01 }
```

### ReferencingRequired

Enum for referencing requirement level.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReferencingRequired {
    /// Always require referencing on startup
    Yes,
    /// Use persisted position if available, else require referencing
    Perhaps,
    /// Never require referencing
    #[default]
    No,
}
```

### ReferencingMode

Enum for the 6 referencing modes.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum ReferencingMode {
    /// No referencing needed
    #[default]
    None = 0,
    /// Reference switch + K0 index pulse
    SwitchThenIndex = 1,
    /// Reference switch only
    SwitchOnly = 2,
    /// K0 index pulse only
    IndexOnly = 3,
    /// Limit switch + K0 index pulse
    LimitThenIndex = 4,
    /// Limit switch only
    LimitOnly = 5,
}
```

### DigitalIOConfig

Configuration for digital I/O points.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigitalIOConfig {
    /// I/O point name
    pub name: String,
    
    /// Optional description
    pub description: Option<String>,
    
    /// Initial value for simulation (inputs only)
    /// Values: "on" or "off" (default: "off")
    #[serde(default, with = "on_off_bool")]
    pub initial_value: bool,
    
    /// Linked DI reactions for simulation (outputs only)
    /// Format: ["on"/"off", delay_s, di_index, "on"/"off"]
    /// - trigger: DO state that triggers ("on"/"off")
    /// - delay_s: delay in seconds
    /// - di_index: index of DI to affect
    /// - result: DI state to set ("on"/"off")
    #[serde(default)]
    pub linked_inputs: Vec<LinkedReaction>,
}

/// Serde helper for on/off string to bool conversion
mod on_off_bool {
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<bool, D::Error>
    where D: Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err(serde::de::Error::custom("expected 'on' or 'off'"))
        }
    }
    
    pub fn serialize<S>(value: &bool, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_str(if *value { "on" } else { "off" })
    }
}

/// Linked DI reaction: [trigger, delay_s, di_index, result]
/// Deserializes from: ["on", 0.1, 0, "off"]
pub type LinkedReaction = (OnOff, f64, usize, OnOff);

/// On/Off enum for readable config
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnOff {
    On,
    Off,
}

impl From<OnOff> for bool {
    fn from(v: OnOff) -> bool { matches!(v, OnOff::On) }
}

impl From<bool> for OnOff {
    fn from(v: bool) -> OnOff { if v { OnOff::On } else { OnOff::Off } }
}
```

**Example: Pneumatic Cylinder Simulation**

```toml
[[digital_inputs]]
name = "di_cylinder_closed"
initial_value = "on"

[[digital_inputs]]
name = "di_cylinder_open"
initial_value = "off"

# DO controls cylinder valve
# Format: [trigger, delay_s, di_index, result]
[[digital_outputs]]
name = "do_cylinder_extend"
linked_inputs = [
    ["on",  0.1, 0, "off"],  # DO ON  -> 0.1s -> DI[0] OFF (closed sensor)
    ["on",  0.8, 1, "on" ],  # DO ON  -> 0.8s -> DI[1] ON  (open sensor)
    ["off", 0.1, 1, "off"],  # DO OFF -> 0.1s -> DI[1] OFF (open sensor)
    ["off", 0.8, 0, "on" ],  # DO OFF -> 0.8s -> DI[0] ON  (closed sensor)
]
```

### AnalogIOConfig

Configuration for analog I/O points with scaling.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalogIOConfig {
    /// I/O point name
    pub name: String,
    
    /// Minimum scaled value (engineering units)
    #[serde(default)]
    pub min_value: f64,
    
    /// Maximum scaled value (engineering units)
    #[serde(default = "default_max")]
    pub max_value: f64,
    
    /// Scaling curve configuration
    #[serde(default)]
    pub curve: AnalogCurve,
    
    /// Engineering unit name (e.g., "bar", "°C", "V")
    pub unit: Option<String>,
    
    /// Initial value for simulation (inputs only, in engineering units)
    /// Default: min_value
    pub initial_value: Option<f64>,
}

fn default_max() -> f64 { 1.0 }
```

### AnalogCurve

Scaling curve definition. All curves are polynomials internally: `f(n) = a×n³ + b×n² + c×n + d`

```rust
/// Analog scaling curve using polynomial representation.
/// 
/// All curves are polynomials: f(n) = a×n³ + b×n² + c×n + d
/// where n = normalized value (0.0-1.0)
/// 
/// Constraint: a + b + c + d = 1.0 (ensures f(1) = 1)
/// 
/// Named presets in config:
/// - "linear": a=0, b=0, c=1, d=0
/// - "quadratic": a=0, b=1, c=0, d=0
/// - "cubic": a=1, b=0, c=0, d=0
/// - Custom coefficients also supported
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(from = "AnalogCurveConfig", into = "AnalogCurveConfig")]
pub struct AnalogCurve {
    /// Cubic coefficient (n³)
    pub a: f64,
    /// Quadratic coefficient (n²)
    pub b: f64,
    /// Linear coefficient (n)
    pub c: f64,
    /// Constant offset
    pub d: f64,
}

impl Default for AnalogCurve {
    fn default() -> Self {
        Self::LINEAR
    }
}

impl AnalogCurve {
    /// Linear: f(n) = n
    pub const LINEAR: Self = Self { a: 0.0, b: 0.0, c: 1.0, d: 0.0 };
    
    /// Quadratic: f(n) = n²
    pub const QUADRATIC: Self = Self { a: 0.0, b: 1.0, c: 0.0, d: 0.0 };
    
    /// Cubic: f(n) = n³
    pub const CUBIC: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 0.0 };
    
    /// Create custom polynomial
    pub const fn new(a: f64, b: f64, c: f64, d: f64) -> Self {
        Self { a, b, c, d }
    }
}

/// Config representation for serde (supports named presets)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AnalogCurveConfig {
    /// Named preset: "linear", "quadratic", "cubic"
    Named(String),
    /// Custom coefficients
    Custom { a: f64, b: f64, c: f64, d: f64 },
}

impl From<AnalogCurveConfig> for AnalogCurve {
    fn from(config: AnalogCurveConfig) -> Self {
        match config {
            AnalogCurveConfig::Named(name) => match name.as_str() {
                "linear" => Self::LINEAR,
                "quadratic" => Self::QUADRATIC,
                "cubic" => Self::CUBIC,
                _ => Self::LINEAR, // fallback
            },
            AnalogCurveConfig::Custom { a, b, c, d } => Self { a, b, c, d },
        }
    }
}
```

### AnalogCurve Scaling Functions

```rust
impl AnalogCurve {
    /// Evaluate polynomial: f(n) = a×n³ + b×n² + c×n + d
    #[inline]
    pub fn eval(&self, n: f64) -> f64 {
        self.a * n * n * n + self.b * n * n + self.c * n + self.d
    }
    
    /// Convert normalized (0.0-1.0) to scaled value
    pub fn to_scaled(&self, normalized: f64, min: f64, max: f64) -> f64 {
        min + self.eval(normalized) * (max - min)
    }
    
    /// Convert scaled value to normalized (0.0-1.0)
    /// Uses Newton-Raphson for non-linear curves
    pub fn to_normalized(&self, scaled: f64, min: f64, max: f64) -> f64 {
        let range = max - min;
        if range.abs() < f64::EPSILON {
            return 0.0;
        }
        let target = (scaled - min) / range;
        
        // For linear (c=1, others=0), direct solution
        if self.a == 0.0 && self.b == 0.0 && self.d == 0.0 {
            return target / self.c;
        }
        
        // Newton-Raphson iteration for inverse
        let mut n = target; // initial guess
        for _ in 0..10 {
            let f = self.eval(n) - target;
            let df = 3.0 * self.a * n * n + 2.0 * self.b * n + self.c;
            if df.abs() < f64::EPSILON { break; }
            n -= f / df;
            n = n.clamp(0.0, 1.0);
        }
        n
    }
    
    /// Validate coefficients sum to 1.0
    pub fn validate(&self) -> Result<(), String> {
        let sum = self.a + self.b + self.c + self.d;
        if (sum - 1.0).abs() > 0.001 {
            return Err(format!(
                "Polynomial coefficients must sum to 1.0, got {sum}"
            ));
        }
        Ok(())
    }
}
```

---

## Constants (in `evo_common::prelude`)

System-wide constants exported via prelude for global access.

```rust
// evo_common/src/prelude.rs

/// System cycle time in microseconds (1ms = 1000us)
/// Used by all real-time components: HAL, Control Unit, etc.
pub const SYSTEM_CYCLE_TIME_US: u32 = 1000;

/// System cycle time as Duration
pub const SYSTEM_CYCLE_TIME: Duration = Duration::from_micros(SYSTEM_CYCLE_TIME_US as u64);
```

## Constants (in `evo_common::hal::consts`)

HAL-specific constants.

```rust
/// Maximum number of axes
pub const MAX_AXES: usize = 64;

/// Maximum number of digital inputs
pub const MAX_DI: usize = 1024;

/// Maximum number of digital outputs
pub const MAX_DO: usize = 1024;

/// Maximum number of analog inputs
pub const MAX_AI: usize = 1024;

/// Maximum number of analog outputs
pub const MAX_AO: usize = 1024;

/// Default configuration file path
pub const DEFAULT_CONFIG_PATH: &str = "/etc/evo/machine.toml";

/// Default state file name
pub const DEFAULT_STATE_FILE: &str = "hal_state.bin";
```

---

## Shared Memory Layout (in `evo_hal_sim::shm`)

### HalShmData

The main shared memory structure.

```rust
#[repr(C, align(64))]
pub struct HalShmData {
    /// Header with version and metadata
    pub header: HalShmHeader,
    
    /// Axis data array (fixed size)
    pub axes: [AxisShmData; MAX_AXES],
    
    /// Digital inputs (bitfield)
    pub digital_inputs: [u8; MAX_DI / 8],
    
    /// Digital outputs (bitfield)
    pub digital_outputs: [u8; MAX_DO / 8],
    
    /// Analog inputs (dual representation)
    pub analog_inputs: [AnalogShmData; MAX_AI],
    
    /// Analog outputs (dual representation)
    pub analog_outputs: [AnalogShmData; MAX_AO],
}
```

### HalShmHeader

```rust
#[repr(C, align(64))]
pub struct HalShmHeader {
    /// Magic number for validation
    pub magic: u64,
    
    /// Version counter (atomic)
    pub version: AtomicU64,
    
    /// Configured axis count
    pub axis_count: u32,
    
    /// Configured DI count
    pub di_count: u32,
    
    /// Configured DO count
    pub do_count: u32,
    
    /// Configured AI count
    pub ai_count: u32,
    
    /// Configured AO count
    pub ao_count: u32,
    
    /// Cycle time in microseconds
    pub cycle_time_us: u32,
    
    /// Padding for alignment
    _padding: [u8; 32],
}
```

### AxisShmData

Per-axis shared memory structure.

```rust
#[repr(C)]
pub struct AxisShmData {
    // === Command Section (written by Control Unit) ===
    /// Target position in user units
    pub target_position: f64,
    
    /// Command flags
    pub command: AxisCommand,
    
    // === Status Section (written by HAL) ===
    /// Actual position in user units
    pub actual_position: f64,
    
    /// Actual velocity in user units/second
    pub actual_velocity: f64,
    
    /// Current lag error
    pub lag_error: f64,
    
    /// Status flags
    pub status: AxisStatus,
    
    /// Padding for 256-byte alignment
    _padding: [u8; 192],
}
```

### AxisCommand

```rust
#[repr(C)]
pub struct AxisCommand {
    /// Enable axis
    pub enable: bool,
    
    /// Reset error
    pub reset: bool,
    
    /// Start referencing
    pub reference: bool,
    
    /// Reserved flags
    _reserved: [u8; 5],
}
```

### AxisStatus

```rust
#[repr(C)]
pub struct AxisStatus {
    /// Axis ready for motion
    pub ready: bool,
    
    /// Axis in error state
    pub error: bool,
    
    /// Axis is referenced
    pub referenced: bool,
    
    /// Axis is currently referencing
    pub referencing: bool,
    
    /// Axis is moving
    pub moving: bool,
    
    /// At target position (within in_position_window)
    /// True when |actual_position - target_position| <= in_position_window
    pub in_position: bool,
    
    /// Error code (0 = no error)
    pub error_code: u16,
    
    /// Reserved
    _reserved: [u8; 6],
}
```

### AnalogShmData

Dual representation for analog I/O.

```rust
#[repr(C)]
pub struct AnalogShmData {
    /// Normalized value (0.0 - 1.0)
    pub normalized: f64,
    
    /// Scaled value in engineering units
    pub scaled: f64,
}
```

---

## State Persistence (in `evo_hal_driver_sim::state`)

### PersistedState

Structure saved to state file on shutdown (simulation driver specific).

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct PersistedState {
    /// Format version for migration
    pub version: u32,
    
    /// Timestamp of last save
    pub saved_at: u64,
    
    /// Per-axis state
    pub axes: Vec<PersistedAxisState>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistedAxisState {
    /// Axis name (for matching on load)
    pub name: String,
    
    /// Last known position
    pub position: f64,
    
    /// Was axis referenced?
    pub referenced: bool,
}
```

---

## Simulation Driver Internals (in `evo_hal_driver_sim`)

### SimulationDriver

```rust
/// Simulation driver implementing the HalDriver trait.
pub struct SimulationDriver {
    /// Loaded axis configurations
    axes: Vec<AxisConfig>,
    
    /// Per-axis simulation state
    axis_simulators: Vec<AxisSimulator>,
    
    /// Digital I/O simulation
    io_simulator: IOSimulator,
    
    /// State persistence manager
    persistence: Option<StatePersistence>,
    
    /// Driver-specific config
    config: SimDriverConfig,
}

impl HalDriver for SimulationDriver {
    fn name(&self) -> &'static str { "simulation" }
    fn version(&self) -> &'static str { env!("CARGO_PKG_VERSION") }
    
    fn init(&mut self, config: &MachineConfig) -> Result<(), HalError> {
        // Load axis configs, initialize simulators, restore state
    }
    
    fn cycle(&mut self, commands: &HalCommands, dt: Duration) -> HalStatus {
        // Update physics, handle referencing, check lag errors
    }
    
    fn shutdown(&mut self) -> Result<(), HalError> {
        // Persist state to file
    }
}
```

### SimDriverConfig

```rust
/// Simulation-specific configuration (from [driver_config] section).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimDriverConfig {
    /// Path to state persistence file
    pub state_file: Option<PathBuf>,
    
    /// Enable physics simulation (false = instant position tracking)
    #[serde(default = "default_true")]
    pub enable_physics: bool,
    
    /// Simulated I/O update rate divisor (1 = every cycle)
    #[serde(default = "default_one")]
    pub io_update_divisor: u32,
}
```

---

## Validation Rules

### MachineConfig::validate()

1. `driver` not empty
2. `cycle_time_us` > 0
3. `axes.len()` <= MAX_AXES
4. `digital_inputs.len()` <= MAX_DI
5. `digital_outputs.len()` <= MAX_DO
6. `analog_inputs.len()` <= MAX_AI
7. `analog_outputs.len()` <= MAX_AO
8. All axis names unique
9. All I/O names unique within category

### AxisConfig::validate()

1. `name` not empty
2. For `Positioning`: `encoder_resolution`, `max_velocity`, `max_acceleration`, `lag_error_limit` required and > 0
3. For `Slave`: `master_axis` required, must be < own index, master must not be Slave type
4. For `Measurement`: `encoder_resolution` required and > 0
5. `soft_limit_negative` < `soft_limit_positive` (if both set)
6. Referencing config valid for axis type
7. `in_position_window` >= 0 (0 = exact match required)

### AnalogIOConfig::validate()

1. `name` not empty
2. `max_value` > `min_value`
