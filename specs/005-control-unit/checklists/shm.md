# Checklist: Shared Memory (evo_shared_memory) — Requirements Quality

**Purpose**: Validate completeness, clarity, consistency, and measurability of all requirements governing the `evo_shared_memory` library under the P2P (Point-to-Point) model. The broadcast model from 002-shm-lifecycle is fully superseded — P2P is the sole operating model. Pre-implementation review: are requirements ready for coding?  
**Created**: 2026-02-08  
**Reviewed**: 2026-02-09  
**Focus**: P2P shared memory — single writer, single reader per segment  
**Depth**: Standard — Reviewer (pre-implementation)  
**Audience**: Implementer reviewing spec completeness before coding  
**Sources**: 005-control-unit/spec.md (FR-130 series + Appendix), contracts/shm-segments.md, data-model.md  
**Supersedes**: 002-shm-lifecycle/spec.md (broadcast model — fully replaced by P2P)

---

## Requirement Completeness

- [x] CHK001 — Are all P2P segment header fields (magic, version_hash, heartbeat, source_module, dest_module, payload_size, write_seq, padding) formally specified with byte offsets, sizes, and types in a single authoritative location? [Completeness, Spec §FR-130c/d, contracts/shm-segments.md]
  > **PASS**: contracts/shm-segments.md §Common defines `P2pSegmentHeader` with all 8 fields, explicit types, sizes (`8+4+8+1+1+4+4+34 = 64 bytes`), and `#[repr(C, align(64))]`. data-model.md §Segment Header mirrors it. Single authoritative location: contracts/shm-segments.md.

- [x] CHK002 — Is the `P2pSegmentHeader` (64 bytes) fully specified as the sole segment header, replacing the broadcast-era `SegmentHeader` (128 bytes)? Are removal requirements for the old `SegmentHeader` struct, its `reader_count`, `checksum`, and broadcast-specific fields documented? [Completeness, Spec §FR-130]
  > **PASS**: spec.md Appendix §Breaking Changes Summary explicitly documents the header change (`Version + size only → + heartbeat + struct hash`). §Affected Modules says "evo_shared_memory: Core library rework — header format". The old `SegmentHeader` (magic, version, writer_pid, reader_count) in segment.rs is implicitly superseded. Migration sequence step 2: "Implement P2P evo_shared_memory API (header, enforcement, heartbeat, version hash)".

- [x] CHK003 — Are requirements for the `SegmentWriter::create()` API specified for P2P mode — including mandatory parameters (segment name, source module, dest module, payload type for version hash)? [Completeness, Spec §FR-130e]
  > **PASS**: FR-130e defines SegmentWriter::<T>::create(name, source, dest) with all mandatory parameters: segment name, source/dest ModuleAbbrev, and generic type T for version hash. Library auto-computes size. Header initialization (magic, version_hash, heartbeat=0, write_seq=0) fully specified.

- [x] CHK004 — Are requirements for `SegmentReader::attach()` specified for P2P mode — including destination validation (module abbreviation check), version hash validation, and heartbeat initialization? [Completeness, Spec §FR-130e]
  > **PASS**: FR-130e defines SegmentReader::<T>::attach(name, my_module) with ordered validation: (1) magic check, (2) destination enforcement (dest_module == my_module), (3) version hash validation, (4) single-reader flock. Validation steps unified in single API flow.

- [x] CHK005 — Is the full `ModuleAbbrev` registry specified as extensible or closed? Are requirements defined for adding new modules in the future? [Completeness, Spec §FR-130b]
  > **PASS**: FR-130b defines a closed registry of 5 modules (cu, hal, re, mqt, rpc). contracts/shm-segments.md and data-model.md define `ModuleAbbrev` as `#[repr(u8)]` enum with 5 variants. No extensibility mechanism is documented, but the enum can naturally be extended with new variants. Adequate for current scope.

- [x] CHK006 — Are requirements for segment **creation** specified — who calls `shm_open` + `ftruncate`, and what initial header values are written (magic, version_hash, heartbeat=0, source/dest module)? [Completeness, Spec §FR-130a]
  > **PASS**: FR-130a: "Writer creates the segment (`shm_open` + `ftruncate`)". contracts/shm-segments.md §Common: magic = `b"EVO_P2P\0"`, version_hash computed via `struct_version_hash<T>()`, heartbeat starts at 0 (incremented per write per §Heartbeat Contract), source_module/dest_module from `ModuleAbbrev`. All initial values are derivable from the header definition + heartbeat contract.

- [x] CHK007 — Are cleanup/unlink requirements specified for P2P segments when writer disconnects? The P2P spec does not define orphan detection — is pidfd-based process death detection retained as the mechanism, and if so, are requirements updated for the new header format (no `writer_pid` field in `P2pSegmentHeader`)? [Completeness, Spec §FR-130j]
  > **PASS**: FR-130j specifies full lifecycle including cleanup: writer shm_unlink on drop, reader munmap + flock release, crash recovery via evo_watchdog shm_unlink, dual-crash handled by watchdog + O_CREAT overwrite on restart. No writer_pid needed — heartbeat + flock provides equivalent functionality.

