# Data Model: Control Unit — Axis Control Brain

**Date**: 2026-02-08 | **Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

> All types defined here live in `evo_common::control_unit` (FR-140) unless noted.
> All SHM structs use `#[repr(C, align(64))]` for zero-copy binary access (Research Topic 6).
> All enums use `#[repr(u8)]` for compact memory layout (Research Topic 9).

---

## LEVEL 1: Global State

### MachineState (FR-001)

```text
enum MachineState : u8 {
    Stopped       = 0,  // Initial state after boot
    Starting      = 1,  // System initialization
    Idle          = 2,  // Ready, no active motion
    Manual        = 3,  // Manual operation
    Active        = 4,  // Recipe/program running
    Service       = 5,  // Service mode
    SystemError   = 6,  // Critical fault
}
```

**Transitions** (FR-002):
```text
Stopped → Starting         // power-on
Starting → Idle            // init complete
Idle ↔ Manual              // first/last manual command
Idle|Manual → Active       // program start
Active → Idle|Manual       // program complete/stop
any → Service              // service mode (authorized)
any → SystemError          // critical fault or SAFETY_STOP
```

**Validation**: Service requires authorization flag. SystemError only exits via explicit reset sequence (FR-122).

---

### SafetyState (FR-010)

```text
enum SafetyState : u8 {
    Safe             = 0,  // All safety OK
    SafeReducedSpeed = 1,  // Hardware speed limit active
    SafetyStop       = 2,  // Emergency — per-axis safe stop
}
```

**Relationships**: Overrides MachineState behavior (FR-011). SafetyStop forces MachineState → SystemError.

---

## LEVEL 2: Per-Axis Safe Stop

### SafeStopCategory (FR-013)

```text
enum SafeStopCategory : u8 {
    STO = 0,  // Immediate power cut
    SS1 = 1,  // Controlled decel → power cut (DEFAULT)
    SS2 = 2,  // Controlled decel → hold position
}
```

### SafeStopConfig (FR-015)

```text
struct SafeStopConfig {
    category:          SafeStopCategory,  // default: SS1
    max_decel_safe:    f64,               // [mm/s²] safe deceleration
    sto_brake_delay:   f64,               // [s] delay before brake after STO
    ss2_holding_torque: f64,              // [%] holding torque for SS2 (default: 20.0)
}
```

---

## LEVEL 3: Per-Axis State (Orthogonal)

### AxisId (FR-143)

```text
type AxisId = u8;  // 1-based (1..=64), maps to array index `id - 1`
```

### AxisState (container)

```text
struct AxisState {
    axis_id:          AxisId,           // 1-based
    power:            PowerState,       // 1 byte
    motion:           MotionState,      // 1 byte
    operational:      OperationalMode,  // 1 byte
    coupling:         CouplingState,    // 1 byte
    gearbox:          GearboxState,     // 1 byte
    loading:          LoadingState,     // 1 byte
    safety_flags:     AxisSafetyState,  // 8 bytes (8 bools)
    error:            AxisErrorState,   // variable — bitflags per category
    control_state:    AxisControlState, // PID integral, filter state, DOB state
    source_lock:      AxisSourceLock,   // command ownership tracking
    referenced:       bool,            // from HAL via evo_hal_cu
}
```

**Memory**: ~260 bytes per axis. 64 axes = ~16.6 KB (fits L1 cache).

---

### PowerState (FR-020)

```text
enum PowerState : u8 {
    PowerOff     = 0,
    PoweringOn   = 1,
    Standby      = 2,
    Motion       = 3,
    PoweringOff  = 4,
    NoBrake      = 5,  // Service: drive OFF, brake released
    PowerError   = 6,
}
```

**State data** (for POWERING_ON/OFF sequence tracking):
```text
struct PowerSequenceState {
    step:              u8,      // current step in sequence (0-10)
    step_timer:        f64,     // [s] time spent in current step
    holding_timer:     f64,     // [s] for gravity-affected axis position check
}
```

---

### MotionState (FR-030)

```text
enum MotionState : u8 {
    Standstill       = 0,
    Accelerating     = 1,
    ConstantVelocity = 2,
    Decelerating     = 3,
    Stopping         = 4,
    EmergencyStop    = 5,
    Homing           = 6,
    GearAssistMotion = 7,
    MotionError      = 8,
}
```

