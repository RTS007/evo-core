# Feature Specification: RT System Integration — SHM P2P, Watchdog, HAL↔CU Cooperation

**Feature Branch**: `006-rt-shm-integration`  
**Created**: 2026-02-10  
**Status**: Draft  
**Input**: User description: "Watchdog uruchamia HAL i CU, wymiana danych SHM P2P, oczyszczenie kodu, usunięcie evo_shared_memory, pełna integracja io.toml, one source of truth, szkielet komunikacji dla wszystkich programów RT"

---

## 🏗️ Architecture Overview

This specification delivers the **foundational runtime integration** of the EVO system: a working pipeline where the watchdog (`evo`) spawns HAL and CU as child processes, both exchange real-time data through P2P shared memory segments, and all RT programs share a unified configuration and I/O model. The legacy `evo_shared_memory` crate is removed entirely.

### Key Architectural Decisions

1. **P2P SHM as the sole IPC mechanism for RT** — All real-time data flows through `evo_common`'s `TypedP2pWriter<T>` / `TypedP2pReader<T>`. No other SHM library exists in the workspace.
2. **One source of truth for configuration** — Machine parameters (axes, limits, kinematics) live in the machine config file. I/O definitions live in `io.toml`. No parameter is duplicated across HAL and CU configs.
3. **Unified RT bootstrap** — Every RT program (HAL, CU, and future modules) follows the same startup sequence: load shared config → build `IoRegistry` → create/attach P2P segments → enter RT loop.
4. **Watchdog as process supervisor** — `evo` spawns, monitors, and restarts HAL and CU with ordered startup (HAL first, then CU) and graceful shutdown propagation.
5. **Remove `evo_shared_memory`** — The entire crate is deleted from the workspace. Its useful P2P primitives migrate to `evo_common::shm::p2p`.

### System Topology

```
┌─────────────────────────────────────────────────────────────┐
│                     evo (Watchdog)                          │
│   - spawns HAL, then CU (ordered)                           │
│   - monitors child PIDs (waitpid / SIGCHLD)                 │
│   - restart with backoff on crash                           │
│   - graceful shutdown: SIGTERM → timeout → SIGKILL          │
│   - orphan SHM cleanup (shm_unlink)                         │
└──────────┬──────────────────────────────┬───────────────────┘
           │ spawn                        │ spawn
           ▼                              ▼
┌──────────────────────┐        ┌──────────────────────────┐
│     evo_hal          │        │   evo_control_unit       │
│                      │        │                          │
│  WRITES:             │        │  READS:                  │
│   evo_hal_cu         │◄──────►│   evo_hal_cu             │
│   evo_hal_re (plhld) │        │  WRITES:                 │
│  READS:              │        │   evo_cu_hal             │
│   evo_cu_hal         │◄──────►│   evo_cu_hal             │
│   evo_re_hal (skel)  │        │   evo_cu_mqt             │
│                      │        │   evo_cu_re (placeholder)│
│  loads: machine.toml │        │  evo_cu_rpc (placeholder)│
│  loads: io.toml      │        │  READS:                  │
│  builds: IoRegistry  │        │   evo_re_cu (optional)   │
│                      │        │   evo_rpc_cu (optional)  │
│                      │        │                          │
│                      │        │  loads: machine.toml     │
│                      │        │  loads: io.toml          │
│                      │        │  builds: IoRegistry      │
└──────────────────────┘        └──────────────────────────┘
           ▲                              ▲
           │ evo_re_hal (SHM P2P)         │ evo_re_cu (SHM P2P)
           │ evo_hal_re (SHM P2P)         │ evo_cu_re (SHM P2P)
           ▼                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  evo_recipe_executor                        │
│  WRITES: evo_re_cu, evo_re_hal, evo_re_mqt, evo_re_rpc    │
│  READS:  evo_cu_re, evo_hal_re                             │
│  Recipe logic: set DO, read DI, check positions, motion    │
└─────────────────────────────────────────────────────────────┘
```

### P2P Connection Architecture (Complete)

Every SHM connection in the EVO system is listed below. The P2P protocol enforces exactly **one writer** and **one reader** per segment. Segments are grouped by readiness level.

#### Active Connections (fully implemented in this spec)****

| # | Segment Name  | Writer | Reader | Payload Struct | Content |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 1 | `evo_hal_cu`  | HAL    | CU     | `HalToCuSegment` | Axis feedback (pos, vel, torque), DI bank, AI values, per-axis flags |
| 2 | `evo_cu_hal`  | CU     | HAL    | `CuToHalSegment` | `ControlOutputVector` per axis, DO bank, AO values, enable commands |

#### Skeleton Connections (types defined + stub init code in this spec)

| # | Segment Name  | Writer | Reader | Payload Struct | Content |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 3 | `evo_cu_mqt`  | CU     | MQTT   | `CuToMqtSegment` | Status snapshot: machine state, axis states, errors, safety state |
| 4 | `evo_hal_mqt` | HAL    | MQTT   | `HalToMqtSegment` | Raw HAL data stream: all I/O states, axis positions, driver state — continuous telemetry for service/diagnostics/oscilloscope |
| 5 | `evo_re_cu`   | RE     | CU     | `ReToCuSegment` | Motion requests, program commands, `AllowManualMode` |
| 6 | `evo_re_hal`  | RE     | HAL    | `ReToHalSegment` | Direct I/O commands from RE: set DO, read DI, position checks — fast path for recipe logic without CU intermediary |
| 7 | `evo_re_mqt`  | RE     | MQTT   | `ReToMqtSegment` | Recipe execution telemetry: current step, program name, cycle count, RE state — continuous stream for service/dashboard |
| 8 | `evo_re_rpc`  | RE     | gRPC   | `ReToRpcSegment` | Recipe status/acks for Dashboard/API: execution progress, step result, error feedback |
| 9 | `evo_rpc_cu`  | gRPC   | CU     | `RpcToCuSegment` | External commands: jog, mode change, config reload, service bypass |
| 10 | `evo_rpc_hal` | gRPC   | HAL    | `RpcToHalSegment` | Direct HAL commands: set DO, set AO, driver config — action requests with ack expected via `evo_hal_rpc` |
| 11 | `evo_rpc_re` | gRPC   | RE     | `RpcToReSegment` | Placeholder for gRPC→RE commands; payload defined in a separate spec |

#### Placeholder Connections (types defined only — no init code yet)

| # | Segment Name  | Writer | Reader | Payload Struct | Content |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 12 | `evo_cu_re`   | CU     | RE     | `CuToReSegment` | Ack, execution status, axis availability, error feedback |
| 13 | `evo_cu_rpc`  | CU     | gRPC   | `CuToRpcSegment` | Full diagnostic snapshot for Dashboard/API (all axis states, all I/O, cycle timing) |
| 14 | `evo_hal_rpc` | HAL    | gRPC   | `HalToRpcSegment` | HAL action responses/acks: DO set confirmation, driver state, error feedback |
| 15 | `evo_hal_re`  | HAL    | RE     | `HalToReSegment` | HAL feedback to RE: current I/O states (DI/DO), axis positions, velocities, drive status — fast read path for recipe logic |

#### Connections that do NOT exist (with rationale)

| Pair | Why not? |
| :--- | :--- |
| MQTT → CU | MQTT is output-only (telemetry publishing to cloud/SCADA). All inbound commands enter through gRPC/API (`evo_rpc_cu`). |
| MQTT → HAL | MQTT is output-only. HAL writes to MQTT (`evo_hal_mqt`); MQTT never writes back to HAL. Inbound commands to HAL enter through gRPC (`evo_rpc_hal`). |
| Dashboard ↔ any (SHM) | Dashboard communicates exclusively via gRPC and MQTT — operator commands (jog, start, stop) through gRPC, status/telemetry visualization through MQTT. SHM is for deterministic RT data paths only. In the future, Dashboard will combine both protocols: gRPC for action-reaction (command + ack) and MQTT for fastest possible data visualization. |
| Diagnostic ↔ any (SHM) | Diagnostic has no SHM segments. It consumes telemetry from all RT programs via MQTT (`evo_cu_mqt`, `evo_hal_mqt`, `evo_re_mqt`) and uses gRPC for interactive queries (`evo_cu_rpc`, `evo_hal_rpc`, `evo_re_rpc`, `evo_rpc_re`). Same pattern as Dashboard. |
| Watchdog ↔ any (SHM) | Watchdog uses OS-level process management (`waitpid`, `SIGCHLD`). It MAY read P2P segment headers (heartbeat counter) for advanced hang detection, but does **not** own or participate in any data segment. |
| API ↔ any (SHM) | `evo_api` is the non-RT↔WWW HTTP/REST gateway. It has no SHM segments. It connects to RT programs indirectly through `evo_grpc` (gRPC client) and receives telemetry through `evo_mqtt` (MQTT subscriber). |

#### Communication Protocol Design Principles

| Protocol | Direction | Purpose | RT Programs |
| :--- | :--- | :--- | :--- |
| **SHM P2P** | RT ↔ RT | Deterministic real-time data exchange (≤ 5µs) | HAL ↔ CU, RE ↔ HAL, RE ↔ CU, CU → MQTT, HAL → MQTT, gRPC ↔ CU, gRPC ↔ HAL, gRPC ↔ RE |
| **MQTT** | RT → non-RT | Continuous telemetry stream for visualization, logging, oscilloscope, service diagnostics | HAL → any subscriber, CU → any subscriber, RE → any subscriber |
| **gRPC** | non-RT ↔ RT bridge | Action-reaction pattern via `evo_grpc` (SHM↔gRPC bridge). Send command, receive ack/response. | evo_api/Dashboard/Diagnostic ↔ evo_grpc ↔ CU/HAL/RE |
| **HTTP/REST** | non-RT ↔ WWW | External API for web clients, mobile apps, SCADA. `evo_api` exposes REST endpoints. | evo_api ↔ evo_grpc (gRPC client), evo_api ↔ evo_mqtt (subscriber) |

> **Key insight**: Both MQTT and gRPC (`evo_grpc`) MUST have SHM connections to **every RT program** (HAL, CU, RE). Each RT module writes exactly 4 outbound segments: one to each of the other 2 RT modules + one to MQTT + one to gRPC. MQTT provides the fastest possible data stream for visualization. `evo_grpc` provides the action-reaction feedback loop (SHM↔gRPC bridge). `evo_api` is a separate program (non-RT↔WWW) that connects to `evo_grpc` via gRPC protocol and to `evo_mqtt` as a subscriber — it has no SHM segments. Dashboard and Diagnostic leverage both gRPC and MQTT — they have no SHM segments.

## Clarifications

### Session 2026-02-10

- Q: What should `RpcToReSegment` contain? → A: Placeholder only; its content is defined in a separate spec.
- Q: How is I/O control ownership resolved between CU, RE, and HAL? → A: Role-assigned pins are controlled via CU only; RE may read any state, and RE→HAL direct commands are allowed only for pins without an `IoRole` assignment.
- Q: What happens if RE sends a command for a role-assigned pin? → A: HAL ignores it and logs `ERR_IO_ROLE_OWNED`.
- Q: What about pins without `IoRole` and CU control? → A: CU does not control pins without `IoRole` (CU only operates on role-assigned pins).
- Q: Does CU read pins without `IoRole`? → A: No; CU reads only role-assigned pins via `IoRegistry`.
- Q: What error code should be used when RE tries to control a role-assigned pin? → A: Use the existing error schema already defined for similar cases.
- Q: Are RE payload details designed in this spec? → A: No; RE payload details are out of scope for this spec.
- Q: What are MAX_AI and MAX_AO values? → A: MAX_AI = 1024, MAX_AO = 1024 (same as MAX_DI/MAX_DO). Already defined in hal/consts.rs, must move to evo_common::consts for global access.
- Q: Is evo_grpc the same as evo_api? → A: No. evo_grpc handles RT ↔ non-RT (SHM P2P bridge, has SHM segments). evo_api handles non-RT ↔ WWW (HTTP/REST external gateway, no SHM). Both are separate programs.
- Q: What does watchdog "degraded state" mean after max restarts? → A: Watchdog stays alive, stops restarting the failed child, logs a single Critical error (no periodic messages). Operator or external system must intervene.
- Q: What POSIX file mode should shm_open use? → A: 0o600 (owner read/write only). All EVO processes run as the same user.
- Q: Where are watchdog and other program-specific parameters configured? → A: Separate config.toml for system/program configuration. Contains sections for all programs (watchdog, hal, cu, re, mqtt, grpc, api, dashboard). machine.toml = machine-specific params; config.toml = EVO system params independent of which machine is controlled.