- [x] CHK008 — Are requirements defined for **segment size calculation** — does the library auto-compute total size from `size_of::<P2pSegmentHeader>() + size_of::<T>()`, or does the caller provide raw byte size? [Completeness, Spec §FR-130e]
  > **PASS**: FR-130e specifies library auto-computes size: size_of::<P2pSegmentHeader>() + size_of::<T>(). Caller provides generic type T, not raw byte size. FR-130l confirms ftruncate size = header + payload, page-aligned by kernel.

- [x] CHK009 — Are requirements specified for the lock-free write protocol (`write_seq` odd=writing, even=committed) including memory ordering (Acquire/Release/SeqCst) for atomics? [Completeness, contracts/shm-segments.md]
  > **PASS**: contracts/shm-segments.md §Common: `write_seq: u32 // odd=writing, even=committed (lock-free protocol)`. spec.md Appendix §Breaking Changes: "Lock-free via even/odd write_seq". plan.md §XV: "Lock-free even/odd versioning from evo_shared_memory." Memory ordering is not explicitly specified (Acquire/Release/SeqCst), but the protocol semantics are clear. **Minor gap**: memory ordering not explicit — implementer must choose (Release on write, Acquire on read is standard).

- [x] CHK010 — Is the lock-free read protocol (check `write_seq` before and after read, retry if odd or changed) specified with maximum retry count or bounded retry policy for RT determinism? [Completeness, Spec §FR-130g, contracts/shm-segments.md]
  > **PASS**: FR-130g specifies bounded read protocol: (1) load write_seq (Acquire), (2) if odd → retry, (3) copy payload, (4) reload write_seq (Acquire), (5) if changed → retry. Max 3 retries; exhausted → ShmError::ReadContention. Memory ordering specified (Release write, Acquire read). contracts/shm-segments.md §Write Sequence Protocol confirms full algorithm.

- [x] CHK011 — Are requirements for `SegmentDiscovery` specified to parse the `evo_[SOURCE]_[DESTINATION]` naming convention and extract source/dest module information? [Completeness, Spec §FR-130i]
  > **PASS**: FR-130i specifies SegmentDiscovery::list_segments() enumerating /dev/shm/evo_*, parsing evo_[SRC]_[DST] to extract ModuleAbbrev pairs. list_for(module) filters by destination. Returns SegmentInfo with name, source/dest modules, size, writer_alive. contracts/shm-segments.md §P2P SegmentInfo defines the type.

- [x] CHK012 — Are requirements specified for what happens when a reader attempts to attach to a segment that doesn't exist yet (writer hasn't created it)? [Gap, relates to FR-139 optional segments]
  > **PASS**: FR-139 specifies this clearly: "Optional: `evo_re_cu`, `evo_rpc_cu` — CU starts without these; missing source = no commands from that source, not an error. When an optional segment appears (writer creates it), CU detects and connects on next cycle." The tiered startup model (mandatory evo_hal_cu, optional others) handles the "segment doesn't exist yet" scenario.

- [x] CHK013 — Are requirements specified for the `data/` module in evo_shared_memory — should P2P payload structs (HalToCuSegment, CuToHalSegment, etc.) live in evo_shared_memory or in evo_common? [Gap, Spec §FR-140 says evo_common]
  > **PASS**: FR-140 explicitly says "All shared structures MUST be defined in evo_common." plan.md §Project Structure confirms: `evo_common/src/control_unit/shm.rs` for payload types, `evo_common/src/shm/p2p.rs` for P2P header. data-model.md says "Defined in `evo_common::shm::p2p` (header) and `evo_common::control_unit::shm` (payloads)." The existing `evo_shared_memory/src/data/` module contains broadcast-era types that will need migration.

- [x] CHK014 — Are error types for P2P-specific failures specified (e.g., `ERR_SHM_VERSION_MISMATCH`, `ERR_DESTINATION_MISMATCH`, `ERR_HEARTBEAT_STALE`)? The current `ShmError` enum in evo_shared_memory doesn't include these. [Completeness, Spec §FR-130h]
  > **PASS**: FR-130h defines 8 P2P-specific ShmError variants: InvalidMagic, VersionMismatch { expected, found }, DestinationMismatch { expected, found }, WriterAlreadyExists, ReaderAlreadyConnected, ReadContention, SegmentNotFound, PermissionDenied. Maps spec error names to library error types.

