# SHM Segment Contracts

**Date**: 2026-02-08 | **Spec**: [../spec.md](../spec.md) | **Data Model**: [../data-model.md](../data-model.md)

> All segments use P2P naming: `evo_[SOURCE]_[DESTINATION]` (FR-130b).
> All payloads are `#[repr(C, align(64))]`, zero-copy, binary.
> Header includes heartbeat counter (FR-130c) and struct version hash (FR-130d).

---

## Segment Index

| Segment       | Source | Dest | Direction  | FR   | Size (64 axes) | Cycle |
|---------------|--------|------|------------|------|-----------------|-------|
| evo_hal_cu    | HAL    | CU   | HAL → CU   | FR-131 | ~2.3 KB      | 1 ms  |
| evo_cu_hal    | CU     | HAL  | CU → HAL   | FR-132 | ~3.3 KB      | 1 ms  |
| evo_re_cu     | RE     | CU   | RE → CU    | FR-133 | ~1 KB         | async |
| evo_cu_mqt    | CU     | MQT  | CU → MQT   | FR-134 | ~4.5 KB      | 10 ms |
| evo_rpc_cu    | RPC    | CU   | RPC → CU   | FR-132c | ~88 B         | async |
| evo_cu_re     | CU     | RE   | CU → RE    | FR-134a | placeholder   | async |

---

## 1. evo_hal_cu — HAL → Control Unit

**Purpose**: Real-time axis feedback and I/O state from HAL to CU.  
**Writer**: `evo_hal` (single writer)  
**Reader**: `evo_control_unit` (single reader)  
**Update rate**: Every HAL cycle (1 ms)  
**Staleness**: Heartbeat must increment every cycle. CU triggers SAFETY_STOP if stale for 3 consecutive reads.

### Layout

```rust
#[repr(C, align(64))]
pub struct HalToCuSegment {
    pub header:     P2pSegmentHeader,  // 64 bytes
    pub axis_count: u8,                // active axis count (1..=64)
    pub _pad:       [u8; 63],          // align next field
    pub axes:       [HalAxisFeedback; 64],  // 64 × 24 = 1536 bytes
    pub di_bank:    [u64; 16],         // 1024 DI bits = 128 bytes
    pub ai_values:  [f64; 64],         // 64 analog inputs = 512 bytes
}
// Total: 64 + 64 + 1536 + 128 + 512 = 2304 bytes ≈ 2.3 KB

#[repr(C)]
pub struct HalAxisFeedback {
    pub actual_position: f64,    // [mm] encoder-derived
    pub actual_velocity: f64,    // [mm/s] encoder-derived
    pub drive_status:    u8,     // bitfield: ready|fault|enabled|referenced
    pub fault_code:      u16,    // drive-specific fault code
    pub _padding:        [u8; 5],
}
// Size: 24 bytes per axis (8+8+1+2+5)
```

### drive_status bitfield

| Bit | Name         | Description                    |
|-----|-------------|--------------------------------|
| 0   | ready       | Drive is ready to enable       |
| 1   | fault       | Drive has active fault         |
| 2   | enabled     | Drive output stage is ON       |
| 3   | referenced  | Encoder reference found        |
| 4   | zerospeed   | Drive detected zero speed      |
| 5-7 | reserved    | Must be 0                      |

### Contract

- CU MUST read this segment every cycle before computing control output.
- CU MUST check `header.heartbeat` > previous read. If equal for `N` consecutive reads (N=3), trigger SAFETY_STOP on all axes.
- CU MUST check `header.version_hash` matches compiled-in hash. Mismatch = refuse startup.
- CU MUST only read `axes[0..axis_count]`. Remaining slots contain stale/zero data.
- Field `actual_position` and `actual_velocity` are in user units (mm, mm/s) after HAL applies gear ratio and scale.
- **I/O Access**: `di_bank` and `ai_values` are accessed by CU via `IoRegistry` role-based API (FR-152). HAL maps physical pin → bit/index at startup from `io.toml`. CU maps `IoRole` → same bit/index. Pin-to-index mapping convention: DI bit N = pin N; AI index = declaration order in `io.toml`. See contracts/io-config.md §6.

---

## 2. evo_cu_hal — Control Unit → HAL

**Purpose**: Control commands from CU to HAL drives.  
**Writer**: `evo_control_unit` (single writer)  
**Reader**: `evo_hal` (single reader)  
**Update rate**: Every CU cycle (1 ms)

### Layout

