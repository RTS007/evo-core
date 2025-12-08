# Soft Real-Time Linux Application Constitution

## Core Principles

### I. Soft Real-Time Performance Guarantees
- Time-critical paths MUST have documented target execution times and explicit deadlines with acceptable miss rates.
- All deadlines MUST be enumerated with unique identifiers, periods, criticality class (A/B/C), and maximum acceptable miss rates:
  - Class A (Critical): <0.01% miss rate, immediate degradation required
  - Class B (Important): <0.1% miss rate, graceful degradation permitted  
  - Class C (Best-effort): <1% miss rate, delay/drop acceptable
- Prefer bounded operations on critical threads; avoid unbounded locks and minimize memory allocation during deadline-sensitive operations.
- Priority inheritance/ceiling protocols MUST be explicitly implemented for any shared resources between different priority levels.
- Code that may cause deadline violations MUST implement strategies to maintain timing guarantees.
- Dynamic allocation in deadline-critical loops MUST be minimized and monitored for timing impact.
Rationale: Soft real-time requires explicit deadlines with controlled degradation; occasional misses are acceptable but must be bounded per criticality class.

### II. Test-First & Verification Hierarchy
- Write failing tests before implementation (TDD) for all functional and timing contracts.
- Multi-layer test strategy: (1) Unit (logic), (2) Contract/API, (3) Timing/Deadline, (4) Integration (I/O & concurrency), (5) System Acceptance (scenario & deadline validation), (6) Regression (historic perf baselines).
- Merge blocked unless: all tests green, coverage ≥ 90% critical modules, ≥ 85% for timing-sensitive components.
- Timing tests MUST assert deadline miss rates within acceptable bounds; single outliers acceptable if within miss rate budget.
Rationale: Early detection reduces uncertainty and protects timing guarantees.

### III. Code Quality & Static Analysis
- Compiler warnings MUST be eliminated; high-severity warnings MUST be resolved or explicitly waived with rationale.
- Mandatory static analysis (e.g., clang-tidy, cppcheck) gate; high severity issues MUST be resolved or explicitly waived with rationale.
- Cyclomatic complexity > 15 requires refactor or formal justification.
- All public interfaces MUST include pre/post conditions and error contract.
Rationale: High code quality correlates with better timing predictability and fewer defects.

### IV. Consistent Operator & System Interface Experience
- All CLI / API outputs MUST support both human-readable and JSON machine modes.
- Error messages: stable codes + structured payload (code, severity, cause, remediation).
- Configuration parameters MUST be centrally declared with: name, type, default, bounds, restart impact.
Rationale: Consistency accelerates debugging and safe operations in soft real-time contexts.

### V. Performance & Resource Bound Guarantees
- Each module MUST own explicit budgets: CPU %, memory peak, typical heap alloc count during operation.
- Performance regressions > 3% of documented budget block merge unless justified and budgets updated.
- Introduce performance baseline snapshot per release; CI MUST compare against last main.
- Cache and NUMA affinity policies documented; changes treated as architecture changes (MINOR/MAJOR).
Rationale: Bounded resource usage preserves timing guarantees.

### VI. Observability & Traceability
- Structured logging with timestamp (monotonic & wall), thread, component, severity.
- Performance-critical path logs MUST be optimized (lock-free ring buffer or deferred flush).
- Observability overhead MUST NOT exceed 2% of RT thread CPU budget; excessive tracing disables automatically.
- Mandatory tracepoints for: loop start/end, missed deadline, overload, state transition, configuration change.
- Every production incident MUST be trace-reconstructable from logs + metrics snapshot.
Rationale: Fast root cause analysis prevents recurrence and reduces MTTR without compromising timing.

### VII. Configuration & Versioning Discipline
- Immutable configuration after START unless explicitly declared hot‑reloadable.
- Semantic Versioning: MAJOR (breaking API/behavior or removed deadlines), MINOR (new principle/feature, new deadlines), PATCH (non-semantic clarifications, perf neutral refactors).
- All config schema changes require migration notes + backward compatibility strategy or explicit break notice.
Rationale: Predictable evolution keeps deployments stable and auditable.