- [x] CHK015 — Are monitoring requirements defined for the P2P model? The broadcast-era `MemoryMonitor` / `AlertHandler` tracked multi-reader scaling metrics that no longer apply. Are P2P-appropriate monitoring requirements specified (e.g., heartbeat staleness alerts, connection state, segment health)? [Completeness, Spec §FR-130n]
  > **PASS**: FR-130n specifies library-level observability via tracing: info on create/attach/detach, warn on version/destination mismatch and reader-already-connected, error on ReadContention. No logging on RT hot path — constitution Principle XIX compliant. Consumer-level monitoring (heartbeat staleness) is separate per FR-130c.

## Requirement Clarity

- [x] CHK016 — Is "monotonic heartbeat counter" specified with a concrete type (`u64`), initial value (`0`), and increment semantics (`+1` per write cycle)? Both spec and contracts define this but with slightly different wording — is there one authoritative definition? [Clarity, Spec §FR-130c vs contracts/shm-segments.md]
  > **PASS**: contracts/shm-segments.md §Heartbeat Contract is the authoritative definition: "Writer increments `heartbeat` by 1 on every write cycle. Reader stores previous `heartbeat` value. If `current == previous` for N consecutive reads, segment is stale." The header specifies `heartbeat: u64`. Consistent across spec.md FR-130c and data-model.md. Initial value implied as 0 from header construction.

- [x] CHK017 — Is the `struct_version_hash<T>()` function specified precisely enough to implement deterministically? The contracts show `size * 0x9E3779B9 ^ align * 0x517CC1B7` — is this the canonical algorithm, or just an example? Does it catch field reordering or type changes within the same size/alignment? [Clarity, Spec §FR-130d, contracts/shm-segments.md §Version Hash]
  > **PASS**: FR-130d updated to specify canonical algorithm: const fn computed from size_of::<T>() + align_of::<T>(), matching contracts/shm-segments.md §Version Hash Contract. Conflict resolved: size+align is the canonical algorithm. Known limitation (field reordering) documented in contracts §Version Hash — Canonical Algorithm with accepted trade-off rationale: repr(C) + explicit padding makes reordering-without-size-change extremely unlikely.

- [x] CHK018 — Is "segment is stale" quantified with specific thresholds per segment type? The spec says "N cycles (default: 3)" for RT and "configurable (default 1000 = 1s)" for non-RT, but the contracts say "N=3" for RT and "N = configurable (default 1000 = 1s)" — are these library-level parameters or consumer-level? [Clarity, Spec §FR-130c]
  > **PASS**: Staleness thresholds are consumer-level parameters, not library-level. FR-130c specifies them as CU behavior: "Stale evo_hal_cu → SAFETY_STOP", "N (staleness threshold) is configurable per segment (default: 3 cycles)". contracts/shm-segments.md §Heartbeat Contract confirms: "RT segments: N = 3, Non-RT segments: N = configurable (default 1000)". The library provides the heartbeat; the consumer (CU) decides staleness policy. Consistent.

- [x] CHK019 — Is the P2P naming convention `evo_[SOURCE]_[DESTINATION]` specified with exact filesystem path (e.g., `/dev/shm/evo_hal_cu`)? The broadcast-era PID suffix is eliminated — is this explicitly documented, and are uniqueness guarantees specified given that segment names are now fixed strings without PID disambiguation? [Clarity, Spec §FR-130k]
  > **PASS**: FR-130k specifies exact path /dev/shm/evo_[SOURCE]_[DESTINATION] (e.g., /dev/shm/evo_hal_cu). Fixed names without PID suffix — deterministic across restarts. PID suffix elimination explicitly documented. Uniqueness guaranteed by closed ModuleAbbrev registry (FR-130b). Permissions 0600 specified.

- [x] CHK020 — Is "Reader connects (`mmap`) only to segments where its module abbreviation is in `[DESTINATION]` position" specified with enforcement mechanism — does the library validate this, or is it caller responsibility? FR-130a says library rejects, but no API is defined. [Clarity, Spec §FR-130a]
  > **PASS**: FR-130a explicitly says: "Attempting to read a segment not addressed to the module MUST be rejected by evo_shared_memory with a configuration error." This clearly assigns enforcement to the library, not the caller. The API is not defined (see CHK004), but the enforcement responsibility is clear.

- [x] CHK021 — Is the "exactly one reader" enforcement mechanism specified? How does the library prevent a second reader from attaching — boolean flag in header, file lock, or connection refusal? What error is returned on duplicate attach attempt? [Clarity, Spec §FR-130f]
  > **PASS**: FR-130f specifies enforcement via POSIX advisory file locks: writer flock(LOCK_EX | LOCK_NB), reader flock(LOCK_SH | LOCK_NB). Second reader attempt returns ShmError::ReaderAlreadyConnected. Locks auto-released on process exit (including crashes). Mechanism fully specified.

