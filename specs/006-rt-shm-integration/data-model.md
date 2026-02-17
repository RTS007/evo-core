# Data Model — 006-rt-shm-integration

Extracted from [spec.md](spec.md). Every entity is traced to its defining FR / section.

---

## Table of Contents

1. [SHM Segment Types (15)](#1-shm-segment-types-15)
2. [P2P Protocol Types](#2-p2p-protocol-types)
3. [Config Types](#3-config-types)
4. [HAL Types](#4-hal-types)
5. [CU Types](#5-cu-types)
6. [Watchdog Types](#6-watchdog-types)
7. [Constants](#7-constants)
8. [Entity Relationship Map](#8-entity-relationship-map)

---

## 1. SHM Segment Types (15)

All defined in `evo_common::shm::segments` — FR-010, FR-011 through FR-014f.  
Every struct: `#[repr(C)]`, fixed-size types only (no `String`, `Vec`, `HashMap`, `Option`).

### 1.1 Active Connections (fully implemented)

#### `HalToCuSegment` — #1

| Property | Value |
|---|---|
| **Segment name** | `evo_hal_cu` |
| **Writer → Reader** | HAL → CU |
| **Readiness** | Active (fully implemented) |
| **Defining FRs** | FR-011, FR-030, FR-035 |
| **Content** | Axis feedback (pos, vel, torque), DI bank, AI values, per-axis flags |

**Fields** (FR-011):

| Field | Type | Notes |
|---|---|---|
| `axes` | `[HalAxisFeedback; MAX_AXES]` | See §1.16 for sub-struct |
| `di_bank` | `[u64; 16]` | 1024 digital inputs, word-packed |
| `ai_values` | `[f64; MAX_AI]` | Analog inputs in engineering units (MAX_AI = 1024) |
| `axis_count` | `u8` | Number of active axes |

**Validation**: Heartbeat increments every HAL cycle. Version hash checked at attach (FR-002).  
**Relationships**: Written by HAL (FR-030); Read by CU (FR-040, FR-042). Converted from `HalStatus` via `evo_common::shm::conversions` (FR-035).

---

#### `CuToHalSegment` — #2

| Property | Value |
|---|---|
| **Segment name** | `evo_cu_hal` |
| **Writer → Reader** | CU → HAL |
| **Readiness** | Active (fully implemented) |
| **Defining FRs** | FR-012, FR-031, FR-035, FR-040 |
| **Content** | `ControlOutputVector` per axis, DO bank, AO values, enable commands |

**Fields** (FR-012):

| Field | Type | Notes |
|---|---|---|
| `axes` | `[CuAxisCommand; MAX_AXES]` | See §1.17 for sub-struct |
| `do_bank` | `[u64; 16]` | 1024 digital outputs, word-packed |
| `ao_values` | `[f64; MAX_AO]` | Analog outputs in engineering units (MAX_AO = 1024) |
| `axis_count` | `u8` | Number of active axes |

**Validation**: Heartbeat increments every CU cycle. If segment does not exist, HAL uses default (zero) commands (FR-031).  
**Relationships**: Written by CU (FR-040); Read by HAL (FR-031). Converted to `HalCommands` via `evo_common::shm::conversions` (FR-035).

---

### 1.2 Skeleton Connections (types defined + stub init code)

#### `CuToMqtSegment` — #3

| Property | Value |
|---|---|
| **Segment name** | `evo_cu_mqt` |
| **Writer → Reader** | CU → MQTT |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-013, FR-040, FR-090 |
| **Content** | Status snapshot: machine state, axis states, errors, safety state |

**Fields** (FR-013, spec 005 FR-134):

| Field | Type | Notes |
|---|---|---|
| `machine_state` | `MachineState` | Enum — current machine state |
| `safety_state` | `SafetyState` | Enum — current safety state |
| per-axis | all 6 orthogonal state machines | + safety flags + error states |
| `error_flags` | `u32` (or wider) | FR-043: NOT truncated via `as u16`/`as u8` |

> No event ring buffer — snapshot only.

**Relationships**: Written by CU every cycle (FR-040); Read by `evo_mqtt` (FR-090).

---

#### `HalToMqtSegment` — #4

| Property | Value |
|---|---|
| **Segment name** | `evo_hal_mqt` |
| **Writer → Reader** | HAL → MQTT |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014c, FR-030a, FR-090 |
| **Content** | Raw HAL data stream for service/diagnostics/oscilloscope |

**Fields** (FR-014c):

| Field | Type | Notes |
|---|---|---|
| *(all of `HalToCuSegment`)* | — | Axis feedback, DI bank, AI values |
| `do_bank` | `[u64; 16]` | Current output state for I/O visualization |
| `ao_values` | `[f64; MAX_AO]` | Current AO state |
| `cycle_time_ns` | `u64` | Driver cycle time in nanoseconds |
| `driver_state` | `[u8; MAX_AXES]` | Per-axis driver state enum discriminant |

**Relationships**: Written by HAL every cycle (FR-030a); Read by `evo_mqtt` (FR-090). Continuous telemetry for MQTT publishing.

---

#### `ReToCuSegment` — #5

| Property | Value |
|---|---|
| **Segment name** | `evo_re_cu` |
| **Writer → Reader** | RE → CU |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014, FR-040 (CU optionally attaches), FR-090 |
| **Content** | Motion requests, program commands, `AllowManualMode` |

**Fields**: Skeleton — details deferred to RE spec.  
**Relationships**: Written by `evo_recipe_executor` (FR-090); Read by CU via `try_attach_re()` (FR-044).

---

#### `ReToHalSegment` — #6

| Property | Value |
|---|---|
| **Segment name** | `evo_re_hal` |
| **Writer → Reader** | RE → HAL |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014e, FR-030c, FR-036, FR-090 |
| **Content** | Direct I/O commands from RE (set DO, set AO) — **only for pins without `IoRole` assignment** |

**Fields** (FR-014e):

| Field | Type | Notes |
|---|---|---|
| `set_do` | (pin, value) | Direct DO command |
| `set_ao` | (pin, value) | Direct AO command |
| `request_id` | `u64` | For ack correlation |

**Validation**: HAL ignores commands for role-assigned pins → logs `ERR_IO_ROLE_OWNED` (FR-036, Clarifications).  
**Relationships**: Written by `evo_recipe_executor` (FR-090); Read by HAL optionally (FR-030c).

---

#### `ReToMqtSegment` — #7

| Property | Value |
|---|---|
| **Segment name** | `evo_re_mqt` |
| **Writer → Reader** | RE → MQTT |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014b/FR-014f, FR-090 |
| **Content** | Recipe execution telemetry |

**Fields** (FR-014f):

| Field | Type | Notes |
|---|---|---|
| `current_step` | `u16` | Current recipe step |
| `program_name` | fixed-size array | Recipe/program name |
| `cycle_count` | `u64` | Execution cycle count |
| `re_state` | enum | RE state |
| `error_code` | `u32` | Error code |

**Relationships**: Written by `evo_recipe_executor` (FR-090); Read by `evo_mqtt` (FR-090).

---

#### `ReToRpcSegment` — #8

| Property | Value |
|---|---|
| **Segment name** | `evo_re_rpc` |
| **Writer → Reader** | RE → gRPC |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014b/FR-014f, FR-090 |
| **Content** | Recipe status/acks for Dashboard/API |

**Fields** (FR-014f):

| Field | Type | Notes |
|---|---|---|
| `execution_progress` | percent | Execution progress |
| `step_result` | enum | Step result |
| `error_message` | fixed-size array | Error message |
| `request_id` | `u64` | For ack correlation |

**Relationships**: Written by `evo_recipe_executor` (FR-090); Read by `evo_grpc` (FR-090).

---

#### `RpcToCuSegment` — #9

| Property | Value |
|---|---|
| **Segment name** | `evo_rpc_cu` |
| **Writer → Reader** | gRPC → CU |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014, FR-040, FR-090 |
| **Content** | External commands: jog, mode change, config reload, service bypass |

**Fields**: Skeleton — external commands.  
**Relationships**: Written by `evo_grpc` (FR-090); Read by CU via `try_attach_rpc()` (FR-044).

---

#### `RpcToHalSegment` — #10

| Property | Value |
|---|---|
| **Segment name** | `evo_rpc_hal` |
| **Writer → Reader** | gRPC → HAL |
| **Readiness** | Skeleton |
| **Defining FRs** | FR-014d, FR-030b, FR-090 |
| **Content** | Direct HAL commands: set DO, set AO, driver config |

**Fields** (FR-014d):

| Field | Type | Notes |
|---|---|---|
| action command | set_do, set_ao, driver_command | Action type |
| target pin/axis | `u16` | Target pin or axis index (skeleton — resolved in gRPC spec) |
| value | `f64` | Command value (skeleton — resolved in gRPC spec) |
| `request_id` | `u64` | For ack correlation |

**Relationships**: Written by `evo_grpc` (FR-090); Read by HAL optionally (FR-030b).

---

#### `RpcToReSegment` — #11

| Property | Value |
|---|---|
| **Segment name** | `evo_rpc_re` |
| **Writer → Reader** | gRPC → RE |
| **Readiness** | Skeleton (payload defined in separate spec) |
| **Defining FRs** | FR-014d, FR-090, Clarifications |
| **Content** | Placeholder — content defined in a separate spec |

**Fields**: Empty struct with heartbeat only (FR-014d).  
**Relationships**: Written by `evo_grpc` (FR-090); Read by `evo_recipe_executor` (FR-090).

---

### 1.3 Placeholder Connections (types defined only — no init code)

#### `CuToReSegment` — #12

| Property | Value |
|---|---|
| **Segment name** | `evo_cu_re` |
| **Writer → Reader** | CU → RE |
| **Readiness** | Placeholder |
| **Defining FRs** | FR-014, FR-040 |
| **Content** | Ack, execution status, axis availability, error feedback |

**Fields**: Reserved placeholder — empty struct with heartbeat only (FR-014).  
**Relationships**: Written by CU (FR-040 creates writer); Read by `evo_recipe_executor` (FR-090).

---

#### `CuToRpcSegment` — #13

| Property | Value |
|---|---|
| **Segment name** | `evo_cu_rpc` |
| **Writer → Reader** | CU → gRPC |
| **Readiness** | Placeholder |
| **Defining FRs** | FR-014a, FR-040 |
| **Content** | Full diagnostic snapshot for Dashboard/API (superset of MQT) |

**Fields** (FR-014a):

| Field | Type | Notes |
|---|---|---|
| *(all of `CuToMqtSegment`)* | — | Superset |
| per-axis PID state | error, integral, output | For tuning visualization |
| `last_cycle_ns` | `u64` | Last cycle duration in nanoseconds |
| `max_cycle_ns` | `u64` | Max observed cycle duration in nanoseconds |
| `jitter_histogram_us` | fixed-size array | Jitter histogram |

**Relationships**: Written by CU (FR-040 creates writer); Read by `evo_grpc` (FR-090). Served via gRPC to Dashboard/API.

---

#### `HalToRpcSegment` — #14

| Property | Value |
|---|---|
| **Segment name** | `evo_hal_rpc` |
| **Writer → Reader** | HAL → gRPC |
| **Readiness** | Placeholder |
| **Defining FRs** | FR-014d, FR-030b |
| **Content** | HAL action responses/acks (DO set confirmation, driver state, error feedback) |

**Fields** (FR-014d):

| Field | Type | Notes |
|---|---|---|
| `request_id` | `u64` | Correlates to inbound request |
| `result_code` | `u32` | Result of action (skeleton — resolved in gRPC spec) |
| `error_message` | fixed-size array | Error feedback |

**Relationships**: Written by HAL (FR-030b creates writer); Read by `evo_grpc` (FR-090).

---

#### `HalToReSegment` — #15

| Property | Value |
|---|---|
| **Segment name** | `evo_hal_re` |
| **Writer → Reader** | HAL → RE |
| **Readiness** | Placeholder |
| **Defining FRs** | FR-014e, FR-030c |
| **Content** | HAL feedback to RE: current I/O states, axis positions, velocities, drive status |

**Fields** (FR-014e):

| Field | Type | Notes |
|---|---|---|
| `di_bank` | `[u64; 16]` | Current DI states |
| `do_bank` | `[u64; 16]` | Current DO states |
| `ai_values` | `[f64; MAX_AI]` | Current AI values |
| `ao_values` | `[f64; MAX_AO]` | Current AO values |
| per-axis | position, velocity, drive_ready | Per-axis feedback |

**Relationships**: Written by HAL every cycle (FR-030c); Read by `evo_recipe_executor` (FR-090). Fast read path for recipe decision logic.

---

### 1.16 Sub-struct: `HalAxisFeedback`

Defined in: `evo_common::shm::segments` (FR-011, Key Entities).

| Field | Type | Notes |
|---|---|---|
| `position` | `f64` | Current position |
| `velocity` | `f64` | Current velocity |
| `torque_estimate` | `f64` | Torque estimate (audit G15) |
| `drive_ready` | `bool` | Drive ready flag |
| `drive_fault` | `bool` | Drive fault flag |
| `referenced` | `bool` | Axis referenced (homed) |
| `active` | `bool` | Axis active flag |

---

### 1.17 Sub-struct: `CuAxisCommand`

Defined in: `evo_common::shm::segments` (FR-012, Key Entities).

| Field | Type | Notes |
|---|---|---|
| `calculated_torque` | (part of `ControlOutputVector`) | 4-field vector |
| `target_velocity` | (part of `ControlOutputVector`) | |
| `target_position` | (part of `ControlOutputVector`) | |
| `torque_offset` | (part of `ControlOutputVector`) | |
| `enable` | `bool` | Axis enable command |
| `brake_release` | `bool` | Brake release command |

---

### Segment Summary Table

| # | Segment Name | Writer | Reader | Payload Struct | Readiness | Primary FR |
|---|---|---|---|---|---|---|
| 1 | `evo_hal_cu` | HAL | CU | `HalToCuSegment` | Active | FR-011 |
| 2 | `evo_cu_hal` | CU | HAL | `CuToHalSegment` | Active | FR-012 |
| 3 | `evo_cu_mqt` | CU | MQTT | `CuToMqtSegment` | Skeleton | FR-013 |
| 4 | `evo_hal_mqt` | HAL | MQTT | `HalToMqtSegment` | Skeleton | FR-014c |
| 5 | `evo_re_cu` | RE | CU | `ReToCuSegment` | Skeleton | FR-014 |
| 6 | `evo_re_hal` | RE | HAL | `ReToHalSegment` | Skeleton | FR-014e |
| 7 | `evo_re_mqt` | RE | MQTT | `ReToMqtSegment` | Skeleton | FR-014f |
| 8 | `evo_re_rpc` | RE | gRPC | `ReToRpcSegment` | Skeleton | FR-014f |
| 9 | `evo_rpc_cu` | gRPC | CU | `RpcToCuSegment` | Skeleton | FR-014 |
| 10 | `evo_rpc_hal` | gRPC | HAL | `RpcToHalSegment` | Skeleton | FR-014d |
| 11 | `evo_rpc_re` | gRPC | RE | `RpcToReSegment` | Skeleton | FR-014d |
| 12 | `evo_cu_re` | CU | RE | `CuToReSegment` | Placeholder | FR-014 |
| 13 | `evo_cu_rpc` | CU | gRPC | `CuToRpcSegment` | Placeholder | FR-014a |
| 14 | `evo_hal_rpc` | HAL | gRPC | `HalToRpcSegment` | Placeholder | FR-014d |
| 15 | `evo_hal_re` | HAL | RE | `HalToReSegment` | Placeholder | FR-014e |

---

## 2. P2P Protocol Types

All defined in `evo_common::shm::p2p` — FR-001 through FR-009.

### 2.1 `P2pSegmentHeader`

Defined in: FR-002, Key Entities.  
Size: **64 bytes** fixed.

| Field | Size | Type | Notes |
|---|---|---|---|
| `magic` | 8 B | `[u8; 8]` | `b"EVO_P2P\0"` — validated at attach |
| `write_seq` | 4 B | `AtomicU32` | Odd = write in progress, even = consistent. Acquire/Release ordering |
| `heartbeat` | 8 B | `u64` (atomic) | Incremented every write cycle. Stale detection: unchanged for N consecutive reads (default N=3) |
| `version_hash` | 4 B | `u32` | `const fn struct_version_hash<T>()` — mismatch → `ShmError::VersionMismatch` |
| `source_module` | 1 B | `ModuleAbbrev` | Writer's module |
| `dest_module` | 1 B | `ModuleAbbrev` | Expected reader's module — validated at attach |
| `reserved` | 38 B | `[u8; 38]` | Padding to 64 bytes |

**Lock-free write protocol** (FR-002):
1. Set `write_seq` to odd (Release)
2. Copy payload
3. Increment heartbeat
4. Set `write_seq` to even (Release)

**Lock-free read protocol** (FR-002):
1. Load `write_seq` (Acquire) → if odd, retry
2. Copy payload
3. Reload `write_seq` (Acquire) → if changed, retry
4. Max 3 retries → `ShmError::ReadContention`

---

### 2.2 `TypedP2pWriter<T>`

Defined in: FR-001, FR-002, FR-008, Key Entities.  
Location: `evo_common::shm::p2p`

| API | Description |
|---|---|
| `::create(name, source, dest)` | Creates SHM segment via `shm_open(O_CREAT, 0o600)` + `mmap`. Writes `P2pSegmentHeader`. Acquires `flock(LOCK_EX \| LOCK_NB)`. |
| `.write(&T)` | Lock-free write: seq odd → copy → heartbeat++ → seq even. Zero heap, zero syscall, zero mutex. |
| `Drop` | Calls `shm_unlink` + `munmap` + releases flock (FR-008) |

**Enforcement**: Single-writer via `flock(LOCK_EX | LOCK_NB)` — second writer gets `ShmError::WriterAlreadyExists` (FR-002).  
**RT-safety** (FR-003): No mutex, no heap, no syscalls, no panic in hot path.

---

### 2.3 `TypedP2pReader<T>`

Defined in: FR-001, FR-002, FR-008, Key Entities.  
Location: `evo_common::shm::p2p`

| API | Description |
|---|---|
| `::attach(name, my_module)` | Opens existing SHM via `shm_open(O_RDONLY)` + `mmap`. Validates magic, destination module, version hash. Acquires `flock(LOCK_SH \| LOCK_NB)`. |
| `.read() -> Result<T>` | Lock-free read with bounded retry (max 3). Returns `ShmError::ReadContention` if exhausted. Returns `ShmError::HeartbeatStale` if heartbeat frozen for N reads. |
| `Drop` | Calls `munmap` + releases flock (FR-008) |

**Enforcement**: Single-reader via `flock(LOCK_SH | LOCK_NB)` — second reader gets `ShmError::ReaderAlreadyConnected` (FR-002).  
**Validation at attach** (FR-002):
- Magic must be `b"EVO_P2P\0"` → else `ShmError::InvalidMagic`
- `dest_module` must match `my_module` → else `ShmError::DestinationMismatch`
- `version_hash` must match `struct_version_hash::<T>()` → else `ShmError::VersionMismatch`

---

### 2.4 `ShmError` (9 variants)

Defined in: FR-004 (spec 005 FR-130h).  
Location: `evo_common::shm::p2p`

| Variant | Trigger | Category |
|---|---|---|
| `InvalidMagic` | Header magic ≠ `b"EVO_P2P\0"` | Attach validation |
| `VersionMismatch` | `version_hash` mismatch between writer and reader binary | Attach validation |
| `DestinationMismatch` | Reader's `ModuleAbbrev` ≠ header's `dest_module` | Attach validation |
| `WriterAlreadyExists` | `flock(LOCK_EX \| LOCK_NB)` fails — another writer holds lock | Create enforcement |
| `ReaderAlreadyConnected` | `flock(LOCK_SH \| LOCK_NB)` fails — another reader holds lock | Attach enforcement |
| `ReadContention` | 3 consecutive read retries exhausted (torn read) | Read hot path |
| `SegmentNotFound` | `shm_open` fails — segment file does not exist | Attach |
| `PermissionDenied` | `shm_open` fails — file mode mismatch | Attach |
| `HeartbeatStale` | Heartbeat unchanged for N consecutive reads (default N=3) | Read — writer dead |

---

### 2.5 `ModuleAbbrev`

Defined in: FR-005.  
Location: `evo_common::shm::p2p`

| Variant | Maps to binary | `as_str()` |
|---|---|---|
| `Hal` | `evo_hal` | `"hal"` |
| `Cu` | `evo_control_unit` | `"cu"` |
| `Re` | `evo_recipe_executor` | `"re"` |
| `Rpc` | `evo_grpc` (RT↔non-RT SHM bridge) | `"rpc"` |
| `Mqt` | `evo_mqtt` | `"mqt"` |

Derives: `Copy`, `Clone`, `PartialEq`, `Eq`.  
**Note**: `evo_api`, Dashboard, Diagnostic have **no** `ModuleAbbrev` — they have no SHM segments.

Segment naming: `evo_[SOURCE]_[DESTINATION]` via `ModuleAbbrev::as_str()` (FR-006).  
Fixed names — no PID suffix, deterministic across restarts.

---

### 2.6 `SegmentDiscovery`

Defined in: FR-007.  
Location: `evo_common::shm::p2p`

| API | Returns | Notes |
|---|---|---|
| `list_segments()` | `Vec<SegmentInfo>` | Enumerates `/dev/shm/evo_*` |
| `list_for(module)` | `Vec<SegmentInfo>` | Segments addressed to a given module |

**`SegmentInfo`**:

| Field | Type | Notes |
|---|---|---|
| `name` | `String` | Segment name |
| `source_module` | `ModuleAbbrev` | Parsed from name |
| `dest_module` | `ModuleAbbrev` | Parsed from name |
| `size` | `usize` | Mapped size |
| `writer_alive` | `bool` | Probed via non-blocking flock test |

---

## 3. Config Types

### 3.1 `SystemConfig` / `WatchdogConfig`

Defined in: FR-059a, Clarifications.  
Location: `evo_common::config`  
Source file: `config.toml`

**`SystemConfig`** — top-level struct with per-program sub-structs:

| Section | Sub-struct | Status |
|---|---|---|
| `[watchdog]` | `WatchdogConfig` | Defined (see below) |
| `[hal]` | stub | Placeholder |
| `[cu]` | stub | Placeholder |
| `[re]` | stub | Placeholder |
| `[mqtt]` | stub | Placeholder |
| `[grpc]` | stub | Placeholder |
| `[api]` | stub | Placeholder |
| `[dashboard]` | stub | Placeholder |
| `[diagnostic]` | stub | Placeholder |

**`WatchdogConfig`** fields (FR-059a, FR-022):

| Field | Type | Default | Notes |
|---|---|---|---|
| `max_restarts` | `u32` | 5 | Max consecutive restarts before degraded state |
| `initial_backoff_ms` | `u64` | 100 | Initial restart delay |
| `max_backoff_s` | `u64` | 30 | Maximum restart delay |
| `stable_run_s` | `u64` | 60 | Successful run duration to reset backoff counter |
| `sigterm_timeout_s` | `f64` | 2.0 | Timeout before escalating to SIGKILL |
| `hal_ready_timeout_s` | `f64` | 5.0 | Timeout waiting for `evo_hal_cu` segment |

**Validation**: FR-054 — all numeric params have min/max bounds as `const` in `evo_common`, validated at load time.  
**State transitions**: After `max_restarts` exceeded → watchdog enters degraded state (stays alive, stops restarting, logs single CRITICAL error).

---

### 3.2 `MachineConfig`

Defined in: Architecture section (machine.toml), FR-050.  
Location: `evo_common::config`  
Source file: `machine.toml`

**Sections**:

| Section | Fields | Notes |
|---|---|---|
| `[machine]` | `name: String` | Machine identity |
| `[global_safety]` | `default_safe_stop: String` ("SS1"/"SS2"/"STO"), `safety_stop_timeout: f64`, `recovery_authorization_required: bool` | Global safety params |
| `[service_bypass]` | `bypass_axes: Vec<u8>`, `max_service_velocity: f64` | Service mode |

**Validation**:
- No `[[axes]]` array or `axes_dir` → `ConfigError::UnknownField` (FR-056)
- Unknown fields → `ConfigError::UnknownField` (FR-053, strict parsing)
- Axis files are **not listed** in `machine.toml` — auto-discovered (FR-055)

**Relationships**: Loaded by both HAL and CU (FR-050). Machine-specific parameters only; system params in `config.toml`.

---

### 3.3 `IoConfig` / `IoGroup` / `IoPoint`

Defined in: FR-051, FR-052.  
Location: `evo_common::io`  
Source file: `io.toml`

**`IoConfig`** — parsed representation of `io.toml`:
- Contains groups of I/O points
- Single source of truth for all I/O pin assignments, NC/NO logic, debounce, scaling curves, functional roles

**`IoGroup`** — group of related I/O points.

**`IoPoint`** — individual I/O pin:
- Pin number, type (DI/DO/AI/AO), logic (NC/NO), debounce, scaling curve, offset
- `IoRole` assignment (optional)

**`IoRole`** — functional role enum/string:
- Convention: `FunctionAxisNumber` (e.g., `EStop`, `LimitMin1`, `BrakeOut1`, `BrakeIn3`)
- Single flat list
- Cross-referenced from axis files (e.g., `do_brake = "BrakeOut1"` in axis TOML)
- Resolved at runtime via `IoRegistry`

**`IoRegistry`** — runtime role→pin resolver (FR-034, FR-052, Key Entities):

| API | Description |
|---|---|
| Built from `io.toml` at startup | Both HAL and CU build identical instances |
| `read_di(role) -> bool` | Read digital input by role |
| `read_ai(role) -> f64` | Read analog input by role |
| `write_do(role, value)` | Write digital output by role |
| `write_ao(role, value)` | Write analog output by role |

**Validation**:
- Missing required role → `ERR_IO_ROLE_MISSING` (e.g., axis 3 has brake but `BrakeIn3` not in `io.toml`)
- Role on wrong I/O type → `ERR_IO_ROLE_TYPE_MISMATCH`
- NC/NO inversion handled transparently (FR-015)

**I/O ownership rules** (FR-036, Clarifications):
- Role-assigned pins → controlled **only** by CU (via `evo_cu_hal`)
- RE may **read** any I/O state (via `evo_hal_re`)
- RE may **write** only pins **without** `IoRole` (via `evo_re_hal`)
- CU does NOT control or read pins without `IoRole`
- HAL ignores RE commands for role-assigned pins → `ERR_IO_ROLE_OWNED`

---

### 3.4 `AxisConfig`

Defined in: FR-055, FR-057, Key Entities.  
Location: `evo_common::config`  
Source file: `axis_NN_name.toml` (per-axis, auto-discovered)

**File naming**: `axis_NN_name.toml` — NN = axis number (01–64), name = free-form label.

| Section | Required | Key Fields |
|---|---|---|
| `[axis]` | **Yes** | `id: u8` (MUST match NN), `name: String`, `type: "linear"\|"rotary"` |
| `[kinematics]` | **Yes** | `max_velocity`, `max_acceleration` (opt), `safe_reduced_speed_limit`, `min_pos`, `max_pos`, `in_position_window` |
| `[control]` | **Yes** | `kp`, `ki`, `kd`, `tf` (def 0.001), `tt` (def 0.01), `kvff` (def 0.0), `kaff` (def 0.0), `friction` (def 0.0), `jn` (def 0.01), `bn` (def 0.001), `gdob` (def 200.0), `f_notch` (def 0), `bw_notch` (def 0), `flp` (def 0), `out_max` (def 100.0), `lag_error_limit`, `lag_policy` (def "Error": "Unwanted"/"Warning"/"Error") |
| `[safe_stop]` | **Yes** | `category: "SS1"\|"SS2"\|"STO"`, `max_decel_safe`, `sto_brake_delay` (def 0.1), `ss2_holding_torque` (def 0.0) |
| `[homing]` | **Yes** | `method: "HomeSensor"\|"TorqueLimit"\|"IndexPulse"`, `speed`, `torque_limit` (def 30.0), `timeout` (def 30.0), `approach_direction` (def "Positive": "Positive"/"Negative") |
| `[brake]` | Optional | `do_brake: IoRole`, `di_released: IoRole`, `release_timeout` (def 2.0), `engage_timeout` (def 1.0) |
| `[tailstock]` | Optional | `coupled_axis: u8`, `clamp_role: IoRole`, `clamped_role: IoRole` (opt), `max_force` (opt) |
| `[guard]` | Optional | `di_guard: IoRole`, `stop_on_open` (def "SS1") |
| `[coupling]` | Optional | `master_axis: u8`, `ratio` (def 1.0), `max_sync_error` |

**Validation** (FR-055a):
- `[axis].id` MUST match NN in filename → else `ConfigError::AxisIdMismatch { file, expected, found }`
- No duplicate NN → else `ConfigError::DuplicateAxisId(NN)`
- Zero axis files → `ConfigError::NoAxesDefined`
- `#[serde(deny_unknown_fields)]` (FR-053)
- All numeric params validated against min/max bounds (FR-054)

**Relationships**: Both HAL and CU load all axis files (FR-050). HAL uses kinematics for simulation limits; CU uses control params for PID and safety.

---

### 3.5 `ConfigLoader`

Defined in: FR-053, FR-055, FR-056, Key Entities.  
Location: `evo_common::config`

| API | Description |
|---|---|
| Loads `config.toml` | Parses `SystemConfig` with per-program sub-structs |
| Loads `machine.toml` | Parses `MachineConfig` (global safety, service bypass) |
| Loads `io.toml` | Parses `IoConfig` (all I/O definitions) |
| Auto-discovers `axis_*_*.toml` | Glob pattern scan in same directory, sorted by NN |
| Validates consistency | Axis ID vs filename, duplicate detection, required roles |

**Error types**:

| Error | Trigger |
|---|---|
| `ConfigError::UnknownField` | Unknown field in any TOML (strict parsing) |
| `ConfigError::ParseError` | Missing mandatory field |
| `ConfigError::AxisIdMismatch { file, expected, found }` | `[axis].id` ≠ NN in filename |
| `ConfigError::DuplicateAxisId(u8)` | Two axis files with same NN |
| `ConfigError::NoAxesDefined` | Zero axis files found |
| `ConfigError::ValidationError` | Numeric out of bounds, axes > MAX_AXES |
| `ConfigError::FileNotFound` | Config file missing (hot-reload rejection) |

**Design rules**:
- Flat directory — no subdirectories (FR-055)
- Only new per-axis format — `[[axes]]` array is rejected (FR-056)
- Self-documenting TOML header in every file (Architecture section)
- Breaking change: no backward compatibility layer

---

### 3.6 `AnalogCurve`

Defined in: FR-081.  
Location: `evo_common::io::config` (single definition — duplicate in `hal::config` removed)

Matching `io.toml` spec:
- Preset string: `"linear"`, `"quadratic"`, `"cubic"`
- Custom: `[a, b, c]` polynomial coefficients
- Separate `offset` field

---

## 4. HAL Types

### 4.1 `HalStatus`

Defined in: FR-030, FR-035 (existing in `hal/types.rs`).

Internal HAL type produced by `driver.cycle()`. Contains axis positions, velocities, I/O state.

**Conversion** (FR-035, `evo_common::shm::conversions`):
- `HalStatus → HalToCuSegment` — pack axis feedback, DI bank (word-packed with NC/NO inversion), AI values (scaled to engineering units)

---

### 4.2 `HalCommands`

Defined in: FR-031, FR-035 (existing in `hal/types.rs`).

Internal HAL type consumed by `driver.cycle()`. Contains control outputs, DO commands, AO values.

**Conversion** (FR-035, `evo_common::shm::conversions`):
- `CuToHalSegment → HalCommands` — unpack axis commands, DO bank, AO values

---

### 4.3 I/O Helper Functions

Defined in: FR-015.  
Location: `evo_common::shm::io_helpers`

| Function | Signature | Notes |
|---|---|---|
| `get_di` | `(bank: &[u64; 16], pin: usize) -> bool` | Read DI from word-packed bank |
| `set_do` | `(bank: &mut [u64; 16], pin: usize, value: bool)` | Write DO to word-packed bank |

Both handle NC/NO inversion when used through `IoRegistry`.

Bit-packing convention: bit N of word W = pin `N*64+W`.

---

## 5. CU Types

### 5.1 `CycleRunner`

Defined in: FR-040, FR-041, Key Entities.  
Location: `evo_control_unit`

**Extended runtime state** (FR-041):

| Field | Type | Notes |
|---|---|---|
| `io_registry` | `IoRegistry` | For safety evaluation & I/O role resolution |
| `axis_control_state` | `[AxisControlState; MAX_AXES]` | Per-axis runtime state |
| `ucps` | per-axis `UniversalControlParameters` | Loaded from axis configs |

**Cycle body** (FR-040, FR-042):
1. Read `evo_hal_cu` → axis feedback + `di_bank` + `ai_values`
2. Process state machines
3. Write `evo_cu_hal` → control outputs + `do_bank` + `ao_values`
4. Write `evo_cu_mqt` → status snapshot

**SHM connections** (FR-040):
- **Mandatory attach**: `evo_hal_cu` (wait with timeout)
- **Create writers**: `evo_cu_hal`, `evo_cu_mqt`, `evo_cu_re`, `evo_cu_rpc`
- **Optional attach** (retry once/sec — FR-044): `evo_re_cu`, `evo_rpc_cu`

---

### 5.2 `ShmBundle`

Defined in: Key Entities.

Container for all CU's SHM readers and writers. Bundles the mandatory and optional P2P connections.

---

### 5.3 `AxisControlState`

Defined in: FR-041, Key Entities.

Per-axis runtime state for CU control loop:
- PID integrator state
- DOB (Disturbance Observer) state
- Filter states (notch, low-pass)
- Per-axis control parameters reference

Array: `[AxisControlState; MAX_AXES]`

---

## 6. Watchdog Types

### 6.1 `WatchdogTrait`

Defined in: FR-027, Key Entities.  
Location: `evo_common` (trait definition); implemented in `evo` binary.

| Method | Description |
|---|---|
| `spawn_module(name, config)` | Start a child process |
| `health_check(name)` | Query module liveness |
| `restart_module(name)` | Stop + restart with backoff |
| `shutdown_all()` | Ordered shutdown of all modules |

**State transitions** (FR-022, FR-023):
- **Startup order**: HAL → (wait for `evo_hal_cu` segment, timeout 5s) → CU
- **Shutdown order**: CU → (SIGTERM, wait 2s) → HAL → (SIGTERM, wait 2s) → SIGKILL remaining → `shm_unlink` all `evo_*`
- **Crash handling**: detect via `waitpid` → restart with exponential backoff (100ms → 30s max)
- **Degraded state**: after `max_restarts` exceeded → stop restarting, log single CRITICAL, await operator

**Supplementary monitoring** (FR-028): MAY read P2P segment headers (first 64 bytes) directly for hang detection — heartbeat frozen while process is alive.

---

## 7. Constants

All in `evo_common::consts` — FR-080, FR-083.  
Single definition, all other crates import.

### 7.1 `MAX_*` Constants

| Constant | Type | Value | FR | Notes |
|---|---|---|---|---|
| `MAX_AXES` | `usize` | 64 | FR-080 | Was triplicated (2 types) — now single definition |
| `MAX_DI` | `usize` | 1024 | FR-080 | Moved from `hal::consts` |
| `MAX_DO` | `usize` | 1024 | FR-080 | Moved from `hal::consts` |
| `MAX_AI` | `usize` | 1024 | FR-080, Clarifications | Moved from `hal::consts` |
| `MAX_AO` | `usize` | 1024 | FR-080, Clarifications | Moved from `hal::consts` |

### 7.2 `DEFAULT_*` Constants

| Constant | Type | Value | FR | Notes |
|---|---|---|---|---|
| `DEFAULT_CONFIG_PATH` | `&str` | `"/etc/evo"` | FR-080 | Global config directory |
| `DEFAULT_STATE_FILE` | `&str` | `"/etc/evo/hal_state"` | FR-080 | Same directory as config |

### 7.3 Timing Constants

| Constant | Type | Value | FR | Notes |
|---|---|---|---|---|
| `CYCLE_TIME_US` | `u64` | `1000` | FR-083 | Single definition (1kHz cycle) |

### 7.4 P2P Protocol Constants

| Constant | Type | Value | FR | Notes |
|---|---|---|---|---|
| `P2P_SHM_MAGIC` | `[u8; 8]` | `b"EVO_P2P\0"` | FR-002, FR-082 | Only magic constant — `EVO_SHM_MAGIC` removed |

### 7.5 Removed Constants

| Constant | Reason | FR |
|---|---|---|
| `EVO_SHM_MAGIC` | Broadcast-era artifact — removed entirely | FR-082 |
| `HAL_SERVICE_NAME` | Artifact of `evo_shared_memory` — use `ModuleAbbrev` instead | FR-080 |

---

## 8. Entity Relationship Map

```
┌─────────────────────────────────────────────────────────────────────┐
│                        evo_common::consts                           │
│  MAX_AXES, MAX_DI, MAX_DO, MAX_AI, MAX_AO, CYCLE_TIME_US           │
│  DEFAULT_CONFIG_PATH, DEFAULT_STATE_FILE                            │
└──────────┬──────────────────────────────────────────────────────────┘
           │ imported by all crates
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     evo_common::config                              │
│  ConfigLoader ──loads──▶ SystemConfig (config.toml)                 │
│                         MachineConfig (machine.toml)                │
│                         IoConfig (io.toml) ──builds──▶ IoRegistry   │
│                         AxisConfig[] (axis_NN_*.toml)               │
└──────────┬──────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│                   evo_common::shm::p2p                              │
│  TypedP2pWriter<T> / TypedP2pReader<T>                              │
│  P2pSegmentHeader (64B)                                             │
│  ModuleAbbrev (Hal, Cu, Re, Rpc, Mqt)                               │
│  ShmError (9 variants)                                              │
│  SegmentDiscovery                                                   │
└──────────┬──────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│               evo_common::shm::segments                             │
│  15 segment structs (#[repr(C)], fixed-size)                        │
│  HalAxisFeedback, CuAxisCommand (sub-structs)                       │
└──────────┬──────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│             evo_common::shm::conversions                            │
│  HalStatus → HalToCuSegment                                        │
│  CuToHalSegment → HalCommands                                      │
└──────────┬──────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│             evo_common::shm::io_helpers                             │
│  get_di(bank, pin), set_do(bank, pin, value)                        │
│  NC/NO inversion via IoRegistry                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Per-Program SHM Ownership

| Program | Outbound Writers (4 each for RT) | Inbound Readers |
|---|---|---|
| **HAL** | `evo_hal_cu`, `evo_hal_mqt`, `evo_hal_rpc`, `evo_hal_re` | `evo_cu_hal`, `evo_re_hal` (opt), `evo_rpc_hal` (opt) |
| **CU** | `evo_cu_hal`, `evo_cu_mqt`, `evo_cu_rpc`, `evo_cu_re` | `evo_hal_cu` (mandatory), `evo_re_cu` (opt), `evo_rpc_cu` (opt) |
| **RE** | `evo_re_cu`, `evo_re_hal`, `evo_re_mqt`, `evo_re_rpc` | `evo_cu_re`, `evo_hal_re`, `evo_rpc_re` |
| **gRPC** (`evo_grpc`) | `evo_rpc_cu`, `evo_rpc_hal`, `evo_rpc_re` | `evo_cu_rpc`, `evo_hal_rpc`, `evo_re_rpc` |
| **MQTT** (`evo_mqtt`) | *(none — read-only)* | `evo_cu_mqt`, `evo_hal_mqt`, `evo_re_mqt` |
| **API** (`evo_api`) | *(no SHM)* | *(no SHM)* — gRPC client + MQTT subscriber |
| **Dashboard** | *(no SHM)* | *(no SHM)* — gRPC + MQTT |
| **Diagnostic** | *(no SHM)* | *(no SHM)* — gRPC + MQTT |
| **Watchdog** (`evo`) | *(no SHM data segments)* | MAY read segment headers for hang detection |

---

*Generated from spec.md (1231 lines). All FR references point to the Functional Requirements section of the spec.*