---

### Per-Axis Configuration Architecture

Each axis has a dedicated configuration file. This eliminates the monolithic `[[axes]]` array and makes parameters easy to find and modify per axis. All configuration files live in a single **flat directory** — no subdirectories. This makes it easy to work with from console or touchscreen.

#### Directory Structure

```
config/
├── config.toml                   # System: program params (watchdog, hal, cu, re, mqtt, grpc, api, dashboard)
├── machine.toml                  # Machine: machine name, safety, service (machine-specific)
├── io.toml                       # All I/O definitions (roles, pins, logic, scaling)
├── axis_01_x.toml                # Axis 1: X-Axis (linear)
├── axis_02_y.toml                # Axis 2: Y-Axis (linear)
├── axis_03_z.toml                # Axis 3: Z-Axis (linear, tailstock, guard)
├── axis_04_a.toml                # Axis 4: A-Axis (rotary)
├── axis_05_b.toml                # Axis 5: B-Axis (rotary)
├── axis_06_c.toml                # Axis 6: C-Axis (rotary)
├── axis_07_spindle.toml          # Axis 7: Spindle (high-speed rotary)
└── axis_08_tailstock.toml        # Axis 8: Tailstock (coupled to Z)
```

#### machine.toml (global only — no axis parameters, no axis file list)

```toml
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

Axis files are **not listed** in `machine.toml`. They are auto-discovered from the same directory (see FR-055).

#### config.toml (system/program configuration — machine-independent)

```toml
[watchdog]
max_restarts = 5
initial_backoff_ms = 100
max_backoff_s = 30
stable_run_s = 60
sigterm_timeout_s = 2.0
hal_ready_timeout_s = 5.0

[hal]
# cycle_time_us = 1000  # Future

[cu]
# cycle_time_us = 1000  # Future

[re]
# placeholder

[mqtt]
# placeholder

[grpc]
# placeholder

[api]
# placeholder

[dashboard]
# placeholder

[diagnostic]
# placeholder
```

`config.toml` contains parameters for starting and running the EVO system, independent of which physical machine is controlled. Machine-specific parameters (axes, kinematics, safety) live in `machine.toml` and axis files.

#### Self-Documenting TOML Header Rule

**Every TOML configuration file** (`config.toml`, `machine.toml`, `io.toml`, and every `axis_NN_*.toml`) MUST include a header comment block at the top of the file documenting all available parameters: their names, types, default values, whether they are required or optional, and valid value ranges/enums. This makes every config file self-documenting for service engineers working from console or touchscreen — no external documentation lookup is needed.

#### Per-axis file (e.g., axis_01_x.toml)

```toml
# ╔════════════════════════════════════════════════════════════════════════════╗
# ║  AXIS CONFIGURATION                                                       ║
# ╚════════════════════════════════════════════════════════════════════════════╝
#
# ┌─── FILE NAMING ─────────────────────────────────────────────────────────────┐
# │  Pattern: axis_NN_name.toml                                                │
# │  NN = axis number (01-64), used for sort order and uniqueness              │
# │  name = free-form human-readable label (x, spindle, tailstock, konik, etc.)│
# │  The name part after NN has no functional meaning — only NN matters.       │
# │  Examples: axis_01_x.toml, axis_08_tailstock.toml, axis_08_konik.toml      │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [axis] — Identity (REQUIRED) ───────────────────────────────────────────┐
# │  id               Axis number, MUST match NN in filename        (REQUIRED) │
# │  name             Human-readable name                           (REQUIRED) │
# │  type             "linear" | "rotary"                           (REQUIRED) │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [kinematics] — Motion limits (REQUIRED) ────────────────────────────────┐
# │  max_velocity              Max axis speed [units/s]             (REQUIRED) │
# │  max_acceleration          Max acceleration [units/s²]          (optional) │
# │  safe_reduced_speed_limit  Speed limit in safe mode [units/s]   (REQUIRED) │
# │  min_pos                   Software lower limit [units]         (REQUIRED) │
# │  max_pos                   Software upper limit [units]         (REQUIRED) │
# │  in_position_window        Position settled tolerance [units]    (REQUIRED) │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [control] — PID / DOB / Filter parameters (REQUIRED) ──────────────────┐
# │  kp                Proportional gain                            (REQUIRED) │
# │  ki                Integral gain                                (REQUIRED) │
# │  kd                Derivative gain                              (REQUIRED) │
# │  tf                Derivative filter time constant [s]       (def: 0.001) │
# │  tt                Anti-windup tracking time [s]             (def: 0.01)  │
# │  kvff              Velocity feedforward gain                 (def: 0.0)   │
# │  kaff              Acceleration feedforward gain             (def: 0.0)   │
# │  friction          Static friction compensation              (def: 0.0)   │
# │  jn                Nominal inertia for DOB                   (def: 0.01)  │
# │  bn                Nominal damping for DOB                   (def: 0.001) │
# │  gdob              DOB filter bandwidth [rad/s]              (def: 200.0) │
# │  f_notch           Notch filter center frequency [Hz]        (def: 0, off)│
# │  bw_notch          Notch filter bandwidth [Hz]               (def: 0, off)│
# │  flp               Low-pass filter cutoff [Hz]               (def: 0, off)│
# │  out_max           Output saturation limit [%]               (def: 100.0) │
# │  lag_error_limit   Following error limit [units]                (REQUIRED) │
# │  lag_policy        "Unwanted" | "Warning" | "Error"          (def: Error) │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [safe_stop] — Safety stop parameters (REQUIRED) ────────────────────────┐
# │  category          "SS1" | "SS2" | "STO"                       (REQUIRED) │
# │  max_decel_safe    Max deceleration for safe stop [units/s²]   (REQUIRED) │
# │  sto_brake_delay   Delay before STO brake engages [s]       (def: 0.1)   │
# │  ss2_holding_torque Holding torque for SS2 [%]               (def: 0.0)   │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [homing] — Homing procedure (REQUIRED) ─────────────────────────────────┐
# │  method            "HomeSensor" | "TorqueLimit" | "IndexPulse"  (REQUIRED) │
# │  speed             Homing speed [units/s]                      (REQUIRED) │
# │  torque_limit      Torque limit during homing [%]           (def: 30.0)  │
# │  timeout           Homing timeout [s]                       (def: 30.0)  │
# │  approach_direction "Positive" | "Negative"                 (def: Positive)│
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [brake] — Mechanical brake (OPTIONAL) ──────────────────────────────────┐
# │  do_brake          IoRole for brake output                     (REQUIRED) │
# │  di_released       IoRole for brake-released feedback          (REQUIRED) │
# │  release_timeout   Time to wait for brake release [s]       (def: 2.0)   │
# │  engage_timeout    Time to wait for brake engage [s]        (def: 1.0)   │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [tailstock] — Tailstock parameters (OPTIONAL) ──────────────────────────┐
# │  coupled_axis      Axis ID this tailstock is coupled to        (REQUIRED) │
# │  clamp_role        IoRole for tailstock clamp output           (REQUIRED) │
# │  clamped_role      IoRole for tailstock clamped feedback        (optional) │
# │  max_force         Maximum tailstock force [N]                 (optional) │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [guard] — Safety guard (OPTIONAL) ──────────────────────────────────────┐
# │  di_guard          IoRole for guard closed sensor              (REQUIRED) │
# │  stop_on_open      Stop category when guard opens           (def: "SS1") │
# └────────────────────────────────────────────────────────────────────────────┘
#
# ┌─── [coupling] — Axis coupling (OPTIONAL) ──────────────────────────────────┐
# │  master_axis       Axis ID of the master                       (REQUIRED) │
# │  ratio             Gear ratio (slave/master)                (def: 1.0)   │
# │  max_sync_error    Max allowed sync error [units]              (REQUIRED) │
# └────────────────────────────────────────────────────────────────────────────┘

[axis]
id = 1
name = "X-Axis"
type = "linear"                   # linear | rotary

[kinematics]
max_velocity = 500.0
max_acceleration = 5000.0         # optional, driver-specific
safe_reduced_speed_limit = 50.0
min_pos = -100.0
max_pos = 1000.0
in_position_window = 0.05

[control]
kp = 100.0
ki = 20.0
kd = 0.5
tf = 0.001
tt = 0.01
kvff = 0.8
kaff = 0.001
friction = 0.5
jn = 0.01
bn = 0.001
gdob = 200.0
f_notch = 800.0
bw_notch = 50.0
flp = 500.0
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0
sto_brake_delay = 0.1
ss2_holding_torque = 20.0

[homing]
method = "HomeSensor"
speed = 20.0
torque_limit = 30.0
timeout = 30.0
approach_direction = "Positive"

[brake]
do_brake = "BrakeOut1"            # IoRole reference → resolved via io.toml
di_released = "BrakeIn1"
release_timeout = 2.0
engage_timeout = 1.0

