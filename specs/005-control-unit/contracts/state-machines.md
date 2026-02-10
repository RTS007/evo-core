# State Machine Transition Contracts

**Date**: 2026-02-08 | **Spec**: [../spec.md](../spec.md) | **Data Model**: [../data-model.md](../data-model.md)

> All state machines use `#[repr(u8)]` enums with exhaustive `match` (Research Topic 9).
> Every transition produces an `EventEntry` written to the `evo_cu_mqt` event ring.

---

## 1. MachineState — Global Machine State

### States
| Value | State       | Description                     |
|-------|-------------|---------------------------------|
| 0     | Stopped     | Initial state after boot        |
| 1     | Starting    | Loading config, validating SHM  |
| 2     | Idle        | Ready, no active motion         |
| 3     | Manual      | Manual jog/positioning          |
| 4     | Active      | Recipe/program running          |
| 5     | Service     | Maintenance mode (authorized)   |
| 6     | SystemError | Critical fault                  |

### Transitions

```text
┌─────────┐  power-on   ┌──────────┐  init-ok  ┌──────┐
│ Stopped │────────────→│ Starting │─────────→│ Idle │
└─────────┘              └──────────┘           └──┬───┘
                              │ init-fail            │
                              ▼                      │
                        ┌─────────────┐              │
                        │ SystemError │←─────────────┤─── any state (critical fault)
                        └─────────────┘              │
                                                     │
                    ┌────────────────────────────────┤
                    │                                │
                    ▼                                ▼
              ┌──────────┐                    ┌──────────┐
              │  Manual  │◄──────────────────►│  Active  │
              └──────────┘  (via Idle only)   └──────────┘
                    │                                │
                    └──────── service-auth ──────────┘
                                    │
                              ┌─────────┐
                              │ Service │
                              └─────────┘
```

### Transition Table

| From        | To          | Guard                                  | Action                                         |
|-------------|-------------|----------------------------------------|-------------------------------------------------|
| Stopped     | Starting    | Power-on event received                | Begin config load, SHM validation               |
| Starting    | Idle        | Config valid, SHM connected, HAL alive | Enable cycle loop                                |
| Starting    | SystemError | Config invalid OR SHM mismatch         | Log error, refuse operation                      |
| Idle        | Manual      | First manual command received          | Acquire source lock for commanding axis          |
| Manual      | Idle        | Manual timeout OR explicit stop        | Release manual source lock                       |
| Idle        | Active      | RE sends first command                 | RE acquires source lock for commanded axes       |
| Manual      | Active      | RE sends first command (no lock conflict)| Transfer lock ownership where needed            |
| Active      | Idle        | RE sends Nop + all axes in position    | Release RE source locks                          |
| Active      | Manual      | RE sends Nop + manual command pending  | Transfer lock, enter Manual                      |
| any         | Service     | Service authorization flag set         | Preserve axis states, allow extended diagnostics |
| Service     | Idle        | Service deauthorized                   | Restore normal operation constraints             |
| any         | SystemError | Critical fault OR unrecoverable error  | Execute safe-stop per axis, freeze outputs       |
| SystemError | Idle        | SafetyState==Safe + all errors cleared + operator reset + authorization (FR-122) | Re-enable cycle loop, axes remain PowerOff until explicit enable |
| SystemError | Stopped     | Unrecoverable fault + explicit full-reset | Full reinitialization required (e.g., SHM mismatch, config corruption) |

### Invariants

- **I-MS-1**: Only ONE MachineState active at any time.
- **I-MS-2**: SystemError exits via recovery reset → Idle (normal path per FR-122) or full-reset → Stopped (unrecoverable faults only).
- **I-MS-3**: Active requires at least one axis with `locked_source == RecipeExecutor`.
- **I-MS-4**: Manual timeout is configurable (default 30s of no manual commands).

---

## 2. SafetyState — Global Safety Overlay

### States
| Value | State            | Description                              |
|-------|------------------|------------------------------------------|
| 0     | Safe             | All safety conditions satisfied          |
| 1     | SafeReducedSpeed | Hardware speed limitation active         |
| 2     | SafetyStop       | Emergency — per-axis safe stop executing |

### Transitions

| From              | To                | Guard                                   | Action                                        |
|-------------------|-------------------|-----------------------------------------|-----------------------------------------------|
| Safe              | SafeReducedSpeed  | Speed limit input active                | Reduce velocity limits on affected axes       |
| Safe              | SafetyStop        | Any CRITICAL error flag set             | Execute per-axis SafeStopCategory protocol    |
| SafeReducedSpeed  | Safe              | Speed limit input cleared               | Restore original velocity limits              |
| SafeReducedSpeed  | SafetyStop        | Any CRITICAL error flag set             | Execute per-axis SafeStopCategory protocol    |
| SafetyStop        | Safe              | All errors cleared + operator reset     | Axes remain disabled until explicitly enabled |