### VIII. Security & Safety Boundaries
- Least privilege: processes/threads run with minimal capabilities; no ambient root for non-essential operations.
- All external inputs validated (range, format, rate). Validation code tested for failure paths.
- Memory safety tools (ASan/UBSan) run in non-RT debug profile nightly; findings triaged within 24h.
- Threat model document MUST be updated for new network endpoints or privilege elevation logic.
Rationale: Security regressions can compromise timing, safety, and integrity.

### IX. Simplicity & Minimal Dependencies
- Introduce a new dependency only if: (a) deterministic behavior proven, (b) license vetted, (c) provides net reduction in code complexity.
- Remove abstractions not pulling weight (no speculative layering).
- Prefer pure functions & data-oriented structures on RT paths.
Rationale: Simplicity reduces timing variance and defect surface.

### X. Change Review & Enforcement
- Each PR MUST map changes to affected principles explicitly in description.
- Principle violation requires either fix-in-PR or approved, time‑boxed exception (recorded in governance log).
- No self-approval for changes touching Principles I, II, V, or VI.
Rationale: Formal enforcement keeps principles living and effective.

### XI. Specification-Driven Development (SDD) Discipline
- All implementations MUST derive from formal specifications (*.spec files) following the standardized schema format with traceable lineage.
- Specifications MUST define: functional behavior, timing contracts, resource bounds, error conditions, and state transitions.
- Specification schema MUST be machine-readable (TOML) with automated validation and code generation capabilities.
- Code generation from specs MUST be deterministic and reproducible; manual deviations require architectural approval.
- Specification changes trigger impact analysis across dependent modules; breaking changes follow MAJOR version semantics.
- Every specification MUST include machine-readable contracts for automated validation and test generation.
Rationale: Formal specifications ensure consistency between intent and implementation while enabling automated verification.

### XII. Error Handling & Graceful Degradation
- All real-time paths MUST define explicit error recovery strategies with bounded execution time.
- System MUST support configurable degraded modes when deadlines cannot be met (fail-safe vs fail-operational).
- Error propagation MUST be non-blocking on critical paths; use lock-free error queues or immediate local handling.
- Recovery procedures MUST be tested under fault injection; MTTR (Mean Time To Recovery) documented per error class.
- No silent failures; all error conditions MUST be observable through metrics or tracepoints.
Rationale: Predictable error handling maintains system stability and enables graceful degradation under stress.

### XIII. Lifecycle & State Management
- System state transitions MUST be explicit, documented, and atomic with rollback capability.
- Initialization phase MUST complete all memory allocation and resource binding before entering real-time mode.
- Shutdown procedures MUST be deterministic with maximum termination time guarantees.
- State persistence (if required) MUST not block real-time operations; use separate background threads.
- Hot-reload capabilities MUST preserve timing guarantees or explicitly suspend real-time operations during reconfiguration.
Rationale: Well-defined lifecycle management prevents timing violations during state transitions.

### XIV. Memory Management & Data Layout
- Performance-critical threads MUST operate on pre-allocated, cache-aligned memory pools where feasible.
- Data structures MUST be designed for cache efficiency; hot paths avoid cache misses through layout optimization.
- Memory pools MUST be sized for expected load scenarios with documented headroom calculations.
- Garbage collection or automatic memory management MAY be used on non-critical paths; manual resource management preferred for critical paths.
- Memory access patterns MUST be predictable; avoid excessive pointer chasing and dynamic dispatch on critical paths.
Rationale: Predictable memory access patterns reduce latency variance and improve performance analysis accuracy.

### XV. Inter-Process Communication & Synchronization
- Performance-critical IPC MUST use zero-copy mechanisms where possible (shared memory, lock-free queues).
- Synchronization primitives MUST avoid unbounded blocking; prefer timeout-based or lock-free algorithms.
- Message passing protocols MUST define maximum message size, queue depth, timeout behavior, and backpressure policy per priority level.
- Cross-process dependencies MUST be explicitly modeled with deadlock detection and prevention.
- Network communication MUST implement rate limiting and priority handling for performance-sensitive traffic.
Rationale: Efficient IPC prevents performance bottlenecks and maintains responsive communication.