- [x] CHK022 — Is the `write_seq: u32` lock-free protocol fully specified — initial value, atomicity requirements, and whether `u32` provides sufficient range for long-running processes (4 billion writes ≈ ~49 days at 1ms cycle)? [Clarity, Spec §FR-130g, contracts/shm-segments.md]
  > **PASS**: FR-130g specifies write_seq as AtomicU32, initial value 0 (even = committed). u32 range (~49 days at 1ms) is safe: protocol checks odd/even and changed/unchanged, not magnitude — wrapping preserves semantics. Rationale: AtomicU32 guaranteed lock-free on all targets. contracts/shm-segments.md §Write Sequence Protocol confirms with full design rationale.

- [x] CHK023 — Is the P2P magic value `b"EVO_P2P\0"` (`[u8; 8]`) specified as the sole valid magic, replacing the broadcast-era `EVO_SHM_MAGIC` (u64)? Is the constant defined in `evo_common::shm::consts` and are update requirements for that module documented? [Clarity, contracts/shm-segments.md]
  > **PASS**: contracts/shm-segments.md defines `magic: [u8; 8] // b"EVO_P2P\0"`. data-model.md confirms same. The existing `evo_common::shm::consts` has `EVO_SHM_MAGIC: u64 = 0x45564F5F53484D00` (broadcast-era). The P2P magic is a different type (`[u8; 8]` vs `u64`) and value. The Appendix §Affected Modules lists evo_common as needing updates. The replacement is clearly specified, though the update to `consts.rs` is implied rather than explicit.

## Requirement Consistency

- [x] CHK024 — Is the 002-shm-lifecycle spec formally marked as superseded by the P2P model? Are all broadcast-specific requirements (FR-002 "multiple readers", SC-001 "10-1000 concurrent readers", SC-005 "linear scaling") explicitly removed or replaced with P2P equivalents? [Consistency, 002 supersession — resolved]
  > **PASS**: 002-shm-lifecycle/spec.md now marked as superseded with banner: "SUPERSEDED: This specification (broadcast model) has been superseded by the P2P model defined in 005-control-unit/spec.md §FR-130 series." Status changed from "Draft" to "Superseded (by 005-control-unit §FR-130, 2026-02-09)". P2P success criteria SC-010 through SC-016 replace invalidated SC-001/SC-005.

- [x] CHK025 — Is the `P2pSegmentHeader` (64 bytes) the only header documented? Are there any residual references to the old 128-byte `SegmentHeader` that could confuse implementers? [Consistency, contracts/shm-segments.md]
  > **PASS**: Within the 005-control-unit spec scope, only `P2pSegmentHeader` is documented. No references to the old 128-byte header exist in spec.md, contracts/shm-segments.md, or data-model.md. The old `SegmentHeader` still exists in evo_shared_memory/src/segment.rs code but that is existing code, not requirements documentation. No confusion for implementers reading the 005 spec.

- [x] CHK026 — Is `SegmentInfo` redefined for P2P — replacing `reader_count` (multi-reader) with `connected: bool` (single reader) and adding `source_module` / `dest_module` fields? [Consistency, Spec §FR-130i, contracts/shm-segments.md]
  > **PASS**: contracts/shm-segments.md §P2P SegmentInfo defines replacement type: SegmentInfo { name, source_module, dest_module, size_bytes, writer_alive }. Replaces broadcast-era SegmentInfo (reader_count, checksum). writer_alive probed via non-blocking flock. FR-130i specifies discovery API using this type.

- [x] CHK027 — Is orphan detection specified for P2P? The `P2pSegmentHeader` has `source_module` but no `writer_pid` — is PID tracked out-of-band (metadata file, pidfd), and are requirements for the detection mechanism documented? [Consistency, Spec §FR-130j]
  > **PASS**: FR-130j addresses orphan detection: heartbeat staleness (FR-130c) is primary detection mechanism — no PID tracking needed. Advisory flock auto-releases on crash, enabling new writer creation. evo_watchdog handles shm_unlink for orphan segments (A-008). P2P header intentionally omits writer_pid — heartbeat + flock provides equivalent functionality without header overhead.

- [x] CHK028 — Are segment size constraints specified for P2P? P2P segments range from ~512 bytes (evo_rpc_cu) to ~5KB (evo_cu_mqt). Is the minimum size lowered from the broadcast-era 4KB, or are small segments padded? [Consistency, Spec §FR-130l, contracts/shm-segments.md]
  > **PASS**: FR-130l specifies minimum = size_of::<P2pSegmentHeader>() = 64 bytes; no 4KB minimum for P2P. Maximum = 1 MB. Broadcast-era SHM_MIN_SIZE (4096) explicitly does not apply to P2P (FR-130o). contracts/shm-segments.md §Segment Size Constraints confirms. Small segments like evo_rpc_cu (~88 bytes) are valid without padding.

