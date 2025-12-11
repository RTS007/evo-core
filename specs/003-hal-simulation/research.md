# Research: HAL Simulation Driver

**Feature**: 003-hal-simulation | **Date**: 2025-12-10

## Research Tasks Completed

### 1. Rust Real-Time Simulation Loop Best Practices

**Decision**: Use `std::thread::sleep` with `Duration` for non-RT mode, with optional `clock_nanosleep` via `libc` for RT mode.

**Rationale**: 
- Standard library sleep is portable and sufficient for non-RT development
- For RT deployments, external supervisor (`chrt`/`taskset`) handles thread priorities
- HAL Sim focuses on simulation logic, not RT scheduling primitives

**Alternatives Considered**:
- `tokio` async runtime - Rejected: adds complexity, not needed for single-threaded simulation loop
- `spin_sleep` crate - Rejected: busy-waiting wastes CPU, violates <5% CPU goal

### 2. TOML Configuration Structure for Multi-File Configs

**Decision**: Main `machine.toml` uses relative paths to axis files in `axes` array.

**Rationale**:
- Serde's `#[serde(default)]` handles optional fields cleanly
- Path resolution relative to main config file location
- `toml` crate v0.8+ supports all needed features

**Config Loading Pattern**:
```rust
// Load main config
let machine: MachineConfig = toml::from_str(&content)?;
// Load each axis file relative to main config directory
for axis_path in &machine.axes {
    let full_path = config_dir.join(axis_path);
    let axis: AxisConfig = toml::from_str(&std::fs::read_to_string(full_path)?)?;
}
```

### 3. Shared Memory Layout Design

**Decision**: Fixed-size `#[repr(C)]` struct with arrays at maximum capacity.

**Rationale**:
- Matches existing `evo_shared_memory` patterns (see `SegmentHeader`)
- Zero-copy read/write operations
- Deterministic memory layout across processes
- Cache-line alignment for hot fields

**SHM Size Calculation**:
```
Header:           128 bytes (aligned)
Axes (64):        64 × 256 bytes = 16,384 bytes
Digital I/O:      2048 × 1 byte  =  2,048 bytes (DI + DO as bitfields)
Analog I/O:       2048 × 16 bytes = 32,768 bytes (AI + AO, dual f64 per channel)
Total:            ~51 KB
```

### 4. Axis Physics Simulation Model

**Decision**: Simple kinematic model with velocity ramping.

**Rationale**:
- Sufficient for control loop testing (not full dynamic simulation)
- Computationally cheap (<1μs per axis per cycle)
- Matches spec requirements (FR-005)

**Algorithm**:
```
Each cycle (dt = cycle_time):
1. error = target_position - actual_position
2. desired_velocity = sign(error) * min(|error|/dt, max_velocity)
3. velocity_delta = desired_velocity - current_velocity
4. limited_delta = clamp(velocity_delta, -max_accel*dt, max_accel*dt)
5. current_velocity += limited_delta
6. actual_position += current_velocity * dt
7. lag_error = |target_position - actual_position|
8. if lag_error > lag_limit: set ERROR state
```

### 5. State Persistence Format

**Decision**: Binary format using `bincode` for speed, with version header.

**Rationale**:
- Fast serialization (<100μs for full state)
- Compact file size
- Version header enables future format migrations

**Alternatives Considered**:
- TOML/JSON - Rejected: slower parsing, larger files
- `rmp-serde` (MessagePack) - Viable alternative, but `bincode` is simpler

### 6. Analog I/O Scaling Curves

**Decision**: Implement three curve types as specified.

**Formulas**:
```
Linear:    scaled = min + normalized * (max - min)
Parabolic: scaled = min + normalized² * (max - min)  
Cubic:     scaled = min + normalized³ * (max - min)
```

**Inverse (for output→normalized)**:
```
Linear:    normalized = (scaled - min) / (max - min)
Parabolic: normalized = sqrt((scaled - min) / (max - min))
Cubic:     normalized = cbrt((scaled - min) / (max - min))
```

### 7. Referencing State Machine

**Decision**: State machine with 5 states per axis.

**States**:
1. `Unreferenced` - Initial state, position unknown
2. `SearchingSwitch` - Moving toward reference/limit switch
3. `SearchingIndex` - Switch found, searching for K0 pulse
4. `Referenced` - Reference complete, position valid
5. `Error` - Referencing failed

**Transitions** depend on `referencing_mode` (0-5) as specified.

### 8. Error Recovery Protocol

**Decision**: Two-phase reset as specified in FR-007a.

**State Machine**:
```
Normal → Error (on lag_error > limit)
Error + Reset + !Enable → Standby (error cleared)
Standby + Enable → Normal (motion permitted)
```

## Dependencies Selected

| Crate | Version | Purpose |
|-------|---------|---------|
| `evo_common` | workspace | Config types, constants |
| `evo_shared_memory` | workspace | SHM primitives |
| `serde` | 1.0 | Serialization |
| `toml` | 0.8 | Config parsing |
| `bincode` | 1.3 | State persistence |
| `clap` | 4.4 | CLI argument parsing |
| `tracing` | 0.1 | Structured logging |
| `thiserror` | 1.0 | Error types |

## Open Questions Resolved

All technical questions from spec clarification sessions have been addressed. No remaining blockers for Phase 1 design.
