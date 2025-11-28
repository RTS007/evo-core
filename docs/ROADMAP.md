# EVO Core Roadmap

A concise, near-term plan for the open-source core, progressing from first foundations to a pilot-ready runtime.

## Phase 1: Foundations
Goal: Establish minimal runtime and communication.

- [ ] Shared Memory lifecycle (single-writer segments, lock-free reads)
- [ ] Minimal IPC (events + state exchange)
- [ ] Basic API Liaison (gRPC) for commands
- [ ] Watchdog for process supervision

## Phase 3: Hardware Abstraction (HAL)
Goal: Connect to hardware and simulation.

- [ ] HAL traits and driver contracts
- [ ] Simulation driver for offline testing
- [ ] EtherCAT basic I/O (CoE)

## Phase 2: Logic Execution (MVP)
Goal: Execute machine logic safely and predictably.

- [ ] Integrate Rhai engine with sandbox
- [ ] Internal virtual bus (agent communication)
- [ ] Non-blocking state machine (tick-based)
- [ ] Hot-swap scripts without stopping runtime

## Phase 4: Control Loop & Determinism
Goal: Stable control on Linux PREEMPT_RT.

- [ ] RTOS preparations
- [ ] Control Unit main loop with safety hooks
- [ ] Jitter measurement and tuning
- [ ] Minimal CLI (start/stop, logs)

## Phase 5: Production Readiness (Pilot)
Goal: Usable in pilot deployments.

- [ ] Config loading/validation (YAML)
- [ ] Startup/shutdown and recovery procedures
- [ ] Basic diagnostics (health, alarms, metrics)
- [ ] Setup and operation docs

## Hardware & Lab (Parallel Track)
Goal: Prepare physical testbed and validate reliability.

- [ ] Lab setup (IPC, PSU, safety relays, wiring harnesses)
- [ ] Reference BOM and procurement list (IPC, I/O, drives, cabling)
- [ ] EtherCAT network bring-up (DC sync, topology, ESI management)
- [ ] Hardware validation matrix (I/O, motion, edge cases, power cycling)
- [ ] Environmental tests (temperature, EMI basics) – smoke tests

## Documentation & Release (Parallel Track)
Goal: Operational clarity and repeatable releases.

- [ ] Core architecture docs (SHM layout, single-writer rules)
- [ ] Developer handbook (build, test, CI, coding standards)
- [ ] Operator SOPs (startup/shutdown, recovery, service mode)
- [ ] Safety boundary documentation (what EVO controls vs. external Safety PLC)
- [ ] Versioning and release notes (SemVer, changelog, artifacts)
- [ ] Quickstart guide for simulation and first hardware run

Notes: Future topics (additional fieldbuses, certifications, IDE) will be planned later.