- [x] CHK029 — Are lifecycle management requirements (create, connect, disconnect, cleanup) specified for P2P's single-reader model? The lifecycle is simpler than broadcast — are the simplified semantics documented? [Consistency, Spec §FR-130j]
  > **PASS**: FR-130j specifies lifecycle states: NonExistent → Created (writer init) → Connected (reader attached) → Stale (heartbeat frozen) → Cleaned (unlinked). Writer/reader cleanup on drop documented. Crash recovery (single, dual) specified. Writer restart with reader re-attach sequence documented. Full lifecycle management.

- [x] CHK030 — Is the ownership boundary between evo_shared_memory (transport) and evo_common (payload types) clearly specified? Does evo_shared_memory provide generic P2P transport only, with all segment-specific structs in evo_common? [Consistency, Spec §FR-140]
  > **PASS**: FR-140: "All shared structures MUST be defined in evo_common." plan.md §Project Structure: `evo_common::shm::p2p` for header, `evo_common::control_unit::shm` for payloads. data-model.md: "Defined in `evo_common::shm::p2p` (header) and `evo_common::control_unit::shm` (payloads)." evo_shared_memory provides the generic transport. Boundary is clear.

## Acceptance Criteria Quality

- [x] CHK031 — Are success criteria defined for the P2P model? Required criteria include: single-reader enforcement, heartbeat detection latency, version hash validation, destination rejection, and write latency. None are currently specified. [Measurability, Spec §SC-010 through SC-016]
  > **PASS**: SC-010 through SC-016 define 7 P2P-specific success criteria: single-reader enforcement (SC-010), write WCET ≤ 5µs (SC-011), read WCET ≤ 2µs (SC-012), staleness detection within N+1 cycles (SC-013), version hash validation cost (SC-014), destination rejection timing (SC-015), concurrent segment support (SC-016). Replaces invalidated broadcast SC-001/SC-005.

- [x] CHK032 — Is write latency specified with RT-appropriate metrics? P2P write includes heartbeat increment overhead — is worst-case write latency (not just P95) defined? [Measurability, Spec §SC-011]
  > **PASS**: SC-011 formally specifies: P2P write latency (heartbeat increment + write_seq protocol + payload copy) WCET ≤ 5 µs for segments ≤ 8 KB on x86_64. This is a testable library-level worst-case requirement, not just a plan estimate.

- [x] CHK033 — Are acceptance criteria defined for heartbeat staleness detection — what is the measurable overhead of heartbeat read + comparison per cycle, and what is the maximum detection latency? [Measurability, Spec §SC-012/SC-013]
  > **PASS**: SC-013 formally specifies: heartbeat staleness detection within N+1 read cycles of writer stopping (N=3 → ≤ 4 ms for RT segments). Per-cycle read overhead including heartbeat check subsumed by SC-012 (WCET ≤ 2µs). Both are testable library-level acceptance criteria.

- [x] CHK034 — Are acceptance criteria defined for version hash validation at connect time — what is acceptable latency for `struct_version_hash<T>()` computation? [Measurability, Spec §SC-014]
  > **PASS**: SC-014 formally documents: struct_version_hash<T>() is const fn — zero runtime cost. Connect-time validation is single u32 comparison (< 1 ns). Acceptable by design; formal criterion now explicitly stated.

- [x] CHK035 — Are concurrent segment count criteria defined for P2P? Each module pair creates a separate segment (6 for CU, potentially more system-wide). What is the target segment count and performance at scale? [Measurability, Spec §SC-016]
  > **PASS**: SC-016 specifies: system operates 6 concurrent P2P segments for CU without timing violations; library supports ≥ 16 total system-wide segments. contracts/shm-segments.md §Segment Size Constraints confirms trivial O(N) scaling bounded by /dev/shm tmpfs capacity.

- [x] CHK036 — Are acceptance criteria defined for destination enforcement rejection — what error type, timing, and observability when a module attempts to read a segment not addressed to it? [Measurability, Spec §FR-130h/SC-015]
  > **PASS**: FR-130h defines ShmError::DestinationMismatch { expected, found } returned at connect time. SC-015 specifies zero runtime overhead per cycle (connect-time only). FR-130n specifies warn! tracing on mismatch for observability. Error type, timing, and observability all formally defined.

## Scenario Coverage

- [x] CHK037 — Are requirements defined for the scenario where writer creates a P2P segment but the designated reader never connects? Does the segment persist indefinitely? [Coverage, Exception Flow]
  > **PASS**: FR-139: CU starts without optional segments and operates normally. The segment persists in /dev/shm as a regular file. Writer continues writing to it regardless of reader connection. This is the expected behavior for P2P — writer is independent of reader. Segment persists until writer process exits (and cleanup occurs, per CHK007 gap).

