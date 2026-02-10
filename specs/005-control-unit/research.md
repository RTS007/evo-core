# Research: Real-Time Control Unit — Hard RT Patterns in Rust on PREEMPT_RT

**Feature Branch**: `005-control-unit`
**Date**: 2026-02-08
**Status**: Complete

---

## Topic 1: Zero-Allocation RT Loop Patterns in Rust

### Decision

Use **plain `#[repr(C)]` fixed-size arrays and structs** for all data in the RT hot loop. No `Vec`, `String`, `HashMap`, or any heap-allocating type in the cycle path. Use `heapless` crate types only where a dynamic-length collection is genuinely needed inside the RT path (e.g., a variable-count command queue).

### Rationale

1. **Plain arrays are cheapest.** The EVO HAL already uses `[AxisShmData; MAX_AXES]`, `[u8; MAX_DI / 8]`, etc. This is the gold standard — zero overhead, zero allocation, fully deterministic. The Control Unit should mirror this pattern for all its internal state.

2. **`heapless::Vec<T, N>` for bounded-dynamic collections.** Where count is not always `MAX`, a `heapless::Vec` gives `push`/`pop` semantics with `O(1)` worst-case (no amortized resizing) and returns `Err(CapacityError)` instead of panicking. Use for: command queues, error buffers, active-axis lists.

3. **`arrayvec::ArrayVec<T, N>` is equivalent** but targets `std` environments. Either works; `heapless` is preferred because it's the de-facto standard in embedded/RT Rust and also provides `heapless::String<N>`, `heapless::spsc::Queue`, and `heapless::mpmc::MpMcQueue` — all useful in our architecture.

4. **String handling:** No `String` or `&str` allocation in the RT path. Use `[u8; N]` with a length byte, or `heapless::String<N>` for display-name fields that must live in SHM. For axis names in SHM, use `[u8; 32]` (fixed, null-padded) matching the existing `#[repr(C)]` layout.

5. **Compile-time / CI enforcement:**
   - **Custom global allocator that aborts in RT threads.** Register a custom `#[global_allocator]` that checks `thread_local!` RT flag; if set, it calls `std::process::abort()` on any allocation. This catches accidental allocations during testing.
   - **`#[cfg(test)]` allocator wrapper** that counts allocations per test and fails if any occur during a "cycle" call.
   - **Clippy lints:** `#![deny(clippy::disallowed_types)]` configured to forbid `String`, `Vec`, `Box`, `HashMap`, `BTreeMap` in the RT module.
   - **Build-time check:** A CI step that runs `cargo test --features rt-alloc-check` with the panicking allocator.

### Alternatives Considered

| Alternative | Verdict | Why rejected |
|---|---|---|
| Pre-allocated `Vec` with `with_capacity` | ❌ Rejected | Still heap-allocated; realloc possible if someone calls `.push()` beyond capacity; the allocator can be invoked during `Drop` |
| `tinyvec::ArrayVec` | ⚠️ Acceptable | Less ecosystem support than `heapless`; no SPSC queue |
| `smallvec` | ❌ Rejected | Falls back to heap when exceeding inline capacity — unacceptable |
| `bumpalo` arena allocator | ⚠️ Niche | Useful for one-shot per-cycle scratch buffers, but adds complexity; not needed if all structures are statically sized |

### Recommended Crate Versions

```toml
heapless = "0.9"        # Vec, String, SPSC/MPMC queues
static_assertions = "1.1" # Compile-time size/alignment checks
```

---

## Topic 2: mlock / Memory Pinning in Rust

### Decision

Call **`mlockall(MCL_CURRENT | MCL_FUTURE)`** once at process startup, **after** all static initialization and pre-allocation but **before** entering the RT loop. Use the `nix` crate for the call. Do **not** use `hugetlbfs` initially; add it only if TLB-miss profiling shows it's needed.

### Rationale

1. **`mlockall` vs `mlock`:** `mlockall(MCL_CURRENT | MCL_FUTURE)` is simpler and more reliable — it locks all current and future mappings, including stack, shared libraries, and SHM segments. `mlock` on individual ranges is error-prone (you must track every allocation). Every serious RT Linux application (LinuxCNC, OROCOS, ros2_control) uses `mlockall`.

2. **When to call:** After all pre-allocation is complete (driver loading, config parsing, SHM segment creation) but before the RT loop starts. Calling order in `HalCore::run()` / CU startup:
   ```
   1. Parse config, load drivers, create SHM segments
   2. Pre-allocate all RT data structures
   3. mlockall(MCL_CURRENT | MCL_FUTURE)
   4. Touch all pages (prefault) — memset or read every page
   5. Set RT scheduling (SCHED_FIFO)
   6. Enter RT loop
   ```

3. **`nix` crate vs raw `libc`:** Use `nix::sys::mman::mlockall` — it wraps the syscall with proper error handling and Rust-idiomatic `MlockAllFlags`:
   ```rust
   use nix::sys::mman::{mlockall, MlockAllFlags};
   mlockall(MlockAllFlags::MCL_CURRENT | MlockAllFlags::MCL_FUTURE)
       .expect("mlockall failed — RT memory pinning required");
   ```
   The `nix` crate is already a dependency of `evo_shared_memory` (v0.30.1 with `mman` feature).

4. **Page prefaulting:** After `mlockall`, touch every page of pre-allocated structures to force page faults now rather than in the RT loop:
   ```rust
   // Prefault stack
   let mut stack_prefault = [0u8; 1024 * 1024]; // 1MB
   std::hint::black_box(&mut stack_prefault);
   ```

5. **Hugetlbfs:** 2MB huge pages reduce TLB misses for large SHM segments. The HAL SHM is ~48KB — well within a single huge page but not large enough to justify the operational complexity of hugetlbfs configuration. **Defer** until profiling shows TLB misses in the cycle path. When needed: `mmap` with `MAP_HUGETLB` flag via `nix::sys::mman::mmap`.

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| Raw `libc::mlockall` | ⚠️ Acceptable | Works but requires `unsafe` block and manual error checking; `nix` is cleaner |
| `mlock` per segment | ❌ Rejected | Error-prone, doesn't cover stack/libs |
| Hugetlbfs from day one | ❌ Deferred | Adds sysadmin complexity (hugepage reservation); not needed for 48KB SHM |
| `madvise(MADV_HUGEPAGE)` (THP) | ⚠️ Future | Transparent Huge Pages can cause latency spikes due to compaction; avoid for RT until measured |

### Required Privileges

`mlockall` requires `CAP_IPC_LOCK` or `RLIMIT_MEMLOCK` >= process memory. Configure via:
```bash
# /etc/security/limits.d/evo-rt.conf
@evo-rt  -  memlock  unlimited
@evo-rt  -  rtprio   99
```
Or set `CAP_IPC_LOCK` capability on the binary.

---

## Topic 3: RT Thread Scheduling in Rust on PREEMPT_RT

### Decision

Use **`SCHED_FIFO` at priority 80** for the main RT cycle thread, with **CPU affinity pinning** to an isolated core. Use `libc::sched_setscheduler` for the scheduling policy (nix does not wrap it) and `nix::sched::sched_setaffinity` for CPU pinning.

### Rationale

1. **`SCHED_FIFO` vs `SCHED_DEADLINE`:**
   - `SCHED_FIFO` is the industry standard for cyclic RT control (LinuxCNC, EtherCAT masters, OROCOS). Priority 80 leaves headroom for kernel IRQ threads (typically 50) and allows a watchdog at higher priority (90+).
   - `SCHED_DEADLINE` is theoretically superior (CBS-based, prevents priority inversion by design) but has practical drawbacks: more complex configuration (runtime, deadline, period parameters), no support in many container runtimes, and limited tooling. **Defer** to a future iteration.

2. **Setting `SCHED_FIFO` from Rust:** The `nix` crate does **not** wrap `sched_setscheduler`. Use `libc` directly:
   ```rust
   use libc::{sched_param, sched_setscheduler, SCHED_FIFO};

   let param = sched_param { sched_priority: 80 };
   let ret = unsafe { sched_setscheduler(0, SCHED_FIFO, &param) };
   if ret != 0 {
       return Err(format!("sched_setscheduler failed: {}", std::io::Error::last_os_error()));
   }
   ```

3. **CPU affinity via `nix`:**
   ```rust
   use nix::sched::{CpuSet, sched_setaffinity};
   use nix::unistd::Pid;

   let mut cpu_set = CpuSet::new();
   cpu_set.set(isolated_cpu_id)?;  // e.g., CPU 2
   sched_setaffinity(Pid::from_raw(0), &cpu_set)?;
   ```

4. **cgroup v2 isolation** (system configuration, not Rust code):
   ```bash
   # /etc/systemd/system/evo-rt.slice
   # Isolate CPU cores 2-3 for RT tasks
   # Boot param: isolcpus=2,3 nohz_full=2,3 rcu_nocbs=2,3

   # cgroup cpuset for RT processes
   echo "2-3" > /sys/fs/cgroup/evo-rt/cpuset.cpus
   echo "0" > /sys/fs/cgroup/evo-rt/cpuset.mems
   ```

