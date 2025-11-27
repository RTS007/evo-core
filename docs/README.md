# EVO Core 🦀

**Universal, Hardware-Agnostic Industrial OS written in Rust.**

EVO Core is a real-time, multi-process operating system designed for industrial automation. It aims to break vendor lock-in by allowing engineers to run advanced control logic on generic IPCs (Industrial PCs) or Raspberry Pis, mixing and matching hardware from different manufacturers.

Built with **Rust**, it prioritizes memory safety, concurrency, and low-latency performance (< 50µs jitter).

---

## 🚀 Key Features

*   **Hardware Agnostic:** Don't be tied to a single vendor. Mix high-end servo drives (e.g., Beckhoff) with cost-effective I/O modules (e.g., Odot, Rtelligent) on the same EtherCAT bus.
*   **Real-Time & Deterministic:** Powered by a Rust-based `Control Unit` designed for `PREEMPT_RT` Linux kernels.
*   **Multi-Process Architecture:** Unlike traditional monolithic Soft-PLCs, EVO runs modules as independent processes. If a non-critical driver crashes, the core system keeps running ("Graceful Degradation").
*   **Hot-Swappable Logic:** Logic is written in **Rhai** scripting language. You can update the behavior of a specific machine agent without stopping the entire control loop.
*   **"Glass Box" Philosophy:** No black boxes. The core control loop is transparent, auditable, and open for inspection.

## 🏗️ Architecture

EVO Core uses a distributed architecture where components communicate via a high-performance **Shared Memory (SHM)** backbone for real-time data and **MQTT/gRPC** for management.

### Core Modules
1.  **EVO:** The watchdog process. It orchestrates the system, manages the SHM lifecycle, and restarts processes if they fail (Fail Fast strategy).
2.  **Recipe Executor:** The "Brain". A multi-agent script host that runs **Rhai** scripts to execute machine logic. It supports a "Virtual Bus" for communication between different parts of the machine logic.
3.  **Control Unit:** The deterministic loop (typically 1ms cycle) that maps hardware inputs to shared memory and executes safety-critical checks.
4.  **HAL (Hardware Abstraction Layer):** Decouples the logic from the physical hardware.
    *   *HAL Core:* Defines the traits for Digital I/O, Analog I/O, and Axes.
    *   *HAL Drivers:* Implementations for specific protocols (EtherCAT, Modbus, Simulation).
5.  **API Liaison:** Exposes the system state to the outside world via gRPC and MQTT.

### Communication Stack
*   **SHM (Shared Memory):** Lock-free, single-writer/multiple-reader architecture. Ensures data exchange between RT processes takes < 1µs.
*   **MQTT:** Used for asynchronous events, telemetry, and inter-process coordination.
*   **gRPC:** Used for synchronous commands (Start/Stop/Load Recipe) from external dashboards or CLIs.

## 🛠️ Technology Stack

*   **Language:** Rust 🦀 (Edition 2024)
*   **Scripting:** Rhai
*   **Communication:** gRPC (Tonic), MQTT (Rumqttc)
*   **OS Target:** Linux (x86_64 / ARM64) with PREEMPT_RT patch recommended for production.

## 📦 Installation & Getting Started

*(Coming Soon)*

### Prerequisites
*   Rust Toolchain (stable)
*   Protobuf compiler

### Building from Source
```bash
# Clone the repository
git clone https://github.com/RTS007/evo-core
cd evo-core

# Build the project
cargo build --release
```

### Running the Simulation
EVO Core includes a `HAL Driver: Simulation` mode, allowing you to test logic without physical hardware.

```bash
# Run the core with simulation config
./target/release/evo_main --config ./config/sim_profile.yaml
```

## 🗺️ Roadmap

*   **Phase 1:** Core Infrastructure (SHM, Watchdog, CI/CD).
*   **Phase 2:** Communication Backbone (API Liaison, Basic Dashboard).
*   **Phase 3:** Logic Engine (Rhai integration, Hot-Swap).
*   **Phase 4:** Hardware Integration (EtherCAT stack, Distributed Clocks).

## ⚠️ Safety Disclaimer

**EVO Core is a Standard Control System, not a Safety PLC.**
It is designed for process control. Functional safety functions (E-Stop, STO, Light Curtains) **MUST** be implemented using dedicated hardware safety relays or certified Safety PLCs in accordance with ISO 13849-1 / IEC 61508. Do not rely on EVO Core for life-critical safety functions.

## 📄 License

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.
See the [LICENSE](LICENSE) file for details.
