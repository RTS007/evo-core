# Configuration File Contracts

**Spec**: [../spec.md](../spec.md) | **Data Model**: [../data-model.md](../data-model.md)

All config files live in a single flat directory. Every file includes a self-documenting header comment block.

## Directory Layout

```
<config-dir>/
├── config.toml              # System/program configuration (machine-independent)
├── machine.toml             # Machine-specific parameters (axes, safety, service)
├── io.toml                  # All I/O definitions (roles, pins, logic, scaling)
├── axis_01_x.toml           # Per-axis config (auto-discovered)
├── axis_02_y.toml
├── ...
└── axis_NN_name.toml
```

Default `<config-dir>`: `/etc/evo` (constant `DEFAULT_CONFIG_PATH` in `evo_common::consts`).
Override: `--config-dir <path>` CLI argument on all programs.

---

## config.toml — System/Program Configuration

**Purpose**: Parameters for starting and running the EVO system, independent of which physical machine is controlled.
**FR**: FR-059a

### Schema

```toml
# ╔══════════════════════════════════════════════════════════════════╗
# ║  EVO SYSTEM CONFIGURATION                                       ║
# ╚══════════════════════════════════════════════════════════════════╝
#
# Parameters for starting and running the EVO system.
# Machine-specific parameters (axes, kinematics, safety) live in machine.toml.
#
# ┌─── [watchdog] ─────────────────────────────────────────────────┐
# │  max_restarts        Max consecutive restarts (u32, def: 5)    │
# │  initial_backoff_ms  Initial restart delay ms (u64, def: 100)  │
# │  max_backoff_s       Max restart delay sec (u64, def: 30)      │
# │  stable_run_s        Stable run to reset backoff (u64, def: 60)│
# │  sigterm_timeout_s   SIGTERM timeout sec (f64, def: 2.0)       │
# │  hal_ready_timeout_s HAL ready timeout sec (f64, def: 5.0)     │
# └────────────────────────────────────────────────────────────────┘

[watchdog]
max_restarts = 5
initial_backoff_ms = 100
max_backoff_s = 30
stable_run_s = 60
sigterm_timeout_s = 2.0
hal_ready_timeout_s = 5.0

[hal]
# Future: cycle_time_us, driver settings

[cu]
# Future: cycle_time_us, state machine params

[re]
# Placeholder

[mqtt]
# Placeholder

[grpc]
# Placeholder

[api]
# Placeholder

[dashboard]
# Placeholder

[diagnostic]
# Placeholder
```

### Rust Types

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    pub watchdog: WatchdogConfig,
    pub hal: Option<toml::Value>,
    pub cu: Option<toml::Value>,
    pub re: Option<toml::Value>,
    pub mqtt: Option<toml::Value>,
    pub grpc: Option<toml::Value>,
    pub api: Option<toml::Value>,
    pub dashboard: Option<toml::Value>,
    pub diagnostic: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchdogConfig {
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,              // 1..=100
    #[serde(default = "default_initial_backoff_ms")]
    pub initial_backoff_ms: u64,        // 10..=10_000
    #[serde(default = "default_max_backoff_s")]
    pub max_backoff_s: u64,             // 1..=300
    #[serde(default = "default_stable_run_s")]
    pub stable_run_s: u64,             // 10..=3600
    #[serde(default = "default_sigterm_timeout_s")]
    pub sigterm_timeout_s: f64,         // 0.5..=30.0
    #[serde(default = "default_hal_ready_timeout_s")]
    pub hal_ready_timeout_s: f64,       // 1.0..=60.0
}
```

---

## machine.toml — Machine-Specific Parameters

**Purpose**: Physical machine identity, global safety, service bypass. No axis parameters, no I/O, no axis file list.
**FR**: FR-050, FR-056

### Schema

```toml
# ╔══════════════════════════════════════════════════════════════════╗
# ║  MACHINE CONFIGURATION                                          ║
# ╚══════════════════════════════════════════════════════════════════╝
#
# Machine-specific parameters. Axis configs are auto-discovered
# from axis_NN_*.toml files in this same directory.
# I/O definitions live in io.toml.
#
# ┌─── [machine] ──────────────────────────────────────────────────┐
# │  name    Machine display name (string, REQUIRED)               │
# └────────────────────────────────────────────────────────────────┘
#
# ┌─── [global_safety] ───────────────────────────────────────────┐
# │  default_safe_stop             "SS1"|"SS2"|"STO" (REQUIRED)    │
# │  safety_stop_timeout           Timeout in seconds (REQUIRED)   │
# │  recovery_authorization_required  bool (REQUIRED)              │
# └────────────────────────────────────────────────────────────────┘
#
# ┌─── [service_bypass] ──────────────────────────────────────────┐
# │  bypass_axes          Array of axis IDs to bypass (REQUIRED)   │
# │  max_service_velocity Max velocity in service mode (REQUIRED)  │
# └────────────────────────────────────────────────────────────────┘