5. **Startup sequence:**
   ```
   1. Pre-allocate everything
   2. mlockall()
   3. Prefault pages
   4. sched_setaffinity() — pin to isolated core
   5. sched_setscheduler(SCHED_FIFO, 80)
   6. Enter RT loop
   ```
   Setting affinity before FIFO prevents the RT thread from starving other cores during the brief transition.

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| `SCHED_RR` | ❌ Rejected | Round-robin time quantum adds jitter; FIFO is deterministic for single-thread-per-core |
| `SCHED_DEADLINE` | ⚠️ Future | Better theoretical guarantees but operationally more complex; revisit when needed |
| `nix` for sched_setscheduler | ❌ N/A | Not exposed in `nix` as of v0.31; must use `libc` directly |
| `pthread_setschedparam` | ⚠️ Acceptable | Works for thread-level (vs process-level); slightly more portable but not needed here since we control the whole process |
| Priority 99 | ❌ Rejected | Leaves no headroom for watchdog or kernel migration threads |

### Required System Configuration

```bash
# Kernel boot parameters (GRUB)
GRUB_CMDLINE_LINUX="isolcpus=2,3 nohz_full=2,3 rcu_nocbs=2,3 nosoftlockup"

# RT privileges
# /etc/security/limits.d/evo-rt.conf
@evo-rt  -  rtprio   99
@evo-rt  -  nice     -20
@evo-rt  -  memlock  unlimited
```

---

## Topic 4: Cycle Time Measurement in Rust

### Decision

Use **`CLOCK_MONOTONIC` via `nix::time::clock_gettime`** for cycle timing, and **`clock_nanosleep` with `TIMER_ABSTIME`** for precise cycle pacing. No busy-wait; use absolute-time sleep for jitter-free cycle boundaries.

### Rationale

1. **Clock source:** `CLOCK_MONOTONIC` is the correct clock for interval measurement — it's not affected by NTP adjustments or `settimeofday`. `std::time::Instant` internally uses `CLOCK_MONOTONIC` on Linux but doesn't expose nanosecond precision or `clock_nanosleep` integration. Use `nix::time` directly.

2. **`clock_nanosleep` with absolute time** is the standard RT cycle pattern:
   ```rust
   use nix::time::{clock_gettime, clock_nanosleep, ClockId, ClockNanosleepFlags};
   use nix::sys::time::TimeSpec;

   let clock = ClockId::CLOCK_MONOTONIC;
   let cycle_ns: i64 = 1_000_000; // 1ms

   let mut next_wake = clock_gettime(clock)?;

   loop {
       // --- RT work ---
       let cycle_start = clock_gettime(clock)?;
       do_cycle_work();
       let cycle_end = clock_gettime(clock)?;

       // Measure actual cycle compute time
       let compute_ns = timespec_diff_ns(cycle_end, cycle_start);

       // Advance absolute wakeup time
       next_wake = timespec_add_ns(next_wake, cycle_ns);

       // Sleep until absolute time (drift-free)
       clock_nanosleep(clock, ClockNanosleepFlags::TIMER_ABSTIME, &next_wake)?;
   }
   ```
   **Why absolute time?** Relative sleep (`nanosleep`) accumulates drift because it doesn't account for the time spent doing work + the time spent entering/exiting sleep. Absolute time anchors each wakeup to a fixed grid.

3. **Busy-wait vs sleep:**
   - Pure busy-wait (`while clock_gettime() < deadline {}`) gives lowest jitter (~100ns) but wastes an entire CPU core and generates heat.
   - Pure `clock_nanosleep` gives ~1-5µs jitter on PREEMPT_RT, which is acceptable for a 1ms cycle (0.1-0.5% jitter).
   - **Hybrid** (sleep until 50µs before deadline, then busy-wait) can achieve ~200ns jitter. **Defer** this optimization — pure `clock_nanosleep` is sufficient for 1ms cycles.

4. **Jitter measurement pattern:**
   ```rust
   struct CycleStats {
       cycle_count: u64,
       last_cycle_ns: i64,
       min_cycle_ns: i64,
       max_cycle_ns: i64,
       sum_cycle_ns: i64,
       sum_sq_cycle_ns: i128, // for std dev
       overruns: u64,          // cycles exceeding deadline
       max_latency_ns: i64,   // max wakeup-to-actual latency
   }
   ```
   Update every cycle (O(1) — no allocation). Report via SHM diagnostic segment every N cycles.

