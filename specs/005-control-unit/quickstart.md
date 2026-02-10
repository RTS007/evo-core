# Quickstart: Control Unit (evo_control_unit)

**Date**: 2026-02-08 | **Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

---

## Prerequisites

- **Rust**: Edition 2024 (nightly recommended; stable with edition 2024 support)
- **Linux**: PREEMPT_RT kernel recommended for production; standard kernel for development
- **Workspace**: `evo-core` workspace root at project root

### System packages (Ubuntu/Debian)

```bash
# RT kernel headers (production only)
sudo apt install linux-headers-$(uname -r)

# Build essentials
sudo apt install build-essential pkg-config
```

---

## Build

From the workspace root (`evo-core/`):

```bash
# Debug build (fast compile, runtime checks)
cargo build -p evo_control_unit

# Release build (LTO, opt-level=3, panic=abort)
cargo build -p evo_control_unit --release

# Check only (no linking, fastest feedback)
cargo check -p evo_control_unit
```

### Build with all workspace dependencies

```bash
# Build CU + evo_common + evo_shared_memory (all required)
cargo build -p evo_control_unit -p evo_common -p evo_shared_memory
```

---

## Run

### Development (no RT privileges)

```bash
# With default config
cargo run -p evo_control_unit -- --config config/machine.toml --io-config config/io.toml

# With trace logging
RUST_LOG=evo_control_unit=trace cargo run -p evo_control_unit -- --config config/machine.toml --io-config config/io.toml
```

### Production (RT privileges)

```bash
# Build release
cargo build -p evo_control_unit --release

# Run with RT scheduling (requires root or CAP_SYS_NICE)
sudo chrt -f 80 ./target/release/evo_control_unit --config config/machine.toml

# Or with capabilities (preferred over root)
sudo setcap cap_sys_nice,cap_ipc_lock+ep ./target/release/evo_control_unit
./target/release/evo_control_unit --config config/machine.toml
```

### Required SHM segments

CU expects these P2P SHM segments to exist (created by their source modules):

| Segment     | Created by  | CU role |
|-------------|-------------|---------|
| evo_hal_cu  | evo_hal     | Reader  |
| evo_cu_hal  | evo_control_unit | Writer |
| evo_re_cu   | evo_recipe_executor | Reader |
| evo_cu_mqt  | evo_control_unit | Writer |
| evo_rpc_cu  | evo_grpc    | Reader  |
| evo_cu_re   | evo_control_unit | Writer |

CU creates its writer segments on startup. Only `evo_hal_cu` must exist before CU enters `Idle`. Other reader segments (`evo_re_cu`, `evo_rpc_cu`) connect dynamically when their source modules start (FR-139).

---

## Test

```bash
# Unit tests
cargo test -p evo_control_unit

# Unit tests with output
cargo test -p evo_control_unit -- --nocapture

# Specific test module
cargo test -p evo_control_unit -- state_machine::tests
cargo test -p evo_control_unit -- control_engine::tests

# Integration tests (requires evo_shared_memory)
cargo test -p evo_control_unit --test integration

# All workspace tests (includes evo_common, evo_shared_memory)
cargo test --workspace
```

### Benchmarks

```bash
# Cycle time benchmarks (requires criterion)
cargo bench -p evo_control_unit

# Specific benchmark
cargo bench -p evo_control_unit -- cycle_benchmark
cargo bench -p evo_control_unit -- pid_benchmark
```

---

## Configuration

### Minimal machine.toml

```toml
[control_unit]
cycle_time_us = 1000      # 1 ms cycle
max_axes = 8

[global_safety]
default_safe_stop = "SS1"

[[axes]]
axis_id = 1
name = "X-Axis"

[axes.control]
kp = 100.0
ki = 10.0
kd = 5.0
tf = 0.001
tt = 0.1
kvff = 0.0
kaff = 0.0
friction = 0.0
jn = 0.0
bn = 0.0
gdob = 0.0
f_notch = 0.0
bw_notch = 0.0
flp = 0.0
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[axes.safe_stop]
category = "SS1"
max_decel_safe = 10000.0
sto_brake_delay = 0.1

[axes.homing]
method = "HardStop"
speed = 10.0
torque_limit = 20.0
timeout = 30.0
current_threshold = 5.0
approach_direction = 1
```