---

### OperationalMode (FR-040)

```text
enum OperationalMode : u8 {
    Position = 0,
    Velocity = 1,
    Torque   = 2,
    Manual   = 3,
    Test     = 4,
}
```

**Constraint**: Blocked when axis is SLAVE_COUPLED or SLAVE_MODULATED (FR-042).

---

### CouplingState (FR-050)

```text
enum CouplingState : u8 {
    Uncoupled      = 0,
    Master         = 1,
    SlaveCoupled   = 2,
    SlaveModulated = 3,
    WaitingSync    = 4,
    Synchronized   = 5,
    SyncLost       = 6,
    Coupling       = 7,
    Decoupling     = 8,
}
```

**Relationships**:
```text
struct CouplingConfig {
    master_axis:       Option<AxisId>,       // None if UNCOUPLED or MASTER
    slave_axes:        heapless::Vec<AxisId, 8>,  // direct slaves (MASTER only)
    coupling_ratio:    f64,                  // slave: target = master * ratio
    modulation_offset: f64,                  // slave: + offset (MODULATED only)
    sync_timeout:      f64,                  // [s] max wait in WAITING_SYNC
    max_lag_diff:      f64,                  // [mm] master-slave lag difference limit
}
```

---

### GearboxState (FR-060)

```text
enum GearboxState : u8 {
    NoGearbox    = 0,
    Gear1        = 1,
    Gear2        = 2,
    Gear3        = 3,
    Gear4        = 4,
    // ... up to GearN
    Neutral      = 250,
    Shifting     = 251,
    GearboxError = 252,
    Unknown      = 253,
}
```

---

### LoadingState (FR-070)

```text
enum LoadingState : u8 {
    Production           = 0,
    ReadyForLoading      = 1,
    LoadingBlocked       = 2,
    LoadingManualAllowed = 3,
}
```

---

## LEVEL 4: Axis Safety Flags

### AxisSafetyState (FR-080)

```text
struct AxisSafetyState {
    tailstock_ok:    bool,
    lock_pin_ok:     bool,
    brake_ok:        bool,
    guard_ok:        bool,
    limit_switch_ok: bool,
    soft_limit_ok:   bool,
    motion_enable_ok: bool,
    gearbox_ok:      bool,
}
```

**Invariant**: Motion blocked when ANY flag is `false` (FR-081).

---

## LEVEL 5: Error Flags

### AxisErrorState (FR-090)

```text
struct AxisErrorState {
    power:    PowerError,     // bitflags
    motion:   MotionError,    // bitflags
    command:  CommandError,   // bitflags
    gearbox:  GearboxError,  // bitflags
    coupling: CouplingError, // bitflags
}
```

### PowerError (bitflags)

```text
bitflags PowerError : u16 {
    BRAKE_TIMEOUT        = 0x0001,
    LOCK_PIN_TIMEOUT     = 0x0002,
    DRIVE_FAULT          = 0x0004,
    DRIVE_NOT_READY      = 0x0008,
    MOTION_ENABLE_LOST   = 0x0010,
    DRIVE_TAIL_OPEN      = 0x0020,  // CRITICAL → SAFETY_STOP
    DRIVE_LOCK_PIN_LOCKED= 0x0040,  // CRITICAL → SAFETY_STOP
    DRIVE_BRAKE_LOCKED   = 0x0080,  // CRITICAL → SAFETY_STOP
}
```

### MotionError (bitflags)

```text
bitflags MotionError : u16 {
    LAG_EXCEED           = 0x0001,
    LAG_CRITICAL         = 0x0002,  // CRITICAL → SAFETY_STOP (if axis.lag_policy == Critical)
    HARD_LIMIT           = 0x0004,
    SOFT_LIMIT           = 0x0008,
    OVERSPEED            = 0x0010,
    ACCELERATION_LIMIT   = 0x0020,
    HOMING_FAILED        = 0x0040,
    COLLISION_DETECTED   = 0x0080,
    ENCODER_FAULT        = 0x0100,
    DRIVE_ZEROSPEED      = 0x0200,  // CRITICAL → SAFETY_STOP
    CYCLE_OVERRUN        = 0x0400,  // CRITICAL → SAFETY_STOP
    NOT_REFERENCED       = 0x0800,
}
```