5. **Note on `std::thread::sleep`:** The existing HAL `core.rs` and CU `main.rs` both use `std::thread::sleep(Duration::from_millis(1))`. This is **incorrect for RT** — `thread::sleep` uses `nanosleep` with relative time and has no clock specification. Must be replaced with `clock_nanosleep` + `TIMER_ABSTIME`.

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| `std::time::Instant` | ❌ Rejected for RT | No `clock_nanosleep` integration; cannot do absolute-time sleep |
| `std::thread::sleep` | ❌ Rejected | Relative sleep → drift accumulation; unspecified clock |
| `timerfd` (epoll-based) | ⚠️ Acceptable | Works but adds syscall overhead (epoll_wait + read); `clock_nanosleep` is simpler for single-thread cycle |
| Busy-wait only | ❌ Rejected | Wastes CPU core; prevents power management; not needed for 1ms cycles |
| Hybrid sleep+busywait | ⚠️ Future | Only if sub-µs jitter is required (it isn't at 1ms cycle) |

---

## Topic 5: SHM Struct Version Hashing at Compile Time

### Decision

Use a **`const fn` hash of `size_of::<T>()` + `align_of::<T>()`** combined with a **manually maintained `LAYOUT_VERSION: u32` constant** per struct. Supplement with `static_assertions` size checks. A proc-macro approach for full field-name hashing is a future enhancement.

### Rationale

1. **The problem:** HAL writes `HalShmData` as raw `#[repr(C)]` bytes. CU reads the same memory. If either side is compiled with a different struct definition (different field order, added field, changed type), silent data corruption occurs.

2. **Practical approach — `const fn` size+align hash:**
   ```rust
   /// Compile-time layout fingerprint.
   /// Changes whenever struct size or alignment changes.
   pub const fn layout_hash<T>() -> u64 {
       let size = std::mem::size_of::<T>() as u64;
       let align = std::mem::align_of::<T>() as u64;
       // FNV-1a style mixing
       let mut h: u64 = 0xcbf29ce484222325;
       h ^= size;
       h = h.wrapping_mul(0x100000001b3);
       h ^= align;
       h = h.wrapping_mul(0x100000001b3);
       h
   }

   // In HalShmHeader:
   pub const LAYOUT_HASH: u64 = layout_hash::<HalShmData>();
   ```
   This detects: added/removed fields (size change), type changes (size change), reordering that changes padding (size may change). It does **not** detect: reordering of same-sized fields, or type changes that happen to have the same size (e.g., `u32` → `f32`).

3. **Manual `LAYOUT_VERSION` as safety net:** Add a manually-bumped constant:
   ```rust
   impl HalShmData {
       /// Bump this whenever the struct layout changes.
       /// Reader refuses connection if version mismatch.
       pub const LAYOUT_VERSION: u32 = 1;
   }
   ```
   This catches same-size reordering that the hash misses.

4. **`static_assertions` for compile-time validation:**
   ```rust
   use static_assertions::{assert_eq_size, assert_eq_align};

   // Ensure HalShmData is exactly the expected size
   assert_eq_size!(HalShmData, [u8; 49472]);

   // Ensure header is 64 bytes
   assert_eq_size!(HalShmHeader, [u8; 64]);

   // Ensure axis data is 256 bytes
   assert_eq_size!(AxisShmData, [u8; 256]);
   ```

5. **Reader validation protocol:**
   ```rust
   fn validate_shm_connection(header: &HalShmHeader) -> Result<(), ShmError> {
       if header.magic != SHM_MAGIC {
           return Err(ShmError::InvalidMagic);
       }
       if header.layout_version != HalShmData::LAYOUT_VERSION {
           return Err(ShmError::LayoutMismatch {
               expected: HalShmData::LAYOUT_VERSION,
               found: header.layout_version,
           });
       }
       if header.layout_hash != HalShmData::LAYOUT_HASH {
           return Err(ShmError::LayoutHashMismatch);
       }
       Ok(())
   }
   ```

6. **Why not a proc-macro now?** A proc macro that hashes field names+types+offsets would be the most robust solution, but:
   - It requires a separate `proc-macro` crate and significant development effort.
   - The `size_of + align_of + manual version` approach catches 99% of real-world changes.
   - `memoffset::offset_of!` is not `const` — it cannot be used in compile-time hashing.
   - **Plan:** Implement the simple approach now; add a `derive(ShmLayout)` proc-macro in a future iteration.

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| Proc-macro field hash | ⚠️ Future | Most robust but high implementation cost; deferred |
| `build.rs` parsing source | ❌ Rejected | Fragile — source parsing is unreliable; proc-macro is the proper way |
| `bincode`/`serde` schema hash | ❌ Rejected | We're not using serialization for RT SHM; doesn't apply |
| `memoffset` for offset verification | ✅ Supplementary | Use in tests to verify field offsets match the SHM layout contract document |
| No versioning | ❌ Rejected | Silent data corruption is unacceptable in safety-critical motion control |

### Recommended Crate Versions

```toml
static_assertions = "1.1"  # Size/alignment compile-time checks
memoffset = "0.9"           # Field offset verification in tests
```

---

## Topic 6: HAL/CU SHM Incompatibility Resolution

### Decision

**Standardize on binary `#[repr(C)]` for ALL real-time SHM segments.** Remove JSON serde serialization from the Control Unit's RT path entirely. Define all SHM data structures as `#[repr(C)]` fixed-size structs in `evo_common` (shared between HAL and CU).

### Rationale

1. **The current incompatibility:**

   | Component | SHM Format | Write Method | Read Method |
   |---|---|---|---|
   | **evo_hal** | Binary `#[repr(C)]` | Raw pointer cast to `HalShmData` | — |
   | **evo_control_unit** | JSON via serde | `serde_json::to_vec()` → `writer.write()` | `serde_json::from_slice()` |

   These are **fundamentally incompatible**. HAL writes raw binary bytes at fixed offsets. CU writes JSON strings like `{"position":0.0,"velocity":0.0,...}`. They cannot read each other's data.

2. **Why binary `#[repr(C)]` wins:**

   | Metric | Binary `#[repr(C)]` | JSON serde |
   |---|---|---|
   | Write latency | **~50ns** (memcpy) | ~5-50µs (serialize) |
   | Read latency | **~50ns** (pointer cast) | ~5-50µs (deserialize) |
   | Allocation in hot loop | **Zero** | `to_vec()` allocates `Vec<u8>` every call |
   | Deterministic timing | **Yes** | No (JSON size varies with values) |
   | 1ms cycle budget | **<0.01%** | 1-10% |
   | SHM size | **Fixed, predictable** | Variable per write |

   At 1ms cycle time, JSON serialization alone could consume 1-10% of the cycle budget. The `serde_json::to_vec()` call on [evo_control_unit/src/main.rs](evo_control_unit/src/main.rs#L132) **allocates a `Vec<u8>` on every cycle** — a hard violation of the zero-allocation RT requirement.

3. **Migration plan:**

   **Step 1:** Move shared SHM structs to `evo_common::shm::types`:
   ```rust
   // evo_common/src/shm/types.rs
   #[repr(C, align(64))]
   pub struct CuShmData {
       pub header: CuShmHeader,
       pub machine_state: MachineState,    // u8 enum
       pub safety_state: SafetyState,      // u8 enum
       pub axes: [CuAxisData; MAX_AXES],
       pub cycle_count: u64,
       pub timestamp_us: u64,
   }

   #[repr(C)]
   pub struct CuAxisData {
       pub power_state: u8,       // PowerState enum as u8
       pub motion_state: u8,      // MotionState enum as u8
       pub operational_mode: u8,  // OperationalMode enum as u8
       pub target_position: f64,
       pub control_output: f64,
       pub error_code: u16,
       pub flags: u16,            // bitfield: ready, error, referenced, etc.
       _padding: [u8; 6],         // align to 32 bytes
   }
   ```

   **Step 2:** Replace CU's `serde_json::to_vec` / `from_slice` with direct pointer casts to/from the SHM `MmapMut`:
   ```rust
   // Write: zero-copy cast to SHM region
   let shm_ptr = mmap.as_mut_ptr() as *mut CuShmData;
   unsafe {
       (*shm_ptr).axes[0].target_position = target;
       (*shm_ptr).header.version.fetch_add(1, Ordering::Release);
   }

   // Read: zero-copy cast from SHM region
   let shm_ptr = mmap.as_ptr() as *const HalShmData;
   let actual_pos = unsafe { (*shm_ptr).axes[0].status.actual_position };
   ```

   **Step 3:** Delete `serde` and `serde_json` dependencies from `evo_control_unit/Cargo.toml`.

   **Step 4:** Existing `ControlState`, `ControlCommand` etc. in `evo_shared_memory/src/data/control.rs` contain `String`, `Vec<(String, f64)>`, `Option<f64>`, and Rust enums — ALL of these are **non-`repr(C)`-safe**:
   - `String` → replace with `[u8; 32]`
   - `Vec<(String, f64)>` → replace with `[(u32, f64); MAX_PARAMS]` + `param_count: u8`
   - `Option<f64>` → replace with `f64` + validity flag bit
   - Rust enums (e.g., `ControlMode`) → replace with `u8` + associated `const` values, or `#[repr(u8)]` enums

4. **P2P SHM segment model (from spec clarifications):**

   | Segment | Writer | Reader | Struct |
   |---|---|---|---|
   | `evo_hal_cu` | evo_hal | evo_control_unit | `HalShmData` (exists) |
   | `evo_cu_hal` | evo_control_unit | evo_hal | `CuCommandData` (new) |
   | `evo_re_cu` | evo_recipe_executor | evo_control_unit | `ReCuCommandData` (new, placeholder) |
   | `evo_rpc_cu` | evo_grpc | evo_control_unit | `RpcCuCommandData` (new, placeholder) |
   | `evo_cu_mqt` | evo_control_unit | evo_mqtt | `CuDiagnosticData` (new) |

   ALL are `#[repr(C)]` fixed-size binary.

5. **Non-RT paths may use serde:** Dashboard, diagnostics, and API modules that are not in the 1ms cycle path can continue using JSON/serde for configuration files, REST APIs, and logging. The rule is: **serde for config & diagnostics, `#[repr(C)]` for RT SHM.**

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| Standardize on JSON for all SHM | ❌ Rejected | Allocation + latency + variable size = incompatible with hard RT |
| Cap'n Proto / FlatBuffers zero-copy | ⚠️ Considered | Zero-copy read but still has encoding overhead on write; adds dependency; `#[repr(C)]` with known layout is simpler and faster |
| Protocol Buffers | ❌ Rejected | Serialization allocates; not zero-copy; designed for network, not SHM |
| `rkyv` (zero-copy deserialization) | ⚠️ Considered | Interesting — archived format is directly readable without deserialization. But adds ~15% write overhead vs raw `#[repr(C)]` and doesn't integrate with SHM pointer casts. Overkill when both sides share the same struct definition |
| Hybrid: binary for HAL↔CU, JSON for others | ❌ Rejected | Inconsistency; all RT paths should use the same mechanism |

### Impact on Existing Code

| File | Change Required |
|---|---|
| `evo_control_unit/src/main.rs` | Replace `serde_json::to_vec/from_slice` with pointer-cast SHM access |
| `evo_control_unit/Cargo.toml` | Remove `serde_json` dependency |
| `evo_shared_memory/src/data/control.rs` | Redesign `ControlState`, `ControlCommand`, `PerformanceMetrics` as `#[repr(C)]` fixed-size (no `String`, `Vec`, `Option`) |
| `evo_common/src/shm/` | Add new `types.rs` module with all shared RT SHM structs |
| `evo_hal/src/shm.rs` | Move `HalShmData` and related types to `evo_common::shm::types` (HAL re-exports) |

---

## Topic 7: Discrete-Time PID Implementation Patterns in Rust

### Decision

Use **backward Euler discretization** for integral and derivative terms, **back-calculation anti-windup** with configurable `Tt` parameter, and a **first-order low-pass filter on the derivative** with configurable `Tf`. All arithmetic in **`f64`**. Component disabling via zero-gain is handled by multiplication (P, FF terms) with explicit guard branches only for stateful components (I accumulator, D filter state).

### Rationale

1. **Backward Euler vs Tustin (bilinear) transform:**

   At sample rate $f_s = 1\text{kHz}$ ($T = 1\text{ms}$), motion control bandwidths are typically 10–200 Hz. The Nyquist frequency is 500 Hz. The difference between discretization methods is significant only when the signal frequency approaches Nyquist.

   | Method | Integral update | Derivative update | Complexity |
   |---|---|---|---|
   | **Backward Euler** | $I_k = I_{k-1} + K_i \cdot T \cdot e_k$ | $D_k = K_d \cdot \frac{e_k - e_{k-1}}{T}$ | Simplest |
   | **Tustin (bilinear)** | $I_k = I_{k-1} + K_i \cdot \frac{T}{2} (e_k + e_{k-1})$ | Requires previous derivative state | Moderate |
   | **Forward Euler** | $I_k = I_{k-1} + K_i \cdot T \cdot e_{k-1}$ | One-sample delay | Simplest but least accurate |

   **Backward Euler wins** because:
   - At 1kHz with control bandwidth <200Hz, the frequency warping error of backward Euler vs Tustin is <0.1% — unmeasurable in practice.
   - Backward Euler is unconditionally stable for any $T$ (implicit method), while forward Euler can go unstable with high gains.
   - Every major industrial PLC vendor (Siemens S7, Beckhoff TwinCAT, B&R Automation Studio) uses backward Euler for their standard PID function blocks.
   - Tustin adds one extra state variable (`prev_error` for integral) with negligible benefit at our sample rate.

2. **Anti-windup: back-calculation with `Tt` parameter:**

   Integral windup occurs when the output saturates at `OutMax` but the integral continues accumulating. Two strategies:

   | Strategy | Implementation | Quality |
   |---|---|---|
   | **Clamping** | Stop accumulating when output is saturated | Simple but causes sluggish recovery when leaving saturation |
   | **Back-calculation** | Feed saturation error back to integral with time constant $T_t$ | Smooth recovery, industry standard, tunable |

   Back-calculation formula (discrete):
   $$I_k = I_{k-1} + T \cdot \left( K_i \cdot e_k + \frac{1}{T_t} \cdot (u_{sat} - u_{unsat}) \right)$$

   Where:
   - $u_{unsat}$ = unclamped PID output
   - $u_{sat}$ = clamped output (`clamp(u_unsat, -OutMax, +OutMax)`)
   - $T_t$ = anti-windup tracking time constant

   **`Tt` tuning rules:**
   - For PI controller: $T_t = T_i = K_p / K_i$
   - For PID controller: $T_t = \sqrt{T_i \cdot T_d}$
   - Rule of thumb: $T_t = T_i$ (conservative, always safe)
   - Setting $T_t = 0$ **disables** back-calculation (clamping only via output limit)

   Back-calculation is the standard in Siemens `MC_PID`, Beckhoff `FB_BasicPID`, and MATLAB/Simulink PID blocks.

3. **Derivative filter with `Tf` parameter:**

   Pure derivative $D(s) = K_d \cdot s$ has infinite gain at high frequencies — it amplifies encoder quantization noise. A first-order low-pass filter on the derivative term is mandatory:

   $$D(s) = \frac{K_d \cdot s}{1 + T_f \cdot s}$$

   Discretized with backward Euler:
   $$D_k = \frac{T_f}{T_f + T} \cdot D_{k-1} + \frac{K_d}{T_f + T} \cdot (e_k - e_{k-1})$$

   Where $T_f$ is the derivative filter time constant. Typical values:
   - $T_f = T_d / N$ where $N = 8..20$ (classic textbook)
   - $T_f = 0.1 \cdot T_d$ is a safe default
   - $T_f = 0$ means unfiltered derivative (not recommended, but allowed)
   - When $K_d = 0$, the filter is irrelevant — the entire D term is skipped

   **Key implementation detail:** When `Kd = 0.0`, do NOT update `derivative_filtered` state — avoid accumulating stale state that would cause a transient spike if `Kd` is later changed to non-zero during tuning.

4. **`f64` vs fixed-point for control math:**

   | Criterion | `f64` | Fixed-point (Q32.32 or similar) |
   |---|---|---|
   | Precision | 52-bit mantissa (~15.7 decimal digits) | Depends on format; Q32.32 = ~9.6 digits |
   | Position range | ±10m at 0.001mm → 10⁷ counts, needs ~24 bits — **trivial** for f64 | Fits Q32.32 but marginal for large range + high resolution |
   | Multiply latency (x86_64) | **3-5 cycles** (~1-2ns) with FPU/SSE2 | 3-5 cycles (same or slower due to scaling) |
   | Accumulation error | Negligible at 1kHz for hours of operation | Requires careful scaling to avoid overflow |
   | Code complexity | Standard Rust `f64` operations | Custom types, manual scaling, error-prone |
   | Debugging | Directly readable | Requires conversion for every debug output |

   At 1kHz on x86_64 with hardware FPU, **`f64` has zero performance disadvantage** over fixed-point and massively better ergonomics. Fixed-point is only justified on embedded MCUs without FPU (ARM Cortex-M0/M3).

   **Encoder resolution check:** Typical industrial encoder: 8192 counts/rev. At 1mm/rev ball screw, resolution = 0.000122mm. Over 10m travel = 81,920,000 counts. `f64` represents this exactly (it's an integer < 2⁵³). No precision loss.

5. **Modular PID: zero-gain disables components:**

   The spec (FR-101) requires that setting gain = 0.0 completely disables any component. Implementation strategy:

   - **P term:** `Kp * error` — naturally zero when `Kp = 0.0`. No branch needed.
   - **I term:** Must **skip accumulation** when `Ki = 0.0` to prevent floating-point drift. Use `if ki != 0.0 { ... }` guard.
   - **D term:** Must **skip filter state update** when `Kd = 0.0`. Use `if kd != 0.0 { ... }` guard. This prevents stale derivative state.
   - **FF terms:** `Kvff * vel`, `Kaff * acc` — naturally zero when gain = 0.0. No branch needed.
   - **Friction:** `Friction * sign(vel)` — naturally zero when `Friction = 0.0`. No branch needed.
   - **DOB:** `if gDOB > 0.0 { ... }` — skip entire observer when disabled.
   - **Filters:** `if fNotch > 0.0 { ... }`, `if flp > 0.0 { ... }` — skip when disabled.

   The branch-on-zero pattern is preferred over always-compute because it avoids numerical artifacts (tiny accumulated drift) and makes debugging clearer (disabled component produces exactly 0.0).

### Recommended Rust Implementation

```rust
/// PID parameters — loaded from config, immutable during RT cycle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PidParams {
    pub kp: f64,       // Proportional gain
    pub ki: f64,       // Integral gain
    pub kd: f64,       // Derivative gain
    pub tf: f64,       // Derivative filter time constant [s]
    pub tt: f64,       // Anti-windup tracking time constant [s]
    pub out_max: f64,  // Output saturation limit
}

/// PID state — mutable, pre-allocated per axis.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PidState {
    pub integral: f64,             // Integral accumulator
    pub prev_error: f64,           // Previous cycle error
    pub derivative_filtered: f64,  // Filtered derivative term
}

impl PidState {
    /// Reset all state (on mode change, enable, or error recovery).
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Compute one PID cycle. Returns (output_saturated, output_unsaturated).
#[inline]
pub fn pid_compute(
    params: &PidParams,
    state: &mut PidState,
    error: f64,
    dt: f64,
) -> (f64, f64) {
    // P term (zero when Kp = 0.0, no branch needed)
    let p = params.kp * error;

    // I term with back-calculation anti-windup
    // (guarded: skip accumulation when Ki = 0.0)
    let i = if params.ki != 0.0 {
        state.integral
    } else {
        0.0
    };

    // D term with first-order low-pass filter
    // (guarded: skip state update when Kd = 0.0)
    let d = if params.kd != 0.0 {
        let tf = params.tf.max(dt); // Tf >= dt for numerical stability
        let alpha = tf / (tf + dt);
        state.derivative_filtered = alpha * state.derivative_filtered
            + (params.kd / (tf + dt)) * (error - state.prev_error);
        state.derivative_filtered
    } else {
        0.0
    };

    state.prev_error = error;

    let u_unsat = p + i + d;
    let u_sat = u_unsat.clamp(-params.out_max, params.out_max);

    // Update integral AFTER saturation check (back-calculation)
    if params.ki != 0.0 {
        let windup_correction = if params.tt > 0.0 {
            (u_sat - u_unsat) / params.tt
        } else {
            0.0
        };
        state.integral += (params.ki * error + windup_correction) * dt;
    }

    (u_sat, u_unsat)
}
```

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| Tustin (bilinear) transform | ⚠️ Acceptable | Better frequency preservation but negligible benefit at 1kHz; adds complexity |
| Forward Euler | ❌ Rejected | Conditionally stable; can diverge with high gains at large T |
| Clamping anti-windup | ❌ Rejected | Sluggish recovery from saturation; no tunability |
| Conditional clamping (stop when output×error > 0) | ⚠️ Acceptable | Better than pure clamping but less tunable than back-calculation |
| Fixed-point arithmetic | ❌ Rejected | No performance benefit on x86_64 with FPU; worse precision and ergonomics |
| `f32` instead of `f64` | ❌ Rejected | Only 23-bit mantissa; accumulation errors after hours at 1kHz; no performance benefit (x86_64 FPU operates in 80-bit internally) |
| Velocity-form PID (incremental) | ⚠️ Future | Bumpless transfer on parameter change; more complex; defer to tuning phase |
| Always-compute (no zero-gain guards) | ❌ Rejected | Causes integral drift and stale derivative state; harder to debug |

---

## Topic 8: Discrete-Time Disturbance Observer (DOB) Implementation

### Decision

Implement a **standard DOB with inverse nominal plant model and first-order Q-filter**. The Q-filter bandwidth is controlled by `gDOB` parameter (rad/s). DOB output is **additive** to the PID+FF output. Setting `gDOB = 0.0` disables the observer entirely with zero computational cost. No dynamic allocation; all state is pre-allocated per axis.

### Rationale

1. **Standard DOB architecture:**

   The DOB estimates an equivalent disturbance torque $\hat{d}$ acting on the plant and adds a compensating signal to the control output:

   ```
   ┌─────────────────────────────────────────────────────────┐
   │                  DOB Architecture                        │
   │                                                          │
   │  u_applied ─┬─→ [Plant] ──→ y (actual velocity)        │
   │             │                    │                       │
   │             │    ┌───────────────┘                       │
   │             │    ↓                                       │
   │             │  [Inverse Nominal Model]                   │
   │             │    │                                       │
   │             │    ↓ d_raw = Jn·ȧ + Bn·v - u              │
   │             │  [Q-filter (low-pass)]                     │
   │             │    │                                       │
   │             │    ↓ d_hat                                 │
   │             └────← (add to control output)               │
   └─────────────────────────────────────────────────────────┘
   ```

   The inverse nominal model converts measured motion into the torque that a nominal plant would require:
   $$d_{raw,k} = J_n \cdot \hat{a}_k + B_n \cdot v_k - u_{k-1}$$

   Where:
   - $J_n$ = nominal inertia [kg·m² or kg] (spec: `Jn` parameter)
   - $B_n$ = nominal viscous damping [N·s/m] (spec: `Bn` parameter)
   - $\hat{a}_k$ = estimated acceleration (from velocity difference)
   - $v_k$ = measured velocity (from HAL SHM)
   - $u_{k-1}$ = previously applied control torque

   The difference $d_{raw}$ between what the nominal model predicts and what was actually applied represents unmodeled dynamics: friction changes, load variations, tool engagement forces, belt tension shifts, etc.

2. **Q-filter design (`gDOB` parameter):**

   The raw disturbance estimate is noisy (especially the acceleration term from numerical differentiation). A low-pass Q-filter is mandatory:

   $$Q(s) = \frac{g_{DOB}}{s + g_{DOB}}$$

   This is a first-order low-pass with bandwidth $g_{DOB}$ [rad/s]. Discretized with backward Euler:

   $$\alpha = \frac{g_{DOB} \cdot T}{1 + g_{DOB} \cdot T}$$
   $$\hat{d}_k = (1 - \alpha) \cdot \hat{d}_{k-1} + \alpha \cdot d_{raw,k}$$

   **`gDOB` tuning guidelines:**
   | `gDOB` [rad/s] | Frequency [Hz] | Behavior | Use case |
   |---|---|---|---|
   | 0 | — | DOB disabled | Simple systems, no load variation |
   | 30–60 | 5–10 Hz | Slow, very smooth | Large inertia, high noise |
   | 60–200 | 10–32 Hz | **Typical industrial** | Standard servo axes |
   | 200–500 | 32–80 Hz | Fast, aggressive | Precision stages with good encoders |
   | >500 | >80 Hz | Risky at 1kHz | Approaches Nyquist; noise amplification |

   **Rule:** $g_{DOB}$ should be 2–5× lower than the PID bandwidth for stability. At 1kHz sample rate, keep $g_{DOB} < 300$ rad/s (safety margin to Nyquist at $\pi \cdot 1000 \approx 3142$ rad/s).

3. **Integration with PID loop (FR-102):**

   The DOB compensation is additive to the PID+FF output, applied **before** output filters:
   ```
   pid_output  = PID(error)
   ff_output   = Kvff * v_cmd + Kaff * a_cmd + Friction * sign(v_cmd)
   dob_output  = DOB(actual_velocity, applied_torque)
   
   raw_output  = pid_output + ff_output + dob_output
   filtered    = notch_filter(lowpass_filter(raw_output))
   final       = clamp(filtered, -OutMax, +OutMax)
   ```

   **Important subtlety — `u_applied` feedback:** The DOB needs to know what torque was actually applied (after saturation). Use the **previous cycle's saturated output** as `u_applied`, not the unsaturated value. This ensures the DOB correctly estimates the disturbance even when the output is clipping.

4. **Acceleration estimation:**

   Numerical differentiation of velocity is noisy. Two approaches:

   | Approach | Formula | Noise sensitivity |
   |---|---|---|
   | **First difference** | $\hat{a}_k = (v_k - v_{k-1}) / T$ | High — amplifies quantization |
   | **Q-filter handles noise** | Same, but Q-filter smooths result | Acceptable if gDOB < 200 rad/s |

   Since the Q-filter already provides low-pass filtering, using first-difference for acceleration is acceptable. The Q-filter bandwidth inherently limits the noise amplification of the differentiation. No separate acceleration filter is needed.

5. **Rust implementation — no dynamic allocation:**

   ```rust
   /// DOB parameters — loaded from config.
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct DobParams {
       pub jn: f64,     // Nominal inertia [kg or kg·m²]
       pub bn: f64,     // Nominal viscous damping [N·s/m or N·m·s/rad]
       pub g_dob: f64,  // Observer bandwidth [rad/s] (0 = disabled)
   }

   /// DOB state — pre-allocated per axis.
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct DobState {
       pub disturbance_estimate: f64,  // Filtered disturbance estimate
       pub prev_velocity: f64,         // Previous cycle velocity for differentiation
       pub prev_applied_torque: f64,   // Previous cycle's saturated output
   }

   impl DobState {
       #[inline]
       pub fn reset(&mut self) {
           *self = Self::default();
       }
   }

   /// Compute one DOB cycle. Returns estimated disturbance torque.
   #[inline]
   pub fn dob_compute(
       params: &DobParams,
       state: &mut DobState,
       actual_velocity: f64,
       dt: f64,
   ) -> f64 {
       if params.g_dob <= 0.0 {
           return 0.0; // DOB disabled
       }

       // Estimate acceleration via first difference
       let accel_est = (actual_velocity - state.prev_velocity) / dt;
       state.prev_velocity = actual_velocity;

       // Inverse nominal model: d_raw = Jn*a + Bn*v - u_prev
       let d_raw = params.jn * accel_est
           + params.bn * actual_velocity
           - state.prev_applied_torque;

       // Q-filter (first-order low-pass, backward Euler)
       let alpha = params.g_dob * dt / (1.0 + params.g_dob * dt);
       state.disturbance_estimate =
           (1.0 - alpha) * state.disturbance_estimate + alpha * d_raw;

       state.disturbance_estimate
   }
   ```

   **Memory per axis:** `DobState` = 3 × `f64` = 24 bytes. For 64 axes: **1.5 KB**. Negligible.

   **Compute per axis:** ~5 multiplies, 3 adds, 1 divide = ~10–15ns on x86_64. For 64 axes: ~1 µs.

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| Second-order Q-filter | ⚠️ Future | Better roll-off, but first-order is standard for DOB and sufficient at 1kHz |
| Extended state observer (ESO/ADRC) | ⚠️ Future | More general than DOB but significantly more complex to tune; DOB is better understood in industry |
| Kalman filter for disturbance | ❌ Rejected | Requires plant model tuning, matrix operations, much higher computational cost |
| No DOB (rely on integral term) | ❌ Rejected | Integral is too slow for sudden load changes (tool engagement); DOB responds within Q-filter bandwidth |
| Separate acceleration low-pass filter | ❌ Rejected | Redundant — Q-filter already smooths the result; extra filter adds phase lag |
| Higher-order inverse model | ⚠️ Future | Second-order model with compliance adds parameters; start with rigid-body model |

---

## Topic 9: State Machine Patterns in Rust for Orthogonal State Machines

### Decision

Use **`#[repr(u8)]` enums per state dimension** with **`match` statements** for transition logic. All 6 state machines per axis are stored in a flat `#[repr(C)]` struct within a pre-allocated `[AxisStateMachines; MAX_AXES]` array. Transitions are **inherently atomic** because the RT loop is single-threaded — all 6 dimensions for one axis are processed sequentially within one cycle iteration. Cross-axis error propagation is handled in a **separate pass** after all per-axis updates.

### Rationale

1. **Pattern comparison for 6 independent state machines per axis:**

   | Pattern | Memory | Dispatch | RT-safe | Fits pre-allocated array? |
   |---|---|---|---|---|
   | **Enum + match** | 1 byte per enum (`#[repr(u8)]`) | Static dispatch, zero overhead | ✅ Yes | ✅ Yes — plain `Copy` types |
   | Trait objects (State pattern) | 8-16 bytes per `Box<dyn State>` | Dynamic dispatch (vtable) | ❌ No — `Box` allocates | ❌ No — heap-allocated |
   | Typestate (compile-time) | Zero runtime | N/A — compile-time only | ✅ Yes | ❌ No — type changes at runtime |
   | `enum_dispatch` crate | 1 byte + inline enum | Static dispatch | ✅ Yes | ✅ Yes |
   | Statechart library (`statig`) | Varies | Varies | ⚠️ Maybe | ⚠️ Maybe |

   **Enum + match wins decisively** because:
   - `#[repr(u8)]` enums are 1 byte each, `Copy`, `Default`-able, and trivially embeddable in `#[repr(C)]` SHM structs.
   - `match` on enums compiles to jump tables — zero dynamic dispatch overhead.
   - The Rust compiler guarantees exhaustive matching — adding a new state forces handling in all transition functions (compile error if missed).
   - No external dependencies, no trait objects, no heap allocation.
   - Direct SHM-writability: the u8 representation is directly written to CU diagnostic SHM (FR-134).

2. **Memory layout for 64 axes × 6 state machines:**

   ```rust
   /// All state machines for one axis.
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct AxisStateMachines {
       // === State enums (6 bytes) ===
       pub power: PowerState,            // 1 byte
       pub motion: MotionState,          // 1 byte
       pub operational: OperationalMode, // 1 byte
       pub coupling: CouplingState,      // 1 byte
       pub gearbox: GearboxState,        // 1 byte
       pub loading: LoadingState,        // 1 byte
       _state_pad: [u8; 2],             // align to 8 bytes

       // === Transition timers (cycle counts for timeouts) ===
       pub power_timer: u32,            // cycles in current power state
       pub motion_timer: u32,           // cycles in current motion state
       pub gearbox_timer: u32,          // cycles in current gearbox state
       pub coupling_timer: u32,         // cycles in coupling wait

       // === Error state (packed) ===
       pub power_error: u16,            // PowerError as u16 bitfield
       pub motion_error: u16,           // MotionError as u16 bitfield
       pub gearbox_error: u16,          // GearboxError as u16 bitfield
       pub coupling_error: u16,         // CouplingError as u16 bitfield
       pub command_error: u16,          // CommandError as u16 bitfield
       _error_pad: [u8; 6],            // align to 8 bytes

       // === Safety flags ===
       pub safety: AxisSafetyFlags,     // 8 bytes (1 bit per flag, packed u8)

       _reserved: [u8; 8],             // future use, total = 56 bytes
   }

   // Size: 56 bytes per axis
   // 64 axes = 3,584 bytes (3.5 KB) — fits in L1 cache (32–64 KB)
   ```

   **Cache analysis:**
   - L1 cache line = 64 bytes on x86_64.
   - One `AxisStateMachines` (56 bytes) fits in a single cache line.
   - Sequential iteration over 64 axes is **perfectly cache-friendly** — linear memory access pattern.
   - Total state array: 3.5 KB. L1 data cache is typically 32–48 KB → **entire array stays in L1 during cycle**.

3. **Atomic state transitions (all 6 dimensions consistent):**

   The concern: if `PowerState` changes to `POWER_ERROR`, must `MotionState` also change to `EMERGENCY_STOP` in the same cycle?

   **Answer: yes, and it's automatically guaranteed** because the RT loop is single-threaded. Processing order for one axis within a single cycle:

   ```
   for axis in 0..axis_count {
       // Phase 1: Read inputs (SHM already read at cycle start)
       let input = &hal_input.axes[axis];

       // Phase 2: Safety flags (Level 4)
       update_safety_flags(axis, input);

       // Phase 3: State machines (Level 3) — ORDER MATTERS
       update_power_state(axis, input);      // may set POWER_ERROR
       update_motion_state(axis, input);     // reads power state, may set EMERGENCY_STOP
       update_operational_mode(axis);        // reads power + motion
       update_coupling_state(axis);          // reads motion state of self + master
       update_gearbox_state(axis, input);    // reads motion state
       update_loading_state(axis);           // reads power state

       // Phase 4: Control engine (if in appropriate state)
       if axes[axis].power == PowerState::Motion {
           compute_control(axis);
       }
   }
   // Phase 5: Cross-axis propagation
   propagate_coupling_errors();
   propagate_critical_errors(); // → may change global SafetyState
   ```

   All 6 state machines see **consistent state** because they execute sequentially. `update_motion_state` can directly read the `power` field that was just updated in the same cycle. No locks, no atomics, no consistency issues.

4. **Error propagation across state machines:**

   Two patterns of propagation:

   **Intra-axis (within one axis, same cycle):**
   - `PowerError::ERR_DRIVE_FAULT` → sets `power = PowerState::PowerError` → `motion_state` update sees `PowerError` and transitions to `MotionState::EmergencyStop`.
   - This is handled by the sequential processing order above.

   **Inter-axis (across axes, same cycle):**
   - `CouplingError::ERR_SLAVE_FAULT` on slave → propagates to master via `propagate_coupling_errors()` pass.
   - `MotionError::ERR_LAG_CRITICAL` on any axis (with `lag_policy == Critical`) → triggers global `SafetyState::SafetyStop` via `propagate_critical_errors()` pass.

   ```rust
   fn propagate_critical_errors(
       axes: &mut [AxisStateMachines; MAX_AXES],
       safety_state: &mut SafetyState,
       axis_count: usize,
   ) {
       for i in 0..axis_count {
           if axes[i].has_critical_error() {
               *safety_state = SafetyState::SafetyStop;
               // Trigger emergency stop on ALL axes
               for j in 0..axis_count {
                   axes[j].motion = MotionState::EmergencyStop;
               }
               return; // First critical error triggers global stop
           }
       }
   }
   ```

5. **Enum definition pattern:**

   ```rust
   /// Per-axis power management state.
   #[repr(u8)]
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
   pub enum PowerState {
       #[default]
       PowerOff = 0,
       PoweringOn = 1,
       Standby = 2,
       Motion = 3,
       PoweringOff = 4,
       NoBrake = 5,
       PowerError = 6,
   }

   // Transition function — pure, no side effects except state mutation
   fn update_power_state(
       sm: &mut AxisStateMachines,
       safety: &AxisSafetyFlags,
       input: &AxisShmStatus,
       config: &AxisConfig,
       dt_cycles: u32,
   ) {
       match sm.power {
           PowerState::PowerOff => {
               if /* start command received && all safety ok */ {
                   sm.power = PowerState::PoweringOn;
                   sm.power_timer = 0;
               }
           }
           PowerState::PoweringOn => {
               sm.power_timer += 1;
               if sm.power_timer > config.power_on_timeout_cycles {
                   sm.power = PowerState::PowerError;
                   sm.power_error |= PowerError::ERR_DRIVE_NOT_READY;
               } else if input.ready && safety.all_ok() {
                   sm.power = PowerState::Standby;
               }
           }
           // ... other states
           _ => {}
       }
   }
   ```

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| Trait objects (`Box<dyn AxisState>`) | ❌ Rejected | Heap allocation; dynamic dispatch overhead; cannot embed in `#[repr(C)]` SHM struct |
| Typestate pattern | ❌ Rejected | Encodes state in types — impossible when state changes at runtime per cycle; only works for compile-time state transitions (e.g., builder pattern) |
| `statig` crate (hierarchical state machines) | ⚠️ Considered | Provides hierarchical/orthogonal SM support, but adds dependency, unclear `#[repr(C)]` support, and overhead for our simple flat enums |
| `enum_dispatch` crate | ⚠️ Acceptable | Eliminates match boilerplate but adds proc-macro dependency; not worth it for 6 small enums |
| Bitfield-packed states (all 6 in one `u32`) | ⚠️ Considered | Saves memory but requires bit manipulation; harder to debug; 6 bytes for 6 enums is already tiny |
| State transition table (2D array lookup) | ⚠️ Acceptable | Data-driven, good for generated code; harder to express complex guards (safety conditions); `match` is more readable for hand-written transitions |

---

## Topic 10: Notch Filter and Low-Pass Filter Implementation

### Decision

Use a **biquad IIR filter (Direct Form II Transposed)** for the notch filter, and a **first-order IIR** for the low-pass filter. Coefficients are pre-calculated from Hz parameters at config load time (not in the RT cycle). Filter state (2 values for biquad, 1 value for low-pass) is pre-allocated per axis. Setting `fNotch = 0.0` or `flp = 0.0` disables the respective filter with zero computational cost.

### Rationale

1. **Biquad notch filter (2nd order):**

   A notch (band-reject) filter eliminates a specific resonance frequency from the control signal. In industrial servo systems, mechanical resonances (spindle + workpiece, belt drive, coupling) at 50–500 Hz cause oscillation. The notch filter removes the resonance without affecting other frequencies.

   **Transfer function (continuous):**
   $$H(s) = \frac{s^2 + \omega_0^2}{s^2 + \frac{\omega_0}{Q} s + \omega_0^2}$$

   Where $\omega_0 = 2\pi f_{Notch}$ and $Q = f_{Notch} / BW_{Notch}$.

   **Coefficient calculation (bilinear transform to z-domain):**

   ```rust
   /// Pre-calculate biquad notch coefficients.
   /// Called once at config load, NOT in RT cycle.
   pub fn notch_coefficients(f_notch: f64, bw_notch: f64, fs: f64) -> BiquadCoeffs {
       let w0 = 2.0 * std::f64::consts::PI * f_notch / fs;
       let cos_w0 = w0.cos();
       let sin_w0 = w0.sin();

       // Q factor: center_freq / bandwidth
       let q = f_notch / bw_notch;
       let alpha = sin_w0 / (2.0 * q);

       // Notch filter coefficients (Audio EQ Cookbook, Robert Bristow-Johnson)
       let b0 = 1.0;
       let b1 = -2.0 * cos_w0;
       let b2 = 1.0;
       let a0 = 1.0 + alpha;
       let a1 = -2.0 * cos_w0;
       let a2 = 1.0 - alpha;

       // Normalize by a0
       BiquadCoeffs {
           b0: b0 / a0,
           b1: b1 / a0,
           b2: b2 / a0,
           a1: a1 / a0,
           a2: a2 / a0,
       }
   }
   ```

   **Why bilinear transform (not impulse invariance or matched-Z)?**
   - Bilinear transform preserves frequency response shape with known frequency warping.
   - At $f_{Notch}$ << $f_s / 2$ (e.g., 120 Hz notch at 1 kHz sample rate), warping is small (~3% at 120 Hz).
   - Pre-warping correction: replace $f_{Notch}$ with $\frac{f_s}{\pi} \tan(\pi f_{Notch} / f_s)$ for exact center frequency. At 120 Hz / 1 kHz: corrected = 128.4 Hz. **Recommended** for precision.

   **Pre-warping formula:**
   ```rust
   let f_prewarped = (fs / std::f64::consts::PI)
       * (std::f64::consts::PI * f_notch / fs).tan();
   ```

2. **Direct Form II Transposed (DF2T) for biquad:**

   Of the four standard biquad implementations (DF1, DF2, DF1T, DF2T), **DF2T has the best numerical properties for f64**:

   ```rust
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct BiquadCoeffs {
       pub b0: f64, pub b1: f64, pub b2: f64,
       pub a1: f64, pub a2: f64,
   }

   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct BiquadState {
       pub s1: f64,  // state variable 1
       pub s2: f64,  // state variable 2
   }

   /// Process one sample through biquad filter (DF2T).
   /// 4 multiplies, 4 adds — ~2-3 ns on x86_64.
   #[inline]
   pub fn biquad_process(
       coeffs: &BiquadCoeffs,
       state: &mut BiquadState,
       input: f64,
   ) -> f64 {
       let output = coeffs.b0 * input + state.s1;
       state.s1 = coeffs.b1 * input - coeffs.a1 * output + state.s2;
       state.s2 = coeffs.b2 * input - coeffs.a2 * output;
       output
   }
   ```

   **Why DF2T:**
   - Only 2 state variables (vs 4 for DF1).
   - Better numerical stability with `f64` than DF2 (avoids large intermediate values).
   - Optimal for single-sample processing (no block processing needed).
   - Used by MATLAB `filter()`, Web Audio API, and most DSP libraries.

3. **First-order low-pass IIR:**

   The output low-pass filter (`flp` parameter) smooths the control signal to reduce high-frequency noise exciting mechanical resonances.

   **Coefficient calculation (exact):**
   $$\alpha = 1 - e^{-2\pi f_{lp} T}$$

   **Filter equation:**
   $$y_k = (1 - \alpha) \cdot y_{k-1} + \alpha \cdot x_k$$

   ```rust
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct LowPassCoeffs {
       pub alpha: f64,  // Pre-calculated: 1 - exp(-2π·flp·T)
   }

   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct LowPassState {
       pub y_prev: f64,
   }

   /// Pre-calculate low-pass coefficient.
   pub fn lowpass_coefficient(f_lp: f64, fs: f64) -> LowPassCoeffs {
       let alpha = 1.0 - (-2.0 * std::f64::consts::PI * f_lp / fs).exp();
       LowPassCoeffs { alpha }
   }

   /// Process one sample through first-order low-pass.
   /// 1 multiply, 2 adds — ~1 ns on x86_64.
   #[inline]
   pub fn lowpass_process(
       coeffs: &LowPassCoeffs,
       state: &mut LowPassState,
       input: f64,
   ) -> f64 {
       let output = state.y_prev + coeffs.alpha * (input - state.y_prev);
       state.y_prev = output;
       output
   }
   ```

4. **Filter state storage per axis:**

   ```rust
   /// Complete filter state per axis — pre-allocated.
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct AxisFilterState {
       pub notch: BiquadState,    // 16 bytes (2 × f64)
       pub lowpass: LowPassState, // 8 bytes (1 × f64)
   }

   /// Complete filter coefficients per axis — calculated at config load.
   #[repr(C)]
   #[derive(Debug, Clone, Copy, Default)]
   pub struct AxisFilterCoeffs {
       pub notch: BiquadCoeffs,     // 40 bytes (5 × f64)
       pub lowpass: LowPassCoeffs,  // 8 bytes (1 × f64)
       pub notch_enabled: bool,     // false when fNotch = 0
       pub lowpass_enabled: bool,   // false when flp = 0
   }
   ```

   **Memory for 64 axes:**
   - Filter state: 24 bytes × 64 = **1,536 bytes** (1.5 KB)
   - Filter coefficients: 50 bytes × 64 = **3,200 bytes** (3.1 KB)
   - Total: **~4.7 KB** — fits in L1 cache.

5. **Disable via zero frequency:**

   ```rust
   #[inline]
   pub fn apply_filters(
       coeffs: &AxisFilterCoeffs,
       state: &mut AxisFilterState,
       input: f64,
   ) -> f64 {
       let mut signal = input;

       if coeffs.notch_enabled {
           signal = biquad_process(&coeffs.notch, &mut state.notch, signal);
       }

       if coeffs.lowpass_enabled {
           signal = lowpass_process(&coeffs.lowpass, &mut state.lowpass, signal);
       }

       signal
   }
   ```

   When `fNotch = 0.0`, `notch_enabled = false` and the biquad is completely skipped (no multiply, no state update). Same for `flp = 0.0`.

6. **Example coefficient values at 1 kHz:**

   | Parameter | Value | Result |
   |---|---|---|
   | `fNotch = 120 Hz, BWnotch = 10 Hz` | Q = 12.0 | Sharp notch at 120 Hz, ~10 Hz wide |
   | `fNotch = 200 Hz, BWnotch = 30 Hz` | Q = 6.67 | Wider notch at 200 Hz |
   | `flp = 300 Hz` | $\alpha = 0.859$ | Gentle smoothing, -3 dB at 300 Hz |
   | `flp = 100 Hz` | $\alpha = 0.468$ | Moderate smoothing, -3 dB at 100 Hz |
   | `flp = 50 Hz` | $\alpha = 0.268$ | Strong smoothing, -3 dB at 50 Hz |

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| FIR notch filter | ❌ Rejected | Requires many taps (30–100) for sharp notch; 30–100× more computation than IIR biquad |
| Direct Form I biquad | ⚠️ Acceptable | Works but needs 4 state variables instead of 2; no numerical advantage in f64 |
| Direct Form II (non-transposed) | ❌ Rejected | Susceptible to large internal signals with narrow bandwidths; DF2T is strictly better |
| Second-order Butterworth low-pass | ⚠️ Future | Sharper cutoff; use if first-order is insufficient; adds biquad cost |
| Multiple cascaded notch filters | ⚠️ Future | For systems with multiple resonances; pre-allocate 2–3 biquad slots per axis; defer until needed |
| `dasp_signal` / `biquad` crate | ⚠️ Acceptable | Correct implementations exist but add dependency; 5 lines of inline code is simpler and auditable |
| Coefficient recalculation in RT cycle | ❌ Rejected | `sin()`, `cos()`, `exp()` are not deterministic-time; calculate once at config load |

---

## Topic 11: Cycle Budget Decomposition for 64-Axis Control Loop

### Decision

64 axes × full control engine (PID + FF + DOB + notch + low-pass + lag monitoring + state machines) **fits comfortably within a 1ms cycle** on modern x86_64. Estimated worst-case compute time: **40–80 µs** (4–8% of budget). No SIMD optimization needed for initial implementation. Reserve SIMD as a future optimization if profiling shows tight budget (unlikely).

### Rationale

1. **Detailed cycle budget decomposition:**

   Assumptions: x86_64 CPU @ 3.0+ GHz, data warm in L1/L2 cache (guaranteed by `mlockall` + prefaulting + isolated core), PREEMPT_RT kernel with `isolcpus`.

   | Phase | Operation | Per-axis | 64 axes | Notes |
   |---|---|---|---|---|
   | **1. SHM Read** | Read `evo_hal_cu` segment | — | **2–4 µs** | ~48 KB memcpy; L2→L1 cache fill |
   | | Read `evo_re_cu` segment | — | **0.5–1 µs** | Small command struct |
   | | Read `evo_rpc_cu` segment | — | **0.5–1 µs** | Small command struct |
   | | Heartbeat validation (3 segments) | — | **< 0.1 µs** | 3 × atomic load + compare |
   | **2. Safety Flags** | Evaluate AxisSafetyState flags | ~30 ns | **~2 µs** | 8 boolean checks per axis, branch-light |
   | **3. State Machines** | 6 state machines × match dispatch | ~60 ns | **~4 µs** | 6 × enum match + timer increment |
   | | Cross-axis error propagation | — | **~1 µs** | Worst case: scan all 64 axes for critical errors |
   | **4. Control Engine** | PID compute (P + I + D with filter) | ~15 ns | **~1 µs** | 8 multiplies, 6 adds, 2 branches |
   | | Feedforward (Kvff + Kaff + Friction) | ~5 ns | **~0.3 µs** | 3 multiplies, 2 adds |
   | | DOB (inverse model + Q-filter) | ~15 ns | **~1 µs** | 5 multiplies, 3 adds, 1 divide |
   | | Notch filter (biquad DF2T) | ~5 ns | **~0.3 µs** | 4 multiplies, 4 adds |
   | | Low-pass filter (1st order) | ~3 ns | **~0.2 µs** | 1 multiply, 2 adds |
   | | Output clamping + lag check | ~3 ns | **~0.2 µs** | 2 compares, 1 abs |
   | **5. SHM Write** | Write `evo_cu_hal` segment | — | **2–4 µs** | ControlOutputVector × 64 axes |
   | | Write `evo_cu_mqt` segment | — | **1–2 µs** | Diagnostic/state snapshot |
   | **6. Cycle Stats** | Update min/max/sum/overrun | — | **< 0.1 µs** | O(1) scalar ops |
   | **7. Overhead** | Function call overhead, cache misses, branch mispredicts | — | **5–10 µs** | Conservative estimate |

   **Total estimated compute time:**

   | Scenario | Time | % of 1 ms |
   |---|---|---|
   | **Best case** (data in L1, no cache misses) | ~25 µs | 2.5% |
   | **Typical** (warm L2, occasional miss) | ~40 µs | 4% |
   | **Worst case** (cold start, all misses) | ~80 µs | 8% |
   | **Pathological** (10× safety margin) | ~400 µs | 40% |

   Even the pathological 10× estimate leaves **600 µs of margin**. The budget is not tight.

2. **Why 64 axes × full control is fast:**

   - **Data size is small.** Per-axis control data (params + state) is ~200 bytes. 64 axes = ~12.8 KB. This fits entirely in L1 data cache (32–48 KB on modern x86_64). After the first cycle, all data is cache-warm.
   - **Computation is trivial.** The full control engine per axis is ~40 floating-point operations. At 3 GHz with 2 FP operations per cycle (SSE2 scalar), that's ~20 ns per axis. 64 axes = ~1.3 µs.
   - **Memory access is sequential.** Iterating `axes[0..64]` in order is the optimal access pattern for CPU prefetchers. Zero random access, zero pointer chasing.
   - **No system calls in the compute path.** Only `clock_gettime` (vDSO, ~20 ns) and `clock_nanosleep` at the end.

3. **Comparison with industrial references:**

   | System | Cycle time | Axes | CPU | Architecture |
   |---|---|---|---|---|
   | Beckhoff TwinCAT 3 | 100 µs | 64+ | x86_64 | Windows + RT extension |
   | LinuxCNC | 1 ms | 9 | x86_64 | PREEMPT_RT |
   | Siemens S7-1500T | 250 µs | 32 | ARM Cortex | Dedicated RTOS |
   | EtherCAT master (SOEM) | 1 ms | 64 | x86_64 | PREEMPT_RT |
   | **EVO (this system)** | **1 ms** | **64** | **x86_64** | **PREEMPT_RT** |

   Beckhoff achieves 100 µs with 64+ axes on similar hardware. Our 1 ms budget is **10× more generous** than the state of the art.

4. **SHM access — the dominant cost:**

   The SHM read/write phases dominate the budget (~8–12 µs out of ~40 µs). This is because SHM access involves:
   - `memcpy` of the entire segment (or pointer-cast read, but the CPU still loads cache lines).
   - Potential cache line bouncing if HAL writes to the same cache line recently.

   **Mitigation already in architecture:**
   - Separate `evo_hal_cu` (HAL writes) and `evo_cu_hal` (CU writes) segments — no false sharing.
   - Read-side and write-side are on separate cache lines.
   - Sequence counter protocol (odd = write in progress) prevents torn reads.

5. **When SIMD would be needed (and why it isn't now):**

   SIMD (AVX2/AVX-512) would help if:
   - The same operation is applied to many axes **with identical parameters** (SIMD needs uniform operation).
   - The bottleneck is FP throughput (it isn't — cache access dominates).
   - The cycle time were 100 µs instead of 1 ms.

   In practice, each axis has **different parameters** (Kp, Ki, etc.), so SIMD would require gather/scatter operations which negate much of the benefit. The control engine is **memory-bound, not compute-bound**.

   **If SIMD were ever needed**, the approach would be:
   - Transpose axis data from array-of-structs (AoS) to struct-of-arrays (SoA): `positions: [f64; 64]`, `velocities: [f64; 64]`, etc.
   - Use `std::arch::x86_64` intrinsics for AVX2 `_mm256_fmadd_pd` (4 axes per SIMD lane for f64).
   - Potential speedup: ~4× for the compute phase (from ~3 µs to ~0.8 µs). Not meaningful when total budget usage is 4%.
   - **Cost:** SoA layout complicates code; AoS is more natural for per-axis state machines.

   **Verdict:** AoS layout (current design) is correct. SIMD is not needed. Defer SoA/SIMD to post-profiling optimization (YAGNI).

6. **Optimization strategies if budget ever becomes tight:**

   Ordered by ease-of-implementation:

   | Strategy | Savings | Complexity | When to use |
   |---|---|---|---|
   | **Skip disabled axes** | Proportional to inactive count | Trivial | If <64 axes are active |
   | **Skip disabled components** | Already implemented (zero-gain guards) | Already done | Default |
   | **Reduce SHM write frequency** | Save 2–4 µs per skipped write | Low | Write diagnostics every Nth cycle |
   | **Batch safety propagation** | Save ~1 µs | Low | Only scan if any error flag set |
   | **Compile with `target-cpu=native`** | 5–15% overall | Trivial (Cargo flag) | Always |
   | **Profile-guided optimization (PGO)** | 5–10% overall | Moderate (build pipeline) | If needed |
   | **SoA layout + SIMD** | ~4× for compute only | High (data restructure) | Only if compute >50% of budget |

   The most impactful optimization is **skip disabled axes**: if only 8 of 64 axes are active, compute time drops from ~40 µs to ~12 µs.

   ```rust
   // Active axis bitmask — set at config load, updated on enable/disable
   let active_axes: u64 = config.active_axis_mask;

   for i in 0..axis_count {
       if active_axes & (1 << i) == 0 {
           continue; // Skip inactive axis — zero compute cost
       }
       // ... full control engine
   }
   ```

7. **Recommended `RUSTFLAGS` for RT binary:**

   ```toml
   # .cargo/config.toml
   [target.x86_64-unknown-linux-gnu]
   rustflags = [
       "-C", "target-cpu=native",     # Use all CPU features (AVX2, etc.)
       "-C", "opt-level=3",           # Maximum optimization
       "-C", "lto=fat",               # Link-time optimization across crates
       "-C", "codegen-units=1",       # Single codegen unit for best optimization
   ]
   ```

   This provides ~10–20% improvement over default release builds with zero code changes.

### Alternatives Considered

| Alternative | Verdict | Why |
|---|---|---|
| 500 µs cycle time | ⚠️ Future | Would require more careful budget management; 1 ms is comfortable |
| 2 ms cycle time (relaxed) | ❌ Rejected | Reduces control bandwidth; 1 ms is industry standard for servo |
| Multi-threaded axis processing | ❌ Rejected | Adds synchronization overhead, complexity, and jitter; single-threaded is faster for 64 axes (no lock contention, no thread wakeup latency) |
| GPU acceleration | ❌ Rejected | PCIe latency (~5–10 µs) exceeds the compute time being offloaded; only viable for >1000 axes |
| Separate control engine process | ❌ Rejected | IPC latency between processes exceeds the compute savings; keep everything in one RT thread |
| FPGA for control math | ⚠️ Future | Sub-µs cycle times possible; massive overkill for 1 ms × 64 axes on x86_64 |

---

## Summary of Key Decisions

| # | Topic | Decision |
|---|---|---|
| 1 | Zero-alloc RT loop | Fixed-size `#[repr(C)]` arrays + `heapless` for bounded collections; panicking allocator in CI |
| 2 | Memory pinning | `mlockall(MCL_CURRENT \| MCL_FUTURE)` via `nix` after pre-allocation; hugetlbfs deferred |
| 3 | RT scheduling | `SCHED_FIFO` priority 80 via `libc`; CPU affinity via `nix`; `isolcpus` kernel param |
| 4 | Cycle timing | `clock_nanosleep` + `TIMER_ABSTIME` on `CLOCK_MONOTONIC` via `nix`; replace `thread::sleep` |
| 5 | Struct version hash | `const fn` size+align hash + manual `LAYOUT_VERSION` + `static_assertions`; proc-macro deferred |
| 6 | HAL/CU SHM compat | **Binary `#[repr(C)]` for all RT SHM**; eliminate JSON serde from CU hot loop; move shared types to `evo_common` |
| 7 | Discrete-time PID | **Backward Euler** discretization; **back-calculation** anti-windup (Tt); **first-order LP** on derivative (Tf); **f64** arithmetic; zero-gain disables via guarded branches on stateful terms |
| 8 | Disturbance observer | **Inverse nominal model + first-order Q-filter**; bandwidth via `gDOB` [rad/s]; additive to PID+FF; `gDOB=0` disables; 24 bytes state per axis |
| 9 | State machines | **`#[repr(u8)]` enums + match** per dimension; 56 bytes per axis (3.5 KB for 64); sequential processing = inherent atomicity; two-pass error propagation |
| 10 | Filters | **Biquad DF2T** for notch; **first-order IIR** for low-pass; coefficients pre-calculated at config; `fNotch=0`/`flp=0` disables; 24 bytes state per axis |
| 11 | Cycle budget | **~40 µs typical for 64 axes** (4% of 1 ms); SHM I/O dominates; no SIMD needed; skip-inactive-axes is best optimization; compile with `target-cpu=native` |

### Dependency Summary

| Crate | Version | Purpose | Already in workspace? |
|---|---|---|---|
| `nix` | 0.30+ | mlockall, sched_setaffinity, clock_gettime, clock_nanosleep | ✅ Yes (evo_shared_memory) |
| `libc` | 0.2.150+ | sched_setscheduler (not in nix) | ✅ Yes |
| `heapless` | 0.9 | Fixed-capacity Vec, String, SPSC queue | ❌ Add |
| `static_assertions` | 1.1 | Compile-time size/alignment checks | ❌ Add |
| `memoffset` | 0.9 | Field offset verification in tests | ❌ Add (dev-dependency) |

*No additional crates needed for Topics 7–11. All control math, filters, state machines, and DOB are implemented as inline Rust code with zero external dependencies.*

### RT Startup Sequence (Consolidated)

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Parse config (TOML)                                      │
│ 2. Load drivers, create SHM segments                        │
│ 3. Pre-allocate ALL RT data structures                      │
│    - AxisStateMachines[64]          (3.5 KB)                │
│    - PidState[64] + PidParams[64]  (5.6 KB)                │
│    - DobState[64] + DobParams[64]  (3.1 KB)                │
│    - FilterState[64] + Coeffs[64]  (4.7 KB)                │
│    - Total RT state: ~17 KB (fits L1 cache)                 │
│ 4. Pre-calculate filter coefficients from Hz params         │
│ 5. Validate SHM layout hashes (reader side)                 │
│ 6. mlockall(MCL_CURRENT | MCL_FUTURE)                       │
│ 7. Prefault all pages (touch every page)                    │
│ 8. sched_setaffinity(isolated_cpu)                          │
│ 9. sched_setscheduler(SCHED_FIFO, 80)                      │
│ 10. next_wake = clock_gettime(CLOCK_MONOTONIC)              │
│ 11. ┌── RT LOOP ─────────────────────────────────────────┐  │
│     │ cycle_start = clock_gettime()                      │  │
│     │                                                     │  │
│     │ // Phase 1: Read (3–6 µs)                          │  │
│     │ read_shm(hal_cu, re_cu, rpc_cu)                    │  │
│     │ validate_heartbeats()                               │  │
│     │                                                     │  │
│     │ // Phase 2: Process (20–40 µs for 64 axes)         │  │
│     │ for axis in active_axes {                           │  │
│     │   evaluate_safety_flags(axis)                       │  │
│     │   update_state_machines(axis)  // 6 dimensions      │  │
│     │   if axis.power == Motion {                         │  │
│     │     pid_output = pid_compute(axis)                  │  │
│     │     ff_output  = feedforward(axis)                  │  │
│     │     dob_output = dob_compute(axis)                  │  │
│     │     raw = pid_output + ff_output + dob_output       │  │
│     │     filtered = apply_filters(axis, raw)             │  │
│     │     output = clamp(filtered, out_max)               │  │
│     │     check_lag_error(axis)                           │  │
│     │   }                                                 │  │
│     │ }                                                   │  │
│     │ propagate_errors()             // cross-axis        │  │
│     │                                                     │  │
│     │ // Phase 3: Write (3–6 µs)                         │  │
│     │ write_shm(cu_hal, cu_mqt)                          │  │
│     │                                                     │  │
│     │ // Phase 4: Housekeeping (<1 µs)                   │  │
│     │ update_cycle_stats()                                │  │
│     │ next_wake += cycle_time_ns                          │  │
│     │ clock_nanosleep(ABSTIME, next_wake)                 │  │
│     └─────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```