### Reference test configuration (SC-002, SC-004)

The file `config/test_8axis.toml` provides the reference configuration for success criteria benchmarks:
- 8 axes with mixed control parameters
- 10 kg load axis for lag verification
- Coupling pair (axes 7-8) for sync testing

---

## Project Structure (post-implementation)

```text
evo_control_unit/
├── Cargo.toml
├── src/
│   ├── main.rs                    # Entry point, RT setup, main loop
│   ├── lib.rs                     # Public API re-exports
│   ├── config.rs                  # ControlUnitConfig loading (TOML)
│   ├── cycle.rs                   # RT cycle loop, timing, overrun detection
│   ├── state.rs                   # State module root
│   ├── state/
│   │   ├── machine.rs             # MachineState transitions
│   │   ├── safety.rs              # SafetyState management
│   │   ├── axis.rs                # AxisState container (all 6 orthogonal)
│   │   ├── power.rs               # PowerState + enable/disable sequences
│   │   ├── motion.rs              # MotionState transitions
│   │   ├── operational.rs         # OperationalMode switching
│   │   ├── coupling.rs            # CouplingState + sync + error propagation
│   │   ├── gearbox.rs             # GearboxState transitions
│   │   └── loading.rs             # LoadingState (config-driven)
│   ├── safety.rs                  # Safety module root
│   ├── safety/
│   │   ├── flags.rs               # AxisSafetyState flag evaluation
│   │   ├── peripherals.rs         # Tailstock, index, brake, guard logic
│   │   ├── stop.rs                # SAFETY_STOP detection + per-axis SafeStopCategory
│   │   └── recovery.rs            # Reset + authorization sequence
│   ├── control.rs                 # Control engine root
│   ├── control/
│   │   ├── pid.rs                 # PID with anti-windup + derivative filter
│   │   ├── feedforward.rs         # Velocity/acceleration FF + friction
│   │   ├── dob.rs                 # Disturbance Observer
│   │   ├── filters.rs             # Biquad notch + 1st-order LP
│   │   ├── output.rs              # ControlOutputVector assembly
│   │   └── lag.rs                 # Lag monitoring + coupling lag diff
│   ├── command.rs                 # Command processing root
│   ├── command/
│   │   ├── source_lock.rs         # Source locking logic
│   │   ├── arbitration.rs         # Command arbitration (RE vs RPC)
│   │   └── homing.rs              # Homing supervision (6 methods)
│   ├── shm.rs                     # SHM integration root
│   ├── shm/
│   │   ├── segments.rs            # P2P segment connection & lifecycle
│   │   ├── reader.rs              # Inbound segment readers (hal_cu, re_cu, rpc_cu)
│   │   └── writer.rs              # Outbound segment writers (cu_hal, cu_mqt, cu_re)
│   ├── error.rs                   # Error module root
│   └── error/
│       └── propagation.rs         # Hierarchical error propagation rules
├── benches/
│   ├── cycle_benchmark.rs         # Full cycle timing measurement
│   └── pid_benchmark.rs           # Control engine throughput
└── tests/
    ├── integration/
    │   ├── startup.rs             # Config load → Idle transition
    │   ├── safety_stop.rs         # CRITICAL error → SAFETY_STOP → recovery
    │   └── coupling.rs            # Master-slave sync and cascade
    └── regression/
        └── sc_benchmarks.rs       # SC-002/SC-004 reference config tests
```

---

## Key Development Commands

```bash
# Format
cargo fmt -p evo_control_unit

# Lint
cargo clippy -p evo_control_unit -- -D warnings

# Documentation
cargo doc -p evo_control_unit --no-deps --open

# Check for unsafe (should be minimal — only SHM and RT setup)
cargo clippy -p evo_control_unit -- -W clippy::undocumented_unsafe_blocks
```
