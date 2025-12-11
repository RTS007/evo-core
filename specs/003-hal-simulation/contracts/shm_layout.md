# SHM Layout Contract: HAL Simulation Driver

**Feature**: 003-hal-simulation | **Version**: 1.0.0 | **Date**: 2025-12-10

## Overview

This document defines the shared memory (SHM) layout for communication between the HAL Simulation Driver and the Control Unit. The layout is fixed-size to enable zero-copy operations and deterministic memory access.

## Memory Layout

```
Offset      Size        Field
────────────────────────────────────────────────────────────
0x0000      64          Header (HalShmHeader)
0x0040      16384       Axes (64 × 256 bytes)
0x4040      128         Digital Inputs (1024 bits)
0x40C0      128         Digital Outputs (1024 bits)
0x4140      16384       Analog Inputs (1024 × 16 bytes)
0x8140      16384       Analog Outputs (1024 × 16 bytes)
────────────────────────────────────────────────────────────
Total:      49472 bytes (~48.3 KB)
```

## Header (64 bytes)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0x00 | 8 | u64 | magic | Magic number: `0x45564F5F48414C00` ("EVO_HAL\0") |
| 0x08 | 8 | AtomicU64 | version | Version counter (incremented on each write cycle) |
| 0x10 | 4 | u32 | axis_count | Number of configured axes (0-64) |
| 0x14 | 4 | u32 | di_count | Number of configured digital inputs (0-1024) |
| 0x18 | 4 | u32 | do_count | Number of configured digital outputs (0-1024) |
| 0x1C | 4 | u32 | ai_count | Number of configured analog inputs (0-1024) |
| 0x20 | 4 | u32 | ao_count | Number of configured analog outputs (0-1024) |
| 0x24 | 4 | u32 | cycle_time_us | System cycle time (from MachineConfig, defaults to DEFAULT_CYCLE_TIME_US) |
| 0x28 | 24 | u8[24] | _reserved | Reserved for future use |

## Axis Data (256 bytes per axis)

Each axis occupies 256 bytes. Array of 64 axes starts at offset 0x0040.

### Command Section (written by Control Unit)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0x00 | 8 | f64 | target_position | Target position in user units |
| 0x08 | 1 | bool | enable | Enable axis (true = enabled) |
| 0x09 | 1 | bool | reset | Reset error (edge-triggered) |
| 0x0A | 1 | bool | reference | Start referencing (edge-triggered) |
| 0x0B | 5 | u8[5] | _cmd_reserved | Reserved command flags |

### Status Section (written by HAL)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0x10 | 8 | f64 | actual_position | Actual position in user units |
| 0x18 | 8 | f64 | actual_velocity | Actual velocity in user units/sec |
| 0x20 | 8 | f64 | lag_error | Current lag error |
| 0x28 | 1 | bool | ready | Axis ready for motion |
| 0x29 | 1 | bool | error | Axis in error state |
| 0x2A | 1 | bool | referenced | Axis is referenced |
| 0x2B | 1 | bool | referencing | Referencing in progress |
| 0x2C | 1 | bool | moving | Axis is moving |
| 0x2D | 1 | bool | in_position | At target position |
| 0x2E | 2 | u16 | error_code | Error code (0 = no error) |
| 0x30 | 208 | u8[208] | _reserved | Reserved for future use |

## Digital I/O (128 bytes each)

Digital inputs and outputs are stored as bitfields.

- **Byte N, Bit B** corresponds to I/O point `N*8 + B`
- Bit 0 = LSB, Bit 7 = MSB

### Access Pattern

```rust
fn get_digital(buffer: &[u8], index: usize) -> bool {
    let byte_idx = index / 8;
    let bit_idx = index % 8;
    (buffer[byte_idx] >> bit_idx) & 1 == 1
}

fn set_digital(buffer: &mut [u8], index: usize, value: bool) {
    let byte_idx = index / 8;
    let bit_idx = index % 8;
    if value {
        buffer[byte_idx] |= 1 << bit_idx;
    } else {
        buffer[byte_idx] &= !(1 << bit_idx);
    }
}
```

## Analog I/O (16 bytes per channel)

Each analog channel has dual representation.

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0x00 | 8 | f64 | normalized | Normalized value (0.0 - 1.0) |
| 0x08 | 8 | f64 | scaled | Scaled value in engineering units |

### Scaling Formulas

All scaling uses polynomial: `f(n) = a×n³ + b×n² + c×n + d` where coefficients sum to 1.0.

**Named presets:**
| Name | a | b | c | d | Formula |
|------|---|---|---|---|--------|
| linear | 0 | 0 | 1 | 0 | f(n) = n |
| quadratic | 0 | 1 | 0 | 0 | f(n) = n² |
| cubic | 1 | 0 | 0 | 0 | f(n) = n³ |

**Normalized → Scaled:**
`scaled = min + f(normalized) × (max - min)`

**Scaled → Normalized:**
Newton-Raphson iteration to solve `f(n) = (scaled - min) / (max - min)`

## Version Protocol

The `version` field in the header uses optimistic concurrency:

### Writer (HAL Simulation)
```rust
// Before writing
let v = version.fetch_add(1, Ordering::AcqRel);
assert!(v % 2 == 0, "Previous write incomplete");

// Write data...

// After writing
version.fetch_add(1, Ordering::Release);
```

### Reader (Control Unit)
```rust
loop {
    let v1 = version.load(Ordering::Acquire);
    if v1 % 2 == 1 { continue; } // Write in progress
    
    // Read data...
    
    let v2 = version.load(Ordering::Acquire);
    if v1 == v2 { break; } // Consistent read
}
```

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| 0x0000 | NO_ERROR | No error |
| 0x0001 | LAG_ERROR | Lag error limit exceeded |
| 0x0002 | SOFT_LIMIT_POS | Positive software limit reached |
| 0x0003 | SOFT_LIMIT_NEG | Negative software limit reached |
| 0x0010 | REF_SWITCH_NOT_FOUND | Reference switch not found during referencing |
| 0x0011 | REF_INDEX_NOT_FOUND | Index pulse not found during referencing |
| 0x0012 | REF_TIMEOUT | Referencing timeout |
| 0x00FF | INTERNAL_ERROR | Internal simulation error |

## Segment Naming

The SHM segment name follows the pattern: `evo_hal_{service_name}`

Example: For `service_name = "sim-01"`, segment name is `evo_hal_sim-01`

## Compatibility

- **Magic number** must match exactly; reject otherwise
- **Version** starts at 0, increments by 2 per write cycle
- All reserved fields must be zero; readers should ignore them
- Future versions may extend reserved areas while maintaining backward compatibility