### CommandError (bitflags)

```text
bitflags CommandError : u8 {
    SOURCE_LOCKED        = 0x01,
    SOURCE_NOT_AUTHORIZED= 0x02,
    SOURCE_TIMEOUT       = 0x04,   // heartbeat stale (FR-130c)
}
```

### GearboxError (bitflags)

```text
bitflags GearboxError : u8 {
    GEAR_TIMEOUT         = 0x01,
    GEAR_SENSOR_CONFLICT = 0x02,
    NO_GEARSTEP          = 0x04,  // CRITICAL → SAFETY_STOP
    GEAR_CHANGE_DENIED   = 0x08,
}
```

### CouplingError (bitflags)

```text
bitflags CouplingError : u8 {
    SYNC_TIMEOUT         = 0x01,
    SLAVE_FAULT          = 0x02,
    MASTER_LOST          = 0x04,
    LAG_DIFF_EXCEED      = 0x08,  // CRITICAL → SAFETY_STOP
}
```

**Propagation** (FR-091, FR-092):
- Non-critical: axis-local only
- CRITICAL (marked above): trigger global SafetyState::SafetyStop

---

## Control Engine State

### AxisControlState

```text
struct AxisControlState {
    // PID state
    integral_accumulator: f64,    // Ki integration sum
    prev_error:           f64,    // for derivative calculation
    derivative_filtered:  f64,    // filtered D term (Tf)

    // DOB state
    dob_prev_velocity:    f64,
    dob_prev_disturbance: f64,
    dob_prev_accel_est:   f64,

    // Filter state
    notch_w1:             f64,    // biquad state variable 1
    notch_w2:             f64,    // biquad state variable 2
    lp_prev_output:       f64,    // low-pass previous output

    // Lag monitoring
    current_lag:          f64,    // |target - actual|
}
```

**Memory**: 80 bytes per axis. Zeroed at startup. Reset on axis disable.

### UniversalControlParameters (FR-100)

```text
struct UniversalControlParameters {
    // PID
    kp:       f64,  // Proportional gain
    ki:       f64,  // Integral gain (0 = disabled)
    kd:       f64,  // Derivative gain (0 = disabled)
    tf:       f64,  // Derivative filter time constant
    tt:       f64,  // Anti-windup tracking time constant

    // Feedforward
    kvff:     f64,  // Velocity feedforward (0 = disabled)
    kaff:     f64,  // Acceleration feedforward (0 = disabled)
    friction: f64,  // Static friction offset (0 = disabled)

    // DOB
    jn:       f64,  // Nominal inertia
    bn:       f64,  // Nominal damping
    gdob:     f64,  // Observer bandwidth (0 = disabled)

    // Filters
    f_notch:  f64,  // Notch frequency Hz (0 = disabled)
    bw_notch: f64,  // Notch bandwidth Hz
    flp:      f64,  // Low-pass cutoff Hz (0 = disabled)
    out_max:  f64,  // Output saturation limit

    // Lag monitoring
    lag_error_limit: f64,     // [mm] max allowed lag
    lag_policy:      LagPolicy, // behavior when lag_error_limit exceeded
}
```

### LagPolicy

```text
#[repr(u8)]
enum LagPolicy {
    Critical  = 0,  // global SAFETY_STOP for ALL axes (e.g., spindle, coupled axes)
    Unwanted  = 1,  // axis-local stop only, axis → MotionState::MOTION_ERROR (DEFAULT)
    Neutral   = 2,  // operator info only — set ERR_LAG_EXCEED flag, no stop
    Desired   = 3,  // expected behavior (e.g., friction axis) — suppress error flag entirely
}
```

### ControlOutputVector (FR-105)

```text
#[repr(C)]
struct ControlOutputVector {
    calculated_torque: f64,  // [Nm] PID + FF + DOB total
    target_velocity:   f64,  // [mm/s] commanded velocity
    target_position:   f64,  // [mm] commanded position
    torque_offset:     f64,  // [Nm] feedforward-only component
}
```