```rust
#[repr(C, align(64))]
pub struct CuToHalSegment {
    pub header:     P2pSegmentHeader,  // 64 bytes
    pub axis_count: u8,
    pub _pad:       [u8; 63],
    pub axes:       [CuAxisCommand; 64],  // 64 × 40 = 2560 bytes
    pub do_bank:    [u64; 16],         // 1024 DO bits = 128 bytes
    pub ao_values:  [f64; 64],         // 64 analog outputs = 512 bytes
}
// Total: 64 + 64 + 2560 + 128 + 512 ≈ 3.3 KB

#[repr(C)]
pub struct CuAxisCommand {
    pub output:  ControlOutputVector,  // 32 bytes (4 × f64)
    pub enable:  u8,                   // 0=disable, 1=enable
    pub mode:    u8,                   // OperationalMode
    pub _pad:    [u8; 6],
}
// Size: 40 bytes per axis

#[repr(C)]
pub struct ControlOutputVector {
    pub calculated_torque: f64,  // [Nm] total PID+FF+DOB output
    pub target_velocity:   f64,  // [mm/s] velocity command
    pub target_position:   f64,  // [mm] position command
    pub torque_offset:     f64,  // [Nm] feedforward-only component
}
// Size: 32 bytes
```

### Contract

- CU MUST write this segment every cycle.
- HAL selects which field from `ControlOutputVector` to use based on drive operational mode.
- If `enable == 0`, HAL MUST ignore all output fields and disable the drive.
- CU MUST set `mode` to match the currently active `OperationalMode` for that axis.
- On SAFETY_STOP: CU sets `enable=0` for STO axes, writes decel ramp for SS1/SS2 axes.
- **I/O Access**: `do_bank` and `ao_values` are written by CU via `IoRegistry` role-based API (FR-152). Same pin-to-index convention as `evo_hal_cu`. See contracts/io-config.md §6.

---

## 3. evo_re_cu — Recipe Executor → Control Unit

**Purpose**: Recipe commands and motion targets from RE to CU.  
**Writer**: `evo_recipe_executor` (single writer)  
**Reader**: `evo_control_unit` (single reader)  
**Update rate**: Asynchronous — written when recipe step starts.

### Layout

```rust
#[repr(C, align(64))]
pub struct ReToCuSegment {
    pub header:     P2pSegmentHeader,  // 64 bytes
    pub command:    ReCommand,         // current command
}

#[repr(C)]
pub struct ReCommand {
    pub command_type: u8,              // ReCommandType enum
    pub axis_mask:    u64,             // bit per axis (bit 0 = axis 1)
    pub targets:      [ReAxisTarget; 64],
    pub sequence_id:  u32,             // monotonic, for ack tracking
    pub _pad:         [u8; 4],
}

#[repr(C)]
pub struct ReAxisTarget {
    pub target_position: f64,    // [mm]
    pub target_velocity: f64,    // [mm/s] max velocity
    pub acceleration:    f64,    // [mm/s²]
    pub deceleration:    f64,    // [mm/s²]
    pub mode:            u8,     // OperationalMode
    pub _pad:            [u8; 7],
}
// Size: 40 bytes per axis
```

### ReCommandType

```rust
#[repr(u8)]
pub enum ReCommandType {
    Nop           = 0,  // No active command
    MoveAbsolute  = 1,
    MoveRelative  = 2,
    MoveVelocity  = 3,
    Home          = 4,
    Stop          = 5,
    EmergencyStop = 6,
    EnableAxis    = 7,
    DisableAxis   = 8,
    SetMode       = 9,
    Couple        = 10,
    Decouple      = 11,
    GearChange    = 12,
    AllowManualMode = 13,  // FR-004: authorize manual mode for axes in axis_mask
}
```

### Contract

- CU checks source lock before accepting commands. Rejects with `CommandError::SOURCE_LOCKED` if axis locked by different source.
- CU acquires source lock for all axes in `axis_mask` atomically on first command from RE.
- Source lock released when RE writes `Nop` or heartbeat goes stale.
- `sequence_id` is echoed back in `evo_cu_re` for command acknowledgment.

---

## 4. evo_cu_mqt — Control Unit → MQTT Bridge

**Purpose**: Diagnostic state for monitoring, logging, and dashboard.  
**Writer**: `evo_control_unit` (single writer)  
**Reader**: `evo_mqtt` (single reader)  
**Update rate**: Every N cycles (configurable, default: 10 = 10 ms)

### Layout

