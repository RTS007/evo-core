# I/O Configuration Contract

**Date**: 2026-02-08 | **Spec**: [../spec.md](../spec.md) | **Data Model**: [../data-model.md](../data-model.md)

> Defines the `io.toml` file format, role resolution algorithm, and validation rules.
> This contract is shared between HAL and CU — both parse the same `io.toml`.

---

## 1. File Format

`io.toml` uses named TOML tables, each representing an I/O group. Each group contains an `io` array of I/O point definitions.

### Minimal Example

```toml
[Safety]
name = "Safety circuits"
io = [
    { type="di", role="EStop", pin=1, logic="NC", debounce=100, name="Main E-Stop", sim=true },
    { type="di", role="SafetyGate", pin=4, logic="NC", debounce=50, name="Light curtain" },
    { type="do", pin=200, init=true, keep_estop=true, name="Safety relay" },
]

[Axes]
name = "Limit switches and homing"
io = [
    { type="di", role="LimitMin1", pin=30, logic="NC", name="Limit switch 1-" },
    { type="di", role="LimitMax1", pin=31, logic="NC", name="Limit switch 1+" },
    { type="di", role="Ref1", pin=34, debounce=30, name="Homing sensor axis 1" },
]

[Pneumatics]
name = "Pneumatics"
io = [
    { type="di", role="PressureOk", pin=10, logic="NC", name="Main pressure OK", sim=true },
    { type="ai", pin=64, max=10.0, unit="bar", average=10, sim=6.0, name="Pressure value" },
    { type="do", pin=100, pulse=500, name="Main valve" },
]
```

### I/O Point Fields by Type

#### Digital Input (`type = "di"`)

| Field          | Type   | Required | Default | Description                                |
|----------------|--------|----------|---------|--------------------------------------------|
| `type`         | string | ✅       |         | Must be `"di"`                             |
| `pin`          | u16    | ✅       |         | Physical pin number                        |
| `role`         | string |          |         | Functional role (IoRole enum)              |
| `name`         | string |          |         | Display label for operator                 |
| `logic`        | string |          | `"NO"`  | `"NO"` (Normally Open) or `"NC"` (Normally Closed) |
| `debounce`     | u16    |          | `15`    | Contact bounce filter [ms]                 |
| `sim`          | bool   |          |         | Simulation value                           |
| `enable_pin`   | u16    |          |         | Conditional enable input pin               |
| `enable_state` | bool   |          | `true`  | Required enable state (with `enable_pin`)  |
| `enable_timeout` | u32  |          | `0`     | Max time between signals [ms] (0=none)     |

#### Digital Output (`type = "do"`)

| Field        | Type   | Required | Default | Description                                  |
|--------------|--------|----------|---------|----------------------------------------------|
| `type`       | string | ✅       |         | Must be `"do"`                               |
| `pin`        | u16    | ✅       |         | Physical pin number                          |
| `role`       | string |          |         | Functional role (IoRole enum)                |
| `name`       | string |          |         | Display label for operator                   |
| `init`       | bool   |          | `false` | Logical initial state (before inversion)     |
| `inverted`   | bool   |          | `false` | Invert logic-to-pin mapping                  |
| `pulse`      | u32    |          | `0`     | Watchdog ms, auto-OFF without refresh (0=none) |
| `keep_estop` | bool   |          | `false` | Do NOT reset on E-Stop                       |

#### Analog Input (`type = "ai"`)

| Field     | Type        | Required | Default    | Description                            |
|-----------|-------------|----------|------------|----------------------------------------|
| `type`    | string      | ✅       |            | Must be `"ai"`                         |
| `pin`     | u16         | ✅       |            | Physical pin number                    |
| `max`     | f64         | ✅       |            | Engineering range maximum              |
| `role`    | string      |          |            | Functional role (IoRole enum)          |
| `name`    | string      |          |            | Display label for operator             |
| `min`     | f64         |          | `0.0`      | Engineering range minimum              |
| `unit`    | string      |          | `"V"`      | Unit of measure                        |
| `average` | u16         |          | `5`        | Moving average samples (1–1000)        |
| `curve`   | string/arr  |          | `"linear"` | Scaling: `"linear"`, `"quadratic"`, `"cubic"`, or `[a, b, c]` |
| `offset`  | f64         |          | `0.0`      | Output offset added after curve        |
| `sim`     | f64         |          |            | Simulation value                       |