**Invariant**: All 4 fields always calculated. HAL selects by drive mode (FR-132a).

---

## Command & Source Locking

### CommandSource

```text
enum CommandSource : u8 {
    None            = 0,
    RecipeExecutor  = 1,  // via evo_re_cu
    GrpcApi         = 2,  // via evo_rpc_cu
    Safety          = 3,  // internal safety override
}
```

### LockReason (FR-135)

```text
enum LockReason : u8 {
    RecipeRunning    = 0,  // Recipe Executor is executing a program on this axis
    ManualControl    = 1,  // Operator has manual control via gRPC/HMI
    HomingInProgress = 2,  // Homing sequence active (FR-030)
    ServiceMode      = 3,  // Axis in SERVICE operational mode
    SafetyPause      = 4,  // Safety system paused motion (SAFETY_STOP active)
    GearAssist       = 5,  // Gear change assistance in progress (FR-062)
}
```

### AxisSourceLock (FR-135)

```text
struct AxisSourceLock {
    locked_source:  CommandSource,         // who holds the lock
    lock_reason:    LockReason,            // codified reason — text mapping in downstream consumer (e.g., evo_mqtt)
    pre_pause_targets: Option<PauseTargets>, // preserved on SAFETY_STOP
}

struct PauseTargets {
    target_position: f64,
    target_velocity: f64,
    operational_mode: OperationalMode,
}
```

### ServiceBypassConfig (FR-001a)

```text
struct ServiceBypassConfig {
    bypass_axes: heapless::Vec<AxisId, 64>,  // axes allowed to operate in SERVICE mode
    max_service_velocity: f64,               // [mm/s] SAFE_REDUCED_SPEED hardware limit
}
```

Per-axis SERVICE bypass list: only axes in `bypass_axes` may be operated during SERVICE mode. All other axes remain locked. Configured in `CuMachineConfig`.

---

## Homing

### HomingMethod (FR-032)

```text
enum HomingMethod : u8 {
    HardStop    = 0,
    HomeSensor  = 1,
    LimitSwitch = 2,
    IndexPulse  = 3,
    Absolute    = 4,
    NoHoming    = 5,
}
```

### HomingDirection (FR-033a)

```text
enum HomingDirection : u8 {
    Positive = 0,   // approach in +direction
    Negative = 1,   // approach in -direction
}
```

Mandatory for all methods except `NoHoming` and `Absolute`. Determines initial travel direction during homing approach phase. Safety-critical: prevents wrong-direction homing into mechanical stops.

### HomingConfig (FR-033)

```text
struct HomingConfig {
    method:            HomingMethod,
    speed:             f64,          // [mm/s] homing speed
    torque_limit:      f64,          // [%] max torque during homing
    timeout:           f64,          // [s] per method
    // Method-specific:
    current_threshold: f64,          // HARD_STOP only
    approach_direction: HomingDirection, // mandatory for HardStop/HomeSensor/LimitSwitch/IndexPulse (FR-033a)
    sensor_role:       IoRole,       // HOME_SENSOR, INDEX_PULSE — resolved from io.toml (FR-149)
    index_role:        IoRole,       // INDEX_PULSE only — resolved from io.toml (FR-149)
    sensor_nc:         bool,         // NC/NO config for sensor
    limit_direction:   i8,           // LIMIT_SWITCH: +1 (high) or -1 (low)
    zero_offset:       f64,          // ABSOLUTE only
}
```

---

## Safety Peripherals

### TailstockConfig (FR-082)

```text
enum TailstockType : u8 {
    None     = 0,  // Type 0: no tailstock
    Standard = 1,  // Type 1: standard with sensors
    Sliding  = 2,  // Type 2: with clamp
    Combined = 3,  // Type 3: type 1+2
    Auto     = 4,  // Type 4: automatic clamp
}

struct TailstockConfig {
    tailstock_type:    TailstockType,
    di_closed:         IoRole,        // e.g., IoRole::TailClosedN — resolved from io.toml
    closed_nc:         bool,
    di_open:           IoRole,        // e.g., IoRole::TailOpenN
    di_clamp_locked:   IoRole,        // Type 2-4 only, e.g., IoRole::TailClampN
}
```

