# Quickstart: HAL Core + Simulation Driver

**Feature**: 003-hal-simulation | **Date**: 2025-12-10

## Prerequisites

- Rust 1.75+ with cargo
- Linux (Ubuntu 22.04+ or similar)
- Access to evo-core workspace

## Build

```bash
# From evo-core root
# Build HAL (includes core and all drivers)
cargo build --release -p evo_hal
```

## Configuration

### 1. Create machine configuration

Create `config/machine.toml`:

```toml
# Root-level config fields come BEFORE [shared] section
# Cycle time in microseconds (defaults to 1000µs = 1ms)
cycle_time_us = 1000

# State persistence (used by all drivers for axis positions across restarts)
state_file = "state/hal_state.bin"

# Drivers to load (simulation cannot be mixed with others)
drivers = ["simulation"]

# Axis configuration files (relative to config dir)
axes = [
    "axes/axis_01.toml",
    "axes/axis_02.toml",
]

# Shared settings section
[shared]
log_level = "info"
name = "hal-01"

# Digital Inputs (sensors)
[[digital_inputs]]
name = "di_start_button"
initial_value = "off"

[[digital_inputs]]
name = "di_stop_button"
initial_value = "on"    # NC switch - normally active

[[digital_inputs]]
name = "di_cylinder_closed"
initial_value = "on"    # Cylinder starts in closed position

[[digital_inputs]]
name = "di_cylinder_open"
initial_value = "off"

# Digital Outputs (actuators)
[[digital_outputs]]
name = "do_motor_enable"

[[digital_outputs]]
name = "do_lamp"

# Pneumatic cylinder with linked DI simulation
# Format: [trigger, delay_s, di_index, result]
[[digital_outputs]]
name = "do_cylinder_extend"
linked_inputs = [
    ["on",  0.1, 2, "off"],  # DO ON  -> 0.1s -> DI[2] OFF (closed sensor)
    ["on",  0.8, 3, "on" ],  # DO ON  -> 0.8s -> DI[3] ON  (open sensor)
    ["off", 0.1, 3, "off"],  # DO OFF -> 0.1s -> DI[3] OFF (open sensor)
    ["off", 0.8, 2, "on" ],  # DO OFF -> 0.8s -> DI[2] ON  (closed sensor)
]

# Analog Inputs
[[analog_inputs]]
name = "ai_pressure"
min_value = 0.0
max_value = 10.0
unit = "bar"
initial_value = 5.0    # Simulation starts at 5 bar
curve = "linear"       # Named preset (default)

[[analog_inputs]]
name = "ai_temperature"
min_value = -20.0
max_value = 150.0
unit = "°C"
curve = "quadratic"    # Named preset (e.g., thermistor)

[[analog_inputs]]
name = "ai_flow"
min_value = 0.0
max_value = 100.0
unit = "l/min"
curve = { a = 0.2, b = 0.3, c = 0.5, d = 0.0 }  # Custom polynomial (a+b+c+d=1)

# Analog Outputs
[[analog_outputs]]
name = "ao_valve"
min_value = 0.0
max_value = 100.0
unit = "%"
curve = "linear"
```

### 2. Create axis configurations

Create `config/axes/axis_01.toml`:

```toml
name = "X_Axis"
axis_type = "positioning"

# Kinematics
encoder_resolution = 1000.0    # increments per mm
max_velocity = 100.0           # mm/s
max_acceleration = 500.0       # mm/s²
lag_error_limit = 5.0          # mm
in_position_window = 0.1       # mm (in_position when |error| <= 0.1)

# Soft limits (optional)
soft_limit_positive = 500.0    # mm
soft_limit_negative = 0.0      # mm

# Referencing
[referencing]
required = "yes"                    # "yes", "perhaps", or "no"
mode = "SwitchThenIndex"            # SwitchThenIndex, SwitchOnly, IndexOnly, LimitThenIndex, LimitOnly, None
reference_switch_position = 0.0     # mm - where the switch is located
index_pulse_position = 0.0          # mm - where index pulse occurs
normally_closed = false
negative_direction = true
speed = 10.0                        # mm/s
timeout_ms = 30000                  # ms - referencing timeout
```

Create `config/axes/axis_02.toml`:

```toml
name = "Y_Axis"
axis_type = "positioning"

encoder_resolution = 1000.0
max_velocity = 80.0
max_acceleration = 400.0
lag_error_limit = 5.0
in_position_window = 0.05      # tighter tolerance for Y axis

soft_limit_positive = 300.0
soft_limit_negative = 0.0

[referencing]
required = "perhaps"           # Use persisted position if available
mode = "SwitchOnly"            # Switch without index
reference_switch_position = 0.0
normally_closed = false
negative_direction = false
speed = 8.0
timeout_ms = 30000
```

