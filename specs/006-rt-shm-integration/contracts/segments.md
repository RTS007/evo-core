# SHM Segment Contracts

**Spec**: [../spec.md](../spec.md) | **Data Model**: [../data-model.md](../data-model.md)

All segment types are `#[repr(C, align(64))]` with `P2pSegmentHeader` as the first field.
All sizes are statically asserted at compile time.

## Naming Convention

Pattern: `evo_[SOURCE]_[DESTINATION]` where SOURCE and DESTINATION are `ModuleAbbrev::as_str()` values.

| Abbrev | `as_str()` | Binary |
|--------|-----------|--------|
| `Hal` | `"hal"` | `evo_hal` |
| `Cu` | `"cu"` | `evo_control_unit` |
| `Re` | `"re"` | `evo_recipe_executor` |
| `Rpc` | `"rpc"` | `evo_grpc` |
| `Mqt` | `"mqt"` | `evo_mqtt` |

## P2P Protocol Header (64 bytes)

Every segment begins with this header. Writer and reader never modify the same fields concurrently — the seqlock (`write_seq`) ensures torn-read detection.

```rust
#[repr(C, align(64))]
pub struct P2pSegmentHeader {
    pub magic: [u8; 8],           // b"EVO_P2P\0"
    pub version_hash: u32,        // struct_version_hash::<T>()
    pub heartbeat: u64,           // monotonic counter
    pub source_module: u8,        // ModuleAbbrev as u8
    pub dest_module: u8,          // ModuleAbbrev as u8
    pub payload_size: u32,        // size_of::<T>() - size_of::<Header>()
    pub write_seq: u32,           // odd=writing, even=committed
    pub _padding: [u8; 28],       // pad to 64
}
static_assert!(size_of::<P2pSegmentHeader>() == 64);
static_assert!(align_of::<P2pSegmentHeader>() == 64);
```

## Writer/Reader API Contract

### TypedP2pWriter\<T\>

```rust
impl<T: Copy + 'static> TypedP2pWriter<T> {
    /// Create a new SHM segment with exclusive write access.
    /// - shm_open(O_CREAT | O_RDWR, 0o600)
    /// - ftruncate to size_of::<T>()
    /// - mmap MAP_SHARED
    /// - flock(LOCK_EX | LOCK_NB) → ShmError::WriterAlreadyExists
    /// - Write header: magic, version_hash, source, dest, payload_size
    pub fn create(
        name: &str,
        source: ModuleAbbrev,
        dest: ModuleAbbrev,
    ) -> Result<Self, ShmError>;

    /// Lock-free write: RT-safe, zero alloc, zero syscall.
    /// 1. write_seq = odd (Release)
    /// 2. memcpy payload
    /// 3. heartbeat += 1
    /// 4. write_seq = even (Release)
    pub fn commit(&mut self, payload: &T);

    /// Returns current heartbeat counter.
    pub fn heartbeat(&self) -> u64;
}

impl<T> Drop for TypedP2pWriter<T> {
    /// shm_unlink + munmap + release flock
    fn drop(&mut self);
}
```

### TypedP2pReader\<T\>

```rust
impl<T: Copy + 'static> TypedP2pReader<T> {
    /// Attach to an existing SHM segment with shared read access.
    /// - shm_open(O_RDONLY)
    /// - mmap MAP_SHARED, PROT_READ
    /// - flock(LOCK_SH | LOCK_NB) → ShmError::ReaderAlreadyConnected
    /// - Validate: magic, version_hash, dest_module
    pub fn attach(
        name: &str,
        expected_dest: ModuleAbbrev,
    ) -> Result<Self, ShmError>;

    /// Lock-free read with bounded retry (max 3).
    /// 1. load write_seq (Acquire) → if odd, retry
    /// 2. memcpy payload
    /// 3. reload write_seq (Acquire) → if changed, retry
    /// 4. After 3 retries → ShmError::ReadContention
    pub fn read(&self) -> Result<T, ShmError>;

    /// Read heartbeat counter (Acquire ordering).
    pub fn heartbeat(&self) -> u64;
}

impl<T> Drop for TypedP2pReader<T> {
    /// munmap + release flock
    fn drop(&mut self);
}
```

## Active Segments

### #1 — HalToCuSegment (`evo_hal_cu`)

Writer: HAL | Reader: CU | Size: 2304B (current) → ~2816B after torque_estimate addition

```rust
#[repr(C, align(64))]
pub struct HalToCuSegment {
    pub header: P2pSegmentHeader,       // 64
    pub axis_count: u8,                 // 1
    pub _pad: [u8; 63],                 // 63
    pub axes: [HalAxisFeedback; 64],    // 24*64 = 1536 (→ 32*64 = 2048 with torque)
    pub di_bank: [u64; 16],             // 128
    pub ai_values: [f64; 64],           // 512
}

#[repr(C)]
pub struct HalAxisFeedback {
    pub position: f64,                  // 8
    pub velocity: f64,                  // 8
    // TODO: add torque_estimate: f64   // 8 (FR-011, audit G15)
    pub status_flags: u8,               // 1 (bit 0=ready, 1=fault, 2=enabled, 3=referenced, 4=zero_speed)
    pub fault_code: u16,                // 2
    pub _padding: [u8; 4],             // 4 (→ adjust after torque)
}
```