### Invariants

- **I-SS-1**: SafetyStop overrides all MachineState behavior (forces SystemError).
- **I-SS-2**: SafetyStop CANNOT be cleared automatically — requires explicit operator reset.
- **I-SS-3**: During SafetyStop, each axis executes its configured `SafeStopCategory` independently.

---

## 3. PowerState — Per-Axis Power

### States
| Value | State       | Description                          |
|-------|-------------|--------------------------------------|
| 0     | PowerOff    | Drive disabled, brake engaged        |
| 1     | PoweringOn  | Multi-step enable sequence           |
| 2     | Standby     | Drive ready, no motion commanded     |
| 3     | Motion      | Drive actively controlling           |
| 4     | PoweringOff | Multi-step disable sequence          |
| 5     | NoBrake     | Service: drive OFF, brake released   |
| 6     | PowerError  | Unrecoverable drive fault            |

### Transitions

| From        | To          | Guard                                   | Action                                         |
|-------------|-------------|-----------------------------------------|------------------------------------------------|
| PowerOff    | PoweringOn  | Enable command + no safety block        | Start enable sequence (brake release, etc.)    |
| PoweringOn  | Standby     | Sequence complete, drive reports ready  | Reset control state (PID integral, etc.)       |
| PoweringOn  | PowerError  | Sequence timeout OR drive fault         | Set PowerError flags, abort sequence           |
| Standby     | Motion      | Motion command received                 | Begin control output calculation               |
| Motion      | Standby     | Motion complete + standstill detected   | Hold position (if Position mode)               |
| Standby     | PoweringOff | Disable command                         | Start disable sequence (brake engage, etc.)    |
| Motion      | PoweringOff | Disable command                         | Controlled stop → disable sequence             |
| PoweringOff | PowerOff    | Sequence complete                       | All outputs zero                               |
| any         | PowerError  | CRITICAL drive fault                    | Immediate: set outputs zero, engage brake      |
| PowerError  | PowerOff    | Error cleared + reset command           | Re-initialization required                     |
| PowerOff    | NoBrake     | Service mode + NoBrake command          | Release brake without enabling drive           |
| NoBrake     | PowerOff    | End NoBrake command                     | Re-engage brake                                |

### Power-On Sequence Steps (FR-021)

```text
Step 0: Check safety flags (all must be OK)
Step 1: Send drive enable command
Step 2: Wait drive_ready (timeout: 5s)
Step 3: Release brake (if BrakeConfig present)
Step 4: Wait brake released confirmation (timeout: BrakeConfig.release_timeout)
Step 5: For gravity axes: check position stable (holding_timer)
Step 6: Zero PID integral and filter states
Step 7: Transition to Standby
```

### Invariants

- **I-PW-1**: No motion output when `PowerState != Motion`.
- **I-PW-2**: PoweringOn/PoweringOff are interruptible by SAFETY_STOP (→ immediate STO/SS1/SS2).
- **I-PW-3**: NoBrake ONLY available in `MachineState::Service`.
- **I-PW-4**: Control state (PID integral, DOB, filters) reset on every PowerOff → PoweringOn transition.

---

## 4. MotionState — Per-Axis Motion

### States
| Value | State            | Description                     |
|-------|------------------|---------------------------------|
| 0     | Standstill       | No motion commanded             |
| 1     | Accelerating     | Velocity increasing             |
| 2     | ConstantVelocity | At target velocity              |
| 3     | Decelerating     | Velocity decreasing             |
| 4     | Stopping         | Controlled stop (not emergency) |
| 5     | EmergencyStop    | Safety-triggered deceleration   |
| 6     | Homing           | Homing procedure active         |
| 7     | GearAssistMotion | Passive follower (gearbox)      |
| 8     | MotionError      | Motion fault active             |

### Transitions

| From             | To               | Guard                                | Action                                    |
|------------------|------------------|--------------------------------------|-------------------------------------------|
| Standstill       | Accelerating     | Motion command + PowerState==Motion  | Begin trajectory generation               |
| Accelerating     | ConstantVelocity | Target velocity reached              | Switch to velocity maintenance            |
| ConstantVelocity | Decelerating     | Approaching target position          | Begin deceleration ramp                   |
| Accelerating     | Decelerating     | Short move (no cruise phase)         | Direct to decel                           |
| Decelerating     | Standstill       | Position reached + velocity < eps    | Motion complete, hold position            |
| any_moving       | Stopping         | Stop command (non-emergency)         | Controlled deceleration to standstill     |
| Stopping         | Standstill       | Velocity < epsilon                   | Stop complete                             |
| any_moving       | EmergencyStop    | SAFETY_STOP triggered                | Max deceleration (safe_stop.max_decel)    |
| EmergencyStop    | Standstill       | Velocity == 0                        | Engage brake per SafeStopCategory         |
| Standstill       | Homing           | Home command + axis unreferenced + `approach_direction` valid (FR-033a) | Begin homing per HomingConfig             |
| Homing           | Standstill       | Homing complete                      | Set referenced=true, set position offset  |
| Homing           | MotionError      | Homing timeout or sensor failure     | Set HOMING_FAILED error                   |
| any              | MotionError      | Lag exceed, hard limit, encoder fail | Set corresponding MotionError flag        |
| MotionError      | Standstill       | Error cleared + reset                | Resume from stopped state                 |