### 3. Slave axis example

```toml
name = "Z_Slave"
axis_type = "slave"

master_axis = 0                # Index of master (X_Axis)
encoder_resolution = 1000.0
slave_ratio = 1.0              # 1:1 ratio with master
slave_offset = 0.0             # Optional offset

[referencing]
required = "no"
mode = "None"
```

### 4. Simple axis example (on/off)

```toml
name = "Gripper"
axis_type = "simple"

# Simple axes move instantly to target
encoder_resolution = 1.0       # 1:1 for simple axes

[referencing]
required = "no"
mode = "None"
```

## Run

```bash
# Start HAL with simulation driver (recommended for development)
./target/release/evo_hal --config config/machine.toml --simulate
./target/release/evo_hal -c config/machine.toml -s

# Or with verbose logging
./target/release/evo_hal --config config/machine.toml -s -v

# Or in background
./target/release/evo_hal --config config/machine.toml -s &
```

## Command Line Options

```
evo_hal [OPTIONS]

Options:
  -c, --config <CONFIG>   Path to machine configuration file (machine.toml) [default: /etc/evo/machine.toml]
  -s, --simulate          Force simulation driver (exclusive - ignores all other drivers)
  -d, --driver <DRIVERS>  Load specific driver (can be specified multiple times)
  -v, --verbose           Enable verbose logging
      --json              Output logs in JSON format
  -h, --help              Print help
  -V, --version           Print version
```

**Driver selection rules:**
1. `--simulate` → Loads ONLY simulation driver (exclusive mode for development)
2. `--driver <NAME>` → Can specify multiple drivers (but NOT simulation with others)
3. `drivers` list in `machine.toml` → Default if no CLI flags

## Verify Operation

### Check SHM segment exists

```bash
# List shared memory segments
ls -la /dev/shm/evo_hal_*
```

### Read SHM with example tool

```bash
# From evo_shared_memory examples
cargo run --example shm_discovery
```

### Monitor with custom reader

```rust
use evo_shared_memory::SegmentReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = SegmentReader::attach("evo_hal_hal-01")?;
    
    loop {
        if reader.has_changed() {
            let data = reader.read()?;
            // Parse HalShmData from data bytes
            println!("Version: {}", reader.version());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
```

## Testing

### Run unit tests

```bash
# All HAL tests (core + drivers)
cargo test -p evo_hal
```

### Run integration tests

```bash
cargo test -p evo_hal --test shm_integration
```

### Test configuration validation

```bash
cargo test -p evo_common --test hal_config_validation
```

## Common Issues

### Permission denied on /dev/shm

```bash
# Check permissions
ls -la /dev/shm

# Ensure user can create files
sudo chmod 1777 /dev/shm
```

### Cycle time violations in logs

This is expected on non-RT systems. For RT operation:

```bash
# Run with real-time priority (requires privileges)
sudo chrt -f 50 ./target/release/evo_hal --config config/machine.toml

# Or configure via systemd unit with CPUAffinity and nice
```

### State file errors

```bash
# Delete state file to start fresh
rm config/hal_state.bin
```

## Architecture Overview

```
┌────────────────────────────────────────────────────────────┐
│                    Control Unit                            │
│                         │                                  │
│                         ▼ (SHM)                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              evo_hal (single crate)                  │  │
│  │  ┌─────────────┐    ┌─────────────┐                  │  │
│  │  │ SHM Manager │◄──►│ RT Loop     │                  │  │
│  │  └─────────────┘    └──────┬──────┘                  │  │
│  │                            │                         │  │
│  │                   ┌────────┴────────┐                │  │
│  │                   │  HalDriver      │                │  │
│  │                   │  (trait object) │                │  │
│  │                   └────────┬────────┘                │  │
│  │                            │                         │  │
│  │  src/drivers/ ─────────────┼──────────────────────   │  │
│  │         ┌──────────────────┼──────────────────┐      │  │
│  │         ▼                  ▼                  ▼      │  │
│  │  ┌─────────────┐    ┌─────────────┐    ┌───────────┐ │  │
│  │  │ simulation/ │    │  ethercat/  │    │ canopen/  │ │  │
│  │  │ (this feat) │    │  (future)   │    │ (future)  │ │  │
│  │  └─────────────┘    └─────────────┘    └───────────┘ │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
```

## Next Steps

1. Integrate with Control Unit by reading/writing SHM
2. Configure systemd service for production deployment
3. Set up monitoring and alerting for timing violations
4. Implement additional HAL drivers (EtherCAT, CANopen)