#### Analog Output (`type = "ao"`)

| Field   | Type        | Required | Default    | Description                            |
|---------|-------------|----------|------------|----------------------------------------|
| `type`  | string      | ✅       |            | Must be `"ao"`                         |
| `pin`   | u16         | ✅       |            | Physical pin number                    |
| `max`   | f64         | ✅       |            | Engineering range maximum              |
| `role`  | string      |          |            | Functional role (IoRole enum)          |
| `name`  | string      |          |            | Display label for operator             |
| `min`   | f64         |          | `0.0`      | Engineering range minimum              |
| `unit`  | string      |          | `"V"`      | Unit of measure                        |
| `init`  | f64         |          | `0.0`      | Initial value in engineering units     |
| `pulse` | u32         |          | `0`        | Watchdog ms, auto-reset to 0 (0=none) |
| `curve` | string/arr  |          | `"linear"` | Scaling shape                          |
| `offset`| f64         |          | `0.0`      | Output offset added after curve        |

### Scaling Curve Formula

```text
f(n) = a·n³ + b·n² + c·n + offset
where n = normalized input (0.0 - 1.0)
```

| Preset       | a   | b   | c   | Result    |
|-------------|-----|-----|-----|-----------|
| `"linear"`  | 0   | 0   | 1   | f(n) = n  |
| `"quadratic"` | 0 | 1   | 0   | f(n) = n² |
| `"cubic"`   | 1   | 0   | 0   | f(n) = n³ |

Custom: `curve = [0.2, 0.0, 0.8]` → f(n) = 0.2·n³ + 0.8·n

---

## 2. Role Naming Convention

Roles follow **FunctionAxisNumber** convention:

| Pattern            | Examples                    | Description                    |
|-------------------|-----------------------------|--------------------------------|
| `Function`        | `EStop`, `PressureOk`       | Global (no axis)               |
| `FunctionN`       | `LimitMin1`, `BrakeOut3`    | Per-axis (N = axis number, 1-based) |

### Role String → IoRole Parsing

```text
"EStop"      → IoRole::EStop
"LimitMin1"  → IoRole::LimitMin(1)
"BrakeOut3"  → IoRole::BrakeOut(3)
"Ref2"       → IoRole::Ref(2)
"PressureOk" → IoRole::PressureOk
"MyCustom"   → IoRole::Custom("MyCustom")
```

Parser extracts trailing digits as axis number. Known prefixes matched first; unknown strings become `Custom`.

---

## 3. Role Resolution Algorithm (Startup)

```text
fn build_io_registry(io_config: &IoConfig) -> Result<IoRegistry, IoConfigError> {
    let mut bindings = HashMap::new();
    
    for group in &io_config.groups {
        for (idx, point) in group.io.iter().enumerate() {
            if let Some(role) = &point.role {
                // 1. Uniqueness check
                if bindings.contains_key(role) {
                    return Err(IoConfigError::DuplicateRole(role));
                }
                
                // 2. Build binding
                let binding = IoBinding {
                    group_key: group.key.clone(),
                    point_idx: idx,
                    io_type: point.io_type,
                    pin: point.pin,
                    logic: point.logic,
                    curve: point.curve,
                    offset: point.offset,
                    min: point.min,
                    max: point.max,
                };
                
                bindings.insert(role.clone(), binding);
            }
        }
    }
    
    Ok(IoRegistry { bindings, .. })
}
```

---

## 4. Validation Rules

### V-IO-1: Pin Uniqueness

No two I/O points (across all groups) may share the same `(type, pin)` pair.