# Optional sections — only present when the axis has them:
# [tailstock]
# [guard]
# [coupling]
```

#### Design Principles

- **Flat directory**: All config files (`config.toml`, `machine.toml`, `io.toml`, `axis_NN_*.toml`) live in a single directory. No subdirectories. This simplifies access for service engineers using console or touchscreen.
- **Auto-discovery**: `ConfigLoader` scans the config directory for files matching `axis_*_*.toml` glob pattern. No explicit axis file list in `machine.toml`. Axis count is determined by the number of unique axis files found.
- **Naming convention**: `axis_NN_name.toml` — NN is the axis number (01–64) used for identity and sort order. The name part after NN is free-form and has no functional meaning (e.g., `axis_08_tailstock.toml`, `axis_08_konik.toml`, `axis_08_tail.toml` are all valid for axis 8).
- **Duplicate detection**: `ConfigLoader` MUST validate that no two axis files have the same NN number. If `axis_08_tailstock.toml` and `axis_08_konik.toml` both exist, startup fails with `ConfigError::DuplicateAxisId(8)`.
- **No duplication**: Axis kinematics live in axis file, I/O pin assignments live in `io.toml`, global safety lives in `machine.toml`. Cross-references use `IoRole` strings (e.g., `"BrakeOut1"`) resolved at runtime via `IoRegistry`.
- **Per-axis peripherals**: Brake, tailstock, guard, coupling, locking pin — defined in the axis file that owns them.
- **Both HAL and CU load all axis files**: HAL uses kinematics for simulation limits; CU uses control parameters for PID and safety for monitoring. Same file, same values.
- **Self-documenting**: Every TOML configuration file (`config.toml`, `machine.toml`, `io.toml`, `axis_NN_*.toml`) includes a comprehensive header comment block describing all available parameters, their types, defaults, and requirements. No external documentation is needed to understand or edit any config file.
- **Breaking changes**: `ConfigLoader` MUST support only the new per-axis file format. The legacy `[[axes]]` array format is an unknown/rejected parameter. There is no backward compatibility layer.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Watchdog Starts HAL and CU, End-to-End Data Flow (Priority: P1)

As a system integrator, I want to run a single `evo` binary that automatically starts HAL and CU in the correct order, so that both processes exchange data through SHM and I can observe axis feedback flowing from HAL to CU and control commands flowing from CU to HAL within the first second of operation.

**Why this priority**: This is the absolute prerequisite for any further development. Without a working watchdog→HAL→CU→SHM pipeline, no other feature (safety, motion, recipes) can be demonstrated or tested. This unblocks all subsequent work.

**Independent Test**: Run `evo --config-dir config/`. Verify HAL starts first, creates `evo_hal_cu` segment, then CU starts and attaches. Check that `evo_hal_cu` heartbeat increments every cycle. Check that `evo_cu_hal` heartbeat increments every cycle. Kill HAL with SIGKILL, verify watchdog detects death and restarts it within configured timeout.

**Acceptance Scenarios**:

1. **Given** the `evo` binary is started with a valid machine config, **When** initialization completes, **Then** HAL is spawned as a child process first, and CU is spawned only after HAL's `evo_hal_cu` segment exists and heartbeat > 0.
2. **Given** HAL and CU are both running, **When** HAL writes axis positions to `evo_hal_cu`, **Then** CU reads those positions in the same or next cycle and they match the values HAL wrote.
3. **Given** HAL and CU are both running, **When** CU writes `ControlOutputVector` to `evo_cu_hal`, **Then** HAL reads those commands in the same or next cycle and applies them to the simulation driver.
4. **Given** HAL crashes (process exits unexpectedly), **When** watchdog detects the death via `waitpid`, **Then** watchdog sends SIGTERM to CU (dependent process), cleans up orphan SHM segments, and restarts HAL followed by CU with exponential backoff.
5. **Given** the system is running, **When** watchdog receives SIGTERM, **Then** it sends SIGTERM to CU first, waits up to 2 seconds, sends SIGTERM to HAL, waits up to 2 seconds, sends SIGKILL to any remaining children, cleans up all `evo_*` SHM segments, and exits cleanly.

---

### User Story 2 — Unified Configuration: One Source of Truth (Priority: P1)

As a developer, I want HAL and CU to load the same machine config file and the same `io.toml` file, building identical views of axes and I/O, so that there are zero duplicated parameters and any change in one config file is reflected consistently in both programs.

**Why this priority**: Without unified configuration, HAL and CU will inevitably drift apart, causing silent data corruption or startup failures. This is equally critical as the data pipeline because it ensures correctness of the data flowing through it.

**Independent Test**: Define a machine with 3 axes and 10 I/O points. Start HAL and CU. Verify both programs agree on axis count, axis IDs, I/O pin assignments, NC/NO logic, and analog scaling. Change a parameter (e.g., `max_velocity` of axis 2), restart, verify both programs see the new value. Remove a required IoRole from `io.toml`, verify both programs refuse to start with `ERR_IO_ROLE_MISSING`.

**Acceptance Scenarios**:

1. **Given** a machine config with 8 axes and an `io.toml` with roles for all safety peripherals, **When** HAL starts, **Then** it parses the machine config for axis parameters and `io.toml` for I/O mappings, building an `IoRegistry` that resolves roles to pins.
2. **Given** the same config files, **When** CU starts, **Then** it parses the same machine config for axis control parameters (PID gains, lag limits, safety stops) and the same `io.toml`, building an identical `IoRegistry`.
3. **Given** `io.toml` is missing `BrakeIn3` role but axis 3 has a brake configured, **When** either HAL or CU starts, **Then** startup fails with `ERR_IO_ROLE_MISSING` listing `BrakeIn3`.
4. **Given** axis 2 has `max_velocity = 500.0` in machine config, **When** both HAL and CU read this value, **Then** both use 500.0 — HAL for simulation limits, CU for overspeed monitoring — from the same `evo_common` config struct, not from separate definitions.
5. **Given** a parameter like `in_position_window` is defined per axis in the machine config, **When** CU evaluates soft limits using `in_position_window` as tolerance (FR-111 from spec 005), **Then** it reads the same value that HAL uses for position settling, without any duplication.

---

### User Story 3 — P2P SHM Library in evo_common (Priority: P1)

As a developer of any EVO module, I want a single, clean P2P SHM API in `evo_common` that provides `TypedP2pWriter<T>` and `TypedP2pReader<T>` with heartbeat, struct version hash, and destination enforcement, so that I can add SHM communication to any program with 5 lines of code and zero RT-safety concerns.

**Why this priority**: This is the transport layer that every other feature depends on. Without a correct, RT-safe P2P library, no data flows. It must be in `evo_common` (not a separate crate) to be the single shared dependency.

**Independent Test**: Write a standalone integration test: process A creates `TypedP2pWriter::<TestStruct>::create("evo_test_seg")`, writes data, process B attaches with `TypedP2pReader::<TestStruct>::attach("evo_test_seg")`, reads data. Verify data matches. Verify heartbeat increments. Verify version hash mismatch is caught. Verify second reader is rejected. Verify stale detection works after writer stops.

**Acceptance Scenarios**:

1. **Given** a writer creates segment `evo_hal_cu` with `TypedP2pWriter::<HalToCuSegment>::create(...)`, **When** a reader attaches with `TypedP2pReader::<HalToCuSegment>::attach(...)`, **Then** reader validates magic (`b"EVO_P2P\0"`), destination module, and struct version hash before allowing reads.
2. **Given** a writer is writing at 1kHz, **When** reader reads, **Then** read latency is ≤ 2µs and write latency is ≤ 5µs (no mutex, no heap allocation, no syscalls in the hot path).
3. **Given** writer process crashes, **When** reader detects heartbeat unchanged for N consecutive reads (default N=3), **Then** reader returns `ShmError::HeartbeatStale` and the segment is considered dead.
4. **Given** a reader tries to attach to `evo_hal_cu` with module abbreviation `re` (not `cu`), **When** destination validation runs, **Then** attach fails with `ShmError::DestinationMismatch`.
5. **Given** writer binary is compiled with `HalToCuSegment` v1 (size=2048B) and reader with v2 (size=2080B), **When** reader attaches, **Then** attach fails with `ShmError::VersionMismatch`.

---

### User Story 4 — Remove evo_shared_memory Crate (Priority: P1)

As a maintainer, I want the `evo_shared_memory` crate completely removed from the workspace so that there is exactly one SHM implementation (P2P in `evo_common`), no dual protocol confusion, and no RT-unsafe code paths (global mutex, heap allocation, `SystemTime::now()` in hot path).

**Why this priority**: The audit (audit.md §4, §11) identified `evo_shared_memory` as the root cause of protocol duplication, RT-safety violations, and dead code. Removing it eliminates 7 RT-safety issues, ~20 dead symbols, and the entire broadcast/P2P protocol conflict.

**Independent Test**: After removal, `cargo build --workspace` succeeds with zero references to `evo_shared_memory`. `grep -r "evo_shared_memory" --include="*.toml" --include="*.rs"` returns zero matches. No files exist under `evo_shared_memory/`.

**Acceptance Scenarios**:

1. **Given** `evo_shared_memory` is listed as a workspace member, **When** the migration is complete, **Then** it is removed from `Cargo.toml` workspace members and its directory is deleted.
2. **Given** `evo`, `evo_hal`, `evo_control_unit`, `evo_grpc`, `evo_recipe_executor` depend on `evo_shared_memory`, **When** migration is complete, **Then** all dependencies are replaced with `evo_common`'s P2P API.
3. **Given** `evo_shared_memory` has tests, benches, and examples, **When** the crate is removed, **Then** equivalent P2P tests exist in `evo_common` covering: create/attach, read/write, heartbeat, version hash, destination enforcement, single-reader enforcement, stale detection.

---

### User Story 5 — HAL Writes Feedback to SHM, Reads Commands from SHM (Priority: P1)

As a control engineer, I want HAL's RT loop to write the full `HalToCuSegment` (axis positions, velocities, DI bank, AI values, referenced flags) to SHM after every driver cycle, and read `CuToHalSegment` (control output vectors, DO bank, AO values) from SHM before every driver cycle, so that simulation data is visible to CU and CU's commands reach the simulated drives.

**Why this priority**: HAL is the data producer for the entire system. Without HAL writing to SHM, CU has no sensor data. Without HAL reading from SHM, CU's commands are discarded. This closes audit blocker 1.1.

**Independent Test**: Start HAL in simulation mode. Attach an external reader to `evo_hal_cu`. Verify axis positions update every cycle. Write a known `ControlOutputVector` to `evo_cu_hal`. Verify HAL's simulation driver receives the velocity/torque commands.

**Acceptance Scenarios**:

1. **Given** HAL simulation driver produces `HalStatus` with axis positions and I/O values, **When** the RT loop completes a cycle, **Then** HAL converts `HalStatus` → `HalToCuSegment` and writes it via `TypedP2pWriter`.
2. **Given** CU has written commands to `evo_cu_hal`, **When** HAL reads the segment at cycle start, **Then** HAL converts `CuToHalSegment` → `HalCommands` and passes them to `driver.cycle()`.
3. **Given** `evo_cu_hal` segment does not exist yet (CU not started), **When** HAL tries to read, **Then** HAL operates with default (zero) commands — no crash, no error, just safe defaults.
4. **Given** `io.toml` defines DI pin 1 as `EStop` with `logic="NC"` and `sim=true`, **When** HAL reads simulation I/O, **Then** DI bit for pin 1 in `HalToCuSegment.di_bank` reflects the NC-inverted simulation value.

---

### User Story 6 — CU Binary Runs the RT Loop (Priority: P1)

As a control engineer, I want the CU binary (`evo_control_unit`) to actually instantiate `CycleRunner` and enter the RT loop at startup, reading from `evo_hal_cu` and writing to `evo_cu_hal` every cycle, so that the control pipeline is operational and not just library code.

**Why this priority**: Audit blocker 1.2 — CU binary currently prints "Config OK" and exits. All RT infrastructure exists in library code but is unreachable. This connects the binary to the engine.

**Independent Test**: Start CU binary with `--config machine.toml`. Verify process enters `CycleRunner::run()`. Verify `evo_cu_hal` segment is created with incrementing heartbeat. Verify `evo_cu_mqt` segment is created with live status. Kill the process, verify clean shutdown (segment cleanup).

**Acceptance Scenarios**:

1. **Given** CU binary starts with valid config and `evo_hal_cu` exists, **When** initialization completes, **Then** `CycleRunner` is instantiated with loaded config, SHM segments are created/attached, and the RT loop begins.
2. **Given** the RT loop is running, **When** each cycle executes, **Then** CU reads `evo_hal_cu` (axis feedback + DI bank + AI values), processes state machines, writes `evo_cu_hal` (control outputs) and `evo_cu_mqt` (status snapshot).
3. **Given** `evo_hal_cu` does not exist at CU startup, **When** CU tries to attach, **Then** CU waits (with timeout) for the segment to appear — it does not crash or exit immediately.

---

### User Story 7 — Unified SHM Data Types (Priority: P2)

As a developer, I want a single set of SHM segment structs (`HalToCuSegment`, `CuToHalSegment`, `CuToMqtSegment`) defined in `evo_common` with matching field types and sizes, so that HAL and CU always agree on data layout and there are no type conversion issues.

**Why this priority**: Audit issue 1.3 — currently HAL and CU use incompatible SHM formats (different magic, different header sizes, different field types). Unification eliminates silent data corruption.

**Independent Test**: Compile HAL and CU against the same `evo_common` version. Verify `size_of::<HalToCuSegment>()` and `align_of::<HalToCuSegment>()` produce identical values in both binaries. Verify `struct_version_hash::<HalToCuSegment>()` matches.

**Acceptance Scenarios**:

1. **Given** `HalToCuSegment` is defined in `evo_common::shm::segments`, **When** both HAL and CU import it, **Then** the struct has identical layout, size, and alignment in both binaries.
2. **Given** DI values are represented as `[u64; 16]` (word-packed, 1024 bits), **When** HAL packs DI values and CU unpacks them, **Then** both use the same bit-packing convention (bit N of word W = pin N*64+W) via shared helper functions in `evo_common`.
3. **Given** analog values are represented as `[f64; 64]` in engineering units, **When** HAL writes a scaled pressure value (6.0 bar), **Then** CU reads 6.0 bar — no additional scaling needed.
4. **Given** per-axis feedback contains `position: f64`, `velocity: f64`, `torque_estimate: f64`, `drive_ready: bool`, `drive_fault: bool`, `referenced: bool`, `active: bool`, **When** HAL populates these from the simulation driver, **Then** CU reads them directly without any struct conversion layer.

---

### User Story 8 — Skeleton P2P Contracts for All Programs (Priority: P2)

As a developer, I want every EVO program (evo_grpc, evo_recipe_executor, evo_mqtt) to have a defined P2P segment contract (segment name, struct type, reader/writer role) even if the program is currently a stub, so that future integration requires only filling in application logic without re-architecting the SHM layer.

**Why this priority**: Audit issue 9.9 — current stubs have no P2P contracts defined. Without contracts, each future integration will force an SHM rearchitecture. Defining contracts now (even for empty programs) ensures consistency.

**Independent Test**: For each stub program, verify that `evo_common` defines the segment struct types. Verify that the stub's `main.rs` contains the P2P reader/writer initialization code (even if the processing loop is a placeholder). Verify that segment names follow the `evo_[SRC]_[DST]` convention and are registered in the module abbreviation registry.

**Acceptance Scenarios**:

1. **Given** `evo_mqtt` needs to read `evo_cu_mqt`, `evo_hal_mqt`, and `evo_re_mqt`, **When** the stub initializes, **Then** it attaches readers for all three segments — even if the read loop just prints data.
2. **Given** `evo_recipe_executor` writes 4 outbound segments (`evo_re_cu`, `evo_re_hal`, `evo_re_mqt`, `evo_re_rpc`) and reads 3 inbound (`evo_cu_re`, `evo_hal_re`, `evo_rpc_re`), **When** the stub initializes, **Then** it creates all writers and readers with correct module abbreviations.
3. **Given** `evo_grpc` needs bidirectional P2P with CU, HAL, and RE, **When** the stub initializes, **Then** it creates writers for `evo_rpc_cu`, `evo_rpc_hal`, and `evo_rpc_re`, and attaches readers for `evo_cu_rpc`, `evo_hal_rpc`, and `evo_re_rpc` — enabling the action-reaction pattern (send command, receive ack).
4. **Given** all segment struct types are defined in `evo_common::shm::segments`, **When** any program imports them, **Then** they compile and match the version hash of their counterparts.
5. **Given** `evo_diagnostic` and `evo_dashboard` have no SHM segments, **When** they need RT data, **Then** they consume it via MQTT (telemetry) and gRPC (interactive queries).

---

### User Story 9 — Dependency Cleanup and Workspace Hygiene (Priority: P2)

As a maintainer, I want all dependency conflicts resolved (`heapless` 0.8→0.9, `nix` 0.29→0.30), unused dependencies removed (`parking_lot`, `bitflags`, `static_assertions`, unused `tokio`), workspace dependencies centralized in root `Cargo.toml`, and dead code files deleted, so that `cargo build --workspace` produces zero warnings about unused imports and the dependency tree is clean.

**Why this priority**: Audit §6 identifies 7 dependency issues causing dual linkage and version drift. Clean dependencies reduce compile time, binary size, and confusion for new developers.

**Independent Test**: `cargo build --workspace 2>&1 | grep -c "warning"` = 0 for dependency-related warnings. `cargo tree -d` shows no duplicate crate versions for workspace dependencies. Unused files (`main_old.rs`, empty `shm/config.rs`) do not exist.

**Acceptance Scenarios**:

1. **Given** `evo_control_unit` uses `heapless = "0.8"` and `evo_common` uses `heapless = "0.9.2"`, **When** migration is complete, **Then** both use `heapless = "0.9"` from `[workspace.dependencies]`.
2. **Given** `evo` (watchdog) depends on `parking_lot` and `tokio` but uses neither, **When** cleanup is complete, **Then** both are removed from its `Cargo.toml`.
3. **Given** `evo_hal/src/main_old.rs` is dead code not referenced by any module, **When** cleanup is complete, **Then** the file is deleted.
4. **Given** no `[workspace.dependencies]` exists, **When** migration is complete, **Then** shared dependencies (`serde`, `tracing`, `heapless`, `nix`, `toml`) are centralized in the root `Cargo.toml`.

---

### User Story 10 — Deduplicated Constants and Types (Priority: P3)

As a developer, I want a single definition for each shared constant (`MAX_AXES`, `CYCLE_TIME_US`) and each shared type (`AnalogCurve`), with all other crates importing from `evo_common`, so that changes propagate automatically and there are no stale copies.

**Why this priority**: Audit issues 2.2 (dual `AnalogCurve`), 2.3 (triple `MAX_AXES`). Deduplication prevents silent behavioral differences.

**Independent Test**: `grep -rn "MAX_AXES" --include="*.rs"` returns exactly one definition (in `evo_common`) and only `use` imports elsewhere. `grep -rn "AnalogCurve" --include="*.rs"` returns exactly one `struct`/`enum` definition.

**Acceptance Scenarios**:

1. **Given** `MAX_AXES` is defined in 3 places with 2 different types, **When** deduplication is complete, **Then** one `pub const MAX_AXES: usize = 64` exists in `evo_common::consts` and all other crates import it.
2. **Given** `AnalogCurve` exists as a struct in `hal/config.rs` and an enum in `io/config.rs`, **When** unification is complete, **Then** one `AnalogCurve` type exists (matching the `io.toml` spec: preset string or `[a, b, c]` polynomial + offset), and both HAL and CU use it.
3. **Given** `EVO_SHM_MAGIC` exists in `evo_common::shm::consts`, **When** cleanup is complete, **Then** it is removed entirely — only `P2P_SHM_MAGIC` exists.

---

### Edge Cases

- What happens when HAL crashes during CU's read phase? → CU detects stale heartbeat on `evo_hal_cu` within N+1 cycles → `ERR_HAL_COMMUNICATION` → `SAFETY_STOP`. Watchdog independently detects HAL death via `waitpid` and restarts it.
- What happens when CU crashes during HAL's read phase? → HAL detects stale heartbeat on `evo_cu_hal` (if attached), falls back to default zero commands. Watchdog restarts CU. HAL continues simulation with safe defaults.
- What if watchdog crashes? → HAL and CU continue running as orphan processes. They remain functional until externally killed. A service manager (systemd) should supervise the watchdog itself.
- What if both HAL and CU crash simultaneously? → Watchdog detects both via `waitpid`, cleans up all SHM segments (`shm_unlink`), restarts HAL first, then CU, with backoff.
- What if `io.toml` has a role assigned to wrong I/O type (DI role on AI pin)? → Startup validation fails with `ERR_IO_ROLE_TYPE_MISMATCH` in both HAL and CU.
- What if machine config has more axes than `MAX_AXES`? → Config validation fails with `ConfigError::ValidationError` at load time.
- What if SHM segment file is left over from a previous crash? → Writer uses `O_CREAT` (without `O_EXCL`) to overwrite stale segments. Watchdog also proactively `shm_unlink`s orphan segments matching `evo_*` pattern on startup.
- What if HAL starts before watchdog (manual debug run)? → CU can still attach to `evo_hal_cu` independently. Watchdog is optional for development — programs can run standalone.
- What if config file is missing during hot-reload attempt? → `ConfigError::FileNotFound`, reload rejected, existing config preserved (FR-146 from spec 005).
- What if a P2P segment write takes longer than expected (>5µs)? → Reader's lock-free protocol retries up to 3 times. If all retries exhausted → `ShmError::ReadContention` (not a safety event, but logged via MQT).

---

## Requirements *(mandatory)*

### Functional Requirements

#### P2P SHM Library (evo_common::shm::p2p)

- **FR-001**: `TypedP2pWriter<T>` and `TypedP2pReader<T>` MUST be implemented in `evo_common::shm::p2p`, providing the sole SHM transport API for the entire workspace. No other SHM library or crate may exist.

- **FR-002**: The P2P library MUST implement the full protocol specified in spec 005 (FR-130a through FR-130p), including:
  - `P2pSegmentHeader` with magic (`b"EVO_P2P\0"`), write_seq (AtomicU32), heartbeat counter, struct version hash, source/dest module abbreviations
  - Lock-free write: set write_seq odd → copy payload → increment heartbeat → set write_seq even (all with Release ordering)
  - Lock-free read with bounded retry (max 3): load write_seq (Acquire) → if odd retry → copy payload → reload write_seq (Acquire) → if changed retry
  - Single-writer enforcement via `flock(LOCK_EX | LOCK_NB)`
  - Single-reader enforcement via `flock(LOCK_SH | LOCK_NB)`
  - Destination validation at attach time
  - Struct version hash validation at attach time via `const fn struct_version_hash<T>()`

- **FR-003**: The P2P library MUST have zero RT-unsafe operations in the read/write hot path:
  - No mutex, no `Mutex<T>`, no `RwLock<T>`
  - No heap allocation (`Vec::push`, `String::from`, `Box::new`)
  - No syscalls (`SystemTime::now()`, `sched_yield()`, file I/O)
  - No `panic!` paths (all errors returned as `Result`)
  - Memory ordering via `AtomicU32` with `Acquire`/`Release` only

- **FR-004**: `ShmError` MUST include all P2P-specific variants as defined in spec 005 FR-130h: `InvalidMagic`, `VersionMismatch`, `DestinationMismatch`, `WriterAlreadyExists`, `ReaderAlreadyConnected`, `ReadContention`, `SegmentNotFound`, `PermissionDenied`, `HeartbeatStale`.

- **FR-005**: `ModuleAbbrev` enum MUST be defined in `evo_common::shm::p2p` with variants: `Hal`, `Cu`, `Re`, `Rpc`, `Mqt`. The enum MUST derive `Copy`, `Clone`, `PartialEq`, `Eq`, and provide `as_str()` for segment name construction. Note: `Rpc` maps to the `evo_grpc` binary (RT↔non-RT SHM P2P bridge). `evo_api` (non-RT↔WWW HTTP/REST gateway), Dashboard, and Diagnostic have no SHM module abbreviation.

- **FR-006**: Segment names MUST follow the pattern `evo_[SOURCE]_[DESTINATION]` using `ModuleAbbrev::as_str()` values (e.g., `evo_hal_cu`). Fixed names without PID suffix — deterministic across restarts.

- **FR-007**: `SegmentDiscovery` MUST be implemented with:
  - `list_segments() -> Vec<SegmentInfo>` — enumerates `/dev/shm/evo_*`
  - `list_for(module: ModuleAbbrev) -> Vec<SegmentInfo>` — segments addressed to a given module
  - `SegmentInfo` containing: name, source module, dest module, size, writer_alive (probed via non-blocking flock test)

- **FR-008**: Segment lifecycle management MUST follow spec 005 FR-130j:
  - Writer `Drop` calls `shm_unlink`
  - Reader `Drop` calls `munmap` + releases flock
  - Orphan detection via heartbeat staleness
  - Writer restart overwrites stale segments via `O_CREAT`

- **FR-009**: `shm_open` MUST use file mode `0o600` (owner read/write only). All EVO processes run under the same OS user. No group or world access to SHM segments.

#### Unified SHM Segment Types (evo_common::shm::segments)

- **FR-010**: All P2P segment payload structs MUST be defined in `evo_common::shm::segments` with `#[repr(C)]` and fixed-size types only (no `String`, `Vec`, `HashMap`, `Option`).

