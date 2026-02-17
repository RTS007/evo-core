# Quickstart: RT System Integration

**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

## Prerequisites

- Rust 2024 edition (rustc 1.85+)
- Linux x86_64 (POSIX SHM required: `/dev/shm` must exist)
- `cargo` (workspace build)

## Build

```bash
cd /path/to/evo-core
cargo build --workspace
```

After Phase D (dependency cleanup), all crates use Rust 2024 edition, unified workspace dependencies, and produce zero warnings.

## Run the Integrated System

### Option 1: Watchdog (production mode)

```bash
# Uses default config directory or specify one:
cargo run -p evo -- --config-dir config/

# Watchdog spawns HAL → waits for evo_hal_cu → spawns CU
# Both processes exchange data via SHM P2P at 1kHz
```

**Startup sequence**:
1. Watchdog loads `config/config.toml` → `[watchdog]` section
2. Spawns `evo_hal --config-dir config/`
3. Polls `/dev/shm/evo_hal_cu` until heartbeat > 0 (timeout: 5s)
4. Spawns `evo_control_unit --config-dir config/`
5. Enters monitoring loop (`waitpid`)

**Shutdown**: `Ctrl+C` or `kill -SIGTERM <watchdog_pid>`
- CU receives SIGTERM first, then HAL
- SHM segments cleaned up via `shm_unlink`

### Option 2: Individual programs (development mode)

```bash
# Terminal 1: HAL standalone
cargo run -p evo_hal -- --config-dir config/

# Terminal 2: CU standalone (waits for evo_hal_cu)
cargo run -p evo_control_unit -- --config-dir config/

# Terminal 3: Verify segments
ls -la /dev/shm/evo_*
# Should show: evo_hal_cu, evo_cu_hal, evo_cu_mqt, evo_hal_mqt, ...
```

## Verify Data Flow

```bash
# Check segment heartbeats are incrementing:
# (Requires a diagnostic tool — or use the integration tests)
cargo test -p evo_common --test p2p_integration

# Run RT stability test (10,000 cycles):
cargo test -p evo_control_unit --test rt_stability
```

## Configuration

All configs in a single flat directory (no subdirectories):

| File | Purpose | Loaded by |
|------|---------|-----------|
| `config.toml` | System params (watchdog backoff, timeouts) | All programs |
| `machine.toml` | Machine identity, global safety, service bypass | HAL, CU |
| `io.toml` | All I/O pin definitions, roles, scaling | HAL, CU |
| `axis_01_x.toml` ... `axis_08_tailstock.toml` | Per-axis parameters | HAL, CU |

### Add a new axis

1. Create `config/axis_09_rotary_b.toml` (copy template from any existing axis file)
2. Set `[axis] id = 9`
3. Fill kinematics, control, safe_stop, homing sections
4. Restart system — `ConfigLoader` auto-discovers the new file

### Modify parameters

1. Edit the relevant TOML file (every file has a self-documenting header)
2. Restart the affected program(s)
3. Both HAL and CU will load the same values

## Tests

```bash
# Unit tests (all crates):
cargo test --workspace

# P2P SHM tests:
cargo test -p evo_common -- shm

# Config loading tests:
cargo test -p evo_common -- config

# CU engine tests:
cargo test -p evo_control_unit

# Benchmarks (P2P latency):
cargo bench -p evo_common -- p2p
```

## Key Paths

| What | Where |
|------|-------|
| P2P library | `evo_common/src/shm/p2p.rs` |
| Segment types | `evo_common/src/shm/segments.rs` + `evo_common/src/control_unit/shm.rs` |
| Conversion functions | `evo_common/src/shm/conversions.rs` |
| I/O bit helpers | `evo_common/src/shm/io_helpers.rs` |
| Global constants | `evo_common/src/consts.rs` |
| Config loader | `evo_common/src/config.rs` |
| Watchdog | `evo/src/main.rs` |
| HAL RT loop | `evo_hal/src/core.rs` |
| CU RT loop | `evo_control_unit/src/engine/runner.rs` |
| CU binary entry | `evo_control_unit/src/main.rs` |

## SHM Segments Overview

After successful startup, `/dev/shm/` contains:

| Segment | Writer → Reader | Active? |
|---------|-----------------|---------|
| `evo_hal_cu` | HAL → CU | ✅ Active |
| `evo_cu_hal` | CU → HAL | ✅ Active |
| `evo_cu_mqt` | CU → MQTT | Skeleton (heartbeat) |
| `evo_hal_mqt` | HAL → MQTT | Skeleton (heartbeat) |
| `evo_hal_rpc` | HAL → gRPC | Placeholder |
| `evo_hal_re` | HAL → RE | Placeholder |

Additional segments appear when stub programs are running.