### XVI. Architectural Governance & Evolution
- Architectural decisions MUST be documented in ADR (Architecture Decision Record) format with rationale and alternatives considered.
- System architecture MUST be validated against soft real-time requirements through formal analysis or simulation.
- Component interfaces MUST be stable; breaking changes require deprecation period and migration strategy.
- Technology stack evolution MUST preserve timing guarantees; performance regression analysis mandatory.
- Architectural reviews MUST include timing impact assessment and resource utilization analysis.
Rationale: Structured architectural governance prevents timing regressions and maintains system coherence.

### XVII. Modular Library-First Architecture
- Every feature MUST begin as a standalone library with well-defined boundaries and minimal dependencies.
- Libraries MUST expose predictable programmatic interfaces with documented timing characteristics for performance-critical paths.
- Real-time and non-real-time functionality MUST be separated into distinct libraries with clear isolation guarantees.
- Library interfaces MUST be versioned independently; breaking changes require MAJOR version bump per library.
- Cross-library dependencies MUST be acyclic and explicitly documented in dependency graph.
- No feature implementation directly in application code without prior library abstraction.
Rationale: Modular design enables independent verification, testing, and timing analysis of components.

### XVIII. Deterministic Interface & Diagnostic Access
- All modules MUST expose programmatic interfaces with bounded execution time guarantees for performance-critical operations.
- Diagnostic interfaces MUST be non-blocking and accessible via async mechanisms (lock-free queues, shared memory snapshots).
- Human-readable diagnostics permitted only on non-real-time threads with explicit performance isolation.
- Configuration and status queries MUST complete within documented time bounds or fail deterministically.
- Emergency diagnostic access MUST not compromise timing guarantees under normal circumstances.
- Interface contracts MUST specify: max execution time, memory usage, and failure modes for performance-critical interfaces.
Rationale: Observability without timing compromise enables effective debugging and monitoring.

### XIX. Non-Real-Time Component Isolation
- All non-real-time components (visualization, logging, configuration tools, diagnostics, reporting) MUST run in separate processes with no shared resources with real-time threads.
- Data flow from real-time to non-real-time components MUST use lock-free, bounded buffers with overwrite-on-full semantics.
- Non-real-time component update rates MUST be decoupled from real-time loop frequencies; lag acceptable vs timing violation.
- Real-time system MUST remain functional with any non-real-time component disabled, crashed, or restarting.
- Non-real-time data sampling MUST not introduce jitter to real-time paths; use dedicated sampling threads or async mechanisms.
- All non-real-time frameworks and libraries MUST be validated for memory leak prevention in long-running scenarios.
- Non-real-time components MAY use standard system libraries, garbage collection, and dynamic allocation without restriction.
Rationale: Complete isolation ensures non-real-time functionality provides operational value without compromising timing guarantees.

### XX. Simulation & Development Mode Support
- Real-time system MUST support deterministic simulation mode for development and testing on non-real-time platforms.
- Simulation mode MUST replace real-time primitives with logical time simulation while preserving all functional behavior.
- Time advancement in simulation MUST be controllable (step-by-step, accelerated, or real-time pace) for testing scenarios.
- All timing contracts and deadlines MUST be validated in simulation using logical time rather than wall clock time.
- Simulation MUST support repeatability: identical inputs produce identical outputs regardless of host system performance.
- Mode switching between simulation and real-time MUST be early initialization decision with zero runtime overhead.
- Simulation data recording MUST capture: logical timestamps, state transitions, deadline violations, and resource usage for analysis.
- Development tools (debuggers, profilers, visualizers) MUST work seamlessly in simulation mode without affecting determinism.
Rationale: Simulation enables development, testing, and validation on standard hardware while maintaining behavioral correctness for soft real-time deployment.