- **FR-011**: `HalToCuSegment` MUST contain:
  - `axes: [HalAxisFeedback; MAX_AXES]` — position (f64), velocity (f64), torque_estimate (f64), drive_ready (bool), drive_fault (bool), referenced (bool), active (bool)
  - `di_bank: [u64; 16]` — 1024 digital inputs, word-packed
  - `ai_values: [f64; MAX_AI]` — analog inputs in engineering units (MAX_AI = 1024, defined in `evo_common::consts`)
  - `axis_count: u8` — number of active axes

- **FR-012**: `CuToHalSegment` MUST contain:
  - `axes: [CuAxisCommand; MAX_AXES]` — `ControlOutputVector` (calculated_torque, target_velocity, target_position, torque_offset), enable (bool), brake_release (bool)
  - `do_bank: [u64; 16]` — 1024 digital outputs, word-packed
  - `ao_values: [f64; MAX_AO]` — analog outputs in engineering units (MAX_AO = 1024, defined in `evo_common::consts`)
  - `axis_count: u8`

- **FR-013**: `CuToMqtSegment` MUST contain the live status snapshot defined in spec 005 FR-134:
  - `machine_state: MachineState`
  - `safety_state: SafetyState`
  - Per-axis: all 6 orthogonal state machines + safety flags + error states
  - No event ring buffer — snapshot only

- **FR-014**: Placeholder segment structs MUST be defined for future modules:
  - `HalToMqtSegment` — raw HAL data stream for MQTT: all I/O states (DI/DO/AI/AO), axis positions/velocities, driver state, cycle timing. Continuous telemetry for service/diagnostics/oscilloscope mode.
  - `RpcToHalSegment` — direct HAL commands from gRPC: set DO, set AO, driver configuration. Action requests expecting ack via `evo_hal_rpc`.
  - `HalToRpcSegment` — HAL responses/acks to gRPC: DO set confirmation, driver state, error feedback.
  - `ReToHalSegment` — direct I/O commands from RE to HAL: set DO, set AO **only for pins without an `IoRole` assignment**. Role-assigned pins are controlled via CU.
  - `HalToReSegment` — HAL feedback to RE: current I/O states (DI/DO banks, AI/AO values), per-axis position/velocity/drive_ready. Direct read for recipe decision logic.
  - `ReToMqtSegment` — recipe execution telemetry for MQTT: current step, program name, cycle count, RE state. Continuous stream for service/dashboard.
  - `ReToRpcSegment` — recipe status/acks for gRPC: execution progress, step result, error feedback.
  - `RpcToReSegment` — placeholder for gRPC→RE commands. Payload defined in a separate spec.
  - `ReToCuSegment` — motion requests, program commands, `AllowManualMode`
  - `RpcToCuSegment` — external commands (manual jog, service mode, config reload)
  - `CuToReSegment` — reserved placeholder (empty struct with heartbeat only)
  - `CuToRpcSegment` — full diagnostic snapshot for gRPC/Dashboard/API (superset of MQT)