Written every HAL cycle. CU reads for axis feedback, DI bank, AI values.
Conversion: `HalStatus → HalToCuSegment` via `evo_common::shm::conversions`.

### #2 — CuToHalSegment (`evo_cu_hal`)

Writer: CU | Reader: HAL | Size: 3328B

```rust
#[repr(C, align(64))]
pub struct CuToHalSegment {
    pub header: P2pSegmentHeader,       // 64
    pub axis_count: u8,                 // 1
    pub _pad: [u8; 63],                 // 63
    pub axes: [CuAxisCommand; 64],      // 40*64 = 2560
    pub do_bank: [u64; 16],             // 128
    pub ao_values: [f64; 64],           // 512
}

#[repr(C)]
pub struct CuAxisCommand {
    pub output: ControlOutputVector,    // 32 (calculated_torque, target_velocity, target_position, torque_offset)
    pub enable: u8,                     // 1
    pub mode: u8,                       // 1 (OperationalMode as u8)
    pub _pad: [u8; 6],                 // 6
}
```

Written every CU cycle. HAL reads for control commands.
Conversion: `CuToHalSegment → HalCommands` via `evo_common::shm::conversions`.

## Skeleton Segments

### #3 — CuToMqtSegment (`evo_cu_mqt`)

Writer: CU | Reader: MQTT | Size: 3712B

Already fully defined (6 orthogonal state machines per axis, MachineState, SafetyState).
FR-043: `error_flags` MUST be `u32` — no truncation.

### #4 — HalToMqtSegment (`evo_hal_mqt`)

Writer: HAL | Reader: MQTT | Size: ~6KB estimated

```rust
#[repr(C, align(64))]
pub struct HalToMqtSegment {
    pub header: P2pSegmentHeader,       // 64
    pub axis_count: u8,                 // 1
    pub _pad: [u8; 63],                 // 63
    // Superset of HalToCuSegment
    pub axes: [HalAxisFeedback; 64],    // 1536+
    pub di_bank: [u64; 16],             // 128
    pub do_bank: [u64; 16],             // 128
    pub ai_values: [f64; 64],           // 512
    pub ao_values: [f64; 64],           // 512
    // Driver diagnostics
    pub cycle_time_ns: u64,             // 8
    pub driver_states: [u8; 64],        // 64 (per-axis driver state enum)
}
```

### #5 — ReToCuSegment (`evo_re_cu`)

Writer: RE | Reader: CU | Size: ~2648B (already defined in spec 005)

Existing struct: `RecipeCommand` + `MotionTarget[64]` + `sequence_id`.

### #6 — ReToHalSegment (`evo_re_hal`)

Writer: RE | Reader: HAL | Size: ~256B

```rust
#[repr(C, align(64))]
pub struct ReToHalSegment {
    pub header: P2pSegmentHeader,       // 64
    pub command_count: u8,              // 1 (number of active commands)
    pub _pad: [u8; 63],                // 63
    pub do_commands: [IoCommand; 16],   // direct DO set commands
    pub ao_commands: [IoCommand; 16],   // direct AO set commands
    pub request_id: u64,                // 8
}

#[repr(C)]
pub struct IoCommand {
    pub pin: u16,                       // target pin number
    pub _pad: [u8; 2],
    pub value_u32: u32,                 // for DO: 0/1, for AO: f32 bits
}
```

**Constraint**: HAL ignores commands for role-assigned pins (FR-036).

### #7 — ReToMqtSegment (`evo_re_mqt`)

Writer: RE | Reader: MQTT | Size: ~256B

```rust
#[repr(C, align(64))]
pub struct ReToMqtSegment {
    pub header: P2pSegmentHeader,       // 64
    pub current_step: u16,              // 2
    pub re_state: u8,                   // 1 (RecipeExecutorState enum)
    pub _pad: [u8; 5],                 // 5
    pub cycle_count: u64,               // 8
    pub error_code: u32,                // 4
    pub _pad2: [u8; 4],               // 4
    pub program_name: [u8; 64],         // 64
}
```

### #8 — ReToRpcSegment (`evo_re_rpc`)

Writer: RE | Reader: gRPC | Size: ~256B

```rust
#[repr(C, align(64))]
pub struct ReToRpcSegment {
    pub header: P2pSegmentHeader,       // 64
    pub progress_pct: u8,               // 1
    pub step_result: u8,                // 1 (enum)
    pub _pad: [u8; 6],                // 6
    pub request_id: u64,                // 8
    pub error_message: [u8; 128],       // 128
}
```