### `any_moving` set

```text
{Accelerating, ConstantVelocity, Decelerating, GearAssistMotion}
```

### Invariants

- **I-MO-1**: MotionState is ONLY updated when `PowerState == Motion`.
- **I-MO-2**: EmergencyStop always uses `SafeStopConfig.max_decel_safe` (never regular decel).
- **I-MO-3**: Homing blocked if axis already referenced (must clear `referenced` first).
- **I-MO-3a**: Homing blocked if `HomingConfig.approach_direction` is not set for methods requiring it (HardStop, HomeSensor, LimitSwitch, IndexPulse) — reject with `ERR_HOMING_CONFIG_INVALID` (FR-033a).
- **I-MO-4**: Lag monitoring active in all moving states. Behavior per `lag_policy`: Critical → SAFETY_STOP all axes; Unwanted → axis-local MOTION_ERROR; Neutral → flag only; Desired → suppressed.

---

## 5. OperationalMode — Per-Axis Control Mode

### States
| Value | State    | Description                          |
|-------|----------|--------------------------------------|
| 0     | Position | Position control (PID on position)   |
| 1     | Velocity | Velocity control (PID on velocity)   |
| 2     | Torque   | Direct torque control                |
| 3     | Manual   | Manual jog (limited velocity)        |
| 4     | Test     | Service mode testing                 |

### Transitions

| From     | To       | Guard                                         | Action                                  |
|----------|----------|-----------------------------------------------|-----------------------------------------|
| any      | any      | PowerState==Standby + not SLAVE_COUPLED       | Switch mode, reset PID integral         |
| any      | Manual   | MachineState==Manual + PowerState==Standby    | Apply manual velocity limits            |
| any      | Test     | MachineState==Service                         | Enable extended diagnostics             |

### Invariants

- **I-OM-1**: Mode change ONLY when `MotionState == Standstill` and `PowerState == Standby`.
- **I-OM-2**: SLAVE_COUPLED and SLAVE_MODULATED axes CANNOT change mode (follows master).
- **I-OM-3**: `ControlOutputVector` always has all 4 fields; HAL selects based on mode.
- **I-OM-4**: PID integral and DOB state reset on every mode change.

---

## 6. CouplingState — Per-Axis Coupling

### States
| Value | State          | Description                     |
|-------|----------------|---------------------------------|
| 0     | Uncoupled      | Independent axis                |
| 1     | Master         | Leading coupled group           |
| 2     | SlaveCoupled   | Following master × ratio        |
| 3     | SlaveModulated | Following master × ratio + offset|
| 4     | WaitingSync    | Synchronizing to master         |
| 5     | Synchronized   | In-sync with master             |
| 6     | SyncLost       | Lost synchronization            |
| 7     | Coupling       | Transition: engaging            |
| 8     | Decoupling     | Transition: disengaging         |

### Transitions

| From       | To             | Guard                                     | Action                                    |
|------------|----------------|--------------------------------------------|-------------------------------------------|
| Uncoupled  | Coupling       | Couple command + master exists + standstill| Begin sync approach                       |
| Coupling   | Master         | This axis designated as master             | Register slave list                       |
| Coupling   | WaitingSync    | This axis is slave, approaching sync       | Start sync timeout timer                  |
| WaitingSync| SlaveCoupled   | Position/velocity within sync tolerance    | Lock OperationalMode to match master      |
| WaitingSync| SlaveModulated | Modulated coupling selected                | Apply modulation offset                   |
| WaitingSync| SyncLost       | Sync timeout exceeded                      | Set SYNC_TIMEOUT error                    |
| Synchronized| SyncLost      | Lag diff > max_lag_diff                    | Set LAG_DIFF_EXCEED (CRITICAL if enabled) |
| SyncLost   | WaitingSync    | Re-sync command                            | Restart sync                              |
| any_coupled| Decoupling     | Decouple command                           | Begin safe decoupling                     |
| Decoupling | Uncoupled      | All slaves acknowledge decoupled           | Clear coupling config                     |
| Master     | Decoupling     | Decouple command OR master fault           | Cascade decouple to all slaves            |

### `any_coupled` set