```text
Violation → ERR_IO_PIN_DUPLICATE { type, pin, group_a, group_b }
```

### V-IO-2: Role Uniqueness

No two I/O points may share the same `role` string.

```text
Violation → ERR_IO_ROLE_DUPLICATE { role, group_a, group_b }
```

### V-IO-3: Role Type Correctness

Each known role has an expected I/O type. If the `io.toml` assigns a role to a point of wrong type, reject.

| Role Pattern | Expected Type |
|-------------|---------------|
| `EStop`, `SafetyGate`, `LimitMinN`, `LimitMaxN`, `RefN`, `EnableN`, `TailClosedN`, `TailOpenN`, `TailClampN`, `IndexLockedN`, `IndexMiddleN`, `IndexFreeN`, `BrakeInN`, `GuardClosedN`, `GuardLockedN`, `PressureOk`, `VacuumOk` | `di` |
| `BrakeOutN` | `do` |
| `Custom(*)` | any |

```text
Violation → ERR_IO_ROLE_TYPE_MISMATCH { role, expected_type, actual_type }
```

### V-IO-4: Role Completeness (per axis)

For each axis in `CuMachineConfig`, validate that all I/O roles required by its peripheral configuration are present in the registry:

| Axis Peripheral        | Required Roles                                     |
|------------------------|-----------------------------------------------------|
| Homing (HomeSensor)    | `RefN`                                              |
| Homing (IndexPulse)    | `RefN` + encoder index (via `index_role`)           |
| Homing (LimitSwitch)   | `LimitMinN` or `LimitMaxN` (per `limit_direction`) |
| Tailstock (Type 1)     | `TailClosedN`, `TailOpenN`                          |
| Tailstock (Type 2-4)   | `TailClosedN`, `TailOpenN`, `TailClampN`            |
| Index (locking pin)    | `IndexLockedN`, `IndexFreeN` (+ optional `IndexMiddleN`) |
| Brake                  | `BrakeOutN`, `BrakeInN`                             |
| Guard                  | `GuardClosedN`, `GuardLockedN`                      |
| Motion enable          | `EnableN`                                           |
| Limit switches         | `LimitMinN`, `LimitMaxN` (always required)          |

```text
Violation → ERR_IO_ROLE_MISSING { role, axis_id, peripheral }
```

### V-IO-5: Global Role Completeness

These roles MUST always be present (regardless of axis configuration):

- `EStop` (safety — always required)

```text
Violation → ERR_IO_ROLE_MISSING { role: "EStop", axis_id: 0, peripheral: "global_safety" }
```

### V-IO-6: Analog Range Validity

For AI/AO points: `min < max`. Average must be in range 1–1000.

```text
Violation → ERR_IO_ANALOG_RANGE { pin, min, max }
```

### V-IO-7: Pin Number Bounds

Pin numbers must be within valid hardware range (0–65535). Validation is permissive at config level; HAL validates against actual hardware capabilities.

---

## 5. Runtime I/O Access Contract

### DI Read (with NC/NO Logic)

```text
fn read_di(role: IoRole, di_bank: &[u64; 16]) -> bool {
    let binding = self.bindings[&role];
    assert!(binding.io_type == Di);
    let raw_bit = extract_bit(di_bank, binding.pin);
    match binding.logic {
        NO => raw_bit,         // NO: true when signal present
        NC => !raw_bit,        // NC: invert — false (wire break) = active
    }
}
```

### AI Read (with Scaling)

```text
fn read_ai(role: IoRole, ai_values: &[f64; 64]) -> f64 {
    let binding = self.bindings[&role];
    assert!(binding.io_type == Ai);
    let raw = ai_values[pin_to_ai_index(binding.pin)];
    let normalized = (raw - binding.min) / (binding.max - binding.min);
    let scaled = binding.curve.evaluate(normalized);
    scaled * (binding.max - binding.min) + binding.min + binding.offset
}
```

### DO Write