- **FR-015**: Shared helper functions for bit-packed I/O MUST be in `evo_common::shm::io_helpers`:
  - `get_di(bank: &[u64; 16], pin: usize) -> bool`
  - `set_do(bank: &mut [u64; 16], pin: usize, value: bool)`
  - Both MUST handle NC/NO inversion when used through `IoRegistry`

#### Watchdog Process Management (evo)

- **FR-020**: `evo` binary MUST spawn HAL and CU as child processes using `std::process::Command`:
  - HAL is spawned first
  - Watchdog waits for `evo_hal_cu` segment to appear (polling `/dev/shm/evo_hal_cu` with configurable timeout, default 5 seconds)
  - Only after `evo_hal_cu` is confirmed does watchdog spawn CU

- **FR-021**: Watchdog MUST monitor child processes via `waitpid` (non-blocking poll) or `SIGCHLD` signal handling:
  - Detect process exit (normal or crash)
  - Detect signal-based termination

- **FR-022**: Watchdog MUST implement restart logic with exponential backoff:
  - Initial delay: 100ms (configurable in `config.toml [watchdog]`)
  - Maximum delay: 30 seconds (configurable)
  - Maximum consecutive restarts: 5 (configurable)
  - After max restarts exceeded: watchdog stays alive, stops restarting the failed child, logs a single `CRITICAL` error. No periodic retry or warning messages — a single log entry is sufficient. Operator or external system (systemd) must intervene.
  - Successful run of >60 seconds (configurable) resets the backoff counter

- **FR-023**: Watchdog MUST implement graceful shutdown propagation:
  - On receiving SIGTERM or SIGINT: send SIGTERM to CU, wait up to 2 seconds, send SIGTERM to HAL, wait up to 2 seconds
  - If child does not exit after SIGTERM timeout: send SIGKILL
  - After all children exit: clean up all `evo_*` SHM segments via `shm_unlink`
  - Exit with code 0

- **FR-024**: Watchdog MUST clean up orphan SHM segments on startup:
  - Enumerate `/dev/shm/evo_*`
  - For each segment: check if writer is alive (non-blocking flock test)
  - If writer is dead: `shm_unlink` the segment
  - This prevents stale segments from previous crashes blocking new writers

- **FR-025**: Watchdog MUST pass configuration directory path to child processes via command-line argument:
  - `evo_hal --config-dir <config_directory_path>`
  - `evo_control_unit --config-dir <config_directory_path>`
  - The config directory contains `config.toml`, `machine.toml`, `io.toml`, and `axis_NN_*.toml` files
  - Each program loads `config.toml` (its own `[program]` section), `machine.toml`, `io.toml`, and axis files from this directory
  - Config paths are resolved relative to the watchdog's working directory

- **FR-026**: Watchdog shutdown order MUST be reverse of startup order:
  - Startup: HAL → CU
  - Shutdown: CU → HAL
  - This ensures CU stops sending commands before HAL stops accepting them

- **FR-027**: Watchdog MUST implement the `WatchdogTrait` (T050) defined in `evo_common`:
  - `spawn_module(name, config)` — start a child process
  - `health_check(name)` — query module liveness
  - `restart_module(name)` — stop + restart with backoff
  - `shutdown_all()` — ordered shutdown of all modules
  - If the trait does not yet exist in `evo_common`, it MUST be created as part of this spec

- **FR-028**: Watchdog MAY read P2P segment headers (heartbeat counter) for advanced hang detection:
  - If a child process is alive (`waitpid` shows running) but its writer heartbeat is frozen for > N cycles, watchdog logs a warning
  - This is supplementary to `waitpid` — not a replacement
  - Header-only read does NOT require `TypedP2pReader` — it reads the first 64 bytes of the mapped segment directly

#### HAL SHM Integration

- **FR-030**: HAL RT loop MUST create `TypedP2pWriter::<HalToCuSegment>` at startup and write the full segment after every `driver.cycle()` call:
  - Convert `HalStatus` → `HalToCuSegment` (position, velocity, DI, AI, referenced flags)
  - Heartbeat increments every cycle

- **FR-030a**: HAL MUST also create `TypedP2pWriter::<HalToMqtSegment>` for `evo_hal_mqt` and write it every cycle:
  - Contains the same raw data as `HalToCuSegment` plus additional driver diagnostics (cycle timing, driver state)
  - This is a continuous telemetry stream consumed by MQTT for service, diagnostics, and oscilloscope mode
  - HAL writes to MQTT in exactly the same pattern as CU writes to `evo_cu_mqt`

- **FR-030b**: HAL MUST create `TypedP2pWriter::<HalToRpcSegment>` for `evo_hal_rpc` (placeholder):
  - Contains ack/response data for gRPC action-reaction pattern (e.g., "DO 7 set to HIGH" confirmation)
  - Optionally attach `TypedP2pReader::<RpcToHalSegment>` to `evo_rpc_hal` (non-blocking, retry periodically) for receiving direct commands from gRPC

- **FR-030c**: HAL MUST create `TypedP2pWriter::<HalToReSegment>` for `evo_hal_re` (placeholder) and optionally attach `TypedP2pReader::<ReToHalSegment>` to `evo_re_hal` (non-blocking, retry periodically):
  - `evo_hal_re`: contains current I/O states (DI/DO banks, AI/AO values), per-axis position/velocity/drive_ready — written every cycle so RE always has fresh data
  - `evo_re_hal`: contains direct I/O commands from RE (set DO, set AO) **only for pins without an `IoRole` assignment** — HAL applies these commands in its RT loop alongside CU commands
  - For role-assigned pins, RE MUST route changes through CU (out of scope here) and HAL applies only CU-authored changes; RE commands for role-assigned pins are ignored and logged using the existing error schema
  - Pins without `IoRole` are **not** controlled by CU; only RE may command them directly via `evo_re_hal`

- **FR-031**: HAL RT loop MUST attempt to attach `TypedP2pReader::<CuToHalSegment>` for reading CU commands:
  - If `evo_cu_hal` exists: read commands and convert `CuToHalSegment` → `HalCommands` for `driver.cycle()`
  - If `evo_cu_hal` does not exist: use default (zero) commands — HAL operates in passive mode

- **FR-032**: HAL MUST populate `HalToCuSegment.di_bank` by reading all DI pins from the driver and packing them using `set_do()` helper (word-packed, bit position = pin number).

- **FR-033**: HAL MUST populate `HalToCuSegment.ai_values` by reading all AI pins from the driver, applying the scaling curve and offset defined in `io.toml`, and storing the result in engineering units.

- **FR-034**: HAL MUST load `io.toml` at startup and build an `IoRegistry` that maps `IoRole → (pin, type, logic, scaling)`. All runtime I/O access in HAL MUST use `IoRegistry` role-based lookup — no direct pin-number access in application code.

- **FR-035**: Conversion functions `HalStatus → HalToCuSegment` and `CuToHalSegment → HalCommands` MUST be defined in `evo_common::shm::conversions` (shared code, testable independently).

- **FR-036**: Role-based I/O ownership MUST be enforced:
  - If an I/O pin has an `IoRole` assignment in `io.toml`, **only CU** is allowed to change its state (via `evo_cu_hal`).
  - RE may **read any I/O state** from `evo_hal_re`.
  - RE may send **direct** commands via `evo_re_hal` **only** for pins **without** an `IoRole` assignment.
  - If RE attempts to command a role-assigned pin, HAL MUST ignore the command and log it using the existing error schema.
  - CU MUST NOT attempt to control pins without an `IoRole` assignment (CU operates only on role-assigned pins).
  - CU reads I/O only via `IoRegistry` roles; pins without `IoRole` are not visible to CU.

#### CU Binary Integration

- **FR-040**: CU `main.rs` MUST instantiate `CycleRunner` with loaded config and enter the RT loop:
  - Parse machine config and `io.toml`
  - Build `IoRegistry`
  - Attach to `evo_hal_cu` (mandatory — wait with timeout)
  - Create `evo_cu_hal`, `evo_cu_mqt`, `evo_cu_re`, `evo_cu_rpc` writers
  - Optionally attach to `evo_re_cu`, `evo_rpc_cu` (non-blocking, retry periodically)
  - Call `CycleRunner::run()` which enters the deterministic loop

- **FR-041**: `CycleRunner` struct MUST be extended with runtime state holders:
  - `IoRegistry` — for safety evaluation and I/O role resolution
  - `AxisControlState[MAX_AXES]` — PID integrator, DOB state, filter states per axis
  - Reference to loaded `UniversalControlParameters` per axis

- **FR-042**: CU cycle body MUST read `di_bank` and `ai_values` from `HalToCuSegment` and make them available to safety evaluation and state machine logic (audit issue G10).

- **FR-043**: CU MUST fix MQT status field truncation (audit G9): `error_flags` MUST be written as `u32` (or wider), NOT truncated via `as u16` or `as u8`. All flag bits must be preserved in `CuToMqtSegment`.

- **FR-044**: CU MUST periodically call `try_attach_re()` and `try_attach_rpc()` (audit G8) to detect late-starting RE and gRPC processes. Retry interval: once per second (not every RT cycle).

#### Unified Configuration Loading

- **FR-050**: Machine configuration MUST be structured so that axis parameters are defined once and consumed by both HAL and CU:
  - Axis kinematics (max_velocity, min_pos, max_pos, in_position_window) — used by HAL for simulation limits and by CU for safety monitoring
  - Control parameters (PID gains, DOB, filters) — used by CU only, but defined in the same per-axis config block
  - Safety stop parameters (category, max_decel_safe) — used by CU only
  - Homing parameters — used by CU for supervision, HAL for execution
  - Peripheral config (brake, tailstock, guard, locking pin) — used by CU for safety, HAL for I/O

- **FR-051**: `io.toml` MUST be the single source of truth for all I/O pin assignments, NC/NO logic, debounce, scaling curves, and functional roles. Both HAL and CU MUST load the same file.

- **FR-052**: `IoConfig`, `IoGroup`, `IoPoint`, `IoRole`, and `IoRegistry` types MUST be defined in `evo_common::io` and used identically by all programs.

- **FR-053**: Config loading MUST use `evo_common::config::ConfigLoader`. Unknown fields in `machine.toml` MUST cause `ConfigError::UnknownField` (no silent ignoring — strict parsing). Missing mandatory fields produce clear `ConfigError::ParseError`. Per-axis files use `#[serde(deny_unknown_fields)]`.

- **FR-054**: All numeric parameters MUST have min/max bounds defined as `const` in `evo_common`, validated at load time (spec 005 FR-156).

#### Per-Axis Configuration Files

- **FR-055**: Machine configuration MUST support a per-axis file architecture with auto-discovery:
  - All config files live in a single flat directory (same directory as `machine.toml`)
  - `ConfigLoader` scans the directory for files matching `axis_*_*.toml` glob pattern
  - Each matching file is parsed as a per-axis config: `[axis]` (id, name, type), `[kinematics]`, `[control]`, `[safe_stop]`, `[homing]`, and optional `[brake]`, `[tailstock]`, `[guard]`, `[coupling]` sections
  - Files are sorted by the NN prefix in `axis_NN_name.toml` for deterministic ordering
  - The name part after NN is free-form and has no functional meaning (`axis_08_tailstock.toml` and `axis_08_konik.toml` are equivalent)
  - Axis count is determined by the number of discovered axis files
  - No `axes_dir` or `axes = [...]` parameter exists in `machine.toml` — discovery is fully automatic

- **FR-055a**: `ConfigLoader` MUST validate axis file consistency at load time:
  - Each axis file’s `[axis].id` MUST match the NN prefix in the filename (e.g., `axis_03_z.toml` MUST have `id = 3`). Mismatch fails with `ConfigError::AxisIdMismatch { file, expected, found }`.
  - No two axis files may have the same NN prefix. If `axis_08_tailstock.toml` and `axis_08_konik.toml` both exist, startup fails with `ConfigError::DuplicateAxisId(8)`.
  - If zero axis files are found, config validation fails with `ConfigError::NoAxesDefined`.