```text
{Master, SlaveCoupled, SlaveModulated, WaitingSync, Synchronized}
```

### Invariants

- **I-CP-1**: SLAVE_COUPLED/SLAVE_MODULATED axes CANNOT receive independent motion commands.
- **I-CP-2**: Master fault or disable → ALL slaves decouple (cascade).
- **I-CP-3**: Maximum 8 direct slaves per master (heapless::Vec<AxisId, 8>).
- **I-CP-4**: Coupling blocked if either axis is in MotionError or Homing.

---

## 7. GearboxState — Per-Axis Gearbox

### States
| Value | State        | Description                    |
|-------|-------------|--------------------------------|
| 0     | NoGearbox   | Axis has no gearbox            |
| 1-249 | GearN       | Currently in gear N            |
| 250   | Neutral     | No gear engaged                |
| 251   | Shifting    | Gear change in progress        |
| 252   | GearboxError| Sensor conflict or timeout     |
| 253   | Unknown     | Initial state before detection |

### Transitions

| From     | To           | Guard                                   | Action                               |
|----------|-------------|------------------------------------------|---------------------------------------|
| Unknown  | Neutral     | Sensor reading consistent: neutral       | Gearbox initialized                   |
| Unknown  | GearN       | Sensor reading consistent: gear N        | Gearbox initialized                   |
| GearN    | Shifting    | Gear change cmd + MotionState==Standstill| Disengage current gear                |
| Neutral  | Shifting    | Gear change cmd + MotionState==Standstill| Begin gear engagement                 |
| Shifting | GearN       | Target gear sensor confirmed             | Apply new gear ratio to control       |
| Shifting | Neutral     | Neutral sensor confirmed                 | Ready for next gear command           |
| Shifting | GearboxError| Timeout OR sensor conflict               | Set GEAR_TIMEOUT / GEAR_SENSOR_CONFLICT|
| GearN    | GearboxError| NO_GEARSTEP (CRITICAL)                  | Trigger SAFETY_STOP                   |
| GearboxError| Unknown  | Error cleared + reset                    | Re-detect gear position               |

### Invariants

- **I-GB-1**: Gear change ONLY when `MotionState == Standstill`.
- **I-GB-2**: `NO_GEARSTEP` (unexpected gear loss) is CRITICAL → SAFETY_STOP.
- **I-GB-3**: GearboxState is `NoGearbox` for axes without gearbox config (never changes).

---

## 8. LoadingState — Per-Axis Loading

### States
| Value | State               | Description                         |
|-------|---------------------|-------------------------------------|
| 0     | Production          | Normal production mode              |
| 1     | ReadyForLoading     | Axis ready for workpiece loading    |
| 2     | LoadingBlocked      | Loading not possible (config)       |
| 3     | LoadingManualAllowed| Manual loading only (reduced speed) |

### Transitions

| From               | To                  | Guard                              | Action                        |
|--------------------|---------------------|------------------------------------|-------------------------------|
| Production         | ReadyForLoading     | All axes in position + recipe idle | Signal loading ready          |
| ReadyForLoading    | Production          | Loading complete signal received   | Resume production             |
| Production         | LoadingBlocked      | Global loading trigger + config: loading_blocked=true (FR-073) | Reject motion commands        |
| Production         | LoadingManualAllowed| Global loading trigger + config: loading_manual=true (FR-073)  | Apply manual speed limits     |
| LoadingBlocked     | Production          | Loading complete signal received   | Restore normal operation      |
| LoadingManualAllowed| Production          | Loading complete signal received   | Restore normal operation      |

### Invariants

- **I-LD-1**: LoadingBlocked and LoadingManualAllowed are config-determined responses to a **runtime** global loading trigger (FR-073) — not static startup assignments. Config fields `loading_blocked` and `loading_manual` determine which state each axis enters when the trigger fires.
- **I-LD-2**: ReadyForLoading requires ALL production axes at target position.

---

## Cross-Machine Invariants

| ID      | Rule                                                        | Enforcement     |
|---------|-------------------------------------------------------------|-----------------|
| I-XM-1  | SAFETY_STOP overrides ALL state machines simultaneously     | Safety check first in cycle |
| I-XM-2  | No axis motion when `SafetyState != Safe`                   | Motion guard    |
| I-XM-3  | Unreferenced axis: limited to MANUAL/SERVICE/Homing at 5% max velocity; ACTIVE commands rejected with ERR_NOT_REFERENCED (FR-035) | Command filter  |
| I-XM-4  | Cycle overrun (>1ms) → SAFETY_STOP for ALL axes (FR-122)   | Cycle timer     |
| I-XM-5  | All state transitions are atomic per-axis within one cycle  | Single-threaded |
| I-XM-6  | State snapshot in evo_cu_mqt reflects END-of-cycle state    | Write after all |