### XXI. Fault Injection & Resilience Validation
- Fault class catalog (timing overruns, memory pressure, IPC delay, data corruption (detectable), watchdog trigger, I/O stall) MUST be maintained in docs/faults/.
- Release pipeline MUST run deterministic fault injection campaigns covering ≥ 90% of listed classes with pass criteria: (a) detection, (b) bounded recovery or safe state, (c) no uncontrolled deadline cascade.
- Injection framework MUST operate in both simulation (logical time) and hardware modes; hardware mode restricted to staging.
- Each critical error path MUST have at least one synthetic trigger in the automated test suite; missing trigger blocks merge.
- Chaos / randomized perturbation tests MUST NOT run on production real-time binaries; builds include an explicit CHAOS_BUILD flag disabled by default.
- MTTR targets per error class documented; measured MTTR drift > 20% vs baseline triggers review.
Rationale: Systematic, controlled exposure to failure modes hardens guarantees beyond nominal operating conditions.

### XXII. Supply Chain Provenance & Build Integrity
- Every build MUST emit a signed SBOM (components, versions, hashes, licenses) stored alongside artifacts.
- All release artifacts MUST be cryptographically signed; signature verification REQUIRED prior to deployment.
- Toolchain component hashes (compiler, linker, build scripts) MUST match a locked manifest; mismatch = hard fail.
- Reproducible build check (clean rebuild bit-for-bit) runs at least daily; divergence blocks release until resolved.
- Third-party dependencies MUST pass: license policy check, CVE scan (no HIGH/CRITICAL unresolved), determinism review for RT usage.
- Provenance attestation (SLSA-style) MUST bind: source commit, builder identity, environment digest, SBOM hash, signature.
- Network fetch during build is forbidden unless whitelisted with pinned hash.
Rationale: Strong provenance prevents silent supply chain compromises that could erode determinism or safety.

### XXIII. Performance Modeling & Latency Analysis
- All primary real-time tasks MUST be modeled with key performance parameters (typical execution time, target period, deadline, priority).
- A performance model MUST be maintained to analyze system load and identify potential bottlenecks under various conditions.
- End-to-end latency targets MUST be decomposed into budgets for key processing stages serving as performance goals.
- CI pipeline MUST run performance regression tests measuring and reporting on latency budgets and task execution times.
- Significant performance degradation (>10% increase in average execution time, frequent deadline misses in tests) MUST trigger performance review.
- System MUST track deadline miss rates and jitter as key health metrics, aiming to keep them below defined targets (e.g., <0.1% miss rate).
- Performance analysis results MUST inform capacity planning and architectural decisions for system evolution.
Rationale: While occasional deadline misses are tolerable in soft real-time systems, systematic performance analysis and monitoring prevent quality degradation and ensure responsive behavior.

### XXIV. System Resource Management & Isolation
- Real-time threads MUST be bound to dedicated CPU cores with cgroup isolation and IRQ affinity configuration.
- DVFS (Dynamic Voltage and Frequency Scaling) MUST be disabled or locked to maximum performance for RT cores.
- Non-RT processes MUST NOT share CPU cores with RT threads; use explicit cpuset isolation.
- System MUST implement CPU shielding preventing non-RT kernel threads from interfering with RT cores.
- Memory allocation for RT processes MUST use hugetlbfs or mlock to prevent page faults during execution.
Rationale: Hardware-level isolation ensures predictable RT performance independent of system load.

### XXV. Error Classification & Response Policy
- Error taxonomy MUST define standardized classes: RECOVERABLE, DEGRADABLE, FATAL with explicit response policies.
- RECOVERABLE errors trigger retry with exponential backoff and bounded attempt count.
- DEGRADABLE errors activate reduced-capability mode with documented performance impact.
- FATAL errors initiate controlled shutdown with state persistence and external notification.
- Error response selection MUST complete within bounded time; default to most conservative policy on timeout.
- Cross-module error propagation MUST follow defined escalation chains preventing cascade failures.
Rationale: Standardized error handling enables predictable system behavior under fault conditions.