```rust
#[repr(C, align(64))]
pub struct CuToMqtSegment {
    pub header:        P2pSegmentHeader,  // 64 bytes
    pub machine_state: u8,     // MachineState
    pub safety_state:  u8,     // SafetyState
    pub axis_count:    u8,
    pub _pad:          [u8; 61],
    pub axes:          [AxisStateSnapshot; 64],  // 64 × 56 = 3584 bytes
}
// Total: 64 + 64 + 3584 ≈ 3.7 KB

#[repr(C)]
pub struct AxisStateSnapshot {
    pub axis_id:     u8,       // AxisId (1-based)
    pub power:       u8,       // PowerState
    pub motion:      u8,       // MotionState
    pub operational: u8,       // OperationalMode
    pub coupling:    u8,       // CouplingState
    pub gearbox:     u8,       // GearboxState
    pub loading:     u8,       // LoadingState
    pub locked_by:   u8,       // CommandSource
    pub safety_flags: u8,      // 8 packed booleans (AxisSafetyState)
    pub error_power:  u16,     // PowerError bitflags
    pub error_motion: u16,     // MotionError bitflags
    pub error_command: u8,     // CommandError bitflags
    pub error_gearbox: u8,     // GearboxError bitflags
    pub error_coupling: u8,    // CouplingError bitflags
    pub _pad:         [u8; 5],
    pub position:    f64,      // [mm] actual
    pub velocity:    f64,      // [mm/s] actual
    pub lag:         f64,      // [mm] |target - actual|
    pub torque:      f64,      // [Nm] current output
}
// Size: 56 bytes per axis
```

### Contract

- Written at lower rate (10 ms default) to minimize RT load.
- All state enums encoded as `u8`. Reader casts to enum for display.
- All diagnostics flow through this segment (FR-134) as a live status snapshot. No direct logging from CU.
- No event ring buffer — only actual CU status is exposed (Session 2026-02-09).

---

## 5. evo_rpc_cu — gRPC API → Control Unit

**Purpose**: Manual commands from dashboard/API.  
**Writer**: `evo_grpc` (single writer)  
**Reader**: `evo_control_unit` (single reader)  
**Update rate**: Asynchronous — written on user action.

### Layout

```rust
#[repr(C, align(64))]
pub struct RpcToCuSegment {
    pub header:     P2pSegmentHeader,
    pub command:    RpcCommand,
}

#[repr(C)]
pub struct RpcCommand {
    pub command_type: u8,      // RpcCommandType
    pub axis_id:      u8,      // target axis (0 = global)
    pub _pad:         [u8; 6],
    pub param_f64:    f64,     // command-specific float param
    pub param_u32:    u32,     // command-specific int param
    pub sequence_id:  u32,     // monotonic, for ack tracking
}
```

### RpcCommandType

```rust
#[repr(u8)]
pub enum RpcCommandType {
    Nop               = 0,
    JogPositive       = 1,
    JogNegative       = 2,
    JogStop           = 3,
    MoveAbsolute      = 4,
    EnableAxis        = 5,
    DisableAxis       = 6,
    HomeAxis          = 7,
    ResetError        = 8,
    SetMachineState   = 9,   // param_u32 = target MachineState
    SetMode           = 10,
    GearChange        = 11,
    AcquireLock       = 12,  // request source lock
    ReleaseLock       = 13,
    AllowManualMode   = 14,  // FR-004: authorize manual mode for axis_id
    ReloadConfig      = 15,  // FR-145: hot-reload config during SAFETY_STOP
}
```

### Contract

- Same source-lock arbitration as RE commands.
- CU checks `locked_source != RecipeExecutor` before accepting RPC commands on an axis.
- JogPositive/JogNegative require `MachineState == Manual`.
- All commands rejected when `SafetyState == SafetyStop` except `ResetError` and `ReloadConfig` (FR-145).

---

## 6. evo_cu_re — Control Unit → Recipe Executor

**Purpose**: Command acknowledgments and axis status for recipe progression.  
**Writer**: `evo_control_unit` (single writer)  
**Reader**: `evo_recipe_executor` (single reader)  
**Status**: DRAFT / PLACEHOLDER (FR-134a) — preliminary layout; not yet formally specified in spec.md. Content definition will be finalized in a future iteration when Recipe Executor is implemented.

### Layout (preliminary — DRAFT)

```rust
#[repr(C, align(64))]
pub struct CuToReSegment {
    pub header:           P2pSegmentHeader,
    pub last_ack_seq_id:  u32,          // last completed sequence_id
    pub ack_status:       u8,           // 0=ok, 1=rejected, 2=error
    pub _pad:             [u8; 3],
    pub axes_in_position: u64,          // bit per axis (bit 0 = axis 1)
    pub axes_in_error:    u64,          // bit per axis
}
```

---

## Common: P2P Segment Header