[machine]
name = "Test 8-Axis CNC"

[global_safety]
default_safe_stop = "SS1"
safety_stop_timeout = 5.0
recovery_authorization_required = true

[service_bypass]
bypass_axes = [1, 2, 3, 4, 5, 6, 7, 8]
max_service_velocity = 50.0
```

### Rejected Fields

The following are legacy parameters. If present, parsing MUST fail with `ConfigError::UnknownField`:
- `[[axes]]` — legacy array format
- `axes_dir` — legacy directory pointer

### Rust Type

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineConfig {
    pub machine: MachineIdentity,
    pub global_safety: GlobalSafetyConfig,
    pub service_bypass: ServiceBypassConfig,
}
```

---

## io.toml — I/O Pin Definitions

**Purpose**: Single source of truth for all I/O pin assignments, roles, logic, scaling.
**FR**: FR-051, FR-052
**Reference**: `specs/005-control-unit/io.toml`

### Schema (excerpt)

```toml
# ╔══════════════════════════════════════════════════════════════════╗
# ║  I/O CONFIGURATION                                              ║
# ╚══════════════════════════════════════════════════════════════════╝
#
# All I/O definitions for the machine. Loaded by HAL and CU.
# IoRole strings cross-reference to axis files (e.g., do_brake = "BrakeOut1").
#
# ┌─── [groups.<name>] ───────────────────────────────────────────┐
# │  name     Group display name (optional)                        │
# │  [[groups.<name>.points]]                                      │
# │    io_type    "DI"|"DO"|"AI"|"AO" (REQUIRED)                   │
# │    pin        Pin number u16 (REQUIRED)                        │
# │    role       IoRole string (optional)                         │
# │    name       Display name (optional)                          │
# │    logic      "NC"|"NO" (DI only, def: "NO")                  │
# │    debounce   Debounce ms (DI only, def: 15)                  │
# │    sim        Simulation value (optional)                      │
# │    min/max    Analog range (AI/AO, def: 0.0/required)         │
# │    unit       Analog unit string (AI/AO, def: "V")            │
# │    curve      "linear"|"quadratic"|"cubic"|[a,b,c] (AI/AO)    │
# │    offset     Analog offset (AI/AO, optional)                 │
# └────────────────────────────────────────────────────────────────┘

[groups.safety]
name = "Safety I/O"

[[groups.safety.points]]
io_type = "DI"
pin = 0
role = "EStop"
name = "Emergency Stop"
logic = "NC"
sim = 1.0

[[groups.safety.points]]
io_type = "AI"
pin = 0
role = "HydraulicPressure"
name = "Hydraulic Pressure"
min = 0.0
max = 10.0
unit = "bar"
curve = "linear"
```