### XXVI. Constitution Implementation Phases
- Phase 1 (Foundation): Principles I, II, III, VII, IX (timing, testing, quality, versioning, simplicity).
- Phase 2 (Architecture): Principles XI, XVII, XVIII (specs, modularity, interfaces).
- Phase 3 (Operations): Principles V, VI, XII, XIII (performance, observability, error handling, lifecycle).
- Phase 4 (Advanced): Remaining principles (security, supply chain, fault injection, simulation).
- Each phase MUST complete with documented compliance before proceeding to next phase.
- Phase implementation timeline: Phase 1 (sprint 1-2), Phase 2 (sprint 3-5), Phase 3 (sprint 6-8), Phase 4 (sprint 9-12).
Rationale: Phased implementation prevents overwhelming teams while establishing critical foundations first.

### XXVII. Timing Test Methodology & Environment
- Timing tests MUST run on isolated hardware with: dedicated cores, disabled interrupts, locked CPU frequency.
- Test environment MUST use kernel command line: isolcpus, nohz_full, rcu_nocbs for RT core isolation.
- Statistical analysis MUST report: mean, p95, p99, p99.9 latencies with sample size ≥10,000 iterations.
- Test results MUST be stable across multiple runs; >5% variance between runs indicates environmental issues.
- Timing test infrastructure MUST be separate from functional tests with dedicated CI resources.
- Performance baseline updates require architectural approval and documented rationale.
Rationale: Reliable timing measurements require controlled environment and statistical rigor.

## Additional Constraints
- Target platform: Linux with PREEMPT_RT patch (version list maintained in build manifest).
- Supported architectures documented; adding/removing architecture triggers MINOR.
- Build artifacts MUST be reproducible (bit-for-bit) with documented toolchain versions.
- Time sources: only CLOCK_MONOTONIC for scheduling; prohibit wall clock for deadlines.
- All third-party libraries pinned (hash + version). Supply chain scanning weekly.

## Development Workflow
1. Open issue describing requirement with: rationale, real-time impact, acceptance tests (including timing).
2. Draft spec using spec template referencing relevant principles explicitly.
3. Define tests (unit + timing) before implementation; ensure initial red state captured in CI.
4. Implement with incremental commits, each passing previously established tests.
5. Run full verification matrix locally (or pre-merge pipeline) including static + timing benchmarks.
6. Submit PR: include principle impact matrix + updated budgets if needed.
7. Review: dual reviewer requirement for Principles I, V, VIII changes.
8. Merge only after all gates green; tag release candidate if version bump implied.
9. Post-merge: update performance baselines and threat model if required.

## Governance
Amendment Procedure:
- Proposal issue citing current text, proposed change, classification (MAJOR/MINOR/PATCH), and rationale.
- Impact analysis: risk to determinism, safety, operator experience.
- Review window: 48h (PATCH), 5 business days (MINOR), 10 business days (MAJOR).
- Acceptance requires: (a) no unresolved critical objections, (b) updated tests/spec templates if impacted.

Compliance:
- Quarterly audit of repository for drift (tool-assisted + manual).
- Violations logged with remediation owner and deadline.
- Repeated violations escalate to architectural review.

Versioning:
- Constitution version increments per Semantic Versioning as defined in Principle VII.
- Ratification date fixed at initial adoption; Last Amended updates on any accepted change.
- Changelog maintained in repository (docs/governance/CHANGELOG.md) mirroring version jumps.

Exceptions:
- Temporary exception requests documented with expiry date (≤ 30 days) and mitigation plan.
- Unrenewed exceptions auto-expire; code must comply or revert.

Enforcement Tools:
- CI policy checks: principle tags present, latency tests exist for modified RT components, static analysis clean.
- Pre-commit hooks: forbid dynamic allocation markers in RT directories after initialization phase.

**Version**: 1.0.0 | **Ratified**: 2025-09-29 | **Last Amended**: 2025-09-29