```rust
#[repr(C, align(64))]
pub struct P2pSegmentHeader {
    pub magic:          [u8; 8],   // b"EVO_P2P\0"
    pub version_hash:   u32,       // const fn hash of struct layout
    pub heartbeat:      u64,       // monotonic cycle counter
    pub source_module:  u8,        // ModuleAbbrev
    pub dest_module:    u8,        // ModuleAbbrev
    pub payload_size:   u32,       // bytes after header
    pub write_seq:      u32,       // odd=writing, even=committed (lock-free protocol)
    pub _padding:       [u8; 34],  // pad to 64 bytes
}
// Total: 8+4+8+1+1+4+4+34 = 64 bytes

#[repr(u8)]
pub enum ModuleAbbrev {
    Cu  = 0,
    Hal = 1,
    Re  = 2,
    Mqt = 3,
    Rpc = 4,
}
```

### Version Hash Contract (FR-130d)

```rust
/// Compile-time version hash for struct compatibility.
/// If layout changes, hash changes, and reader/writer refuse to connect.
pub const fn struct_version_hash<T>() -> u32 {
    // Based on: core::mem::size_of::<T>() + core::mem::align_of::<T>()
    // XOR with compile-time type name hash
    let size = core::mem::size_of::<T>() as u32;
    let align = core::mem::align_of::<T>() as u32;
    size.wrapping_mul(0x9E3779B9) ^ align.wrapping_mul(0x517CC1B7)
}
```

### Heartbeat Contract (FR-130c)

- Writer increments `heartbeat` by 1 on every write cycle.
- Reader stores previous `heartbeat` value. If `current == previous` for N consecutive reads, segment is stale.
- Staleness thresholds:
  - RT segments (evo_hal_cu, evo_cu_hal): N = 3 (3 ms stale → SAFETY_STOP)
  - Non-RT segments (evo_re_cu, evo_rpc_cu): N = configurable (default 1000 = 1s)

### Write Sequence Protocol — Lock-Free (FR-130g)

- `write_seq` MUST be stored as `AtomicU32` (atomic access required for lock-free correctness)
- Initial value: `0` (even = committed, no write in progress)
- Writer protocol: (1) store `write_seq + 1` (odd, `Release`), (2) copy payload, (3) increment heartbeat, (4) store `write_seq + 1` (even, `Release`)
- Reader protocol: (1) load `write_seq` (`Acquire`), (2) if odd → retry, (3) copy payload, (4) reload `write_seq` (`Acquire`), (5) if changed → retry; max 3 retries (FR-130g)
- `u32` range: ~4.3 billion writes ≈ 49.7 days at 1 ms cycle. Wrapping is safe — protocol checks odd/even and changed/unchanged, not magnitude. After wrap, odd/even semantics are preserved
- Rationale: `u32` chosen over `u64` because `AtomicU32` is lock-free on all targets; `write_seq` only needs odd/even distinction, not monotonic ordering

### P2P SegmentInfo (FR-130i)

```rust
/// Runtime information about a discovered P2P segment.
/// Replaces broadcast-era SegmentInfo (which had reader_count, checksum).
pub struct SegmentInfo {
    pub name:          String,          // e.g., "evo_hal_cu"
    pub source_module: ModuleAbbrev,
    pub dest_module:   ModuleAbbrev,
    pub size_bytes:    usize,           // total file size (header + payload)
    pub writer_alive:  bool,            // flock probe: true if writer holds LOCK_EX
}
```

- `writer_alive` derived from attempting `flock(LOCK_EX | LOCK_NB)` on the file — if it fails, writer holds the lock

### Segment Size Constraints

- P2P minimum: 64 bytes (header only). Broadcast-era `SHM_MIN_SIZE = 4096` does not apply.
- All current segments ≤ 8 KB (see Segment Index). Maximum enforced by library: 1 MB.
- Library computes `ftruncate` size as `size_of::<P2pSegmentHeader>() + size_of::<T>()`.
- Segment count: 6 for CU, system-wide bounded only by `/dev/shm` tmpfs capacity.

### Version Hash — Canonical Algorithm

The canonical implementation of `struct_version_hash<T>()` uses `size_of::<T>()` and `align_of::<T>()` as shown in the Version Hash Contract above. This is the **authoritative algorithm** — not an example.

**Known limitation**: This hash catches size changes and alignment changes but does NOT detect field reordering within the same total size/alignment. This is an accepted trade-off:
- `#[repr(C)]` structs with explicit padding (as mandated by this spec) have deterministic field order
- Reordering fields in a `#[repr(C)]` struct almost always changes size or alignment due to padding differences
- A full field-by-field hash would require procedural macros; the size+align approach works at `const fn` level

If a more thorough hash is needed in the future, `struct_version_hash` can be replaced with a derive macro without changing the header format (same `u32` field).