### Rust Types

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IoConfig {
    pub groups: BTreeMap<String, IoGroup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IoGroup {
    pub name: Option<String>,
    pub points: Vec<IoPoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IoPoint {
    pub io_type: IoType,        // DI, DO, AI, AO
    pub pin: u16,
    pub role: Option<String>,   // IoRole string reference
    pub name: Option<String>,
    pub logic: Option<IoLogic>, // NC/NO (DI only)
    pub debounce: Option<u16>,  // ms (DI only)
    pub sim: Option<f64>,       // simulation value
    pub min: Option<f64>,       // analog range start
    pub max: Option<f64>,       // analog range end
    pub unit: Option<String>,   // analog unit
    pub curve: Option<AnalogCurve>, // scaling curve
    pub offset: Option<f64>,    // analog offset
    // ... additional DI/DO-specific fields
}
```

### I/O Ownership Rules

| Pin has IoRole? | CU can write? | RE can write? | CU can read? | RE can read? |
|-----------------|---------------|---------------|--------------|--------------|
| Yes | ✅ via `evo_cu_hal` | ❌ ignored, `ERR_IO_ROLE_OWNED` | ✅ via `IoRegistry` | ✅ via `evo_hal_re` |
| No | ❌ | ✅ via `evo_re_hal` | ❌ | ✅ via `evo_hal_re` |

---

## axis_NN_name.toml — Per-Axis Configuration

**Purpose**: All parameters for a single axis. Auto-discovered by `ConfigLoader`.
**FR**: FR-055, FR-055a, FR-057

### File Naming Convention

Pattern: `axis_NN_name.toml`
- **NN**: Axis number (01–64), zero-padded, used for identity and sort order
- **name**: Free-form human-readable label (no functional meaning)
- Examples: `axis_01_x.toml`, `axis_08_tailstock.toml`, `axis_08_konik.toml`

### Validation Rules

| Rule | Error | FR |
|------|-------|-----|
| `[axis].id` must match NN in filename | `ConfigError::AxisIdMismatch { file, expected, found }` | FR-055a |
| No two files with same NN | `ConfigError::DuplicateAxisId(NN)` | FR-055a |
| Zero axis files found | `ConfigError::NoAxesDefined` | FR-055a |
| Unknown fields in any section | `ConfigError::UnknownField` | FR-053 |
| Numeric values out of bounds | `ConfigError::ValidationError` | FR-054 |

### Schema

See spec.md Per-Axis Configuration Architecture for complete field reference with types, defaults, and valid ranges.

Required sections: `[axis]`, `[kinematics]`, `[control]`, `[safe_stop]`, `[homing]`
Optional sections: `[brake]`, `[tailstock]`, `[guard]`, `[coupling]`

### Rust Type

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisConfig {
    pub axis: AxisIdentity,
    pub kinematics: KinematicsConfig,
    pub control: ControlConfig,
    pub safe_stop: SafeStopConfig,
    pub homing: HomingConfig,
    pub brake: Option<BrakeConfig>,
    pub tailstock: Option<TailstockConfig>,
    pub guard: Option<GuardConfig>,
    pub coupling: Option<CouplingConfig>,
}
```

---

## ConfigLoader API Contract

```rust
impl ConfigLoader {
    /// Load all configs from a directory.
    /// Loads config.toml, machine.toml, io.toml, auto-discovers axis_*_*.toml.
    /// Validates: strict parsing, axis ID consistency, duplicate detection, bounds.
    pub fn load_config_dir(path: &Path) -> Result<FullConfig, ConfigError>;

    /// Load system config only.
    pub fn load_system_config(path: &Path) -> Result<SystemConfig, ConfigError>;

    /// Load machine config only.
    pub fn load_machine_config(path: &Path) -> Result<MachineConfig, ConfigError>;

    /// Load I/O config only.
    pub fn load_io_config(path: &Path) -> Result<IoConfig, ConfigError>;

    /// Discover and load all axis files from a directory.
    /// Glob: axis_*_*.toml, sorted by NN prefix.
    pub fn discover_axis_files(dir: &Path) -> Result<Vec<AxisConfig>, ConfigError>;
}

pub struct FullConfig {
    pub system: SystemConfig,
    pub machine: MachineConfig,
    pub io: IoConfig,
    pub axes: Vec<AxisConfig>,
    pub io_registry: IoRegistry,
}
```

### ConfigError Variants

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("unknown field: {0}")]
    UnknownField(String),
    #[error("validation error: {0}")]
    ValidationError(String),
    #[error("duplicate axis ID: {0}")]
    DuplicateAxisId(u8),
    #[error("axis ID mismatch in {file}: expected {expected}, found {found}")]
    AxisIdMismatch { file: String, expected: u8, found: u8 },
    #[error("no axis files found in config directory")]
    NoAxesDefined,
}
```

---

## Self-Documenting Header Rule

**Every** TOML configuration file MUST include a header comment block at the top documenting:
- All available parameters
- Parameter types
- Default values
- Whether required or optional
- Valid value ranges/enums

This makes every config file self-documenting for service engineers.