### IndexConfig (FR-083)

```text
struct IndexConfig {
    di_locked:       IoRole,          // e.g., IoRole::IndexLockedN
    di_middle:       Option<IoRole>,  // optional, e.g., IoRole::IndexMiddleN
    di_free:         IoRole,          // e.g., IoRole::IndexFreeN
    retract_timeout: f64,  // [s]
    insert_timeout:  f64,  // [s]
}
```

### BrakeConfig (FR-084)

```text
struct BrakeConfig {
    do_brake:        IoRole,          // output command, e.g., IoRole::BrakeOutN
    di_released:     IoRole,          // confirmation input, e.g., IoRole::BrakeInN
    release_timeout: f64,                   // [s]
    engage_timeout:  f64,                   // [s]
    always_free:     bool,                  // some axes don't need holding
    inverted:        bool,                  // output polarity (also configurable in io.toml)
}
```

### GuardConfig (FR-085)

```text
struct GuardConfig {
    di_closed:     IoRole,           // e.g., IoRole::GuardClosedN
    di_locked:     IoRole,           // e.g., IoRole::GuardLockedN
    secure_speed:  f64,    // [mm/s] below this, guard can open
    open_delay:    f64,    // [s] speed must be below secure_speed for this long (default: 2.0)
}
```

### GearAssistConfig (FR-062)

```text
struct GearAssistConfig {
    assist_amplitude:  f64,    // [mm] oscillation amplitude during gear shift
    assist_frequency:  f64,    // [Hz] oscillation frequency
    assist_timeout:    f64,    // [s] max time for gear assist motion
    max_attempts:      u8,     // max oscillation attempts before GearboxError
}
```

---

## I/O Configuration (FR-148–FR-155)

> Defined in `evo_common::io` — shared between HAL and CU.
> Parsed from `io.toml` at startup. Runtime access via `IoRegistry` role-based lookup.

### IoRole (FR-149, FR-151)

```text
enum IoRole {
    // Safety (global)
    EStop,
    SafetyGate,
    EStopReset,

    // Control (global)
    Start,
    Stop,
    Reset,
    Pause,

    // Per-axis DI (convention: FunctionAxisNumber)
    LimitMin(u8),       // e.g., LimitMin(1) = lower limit switch axis 1
    LimitMax(u8),       // e.g., LimitMax(2) = upper limit switch axis 2
    Ref(u8),            // e.g., Ref(1) = homing sensor axis 1
    Enable(u8),         // e.g., Enable(3) = motion enable axis 3

    // Per-axis peripherals
    TailClosed(u8),     // tailstock closed confirmation
    TailOpen(u8),       // tailstock open confirmation
    TailClamp(u8),      // tailstock clamp locked (Type 2-4)
    IndexLocked(u8),    // locking pin locked
    IndexMiddle(u8),    // locking pin middle position
    IndexFree(u8),      // locking pin free
    BrakeIn(u8),        // brake release confirmation (DI)
    BrakeOut(u8),       // brake command (DO)
    GuardClosed(u8),    // safety guard closed
    GuardLocked(u8),    // safety guard locked

    // Pneumatics / general
    PressureOk,
    VacuumOk,

    // Project-specific extension
    Custom(heapless::String<32>),
}
```

**Serialization**: In `io.toml`, roles are strings following `FunctionAxisNumber` convention:
- `"EStop"`, `"LimitMin1"`, `"BrakeOut3"`, `"Ref2"`, `"PressureOk"`
- Deserialized via `impl FromStr for IoRole` with axis number extraction.

### IoPointType

```text
enum IoPointType : u8 {
    Di = 0,  // Digital Input
    Do = 1,  // Digital Output
    Ai = 2,  // Analog Input
    Ao = 3,  // Analog Output
}
```

### IoPoint (FR-153)

