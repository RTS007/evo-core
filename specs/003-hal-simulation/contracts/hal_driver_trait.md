# HAL Driver Trait Contract

**Feature**: 003-hal-simulation | **Version**: 1.0.0 | **Date**: 2025-12-10

## Overview

The `HalDriver` trait defines the contract between HAL Core and hardware/simulation drivers. This abstraction enables:
- Pluggable hardware backends (simulation, EtherCAT, CANopen, PROFINET, etc.)
- Consistent interface for Control Unit communication
- Hot-swappable drivers (future capability)
- Testable isolation of hardware-specific code

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                     evo_hal (single crate)                       │
│  ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐  │
│  │   SHM       │◄──►│  HalCore     │◄──►│  Driver Registry    │  │
│  │   Layout    │    │  (RT Loop)   │    │                     │  │
│  └─────────────┘    └──────┬───────┘    └─────────────────────┘  │
│                            │                                     │
│                            ▼                                     │
│                   ┌────────────────┐                             │
│                   │  HalDriver     │ (trait object)              │
│                   │  trait         │                             │
│                   └────────┬───────┘                             │
│                            │                                     │
│  src/drivers/ ─────────────┼────────────────────                 │
│        ┌───────────────────┼───────────────────┐                 │
│        │                   │                   │                 │
│        ▼                   ▼                   ▼                 │
│ ┌───────────────┐ ┌───────────────┐ ┌───────────────┐            │
│ │ simulation/   │ │ ethercat/     │ │ canopen/      │            │
│ │ (this feature)│ │ (future)      │ │ (future)      │            │
│ └───────────────┘ └───────────────┘ └───────────────┘            │
└──────────────────────────────────────────────────────────────────┘
```

All drivers are located in `evo_hal/src/drivers/` as submodules.

## Trait Definition

```rust
/// Trait defining the interface for HAL drivers.
pub trait HalDriver: Send + Sync {
    //=== Identity ===
    
    /// Returns the driver's unique identifier (e.g., "simulation", "ethercat").
    fn name(&self) -> &'static str;
    
    /// Returns the driver's semantic version.
    fn version(&self) -> &'static str;
    
    //=== Lifecycle ===
    
    /// Initialize the driver with machine configuration.
    /// 
    /// Called once by HAL Core before entering the RT loop.
    /// Driver should:
    /// - Parse driver-specific config from `config.driver_config`
    /// - Load axis configurations from files
    /// - Initialize hardware connections (or simulation state)
    /// - Restore persisted state if applicable
    /// 
    /// # Timing
    /// - No RT constraints (runs before RT loop)
    /// - May block for hardware initialization
    /// 
    /// # Errors
    /// Return `HalError::InitFailed` if initialization cannot complete.
    fn init(&mut self, config: &MachineConfig) -> Result<(), HalError>;
    
    /// Execute one cycle of the driver.
    /// 
    /// Called every `cycle_time_us` microseconds by HAL Core's RT loop.
    /// Driver should:
    /// - Read hardware inputs (or simulate)
    /// - Process commands from `HalCommands`
    /// - Update internal state
    /// - Return status in `HalStatus`
    /// 
    /// # Timing
    /// - MUST complete within `cycle_time_us`
    /// - Should be deterministic (no allocations, no blocking I/O)
    /// 
    /// # Arguments
    /// * `commands` - Commands from Control Unit (extracted from SHM by HAL Core)
    /// * `dt` - Actual elapsed time since last cycle (for physics/interpolation)
    /// 
    /// # Returns
    /// `HalStatus` containing current state of all axes and I/O.
    fn cycle(&mut self, commands: &HalCommands, dt: Duration) -> HalStatus;
    
    /// Graceful shutdown of the driver.
    /// 
    /// Called by HAL Core when shutting down.
    /// Driver should:
    /// - Persist state if applicable
    /// - Close hardware connections
    /// - Release resources
    /// 
    /// # Timing
    /// - No strict RT constraints
    /// - Should complete within 1 second
    fn shutdown(&mut self) -> Result<(), HalError>;
    
    //=== Optional Capabilities ===
    
    /// Check if driver supports hot-swap (runtime replacement).
    /// Default: false
    fn supports_hot_swap(&self) -> bool { false }
    
    /// Get driver-specific diagnostics.
    /// Default: None
    fn diagnostics(&self) -> Option<DriverDiagnostics> { None }
    
    /// Handle driver-specific commands (extensibility point).
    /// Default: No-op, returns None
    fn handle_custom_command(&mut self, _cmd: &[u8]) -> Option<Vec<u8>> { None }
}
```

## Data Contracts

### HalCommands (Input to Driver)

```rust
pub struct HalCommands {
    pub axes: [AxisCommand; MAX_AXES],
    pub digital_outputs: [bool; MAX_DO],
    pub analog_outputs: [f64; MAX_AO],
}