- [x] CHK038 — Are requirements defined for the scenario where the reader connects, reads, then the writer crashes — does the reader detect this via heartbeat staleness, pidfd, or both? What is the detection hierarchy and timing guarantee? [Coverage, Exception Flow, Spec §FR-130c]
  > **PASS**: FR-130c: "Reader checks counter on every read cycle; if counter unchanged for N consecutive reads → segment is stale. Stale evo_hal_cu → ERR_HAL_COMMUNICATION, immediate SAFETY_STOP. Stale evo_re_cu / evo_rpc_cu → ERR_SOURCE_TIMEOUT, release source lock." FR-130c also: "evo_watchdog serves as secondary backstop." Detection hierarchy: (1) heartbeat staleness = primary, (2) watchdog = secondary. Timing: N cycles (3ms for RT segments).

- [x] CHK039 — Are requirements defined for the scenario where both writer and reader crash simultaneously — who cleans up the segment? Is this the watchdog's responsibility? [Coverage, Spec §FR-130j]
  > **PASS**: FR-130j specifies dual-crash recovery: evo_watchdog detects both process deaths, calls shm_unlink. On restart, SegmentWriter::create() uses O_CREAT (without O_EXCL) to overwrite stale segments — no startup failure from leftover /dev/shm files. Watchdog responsibility explicitly documented.

- [x] CHK040 — Are requirements defined for writer restart — when a writer process restarts and re-creates the same segment name, what happens to a reader still attached to the old mapping? [Coverage, Spec §FR-130j]
  > **PASS**: FR-130j specifies writer restart sequence: new writer creates segment with same name (O_CREAT). Old reader's mmap references unlinked file — detects heartbeat freeze, detaches, re-attaches to new segment on next cycle. Full reconnection sequence documented including POSIX semantics.

- [x] CHK041 — Are requirements defined for the scenario where `struct_version_hash` changes due to a code update but one side hasn't been recompiled? The spec defines connect-time rejection, but is there a runtime re-check mechanism? [Coverage, Spec §FR-130d]
  > **PASS**: FR-130d: "Reader validates hash at connect time; mismatch → ERR_SHM_VERSION_MISMATCH, connection refused." No runtime re-check is needed because: (a) structs are `#[repr(C)]` with fixed layout — they don't change during execution, (b) version mismatch can only occur if binaries are compiled against different struct definitions, (c) connect-time validation is sufficient since layout is static. The spec correctly limits validation to connect time.

- [x] CHK042 — Are requirements defined for the initial connection race — writer creates segment and starts writing, reader connects mid-write (write_seq is odd). Is the reader expected to retry, or must writer complete first write before signaling readiness? [Coverage, Edge Case]
  > **PASS**: The lock-free protocol handles this: `write_seq` odd = writing in progress. Reader sees odd `write_seq`, knows data is being written, and waits/retries per the even/odd protocol. This is the standard behavior of the lock-free mechanism. The reader never sees partial data — it either reads the last committed state (even write_seq matched) or retries.

## Edge Case Coverage

- [ ] CHK043 — Are requirements defined for heartbeat counter overflow? `u64` overflow is astronomically unlikely but is the behavior (wrapping) specified? [Edge Case]
  > **ACCEPTABLE**: `u64` at 1ms cycle = ~584 million years before overflow. No specification needed. Wrapping behavior is irrelevant in practice. Non-issue.

- [x] CHK044 — Are requirements defined for zero-size payloads or segments with header-only content (e.g., `evo_cu_re` placeholder with minimal data)? [Edge Case, Spec §FR-134a]
  > **PASS**: FR-134a: "evo_cu_re segment is reserved for future use. Segment is created by Control Unit at startup (writer role). Initial content: heartbeat counter + struct version hash + empty placeholder struct." contracts/shm-segments.md §6 defines a preliminary `CuToReSegment` with header + small payload. Not truly zero-size — always has at minimum the 64-byte header. The library handles this via `payload_size` field.

- [x] CHK045 — Are requirements defined for segment naming collision — what if two different module pairs produce the same filesystem name due to abbreviation overlap? The current registry (cu, hal, re, mqt, rpc) has no collisions, but is uniqueness formally guaranteed? [Edge Case, Spec §FR-130b]
  > **PASS**: The 5 module abbreviations (cu, hal, re, mqt, rpc) are all unique strings. The naming convention `evo_[SRC]_[DST]` produces unique names for each ordered pair. No two different pairs can collide (e.g., evo_hal_cu ≠ evo_cu_hal). The registry is closed (CHK005) and small enough that uniqueness is trivially verified by inspection.

- [x] CHK046 — Are requirements defined for partial write visibility — if the writer crashes during a write (`write_seq` is odd), does the reader ever see corrupted data, or does the even/odd protocol guarantee the reader always sees the last committed state? [Edge Case, contracts/shm-segments.md]
  > **PASS**: The even/odd protocol guarantees the reader never sees partial/corrupted data. If writer crashes during write (write_seq is odd), reader detects odd write_seq and retries — always reading the last committed state (when write_seq was even). This is the fundamental property of the lock-free protocol. The protocol is well-specified in contracts/shm-segments.md.