- **FR-056**: `ConfigLoader` MUST support only the new per-axis file format:
  - If `machine.toml` contains `[[axes]]` array or `axes_dir`, config parsing MUST fail with `ConfigError::UnknownField` — these are legacy parameters that no longer exist.
  - There is no backward compatibility layer. The old format is completely unsupported.
  - If neither axis files nor any legacy parameters are found, config validation fails with `ConfigError::NoAxesDefined`.

- **FR-057**: Per-axis config file schema MUST cover all axis parameters:
  - All fields from the original reference config (`axis_id`, `name`, `max_velocity`, `safe_reduced_speed_limit`, `min_pos`, `max_pos`, `in_position_window`, `control.*`, `safe_stop.*`, `homing.*`, `brake.*`, `tailstock.*`, `guard.*`, `coupling.*`) MUST be representable in the per-axis file
  - The parsed result is an `AxisConfig` struct used identically by HAL and CU

- **FR-058**: Reference configuration `config/test_8axis.toml` MUST be migrated to the per-axis format:
  - Create 8 axis files in `config/` directory (flat, alongside `machine.toml` and `io.toml`)
  - Update `config/machine.toml` to remove any `axes_dir` or `[[axes]]` references
  - Delete the old `test_8axis.toml` — no legacy format is preserved

- **FR-059**: HAL's `evo_hal/config/machine.toml` MUST be fixed to match actual config struct fields (audit 9.1):
  - Remove non-existent fields: `invert` on `DigitalIOConfig`, `min_raw`/`max_raw`/`curve_type` on `AnalogIOConfig`
  - Replace with valid fields from `io.toml` schema (roles, logic, scaling curves)
  - Or delete this file entirely if HAL migrates to using the shared `machine.toml` + `io.toml`