pub struct AxisCommand {
    pub target_position: f64,  // User units
    pub enable: bool,
    pub reset: bool,
    pub reference: bool,
}
```

### HalStatus (Output from Driver)

```rust
pub struct HalStatus {
    pub axes: [AxisStatus; MAX_AXES],
    pub digital_inputs: [bool; MAX_DI],
    pub analog_inputs: [AnalogValue; MAX_AI],
}

pub struct AxisStatus {
    pub actual_position: f64,  // User units
    pub actual_velocity: f64,  // User units/sec
    pub lag_error: f64,
    pub ready: bool,
    pub error: bool,
    pub referenced: bool,
    pub referencing: bool,
    pub moving: bool,
    pub in_position: bool,
    pub error_code: u16,
}
```

## Driver Registration

Drivers register themselves with HAL Core through a factory function:

```rust
// In evo_hal/src/drivers/simulation/mod.rs
pub fn create_driver() -> Box<dyn HalDriver> {
    Box::new(SimulationDriver::new())
}

// Registration macro (future)
hal_driver_register!("simulation", create_driver);
```

HAL Core selects driver(s) based on CLI argument or `machine.toml`:

**Driver Selection Rules:**
1. `--simulate` / `-s` flag → **Exclusive**: loads ONLY simulation driver (ignores all other drivers)
2. `--driver <NAME>` / `-d <NAME>` flag → Can be specified multiple times to load multiple drivers
3. `drivers` list in `machine.toml` → Default driver list

**Important:** Simulation driver cannot run alongside other drivers. Use `--simulate` for development/testing only.

```bash
# Force simulation driver ONLY (exclusive mode)
./evo_hal --config config/machine.toml --simulate
./evo_hal -c config/machine.toml -s

# Load multiple drivers from CLI
./evo_hal --config config/machine.toml --driver ethercat --driver canopen
./evo_hal -c config/machine.toml -d ethercat -d canopen

# Use driver list from config file
./evo_hal --config config/machine.toml
```

```toml
[shared]
service_name = "hal-01"

# State persistence (used by all drivers for axis positions)
state_file = "hal_state.bin"

# Driver list (can be overridden by --simulate or --driver)
# Multiple drivers can run simultaneously (but NOT simulation with others)
drivers = ["ethercat", "canopen"]

# Note: System cycle time is defined globally in evo_common::prelude::SYSTEM_CYCLE_TIME_US (1000us = 1ms)

# Per-driver configuration sections
[driver_config.ethercat]
network_interface = "eth0"
cycle_shift_us = 0

[driver_config.canopen]
can_interface = "can0"
node_id = 1

[driver_config.simulation]
enable_physics = true
```

## Timing Contract

| Operation       | Max Duration  | RT Constraint              |
| --------------- | ------------- | -------------------------- |
| `init()`        | 30 seconds    | None (pre-RT)              |
| `cycle()`       | SYSTEM_CYCLE_TIME_US | **HARD** - must not exceed |
| `shutdown()`    | 1 second      | None (post-RT)             |
| `diagnostics()` | 1 ms          | Soft (non-blocking)        |

The cycle time is defined globally in `evo_common::prelude::SYSTEM_CYCLE_TIME_US` (default: 1000μs = 1ms).

## Error Handling

### In `init()`
- Return `HalError::InitFailed` with descriptive message
- HAL Core will log error and exit

### In `cycle()`
- **DO NOT** return errors - set `AxisStatus.error = true` and `error_code`
- Log timing violations but continue operation
- Never panic in RT context

### In `shutdown()`
- Return `HalError::PersistenceError` if state save fails
- HAL Core will log but continue shutdown

## Implementation Checklist for New Drivers

- [ ] Implement `name()` and `version()`
- [ ] Implement `init()` with config parsing
- [ ] Implement `cycle()` with deterministic timing
- [ ] Implement `shutdown()` with cleanup
- [ ] Add driver to HAL Core registry
- [ ] Write unit tests for driver logic
- [ ] Write integration tests with HAL Core
- [ ] Document driver-specific configuration
- [ ] Benchmark `cycle()` timing

## Simulation Driver Implementation

The simulation driver (`evo_hal_driver_sim`) implements this trait:

- `name()`: `"simulation"`
- `init()`: Loads axis configs, initializes physics simulators, restores state
- `cycle()`: Updates physics, handles referencing, calculates lag errors
- `shutdown()`: Persists axis positions to state file
- `supports_hot_swap()`: `false` (state would be lost)

See `data-model.md` for detailed simulation internals.