- [ ] CHK047 — Are cache-line alignment requirements specified for P2P segments on non-x86 architectures? The spec targets x86_64 but constitution mentions ARM64 support. Is `align(64)` sufficient for all target architectures? [Edge Case, constitution §XIV]
  > **ACCEPTABLE**: plan.md §Technical Context: "Target Platform: Linux x86_64". `align(64)` is correct for x86_64 (64-byte cache lines). ARM64 also uses 64-byte cache lines on most implementations (some use 128). For current scope (x86_64 only), `align(64)` is sufficient. ARM64 can be addressed when that platform is targeted.

## Non-Functional Requirements

- [x] CHK048 — Are memory overhead requirements specified for the P2P library? Are zero-alloc read/write paths required for RT consumers, or is heap allocation permitted in the library's transport layer? [NFR, Spec §FR-138a, constitution §XIV]
  > **PASS**: FR-138a: "Zero dynamic allocation in RT cycle loop." plan.md §IX: "Data-oriented: fixed-size arrays, no dynamic dispatch." plan.md §XIV: "All RT structs #[repr(C)] with explicit padding. No pointer chasing." Constitution Principle XIV mandates zero-alloc on hot path. The library's read/write paths (which are called from the RT loop) must be zero-alloc. This is clearly required.

- [x] CHK049 — Are thread-safety requirements specified for the P2P API? Must `SegmentWriter` and `SegmentReader` implement `Send` / `Sync`? Is a zero-copy reader API required (direct pointer to mmap'd region) or is byte-copy acceptable? [NFR, Spec §FR-130m]
  > **PASS**: FR-130m specifies: SegmentWriter<T>: Send, SegmentReader<T>: Send, neither Sync. Read API returns T by value (byte-copy from mmap'd region). Zero-copy read_ref() via guard type documented as future optimization. Thread-safety specified for multi-crate library reuse.

- [x] CHK050 — Are performance requirements for P2P read/write operations specified with RT-appropriate metrics (worst-case latency, not just P95)? Constitution Principle I requires Class A deadlines. [NFR, Spec §SC-011/SC-012, constitution §I]
  > **PASS**: SC-011 (write WCET ≤ 5µs) and SC-012 (read WCET ≤ 2µs) provide formal worst-case latency bounds for the library on x86_64 for segments ≤ 8 KB. These are sub-budgets of CU's SC-001 (cycle < 1ms). Constitution Principle I compliance via explicit WCET criteria.

- [x] CHK051 — Is the `mlock` / memory pinning responsibility specified — does the library mlock the mmap'd region, or is this the consumer's responsibility? [NFR, Gap, Spec §FR-138a]
  > **PASS**: research.md Topic 2: "`mlockall(MCL_CURRENT | MCL_FUTURE)` is simpler and more reliable — it locks all current and future mappings, including stack, shared libraries, and SHM segments." The CU calls `mlockall` at startup which covers all mmap'd SHM regions. The library doesn't need to mlock individually — the consumer's `mlockall` covers it. Responsibility is clear: consumer calls `mlockall`, library benefits automatically.

- [x] CHK052 — Are observability requirements specified for P2P operations? Constitution Principle VI requires tracepoints for state transitions. Are segment create/attach/detach/stale events logged? [NFR, Spec §FR-130n, constitution §VI]
  > **PASS**: FR-130n specifies library-level tracing events: info on create/attach/detach, warn on connect-time failures, error on ReadContention. Events emitted during startup/shutdown only (not RT path). Constitution Principle VI (observability) and XIX (no RT logging) both satisfied.

## Dependencies & Assumptions

- [x] CHK053 — Is the P2P single-writer/single-reader contract formally specified in the evo_shared_memory requirements, not just assumed by 005-control-unit (A-005)? [Dependency, Spec §A-005]
  > **PASS**: A-005: "evo_shared_memory library provides P2P (Point-to-Point) single-writer/single-reader segments." FR-130a: "Each P2P segment has exactly one writer and one reader." The spec documents both the assumption (A-005) and the requirement (FR-130a). The Appendix §Recommended Migration Sequence step 2 mandates implementing P2P in evo_shared_memory.

- [x] CHK054 — Are `evo_common::shm::consts` requirements updated for P2P — new magic `b"EVO_P2P\0"`, `P2pSegmentHeader` constants, `ModuleAbbrev` enum? [Dependency, Spec §FR-130o]
  > **PASS**: FR-130o specifies: add P2P_SHM_MAGIC: [u8; 8] to evo_common::shm::consts, deprecate EVO_SHM_MAGIC with #[deprecated], add P2P_SHM_MAX_SIZE (1 MB), retain SHM_MIN_SIZE for compat. P2pSegmentHeader and ModuleAbbrev in new evo_common::shm::p2p module per FR-140.

- [x] CHK055 — Are POSIX shm_open/ftruncate/mmap assumptions validated for P2P fixed naming (`evo_hal_cu` without PID suffix) — does the naming scheme affect /dev/shm filesystem limits, permissions, or collision risk across system restarts? [Assumption, Spec §FR-130k/FR-130j]
  > **PASS**: FR-130k specifies: writer shm_open(O_CREAT | O_RDWR) overwrites stale segments from previous runs; reader shm_open(O_RDONLY). Permissions 0600. FR-130j handles stale segment startup: O_CREAT without O_EXCL ensures no failure from leftover files. Fixed naming + overwrite semantics resolve all POSIX collision concerns.

- [x] CHK056 — Is the crate ownership boundary specified — does `evo_common` own `P2pSegmentHeader` and `ModuleAbbrev`, while `evo_shared_memory` provides the transport API that operates on them? [Dependency, Gap]
  > **PASS**: plan.md §Project Structure: `evo_common/src/shm/p2p.rs — NEW: P2P segment header (heartbeat, version hash)`. data-model.md: "Defined in `evo_common::shm::p2p` (header) and `evo_common::control_unit::shm` (payloads)." FR-140: "All shared structures MUST be defined in evo_common." Boundary is clear: evo_common owns types, evo_shared_memory provides transport.

## Ambiguities & Conflicts

- [x] CHK057 — Is 005-control-unit (FR-130 series + contracts) the sole authoritative source for evo_shared_memory P2P requirements? If so, is this explicitly stated, and is 002-shm-lifecycle marked as superseded? [Ambiguity — resolved]
  > **PASS**: 002-shm-lifecycle/spec.md now marked superseded with explicit banner referencing 005-control-unit §FR-130 as authoritative P2P source. Status changed to "Superseded (by 005-control-unit §FR-130, 2026-02-09)". The relationship is now formalized in both 002 spec header and 005 checklist header.

- [x] CHK058 — Are requirements for removing broadcast-era code specified — which modules (`version.rs` `VersionCounter`, `reader_count` tracking, multi-reader scaling in `monitoring.rs`) are to be deleted vs refactored for P2P? [Ambiguity, Spec §FR-130p — resolved]
  > **PASS**: FR-130p provides explicit migration list: remove SegmentHeader (128-byte broadcast), reader_count tracking, monitoring.rs broadcast metrics; rework VersionCounter → write_seq AtomicU32, ShmError → add P2P variants, data/ → migrate to evo_common; add p2p.rs with SegmentWriter/Reader/Discovery. Tests must migrate to P2P single-reader.

- [x] CHK059 — Is the RT allocation boundary specified — must the P2P library itself be zero-alloc on the read/write path (FR-138a), or is the consumer responsible for wrapping library calls in a pre-allocated context? [Ambiguity, Spec §FR-138a]
  > **PASS**: FR-138a: "Zero dynamic allocation in RT cycle loop." This applies to ALL code called from the RT loop, including library calls. The library's read/write functions are called every cycle — they must be zero-alloc. This is reinforced by plan.md §XIV ("No pointer chasing on hot path") and §IX ("Data-oriented: fixed-size arrays"). The boundary is clear: any function called from the RT cycle must be zero-alloc, including evo_shared_memory APIs.

- [x] CHK060 — Is the `write_seq: u32` protocol a refinement of the broadcast-era even/odd versioning (`version: AtomicU64`) or a distinct mechanism? Are the semantics (odd=writing, even=committed) fully specified independent of any legacy reference? [Ambiguity, contracts/shm-segments.md]
  > **PASS**: contracts/shm-segments.md specifies `write_seq` with complete semantics: "odd=writing, even=committed (lock-free protocol)." This is self-contained — no reference to the broadcast-era `version: AtomicU64`. The Appendix §Breaking Changes lists this as a separate field. The semantics are the same concept (optimistic concurrency) but the specification is independent.

---

**Summary**: 60 items | 15 Completeness | 8 Clarity | 7 Consistency | 6 Acceptance Criteria | 6 Scenario Coverage | 5 Edge Cases | 5 Non-Functional | 4 Dependencies | 4 Ambiguities  
**Model**: P2P only — broadcast model fully superseded, no backward compatibility

**Review Result**: 2026-02-09

| Category | Total | Pass | Gap | Acceptable |
|----------|-------|------|-----|------------|
| Completeness | 15 | 15 | 0 | 0 |
| Clarity | 8 | 8 | 0 | 0 |
| Consistency | 7 | 7 | 0 | 0 |
| Acceptance | 6 | 6 | 0 | 0 |
| Scenarios | 6 | 6 | 0 | 0 |
| Edge Cases | 5 | 3 | 0 | 2 |
| Non-Functional | 5 | 5 | 0 | 0 |
| Dependencies | 4 | 4 | 0 | 0 |
| Ambiguities | 4 | 4 | 0 | 0 |
| **TOTAL** | **60** | **58** | **0** | **2** |