```text
fn write_do(role: IoRole, value: bool, do_bank: &mut [u64; 16]) {
    let binding = self.bindings[&role];
    assert!(binding.io_type == Do);
    let physical = if binding.inverted { !value } else { value };
    set_bit(do_bank, binding.pin, physical);
}
```

### AO Write

```text
fn write_ao(role: IoRole, value: f64, ao_values: &mut [f64; 64]) {
    let binding = self.bindings[&role];
    assert!(binding.io_type == Ao);
    // Reverse-scale from engineering units to normalized
    let normalized = (value - binding.min) / (binding.max - binding.min);
    ao_values[pin_to_ao_index(binding.pin)] = normalized;
}
```

---

## 6. HAL ↔ CU Shared I/O Model

```text
                    io.toml
                   /       \
                  /         \
            HAL (reader)   CU (reader)
                |              |
          [physical I/O]  [IoRegistry]
                |              |
         evo_hal_cu SHM ──────┘
         (di_bank, ai_values)
                               |
         evo_cu_hal SHM ───────┘
         (do_bank, ao_values)
```

1. **HAL** reads `io.toml`, builds its own `IoRegistry`, maps roles → physical pins.
2. **HAL** writes raw DI/AI values into `evo_hal_cu` SHM segment (di_bank, ai_values).
3. **CU** reads `io.toml`, builds its own `IoRegistry`, resolves roles → SHM bit/array positions.
4. **CU** reads `evo_hal_cu`, calls `io_registry.read_di(role, &segment.di_bank)` — NC/NO applied automatically.
5. **CU** writes `evo_cu_hal` DO/AO values, calls `io_registry.write_do(role, value, &mut segment.do_bank)`.
6. Both registries use the **same io.toml** — pin assignments are guaranteed consistent.

### Pin ↔ SHM Mapping Convention

- DI: `di_bank` is a 1024-bit array (16 × u64). Bit N = pin N. HAL sets bit by physical pin number.
- AI: `ai_values[N]` corresponds to the Nth analog input point **in declaration order** within `io.toml`. HAL maps pin → index at startup.
- DO/AO: Same convention in `evo_cu_hal`.

---

## 7. Migration from Inline I/O Pattern

### What This Replaces (FR-155)

| Old Pattern | New Pattern |
|------------|-------------|
| `[[digital_inputs]]` in machine.toml | `{ type="di", ... }` in io.toml group |
| `[[analog_outputs]]` in machine.toml | `{ type="ao", ... }` in io.toml group |
| `name = "di_cylinder_closed"` | `role="CylinderClosed"` + optional `name="Cylinder closed"` |
| `reference_switch = 0` (array index) | `sensor_role: IoRole::Ref1` in HomingConfig |
| `sensor_input_name: heapless::String<32>` | `sensor_role: IoRole` |
| `di_closed: heapless::String<32>` in TailstockConfig | `di_closed: IoRole` (e.g., `TailClosed1`) |
| `MachineConfig.digital_inputs: Vec<DigitalIOConfig>` | `IoConfig.groups[].io[]` with `type="di"` |
| `DigitalIOConfig` / `AnalogIOConfig` structs | `IoPoint` struct (unified) |
| Per-axis `end_switch_nc: bool` | `logic="NC"` on the I/O point itself in io.toml |

### HAL Config Migration

Old `evo_hal/config/machine.toml`:
```toml
[[digital_inputs]]
name = "emergency_stop"
description = "Emergency stop button (NC)"
invert = true
```

New `config/io.toml`:
```toml
[Safety]
name = "Safety circuits"
io = [
    { type="di", role="EStop", pin=1, logic="NC", debounce=100, name="Emergency stop button", sim=true },
]
```

### Axis Config Migration

Old `config/axis_01.toml`:
```toml
[referencing]
reference_switch = 0    # DI index into digital_inputs array
normally_closed = false
```

New (in CU machine config, with io.toml providing the pin):
```toml
[homing]
method = "HomeSensor"
sensor_role = "Ref1"      # IoRole, resolved from io.toml
sensor_nc = false
```