```text
struct IoPoint {
    io_type:    IoPointType,
    pin:        u16,                    // physical pin number
    role:       Option<IoRole>,         // functional role (None if unnamed utility I/O)
    name:       Option<String>,         // display label for operator

    // DI-specific
    logic:      DiLogic,                // NO or NC (default: NO)
    debounce:   u16,                    // [ms] contact bounce filter (default: 15)
    enable_pin: Option<u16>,            // conditional enable input pin
    enable_state: bool,                 // required enable state
    enable_timeout: u32,                // [ms] max time between signals (0=none)

    // DO-specific
    init:       bool,                   // initial logical state (default: false)
    inverted:   bool,                   // invert logic-to-pin (default: false)
    pulse:      u32,                    // [ms] watchdog auto-OFF (0=none)
    keep_estop: bool,                   // do NOT reset on E-Stop (default: false)

    // AI-specific
    min:        f64,                    // engineering range minimum (default: 0.0)
    max:        f64,                    // engineering range maximum (REQUIRED for AI/AO)
    unit:       String,                 // unit of measure (default: "V")
    average:    u16,                    // moving average samples (1-1000, default: 5)
    curve:      AnalogCurve,            // scaling shape (default: Linear)
    offset:     f64,                    // output offset after curve (default: 0.0)

    // AO-specific: shares min, max, unit, curve, offset with AI
    // AO adds: init (f64), pulse (watchdog)

    // Simulation
    sim:        Option<f64>,            // simulation value (di/do: 0.0/1.0, ai/ao: float)
}

enum DiLogic : u8 {
    NO = 0,  // Normally Open (default)
    NC = 1,  // Normally Closed
}
```

**Note**: Not all fields apply to every `IoPointType`. Unused fields default to zero/false/None. The runtime `IoRegistry` validates type-correctness (FR-150).

### IoGroup (FR-154)

```text
struct IoGroup {
    key:   String,                  // TOML section key (e.g., "Safety", "Pneumatics")
    name:  Option<String>,          // display name for operator
    io:    Vec<IoPoint>,            // all I/O points in this group
}
```

### IoConfig (FR-148)

```text
struct IoConfig {
    groups: Vec<IoGroup>,           // all groups from io.toml
}
```

### IoBinding (runtime, FR-150)

```text
struct IoBinding {
    group_key:  String,             // which group this point belongs to
    point_idx:  usize,              // index within group.io[]
    io_type:    IoPointType,
    pin:        u16,
    logic:      DiLogic,            // for DI/DO: NC/NO interpretation
    curve:      AnalogCurve,        // for AI/AO: scaling
    offset:     f64,                // for AI/AO: offset after curve
    min:        f64,                // for AI/AO: engineering min
    max:        f64,                // for AI/AO: engineering max
}
```

### IoRegistry (runtime, FR-152)

```text
struct IoRegistry {
    bindings: HashMap<IoRole, IoBinding>,  // role → binding (built at startup)
    di_count: u16,                         // total DI count
    do_count: u16,                         // total DO count
    ai_count: u16,                         // total AI count
    ao_count: u16,                         // total AO count
}

impl IoRegistry {
    fn from_config(config: &IoConfig) -> Result<Self, IoConfigError>;
    fn read_di(&self, role: IoRole, di_bank: &[u64; 16]) -> bool;       // applies NC/NO
    fn read_ai(&self, role: IoRole, ai_values: &[f64; 64]) -> f64;      // applies scaling
    fn write_do(&self, role: IoRole, value: bool, do_bank: &mut [u64; 16]);
    fn write_ao(&self, role: IoRole, value: f64, ao_values: &mut [f64; 64]);
    fn validate_roles_for_axis(&self, axis: &CuAxisConfig) -> Result<(), IoConfigError>;
}
```

**Note**: `IoRegistry` is built at startup (heap allocation allowed). Runtime `read_di`/`read_ai`/`write_do`/`write_ao` are `O(1)` HashMap lookup — no heap allocation. The HashMap is pre-built and immutable after startup.

---

## Configuration

### ControlUnitConfig (FR-141, FR-142)