### #9 — RpcToCuSegment (`evo_rpc_cu`)

Writer: gRPC | Reader: CU | Size: 128B (already defined in spec 005)

Existing struct: `RpcCommand` (command_type, axis_id, params, sequence_id).

### #10 — RpcToHalSegment (`evo_rpc_hal`)

Writer: gRPC | Reader: HAL | Size: ~128B

```rust
#[repr(C, align(64))]
pub struct RpcToHalSegment {
    pub header: P2pSegmentHeader,       // 64
    pub command_type: u8,               // 1 (set_do, set_ao, driver_command)
    pub _pad: [u8; 3],                // 3
    pub target_pin: u16,                // 2
    pub target_axis: u8,                // 1
    pub _pad2: [u8; 1],               // 1
    pub value_f64: f64,                 // 8
    pub value_u32: u32,                 // 4
    pub request_id: u64,                // 8
}
```

### #11 — RpcToReSegment (`evo_rpc_re`)

Writer: gRPC | Reader: RE | Size: 128B

```rust
#[repr(C, align(64))]
pub struct RpcToReSegment {
    pub header: P2pSegmentHeader,       // 64
    pub _reserved: [u8; 64],           // placeholder — payload defined in separate spec
}
```

## Placeholder Segments (types defined, no init code)

### #12 — CuToReSegment (`evo_cu_re`)

Writer: CU | Reader: RE | Size: 128B (already defined)

Existing struct: `last_ack_seq_id`, `ack_status`, `axes_in_position`, `axes_in_error`.

### #13 — CuToRpcSegment (`evo_cu_rpc`)

Writer: CU | Reader: gRPC | Size: ~4KB+

Superset of `CuToMqtSegment` plus: per-axis PID internal state (`error`, `integral`, `output`), cycle timing (`last_cycle_ns`, `max_cycle_ns`, `jitter_histogram_us: [u32; 100]`).

### #14 — HalToRpcSegment (`evo_hal_rpc`)

Writer: HAL | Reader: gRPC | Size: ~256B

```rust
#[repr(C, align(64))]
pub struct HalToRpcSegment {
    pub header: P2pSegmentHeader,       // 64
    pub request_id: u64,                // 8 — correlates to inbound request
    pub result_code: u32,               // 4
    pub _pad: [u8; 4],                // 4
    pub error_message: [u8; 128],       // 128
}
```

### #15 — HalToReSegment (`evo_hal_re`)

Writer: HAL | Reader: RE | Size: ~4KB

```rust
#[repr(C, align(64))]
pub struct HalToReSegment {
    pub header: P2pSegmentHeader,       // 64
    pub axis_count: u8,                 // 1
    pub _pad: [u8; 63],               // 63
    // Full I/O state for recipe decision logic
    pub di_bank: [u64; 16],             // 128
    pub do_bank: [u64; 16],             // 128
    pub ai_values: [f64; 64],           // 512
    pub ao_values: [f64; 64],           // 512
    // Per-axis feedback (position, velocity, drive_ready)
    pub axes: [ReAxisFeedback; 64],     // TBD (subset of HalAxisFeedback)
}
```

## ShmError Contract

```rust
#[derive(Debug, thiserror::Error)]
pub enum ShmError {
    #[error("invalid magic bytes in segment header")]
    InvalidMagic,
    #[error("struct version hash mismatch: expected {expected:#010x}, found {found:#010x}")]
    VersionMismatch { expected: u32, found: u32 },
    #[error("destination module mismatch: expected {expected:?}, found {found:?}")]
    DestinationMismatch { expected: ModuleAbbrev, found: ModuleAbbrev },
    #[error("writer already exists for this segment (flock exclusive)")]
    WriterAlreadyExists,
    #[error("reader already connected to this segment (flock shared)")]
    ReaderAlreadyConnected,
    #[error("read contention: {retries} consecutive torn reads")]
    ReadContention { retries: u32 },
    #[error("segment not found: {name}")]
    SegmentNotFound { name: String },
    #[error("permission denied: {name}")]
    PermissionDenied { name: String },
    #[error("heartbeat stale: unchanged for {cycles} consecutive reads")]
    HeartbeatStale { cycles: u32 },
}
```

## Size Constraints

All segments must fit in a single 4KB or 8KB mmap region (page-aligned):

| Segment | Current Size | Page-Rounded | Status |
|---------|-------------|--------------|--------|
| HalToCuSegment | 2304B | 4KB | ✅ |
| CuToHalSegment | 3328B | 4KB | ✅ |
| CuToMqtSegment | 3712B | 4KB | ✅ |
| HalToMqtSegment | ~3KB | 4KB | ✅ |
| ReToCuSegment | ~2648B | 4KB | ✅ |
| CuToRpcSegment | ~4KB+ | 8KB | ✅ |
| All others | ≤256B | 4KB | ✅ |

Performance: write ≤ 5µs, read ≤ 2µs for any segment ≤ 8KB.