- **FR-059a**: A new `config.toml` MUST be defined for EVO system/program configuration, separate from `machine.toml`:
  - `config.toml` contains parameters for starting and configuring EVO programs, independent of which machine is controlled
  - `machine.toml` contains parameters specific to the physical machine (axes, kinematics, safety, service bypass)
  - `config.toml` MUST include a section per program: `[watchdog]`, `[hal]`, `[cu]`, `[re]`, `[mqtt]`, `[grpc]`, `[api]`, `[dashboard]`, `[diagnostic]`
  - `[watchdog]` section MUST include: `max_restarts` (u32), `initial_backoff_ms` (u64), `max_backoff_s` (u64), `stable_run_s` (u64), `sigterm_timeout_s` (f64), `hal_ready_timeout_s` (f64)
  - Other progvery TOML configuration file** (`config.toml`, `machine.toml`, ram sections are initially stubs (empty or with comments) — parameters added as programs are developed
  - `config.toml` lives in the same flat config directory alongside `machine.toml`, `io.toml`, and axis files
  - `ConfigLoader` MUST load `config.toml` in addition to `machine.toml` and `io.toml`
  - The parsed result is a `SystemConfig` struct in `evo_common::config` with per-program sub-structs

#### Dead Code and Crate Removal

- **FR-060**: The following files MUST be deleted:
  - `evo_hal/src/main_old.rs` — legacy binary not in Cargo.toml
  - `evo_common/src/shm/config.rs` — empty placeholder
  - `evo_hal/src/shm.rs` — dead `HalShmData` types never instantiated (170 lines)

- **FR-061**: The entire `evo_shared_memory` directory MUST be removed:
  - `evo_shared_memory/src/`, `tests/`, `benches/`, `examples/`, `Cargo.toml`
  - Remove from workspace `members` in root `Cargo.toml`
  - Remove `evo_shared_memory` dependency from all crate `Cargo.toml` files

- **FR-062**: Legacy SHM types in `evo_shared_memory::data::*` MUST NOT be migrated. They are JSON-serializable types with `String`/`Vec`/`HashMap` — incompatible with zero-copy P2P. Programs using them (`evo_grpc`, `evo_recipe_executor`) MUST be updated to use P2P segment types from `evo_common`.

- **FR-063**: `evo_hal/src/module_status.rs` and its dependency on `evo_shared_memory` MUST be replaced with P2P-based status reporting or removed if not needed for the HAL↔CU pipeline.

- **FR-064**: Unused public methods in HAL simulation MUST be cleaned up (audit 3.5):
  - `ModuleStatusPublisher::report_error()`, `clear_error()`, `update_custom_data()`, `update_axis_count()`, `update_io_count()` — remove or mark `#[allow(dead_code)]` with TODO if planned for future use
  - `AxisSimulator::get_position()`, `is_referenced()` — remove if never called externally (internal state is accessed via `HalStatus`)
  - `SoftLimitError`, `SoftLimitDirection` — remove or implement (currently defined but never instantiated)
  - Rule: every public symbol MUST be imported by at least one external module, or be gated behind `#[cfg(test)]`, or be removed

#### Dependency Cleanup

- **FR-070**: Root `Cargo.toml` MUST define `[workspace.dependencies]` for all shared dependencies:
  - `serde`, `toml`, `tracing`, `tracing-subscriber`, `heapless`, `nix`, `clap` (if used)
  - Each crate's `Cargo.toml` references workspace deps: `serde = { workspace = true }`

- **FR-071**: Dependency versions MUST be unified:
  - `heapless` → 0.9 everywhere
  - `nix` → 0.30 everywhere
  - `criterion` → latest stable in all benches

- **FR-072**: Unused dependencies MUST be removed:
  - `evo`: remove `parking_lot`, `tokio` (watchdog is sync)
  - `evo_grpc`: remove `parking_lot`
  - `evo_recipe_executor`: remove `parking_lot`
  - `evo_control_unit`: remove `bitflags`, `static_assertions` (both used only in `evo_common`)

- **FR-073**: `evo_common` MUST migrate from `log` to `tracing` for consistency with all other crates.

- **FR-074**: Misleading alias `evo = { package = "evo_common" }` in `evo/Cargo.toml` MUST be renamed to `evo_common = { path = "../evo_common" }` (audit 6.5). All `use evo::` imports in the watchdog MUST be updated to `use evo_common::`.

- **FR-075**: Empty `rt` feature flag in `evo_control_unit/Cargo.toml` MUST be either:
  - Populated with actual gated dependencies (`nix/sched`, `nix/time`, `nix/resource`) and used to guard RT-specific code paths (`rt_setup()`, `mlockall()`, `sched_setscheduler()`), OR
  - Removed entirely if RT setup is always compiled in (audit 6.7)

- **FR-076**: `evo_common::prelude` MUST be either:
  - Made useful: re-export the top ~10 most-imported symbols (`MAX_AXES`, `CYCLE_TIME_US`, `IoRole`, `IoRegistry`, `ShmError`, `ModuleAbbrev`, `TypedP2pWriter`, `TypedP2pReader`) and update at least `evo_hal` and `evo_control_unit` to use `use evo_common::prelude::*`, OR
  - Removed if no crate adopts it (audit 9.2)

#### Code Quality

- **FR-077**: `evo_hal/src/driver_registry.rs` global `LazyLock<RwLock<HashMap>>` SHOULD be refactored to constructor-injection pattern:
  - `DriverRegistry` becomes a regular struct, constructed at HAL startup and passed by reference
  - Tests no longer need `#[ignore]` due to shared mutable global state
  - If full refactoring is too invasive for this spec, at minimum: document the limitation and remove `#[ignore]` from tests by using per-test registry instances (audit 9.3)

- **FR-078**: A short (< 5 min) RT stability test MUST be added to CI as an alternative to the disabled 24-hour soak test (audit 9.8):
  - Run CycleRunner for 10,000 cycles, verify zero deadline misses (within ±10% of configured cycle time)
  - This does not replace the full soak test but catches timing regressions in CI

#### Segment Type Definitions for Placeholder Connections

- **FR-014a**: `CuToRpcSegment` MUST be defined in `evo_common::shm::segments` with at minimum:
  - All fields of `CuToMqtSegment` (superset)
  - Per-axis: PID internal state (error, integral, output) for tuning visualization
  - Cycle timing: last_cycle_ns, max_cycle_ns, jitter_histogram_us (fixed-size array)

- **FR-014b**: `ReToMqtSegment` and `ReToRpcSegment` MUST be defined in `evo_common::shm::segments`:
  - `ReToMqtSegment`: recipe execution telemetry — current_step (u16), program_name (fixed-size array), cycle_count (u64), re_state (enum), error_code (u32). Continuous stream for service/dashboard visualization.
  - `ReToRpcSegment`: recipe status for gRPC action-reaction — execution_progress (percent), step_result (enum), error_message (fixed-size array), request_id (u64). Enables Dashboard to query recipe state and receive acks.

- **FR-014c**: `HalToMqtSegment` MUST be defined in `evo_common::shm::segments` with at minimum:
  - All fields of `HalToCuSegment` (axis feedback, DI bank, AI values)
  - Driver diagnostics: cycle_time_ns, driver_state per axis
  - DO bank and AO values (current output state) for complete I/O visualization
  - This segment enables MQTT to publish raw HAL data for service, oscilloscope mode, and I/O state monitoring

- **FR-014d**: `RpcToHalSegment`, `HalToRpcSegment`, and `RpcToReSegment` MUST be defined in `evo_common::shm::segments`:
  - `RpcToHalSegment`: action command (set_do, set_ao, driver_command), target pin/axis, value, request_id (u64)
  - `HalToRpcSegment`: ack/response (request_id, result_code, error message fixed-size array)
  - `RpcToReSegment`: placeholder segment (empty struct with heartbeat only). Payload defined in a separate spec.
  - These segments enable the gRPC action-reaction pattern with HAL and RE (e.g., "start recipe X" → ack via `evo_re_rpc`)

- **FR-014e**: `ReToHalSegment` and `HalToReSegment` MUST be defined in `evo_common::shm::segments`:
  - `ReToHalSegment`: direct I/O commands from recipe logic — set DO (pin, value), set AO (pin, value), request_id (u64). This is the fast path for recipe I/O operations without CU intermediary.
  - `HalToReSegment`: HAL feedback for RE — current DI bank, DO bank, AI values, AO values, per-axis position/velocity/drive_ready. This gives RE direct read access to all I/O states and axis positions for recipe decision logic (wait for DI, check position, verify DO state).

- **FR-014f**: `ReToMqtSegment` and `ReToRpcSegment` MUST be defined in `evo_common::shm::segments`:
  - `ReToMqtSegment`: recipe execution telemetry — current_step (u16), program_name (fixed-size array), cycle_count (u64), re_state (enum), error_code (u32). Continuous stream for service/dashboard visualization via MQTT.
  - `ReToRpcSegment`: recipe status for gRPC action-reaction — execution_progress (percent), step_result (enum), error_message (fixed-size array), request_id (u64). Enables Dashboard to query recipe state and receive acks.

#### Constant and Type Deduplication

- **FR-080**: All system-wide constants MUST have exactly one definition in `evo_common::consts`. The following constants currently in `evo_common::hal::consts` MUST move to `evo_common::consts` for global access:
  - `pub const MAX_AXES: usize = 64`
  - `pub const MAX_DI: usize = 1024`
  - `pub const MAX_DO: usize = 1024`
  - `pub const MAX_AI: usize = 1024`
  - `pub const MAX_AO: usize = 1024`
  - `pub const DEFAULT_CONFIG_PATH: &str = "/etc/evo"` (global config directory; used by all programs)
  - `pub const DEFAULT_STATE_FILE: &str = "/etc/evo/hal_state"` (must live in the same directory as `DEFAULT_CONFIG_PATH`)
  - All other definitions of these constants MUST be removed and replaced with imports from `evo_common::consts`
  - `HAL_SERVICE_NAME` MUST be removed (artifact of `evo_shared_memory`). Use `ModuleAbbrev` and fixed segment names instead.

- **FR-081**: `AnalogCurve` MUST have exactly one definition in `evo_common::io::config`, matching the `io.toml` specification: preset string (`"linear"`, `"quadratic"`, `"cubic"`) or custom `[a, b, c]` polynomial, plus separate `offset` field. The duplicate struct in `evo_common::hal::config` MUST be removed.

- **FR-082**: `EVO_SHM_MAGIC` MUST be removed entirely from `evo_common::shm::consts`. No broadcast-era code remains after `evo_shared_memory` removal. Only `P2P_SHM_MAGIC` from `evo_common::shm::p2p` exists.

- **FR-083**: `CYCLE_TIME_US` and any other timing constants MUST have single definitions in `evo_common::consts`.

#### Stub Program P2P Skeleton

- **FR-090**: Each stub program MUST be updated to include appropriate initialization code:
  - `evo_mqtt/src/main.rs`: Attach `TypedP2pReader::<CuToMqtSegment>` to `evo_cu_mqt`, attach `TypedP2pReader::<HalToMqtSegment>` to `evo_hal_mqt`, attach `TypedP2pReader::<ReToMqtSegment>` to `evo_re_mqt`
  - `evo_grpc/src/main.rs`: **RT↔non-RT SHM P2P bridge.** Create `TypedP2pWriter::<RpcToCuSegment>` for `evo_rpc_cu`, create `TypedP2pWriter::<RpcToHalSegment>` for `evo_rpc_hal`, create `TypedP2pWriter::<RpcToReSegment>` for `evo_rpc_re`, attach `TypedP2pReader::<CuToRpcSegment>` to `evo_cu_rpc`, attach `TypedP2pReader::<HalToRpcSegment>` to `evo_hal_rpc`, attach `TypedP2pReader::<ReToRpcSegment>` to `evo_re_rpc`. Exposes gRPC service consumed by `evo_api`, Dashboard, and Diagnostic.
  - `evo_api/src/main.rs`: **Non-RT↔WWW HTTP/REST external gateway.** No SHM initialization. Connects to `evo_grpc` as a gRPC client for command/query forwarding, and subscribes to `evo_mqtt` for telemetry data. Exposes REST/HTTP endpoints to web clients, mobile apps, and SCADA systems.
  - `evo_recipe_executor/src/main.rs`: Create `TypedP2pWriter::<ReToCuSegment>` for `evo_re_cu`, create `TypedP2pWriter::<ReToHalSegment>` for `evo_re_hal`, create `TypedP2pWriter::<ReToMqtSegment>` for `evo_re_mqt`, create `TypedP2pWriter::<ReToRpcSegment>` for `evo_re_rpc`, attach `TypedP2pReader::<CuToReSegment>` to `evo_cu_re`, attach `TypedP2pReader::<HalToReSegment>` to `evo_hal_re`, attach `TypedP2pReader::<RpcToReSegment>` to `evo_rpc_re`
  - `evo_diagnostic/src/main.rs`: No P2P initialization — Diagnostic consumes data exclusively via MQTT (through MQTT broker) and gRPC (interactive queries via `evo_grpc`). Same pattern as Dashboard.
  - `evo_dashboard/src/main.rs`: No P2P initialization — communicates exclusively via gRPC (through `evo_grpc`) and MQTT (through `evo_mqtt`)

- **FR-091**: Stub programs MUST depend only on `evo_common` for SHM types — no dependency on `evo_shared_memory` or any other SHM crate.

- **FR-092**: Each stub program's P2P initialization MUST include error handling: if segment creation/attachment fails, log error via `tracing` and exit gracefully.

### Key Entities

- **TypedP2pWriter\<T\>**: Generic SHM segment writer. Creates and owns one segment. Implements lock-free write protocol with heartbeat. `Drop` calls `shm_unlink`.
- **TypedP2pReader\<T\>**: Generic SHM segment reader. Attaches to existing segment with destination/version validation. Implements lock-free read with bounded retry.
- **P2pSegmentHeader**: 64-byte header: magic (8B), write_seq (4B), heartbeat (8B), version_hash (4B), source_module (1B), dest_module (1B), reserved (38B).
- **ModuleAbbrev**: Enum identifying EVO modules (Hal, Cu, Re, Rpc, Mqt). Dashboard and Diagnostic have no SHM abbreviation.
- **HalToCuSegment**: HAL→CU payload: axis feedback array, DI bank, AI values, axis count.
- **CuToHalSegment**: CU→HAL payload: axis commands array, DO bank, AO values, axis count.
- **CuToMqtSegment**: CU→MQTT payload: live status snapshot of machine state and all axes.
- **HalToMqtSegment**: HAL→MQTT payload: raw HAL data stream (all I/O, axis positions, driver state) for service/diagnostics/oscilloscope.
- **CuToRpcSegment**: CU→gRPC payload: superset of MQT (adds PID internals, cycle timing). Served by gRPC to Dashboard/API.
- **HalToRpcSegment**: HAL→gRPC payload: action responses/acks (DO set confirmation, driver state).
- **RpcToHalSegment**: gRPC→HAL payload: direct HAL commands (set DO, set AO, driver config).
- **ReToCuSegment**: RE→CU payload: motion requests, program commands.
- **RpcToCuSegment**: gRPC→CU payload: external commands (jog, mode change).
- **CuToReSegment**: CU→RE payload: ack, execution status (placeholder).
- **ReToHalSegment**: RE→HAL payload: direct I/O commands (set DO, set AO) **only for pins without an `IoRole` assignment** — role-assigned pins are controlled via CU.
- **HalToReSegment**: HAL→RE payload: current I/O states (DI/DO banks, AI/AO values), per-axis position/velocity/drive_ready — direct read for recipe decision logic.
- **ReToMqtSegment**: RE→MQTT payload: recipe execution telemetry (current step, program name, cycle count, RE state).
- **ReToRpcSegment**: RE→gRPC payload: recipe status/acks (execution progress, step result, error feedback).
- **RpcToReSegment**: gRPC→RE payload: placeholder only. Content defined in a separate spec.
- **HalAxisFeedback**: Per-axis struct: position, velocity, torque_estimate, drive_ready, drive_fault, referenced, active.
- **CuAxisCommand**: Per-axis struct: ControlOutputVector (4 fields), enable, brake_release.
- **IoRegistry**: Runtime role→pin resolver built from `io.toml`. Provides `read_di`/`read_ai`/`write_do`/`write_ao` by `IoRole`.
- **IoRole**: Functional role enum (EStop, LimitMinN, BrakeOutN, etc.) — single flat list, convention `FunctionAxisNumber`.
- **IoConfig**: Parsed representation of `io.toml` — groups containing I/O points with all parameters.
- **SegmentDiscovery**: Utility for enumerating active P2P segments in `/dev/shm/`.
- **ShmError**: Unified error type for all P2P operations.
- **ConfigLoader**: Unified config parser. Loads `machine.toml` (global), `io.toml` (I/O), and auto-discovers `axis_NN_*.toml` files from the same config directory. Only the new per-axis file format is supported — no backward compatibility with `[[axes]]`.
- **AxisConfig**: Parsed per-axis configuration. Contains kinematics, control, safe_stop, homing, and optional peripherals.
- **WatchdogTrait**: Trait in `evo_common` for process supervision: `spawn_module`, `health_check`, `restart_module`, `shutdown_all`.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Running `evo --config-dir config/` starts HAL and CU within 3 seconds, with `evo_hal_cu` and `evo_cu_hal` segments active and heartbeats incrementing at the configured cycle rate.
- **SC-002**: Data round-trip: axis position written by HAL appears in CU's read within 2 cycles (≤ 2ms at 1kHz). Control output written by CU appears in HAL's read within 2 cycles.
- **SC-003**: Watchdog detects HAL crash (SIGKILL) and initiates restart within 500ms. System returns to operational state (both segments active) within 10 seconds including backoff.
- **SC-004**: `cargo build --workspace` succeeds with zero references to `evo_shared_memory`. The crate directory does not exist.
- **SC-005**: `grep -rn "MAX_AXES" --include="*.rs" | grep "pub const"` returns exactly 1 result (in `evo_common`). Same for `AnalogCurve` definition.
- **SC-006**: P2P write latency ≤ 5µs and read latency ≤ 2µs for segments ≤ 8KB, measured in benchmarks without RT kernel (baseline).
- **SC-007**: Both HAL and CU parse the same `io.toml` and agree on all role→pin mappings. A test starts both, compares their `IoRegistry` outputs for 10 roles — all match.
- **SC-008a**: `evo_mqtt` compiles and contains `TypedP2pReader` attach calls for `evo_cu_mqt`, `evo_hal_mqt`, `evo_re_mqt`.
- **SC-008b**: `evo_grpc` compiles and contains 3 `TypedP2pWriter` creates (`evo_rpc_cu`, `evo_rpc_hal`, `evo_rpc_re`) and 3 `TypedP2pReader` attaches (`evo_cu_rpc`, `evo_hal_rpc`, `evo_re_rpc`), plus placeholder gRPC service.
- **SC-008c**: `evo_recipe_executor` compiles and contains 4 `TypedP2pWriter` creates (`evo_re_cu`, `evo_re_hal`, `evo_re_mqt`, `evo_re_rpc`) and 3 `TypedP2pReader` attaches (`evo_cu_re`, `evo_hal_re`, `evo_rpc_re`).
- **SC-008d**: `evo_api` compiles with no SHM — placeholder for gRPC client (connects to `evo_grpc`) and MQTT subscriber (connects to `evo_mqtt`), exposes REST/HTTP.
- **SC-008e**: `evo_dashboard` compiles with no SHM — communicates via gRPC (through `evo_grpc`) and MQTT only.
- **SC-008f**: `evo_diagnostic` compiles with no SHM — communicates via gRPC (through `evo_grpc`) and MQTT only.
- **SC-009**: `cargo tree -d` shows zero duplicate versions for `heapless`, `nix`, `serde`, `tracing` within the workspace.
- **SC-010**: Graceful shutdown completes within 5 seconds: watchdog sends SIGTERM to children, children exit, SHM segments cleaned, watchdog exits with code 0.
- **SC-011**: Per-axis config: `config/` contains 8 axis files (flat, alongside `machine.toml`). `ConfigLoader` auto-discovers them and produces valid `AxisConfig` structs. A test verifies all 8 axes are loaded with correct IDs and parameters.
- **SC-012**: Audit resolution: every item in `docs/audit.md` §1–§9 is mapped to a spec 006 FR (✅) or an explicit deferral (⏳) in the Audit Resolution Matrix. Zero items are untracked.
- **SC-013**: P2P connection completeness: all 15 segment types are defined in `evo_common::shm::segments`. Each RT module (HAL, CU, RE) has exactly 4 outbound segments. The "NOT connected" table explicitly lists and justifies every omitted pair.
- **SC-014**: `WatchdogTrait` is implemented in `evo` binary and passes unit tests for spawn, health_check, and shutdown_all methods.

---

## Assumptions

- **A-001**: The P2P protocol specification from spec 005 (FR-130a through FR-130p) is authoritative and this spec implements it without modification.
- **A-002**: `evo_shared_memory` has no external consumers outside this workspace. Removal is a workspace-internal change.
- **A-003**: All current `evo_shared_memory` usage in `evo_grpc` and `evo_recipe_executor` is limited to stub/placeholder code that can be replaced without functional loss.
- **A-004**: HAL simulation driver (`SimulationDriver`) is functional and produces valid `HalStatus` with axis data and I/O state.
- **A-005**: The per-axis file format follows the structure defined in this spec's Per-Axis Configuration Architecture. Axis files are auto-discovered from the same directory as `machine.toml` using the `axis_NN_*.toml` pattern.
- **A-006**: `io.toml` format follows the structure defined in spec 005 (`specs/005-control-unit/io.toml`) — this serves as the reference I/O configuration.
- **A-007**: For this spec, the watchdog does not need RT thread management (chrt/taskset) — that will be added in a future spec after basic process management works.
- **A-008**: CU's `CycleRunner` RT loop already has functional read/write phases in library code — this spec connects them to the binary and SHM, not reimplements the control logic.
- **A-009**: Stub programs (mqtt, grpc, api, recipe_executor) need appropriate initialization — full application logic is out of scope. `evo_grpc` is the RT↔non-RT SHM P2P bridge (has SHM segments); `evo_api` is the non-RT↔WWW HTTP/REST gateway (no SHM, connects to `evo_grpc` via gRPC and `evo_mqtt` as subscriber). Recipe executor writes 4 outbound segments (to CU, HAL, MQTT, gRPC) and reads 3 inbound (from CU, HAL, gRPC). Dashboard and Diagnostic communicate via gRPC (through `evo_grpc`) and MQTT only (no P2P).
- **A-010**: `evo_common` logging migration from `log` to `tracing` is a mechanical find-and-replace that does not change behavior.

---

## Dependencies

- **D-001**: spec 005 (Control Unit) — defines P2P protocol, segment types, and io.toml schema that this spec implements
- **D-002**: spec 003 (HAL Simulation) — HAL simulation driver that produces `HalStatus`
- **D-003**: spec 004 (Common Lib Setup) — `evo_common` crate structure and shared types
- **D-004**: Linux POSIX shared memory (`shm_open`, `mmap`, `flock`, `shm_unlink`)
- **D-005**: `config/axis_01_x.toml` through `config/axis_08_tailstock.toml` — reference per-axis configuration files for integration testing

---

## Scope & Boundaries

### In Scope

- P2P SHM library implementation in `evo_common` (`TypedP2pWriter`, `TypedP2pReader`, `SegmentDiscovery`)
- All SHM segment struct definitions in `evo_common` (15 segment types: active + skeleton + placeholder)
- Conversions between HAL types and SHM segment types
- HAL SHM write (feedback to CU, feedback to RE, telemetry to MQTT, ack to gRPC) and read (commands from CU, commands from RE, commands from gRPC) in RT loop
- CU binary startup connecting `CycleRunner` to SHM
- CU reading `di_bank` and `ai_values` from HAL feedback
- Watchdog process management (spawn, monitor, restart, shutdown, SHM cleanup, `WatchdogTrait`)
- Ordered startup (HAL → CU) and reverse shutdown (CU → HAL)
- Per-axis configuration files (`config/axis_NN_*.toml`) with auto-discovery by `ConfigLoader`
- Flat config directory (no subdirectories) with `config.toml` (system/program params), `machine.toml` (machine params), `io.toml`, and axis files side by side
- `IoRegistry` building from `io.toml` in both HAL and CU
- Removal of entire `evo_shared_memory` crate
- Deletion of dead code files and unused public methods (HAL simulation)
- Dependency cleanup and `[workspace.dependencies]` centralization
- Constant and type deduplication (`MAX_AXES`, `AnalogCurve`, `EVO_SHM_MAGIC`)
- P2P skeleton initialization in all stub programs
- Bit-packed I/O helper functions in `evo_common`
- Fix: alias rename (`evo` → `evo_common`), empty `rt` feature flag, prelude cleanup
- Fix: HAL `machine.toml` invalid fields, MQT truncation, driver registry refactor
- Short RT stability test for CI

### Out of Scope

- CU control logic implementation (PID, DOB, filters, state machines) — deferred to spec 005 Phase C
- Safety evaluation logic (SafeStopExecutor, RecoveryManager) — deferred to spec 005 Phase C
- Command arbitration and source locking (SourceLockTable, MachineStateMachine) — deferred to spec 005 Phase C
- Processing RE/RPC commands beyond attachment (audit G2, G3) — deferred to spec 005 Phase C
- Control output calculation (audit G4, G5, G6) — deferred to spec 005 Phase C
- Safety flags evaluation (audit G7, G16) — deferred to spec 005 Phase C
- CU→RE acknowledgment logic (audit G13) — deferred to RE spec
- HomingSupervisor per-axis — deferred to spec 005 Phase C
- Hot-reload configuration — spec 005 FR-144 through FR-147
- MQTT/gRPC/Recipe Executor application logic — future specs
- RT kernel setup, CPU affinity, memory locking — future operational spec
- Watchdog RT thread management (chrt/taskset, audit W9) — future spec (acknowledged in A-007)
- Digital Twin / visualization — evo_dashboard spec
- Functional safety certification — hardware safety system
- Hot-swap logic scripts — evo_recipe_executor spec

---

## Audit Resolution Matrix

This section maps **every issue** from `docs/audit.md` to a spec 006 FR or an explicit deferral. After spec 006 is completed, `audit.md` SHOULD be archived — all items will be resolved or tracked in downstream specs.

### §1 Blockers (4 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 1.1 | HAL nie pisze do SHM | FR-030, FR-031, FR-032, FR-033, FR-035 | ✅ Resolved |
| 1.2 | CU binary nie uruchamia pętli RT | FR-040 | ✅ Resolved |
| 1.3 | HAL i CU niezgodne formaty SHM | FR-010, FR-011, FR-012, FR-015 | ✅ Resolved |
| 1.4 | Watchdog nie uruchamia procesów | FR-020 through FR-028 | ✅ Resolved |

### §2 Type Conflicts (7 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 2.1 | Dwa nagłówki SHM | FR-001, FR-002, FR-061 (remove evo_shared_memory) | ✅ Resolved |
| 2.2 | Duplikat AnalogCurve | FR-081 | ✅ Resolved |
| 2.3 | Potrójna MAX_AXES | FR-080 | ✅ Resolved |
| 2.4 | Dual IO config | FR-034, FR-051, FR-052 | ✅ Resolved |
| 2.5 | Dual HalCommands/HalStatus vs P2P | FR-035 (conversions) | ✅ Resolved |
| 2.6 | EVO_SHM_MAGIC bez deprecated | FR-082 | ✅ Resolved |
| 2.7 | Legacy JSON-over-SHM | FR-062 | ✅ Resolved |

### §3 Dead Code (~20 symbols)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 3.1 | main_old.rs, shm/config.rs | FR-060 | ✅ Resolved |
| 3.2 | Dead exports in evo_shared_memory | FR-061 (whole crate removed) | ✅ Resolved |
| 3.3 | evo_shared_memory::data::* legacy types | FR-062 | ✅ Resolved |
| 3.4 | HalShmData dead in evo_hal/src/shm.rs | FR-060 | ✅ Resolved |
| 3.5 | Unused public methods in HAL simulation | FR-064 | ✅ Resolved |
| 3.6 | Unused Cargo.toml dependencies | FR-072 | ✅ Resolved |

### §4 RT-safety in evo_shared_memory (7 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 4.1 | Global Mutex in hot path | FR-003, FR-061 (remove crate) | ✅ Resolved |
| 4.2 | Heap allocation in read() | FR-003, FR-061 | ✅ Resolved |
| 4.3 | SystemTime::now() in write() | FR-003, FR-061 | ✅ Resolved |
| 4.4 | sched_yield() in retry loop | FR-003, FR-061 | ✅ Resolved |
| 4.5 | Reader O_RDWR (should be O_RDONLY) | FR-061 | ✅ Resolved |
| 4.6 | .meta files block restart | FR-061 | ✅ Resolved |
| 4.7 | SegmentDiscovery bad size | FR-061 | ✅ Resolved |

### §5 Missing Bridges (7 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 5.1 | No HalStatus → HalToCuSegment | FR-035 | ✅ Resolved |
| 5.2 | No CuToHalSegment → HalCommands | FR-035 | ✅ Resolved |
| 5.3 | HAL nie parsuje io.toml | FR-034, FR-051 | ✅ Resolved |
| 5.4 | TypedP2p only in CU | FR-001 (move to evo_common) | ✅ Resolved |
| 5.5 | CycleRunner missing runtime state | FR-041, FR-042 (IoRegistry, AxisControlState). Remaining: SafeStopExecutor, RecoveryManager, MachineStateMachine, SourceLockTable, HomingSupervisor | ⏳ Partial — control-logic items deferred to spec 005 Phase C |
| 5.6 | di_bank/ai_values ignored | FR-042 | ✅ Resolved |
| 5.7 | RE/gRPC/MQTT no P2P | FR-090, FR-091 | ✅ Resolved |

### §6 Dependency Problems (7 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 6.1 | heapless 0.8 vs 0.9 | FR-071 | ✅ Resolved |
| 6.2 | nix 0.29 vs 0.30 | FR-071 | ✅ Resolved |
| 6.3 | criterion 0.5 vs 0.8 | FR-071 | ✅ Resolved |
| 6.4 | No workspace.dependencies | FR-070 | ✅ Resolved |
| 6.5 | Misleading alias evo=evo_common | FR-074 | ✅ Resolved |
| 6.6 | log vs tracing | FR-073 | ✅ Resolved |
| 6.7 | Empty rt feature flag | FR-075 | ✅ Resolved |

### §7 CycleRunner Gaps (16 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| G1 | main.rs nie instancjuje CycleRunner | FR-040 | ✅ Resolved |
| G2 | RE commands discarded | FR-044 (attach). Command processing | ⏳ Deferred to spec 005 Phase C (command arbitration) |
| G3 | RPC commands discarded | FR-044 (attach). Command processing | ⏳ Deferred to spec 005 Phase C (command arbitration) |
| G4 | control_output always [0,0,0,0] | Control pipeline | ⏳ Deferred to spec 005 Phase C |
| G5 | target_position never set | Control pipeline | ⏳ Deferred to spec 005 Phase C |
| G6 | No state machine ticks | State machine wiring | ⏳ Deferred to spec 005 Phase C |
| G7 | No safety evaluation | Safety logic | ⏳ Deferred to spec 005 Phase C |
| G8 | try_attach_re/try_attach_rpc never called | FR-044 | ✅ Resolved |
| G9 | MQT error flags truncated as u16/u8 | FR-043 | ✅ Resolved |
| G10 | di_bank/ai_values ignored | FR-042 | ✅ Resolved |
| G11 | No AxisControlState[] | FR-041 | ✅ Resolved |
| G12 | No IoRegistry on CycleRunner | FR-041 | ✅ Resolved |
| G13 | CU→RE ack always zero | RE application logic | ⏳ Deferred to RE spec |
| G14 | P2P write_seq written but never read | FR-002 (read protocol uses write_seq for torn-read detection) | ✅ Resolved |
| G15 | HalToCuSegment no torque field | FR-011 (adds torque_estimate) | ✅ Resolved |
| G16 | safety_flags always 0xFF placeholder | Safety evaluation | ⏳ Deferred to spec 005 Phase C |

### §8 Watchdog Gaps (9 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| W1 | No std::process::Command | FR-020 | ✅ Resolved |
| W2 | No waitpid/pidfd | FR-021 | ✅ Resolved |
| W3 | No restart logic | FR-022 | ✅ Resolved |
| W4 | No WatchdogTrait impl | FR-027 | ✅ Resolved |
| W5 | No shm_unlink for orphans | FR-024 | ✅ Resolved |
| W6 | No P2P awareness | FR-024, FR-028 | ✅ Resolved |
| W7 | No ordered startup | FR-020 | ✅ Resolved |
| W8 | No shutdown propagation | FR-023 | ✅ Resolved |
| W9 | No RT thread management | Assumption A-007 | ⏳ Deferred to future operational spec |

### §9 Code Quality (9 items)

| Audit # | Issue | Resolution | FR |
| :--- | :--- | :--- | :--- |
| 9.1 | Bad machine.toml fields | FR-059 | ✅ Resolved |
| 9.2 | Unused prelude | FR-076 | ✅ Resolved |
| 9.3 | Driver registry global state | FR-077 | ✅ Resolved |
| 9.4 | SegmentReader O_RDWR | FR-061 (remove crate) | ✅ Resolved |
| 9.5 | repr(C) with String | FR-062, FR-061 | ✅ Resolved |
| 9.6 | No #[deprecated] on EVO_SHM_MAGIC | FR-082 | ✅ Resolved |
| 9.7 | 4 crates = empty stubs | FR-090 (add P2P skeleton) | ✅ Resolved |
| 9.8 | Disabled soak test, no CI alt. | FR-078 | ✅ Resolved |
| 9.9 | Stubs not ready for P2P | FR-090, FR-091 | ✅ Resolved |

### §10 Suggested Fix Order

The audit's 6-phase plan (A–F) is superseded by this spec's FR numbering. The spec implementation order follows the FR groups naturally (P2P lib → segments → watchdog → HAL → CU → config → cleanup → stubs).

### §11 Decision: Remove evo_shared_memory

FR-061 through FR-063. ✅ Fully adopted.

### Summary

| Category | Total Items | ✅ Resolved in 006 | ⏳ Explicitly Deferred |
| :--- | :--- | :--- | :--- |
| §1 Blockers | 4 | 4 | 0 |
| §2 Type conflicts | 7 | 7 | 0 |
| §3 Dead code | 6 | 6 | 0 |
| §4 RT-safety | 7 | 7 | 0 |
| §5 Missing bridges | 7 | 6 | 1 (partial: 5.5 control-logic items) |
| §6 Dependencies | 7 | 7 | 0 |
| §7 CycleRunner | 16 | 9 | 7 (G2–G7, G13, G16 — control logic) |
| §8 Watchdog | 9 | 8 | 1 (W9 — RT thread mgmt) |
| §9 Code quality | 9 | 9 | 0 |
| §10 Fix order | — | Superseded | — |
| §11 Decision | 1 | 1 | 0 |
| **TOTAL** | **~69** | **~60** | **~9 (all explicitly deferred with target spec)** |

All 9 deferred items are **control-logic or RT-operational concerns** that belong in downstream specs (005 Phase C or operational spec). Zero items are lost or untracked.