```text
struct ControlUnitConfig {
    cycle_time_us:       u32,           // target cycle time in µs (default: 1000)
    max_axes:            u8,            // max axis count (default: 64)
    machine_config_path: String,        // path to machine TOML
    io_config_path:      String,        // path to io.toml (FR-148)
    manual_timeout:      f64,           // [s] Manual→Idle timeout
    hal_stale_threshold: u32,           // RT staleness N cycles (default: 3)
    re_stale_threshold:  u32,           // RE staleness N cycles (default: 1000)
    rpc_stale_threshold: u32,           // RPC staleness N cycles (default: 1000)
    mqt_update_interval: u32,           // diagnostic write every N cycles (default: 10)
}

struct CuMachineConfig {
    axes: Vec<CuAxisConfig>,            // loaded at startup, fixed after STARTING
    global_safety: GlobalSafetyConfig,
}

struct CuAxisConfig {
    axis_id:     AxisId,
    name:        String,                // human-readable ("Spindle", "X-Axis")
    max_velocity: f64,                  // [user units/s] maximum axis velocity (used for 5% unreferenced limit, FR-035)
    safe_reduced_speed_limit: f64,      // [user units/s] velocity limit during SAFE_REDUCED_SPEED (FR-011); mm/s for linear, rpm for rotary
    control:     UniversalControlParameters,
    safe_stop:   SafeStopConfig,
    homing:      HomingConfig,
    tailstock:   Option<TailstockConfig>,
    index:       Option<IndexConfig>,
    brake:       Option<BrakeConfig>,
    guard:       Option<GuardConfig>,
    coupling:    Option<CouplingConfig>,
    gear_assist: Option<GearAssistConfig>,  // gear shifting oscillation params (FR-062)
    motion_enable_input: Option<IoRole>,      // DI role for motion enable signal (FR-021), e.g., IoRole::EnableN
    loading_blocked: bool,
    loading_manual:  bool,
    // NC/NO logic for limit switches, brake, etc. is now per-point in io.toml (FR-153), not per-axis.
    // Removed: end_switch_nc (replaced by DiLogic on each IoPoint).
}
```

### GlobalSafetyConfig

```text
struct GlobalSafetyConfig {
    default_safe_stop:    SafeStopCategory,  // fallback category if axis has no explicit config (default: SS1)
    safety_stop_timeout:  f64,               // [s] max time for all axes to complete safe stop (default: 5.0)
    recovery_authorization_required: bool,   // true → manual authorization needed after reset (FR-122, default: true)
}
```

**Note**: `String` and `Vec` used in config structs only — loaded at startup, never in RT loop.

**Naming**: `CuMachineConfig` and `CuAxisConfig` are prefixed with `Cu` to avoid collision with `evo_common::hal::config::MachineConfig` and `evo_common::hal::config::AxisConfig` which define HAL-level machine and axis configurations. Both type families live in `evo_common` but in separate submodules (`control_unit::config` vs `hal::config`).

---

## P2P SHM Segment Structs

> Defined in `evo_common::shm::p2p` (header) and `evo_common::control_unit::shm` (payloads).

### Segment Header (FR-130c, FR-130d)

```text
#[repr(C, align(64))]
struct P2pSegmentHeader {
    magic:          [u8; 8],     // "EVO_P2P\0"
    version_hash:   u32,         // struct layout hash (FR-130d)
    heartbeat:      u64,         // monotonic counter (FR-130c)
    source_module:  u8,          // ModuleAbbrev enum
    dest_module:    u8,          // ModuleAbbrev enum
    payload_size:   u32,         // bytes after header
    write_seq:      u32,         // odd=writing, even=committed (lock-free protocol)
    _padding:       [u8; 34],    // align to 64 bytes
}
```

### ModuleAbbrev (FR-130b)

```text
enum ModuleAbbrev : u8 {
    Cu  = 0,
    Hal = 1,
    Re  = 2,
    Mqt = 3,
    Rpc = 4,
}
```

### evo_hal_cu (FR-131)

```text
#[repr(C, align(64))]
struct HalToCuSegment {
    header:      P2pSegmentHeader,
    axis_count:  u8,
    _pad:        [u8; 63],
    axes:        [HalAxisFeedback; 64],
    di_bank:     [u64; 16],              // 1024 DI bits
    ai_values:   [f64; 64],             // analog inputs
}

#[repr(C)]
struct HalAxisFeedback {
    actual_position: f64,     // [mm]
    actual_velocity: f64,     // [mm/s]
    drive_status:    u8,      // bitfield: ready|fault|enabled|referenced|zerospeed
    fault_code:      u16,     // drive-specific fault code
    _padding:        [u8; 5],
}
```

**drive_status bitfield**: bit 0=ready, bit 1=fault, bit 2=enabled, bit 3=referenced, bit 4=zerospeed.

**Memory**: 24 bytes per axis.

### evo_cu_hal (FR-132)

```text
#[repr(C, align(64))]
struct CuToHalSegment {
    header:      P2pSegmentHeader,
    axis_count:  u8,
    _pad:        [u8; 63],
    axes:        [CuAxisCommand; 64],
    do_bank:     [u64; 16],             // 1024 DO bits
    ao_values:   [f64; 64],            // analog outputs
}

#[repr(C)]
struct CuAxisCommand {
    output:       ControlOutputVector,  // 32 bytes
    enable:       u8,                   // 0=disable, 1=enable
    mode:         u8,                   // OperationalMode
    _padding:     [u8; 6],
}
```

**Memory**: 40 bytes per axis.

### evo_cu_mqt (FR-134)

```text
#[repr(C, align(64))]
struct CuToMqtSegment {
    header:        P2pSegmentHeader,
    machine_state: u8,              // MachineState
    safety_state:  u8,              // SafetyState
    axis_count:    u8,
    _pad:          [u8; 61],
    axes:          [AxisStateSnapshot; 64],
}

#[repr(C)]
struct AxisStateSnapshot {
    axis_id:        u8,              // AxisId (1-based)
    power:          u8,              // PowerState
    motion:         u8,              // MotionState
    operational:    u8,              // OperationalMode
    coupling:       u8,              // CouplingState
    gearbox:        u8,              // GearboxState
    loading:        u8,              // LoadingState
    locked_by:      u8,              // CommandSource
    safety_flags:   u8,              // 8 packed booleans (AxisSafetyState)
    error_power:    u16,             // PowerError bitflags
    error_motion:   u16,             // MotionError bitflags
    error_command:  u8,              // CommandError bitflags
    error_gearbox:  u8,              // GearboxError bitflags
    error_coupling: u8,              // CouplingError bitflags
    _pad:           [u8; 5],
    position:       f64,             // [mm] actual
    velocity:       f64,             // [mm/s] actual
    lag:            f64,             // [mm] |target - actual|
    torque:         f64,             // [Nm] current output
}
```

---

## Entity Relationship Summary

```text
ControlUnitConfig
  ├── io_config_path → IoConfig (io.toml)
  │     └── IoGroup [1..N]
  │           └── IoPoint [1..M]
  │                 └── role: Option<IoRole>
  └── CuMachineConfig
        └── CuAxisConfig [1..64]
              ├── UniversalControlParameters
              ├── SafeStopConfig
              ├── HomingConfig (sensor_role/index_role → IoRole)
              ├── CouplingConfig?
              ├── TailstockConfig? (di_closed/di_open/di_clamp → IoRole)
              ├── IndexConfig? (di_locked/di_middle/di_free → IoRole)
              ├── BrakeConfig? (do_brake/di_released → IoRole)
              ├── GuardConfig? (di_closed/di_locked → IoRole)
              ├── GearAssistConfig?
              └── motion_enable_input: Option<IoRole>

Runtime (startup):
  IoRegistry (built from IoConfig)
    └── bindings: HashMap<IoRole, IoBinding>

Runtime State (pre-allocated array):
  MachineState (global, 1)
  SafetyState (global, 1)
  AxisState [64]
    ├── PowerState + PowerSequenceState
    ├── MotionState
    ├── OperationalMode
    ├── CouplingState
    ├── GearboxState
    ├── LoadingState
    ├── AxisSafetyState (8 flags)
    ├── AxisErrorState (5 bitflag sets)
    ├── AxisControlState (PID/DOB/filter state)
    ├── AxisSourceLock
    └── referenced: bool

SHM Segments (P2P):
  evo_hal_cu  ──reader──→  CU  ──writer──→  evo_cu_hal
  evo_re_cu   ──reader──→  CU  ──writer──→  evo_cu_mqt
  evo_rpc_cu  ──reader──→  CU  ──writer──→  evo_cu_re (placeholder)
```
