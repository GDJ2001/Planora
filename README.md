<div align="center">

# Planora

### Your PC, clearly.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logos/planora-horizontal-dark.svg" />
  <img alt="Planora" src="assets/brand/logos/planora-horizontal-light.svg" width="550" />
</picture>

See how processes use your computer’s hardware and resources.

[![Native CI](https://github.com/Planora/Planora/actions/workflows/native.yml/badge.svg)](https://github.com/Planora/Planora/actions/workflows/native.yml)
[![Rust 1.95.0](https://img.shields.io/badge/rust-1.95.0-blue.svg)](rust-toolchain.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

</div>

---

**Planora** is a high-performance, native micro-architectural hardware visualizer and task manager written in **Rust 2024** (pinned to Rust 1.95.0). It provides hardware topology inspection, high-frequency process telemetry with PID-reuse prevention, safe affinity controls, and bounded micro-architectural probes for **Windows 10/11 x86-64**, **Linux x86-64**, and **Apple Silicon macOS**.

Planora couples a strictly **unelevated graphical user interface** (`egui`/`eframe 0.37` with custom `epaint` vector graphics) with an **authorized, isolated privileged helper daemon** running over authenticated local IPC.

---

## Key Features

- **Micro-Architectural Topology Canvas**: Interactive pan/zoom canvas rendering physical packages, NUMA domains, core complexes, SMT logical processors, L1/L2/L3 cache hierarchies, and GPU compute nodes.
- **High-Frequency Telemetry**: Configurable sampling interval (50 ms – 1000 ms, default 100 ms) with measured overhead under 30 ms.
- **PID-Reuse Protection**: Tracks processes using both OS Process IDs (PID) and immutable OS creation identities (`SystemBasicProcessInformation` monotonic sequence numbers on Windows 11 with `FILETIME` fallback; `/proc/[pid]/stat` ticks on Linux).
- **Explicit, Safe Affinity Control**: Two-phase revalidation prevents race conditions or applying affinity sets to reused PIDs. Rejects empty or invalid masks.
- **Bounded Hardware Probes**:
  - **Sattolo Line & Page Cache Probes**: Generates a single cyclic permutation traversing 128-byte aligned nodes across working sets from 8 KiB to 64 MiB. Bounded by memory (≤ 64 MiB) and execution time (≤ 12 s), reporting min, median, p95, and p99 load latencies.
  - **Inter-Core CAS Latency Matrix**: Pairwise atomic ping-pong across 2–8 logical CPUs (≤ 20 s deadline).
  - **Architectural Honesty**: Probes requiring x86-specific capabilities (e.g. RDTSCP, CPUID, exact thread-to-core affinity, RAPL) are explicitly marked **Unavailable** on Apple Silicon or unsupported systems rather than simulated.
- **Strong Privilege Isolation**: The graphical desktop application never runs elevated. System modifications and privileged counters are mediated through an authorized helper with strict access control.
- **Design Tokens & SVG Iconography**: Native interface rendered using Planora brand tokens (`assets/design-tokens.json`) and 18 custom vector icons compiled into Rust constants.

---

## Architecture

Planora is structured as a cargo workspace with distinct crates separating domain contracts, hardware discovery, telemetry sampling, benchmarks, IPC authorization, and UI presentation:

```
Planora (Workspace - Rust 2024 / 1.95.0)
 ├── crates/planora-core       # Shared versioned contracts (Metric<T>, Topology, IPC commands)
 ├── crates/planora-topology   # Micro-architectural discovery (CPUID, OS topology APIs, GPUs)
 ├── crates/planora-telemetry  # Telemetry sampler, counter deltas, wraparound & rate calculations
 ├── crates/planora-bench      # Sattolo pointer-chasing and pairwise CAS core matrix
 ├── crates/planora-helper     # Privileged daemon, authenticated IPC, affinity enforcement
 └── crates/planora-ui         # Native eframe/egui desktop application with epaint canvas
```

### Versioned Metrics & Availability Contract

All telemetry and benchmark values are transported through `Metric<T>` contracts carrying unit, collection timestamp, source provenance, and availability status:

```rust
pub struct Metric<T> {
    pub value: Option<T>,
    pub unit: String,
    pub source: String,
    pub sampled_at_ms: u64,
    pub unavailable_reason: Option<String>,
}
```

Unavailable metrics are never coerced to zero. If a platform lacks a performance counter, RAPL power interface, or GPU driver, Planora explicitly renders the metric as **Unavailable** along with the concrete reason.

---

## Privilege & Authorization Runbook

Planora enforces local authorization before monitoring begins. The UI alerts the user when helper authorization is required and provides native elevation hooks.

### Windows 10 / 11
- **Service**: `PlanoraHelper` runs as `LocalSystem` in background (`planora-helper.exe --service`).
- **IPC**: Named pipe `\\.\pipe\planora-helper` restricted with an explicit Security Descriptor Definition Language (SDDL) ACL granting access only to the authorized user's SID (`PLANORA_ALLOWED_SID`).
- **Authorization**: Run `Authorize-Planora.ps1` with Administrator approval, or click the **Authorize Windows Helper** button inside Planora.

### Linux (Ubuntu 24.04 / Fedora)
- **Service**: `planora-helper.service` managed by `systemd`.
- **Privilege**: Granted `CAP_PERFMON` (preferred over broad `CAP_SYS_ADMIN`) via systemd unit file and sysusers `planora.conf`.
- **IPC**: Unix Domain Socket (`/run/planora/helper.sock`) owned by `planora:planora-users` (mode `0660`) with `SO_PEERCRED` caller UID/GID verification.
- **Authorization**: Integrated with Polkit (`org.planora.authorize.policy`) via `/usr/libexec/planora/authorize-user`.

### Apple Silicon macOS
- **LaunchDaemon**: `org.planora.helper.plist` installed in `/Library/LaunchDaemons/`.
- **Authorization**: Managed through macOS `SMAppService` authorization API.

---

## Building from Source

### Prerequisites
- **Rust Toolchain**: Pinned to Rust 1.95.0 (managed automatically via `rust-toolchain.toml`).
- **Windows**: [WiX Toolset v5](https://wixtoolset.org/) (for building MSI installer).
- **Linux (Ubuntu/Debian)**: `sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libpolkit-gobject-1-dev`
- **Linux (Fedora/RHEL)**: `sudo dnf install -y gcc clang cmake pkgconf-pkg-config libX11-devel libxkbcommon-devel wayland-devel rpm-build`
- **macOS**: Xcode Command Line Tools.

### Build Workspace Binaries

```sh
# Run cargo checks and tests
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# Build release binaries
cargo build --release --bins --locked
```

The compiled binaries will be in `target/release/`:
- `planora` (or `planora.exe`): Desktop application.
- `planora-helper` (or `planora-helper.exe`): Privileged local daemon.

---

## Packaging

Planora provides automated packaging scripts for all target operating systems:

### Windows (MSI Installer)
```powershell
dotnet tool install --global wix --version 5.0.2
wix build packaging/windows/Planora.wxs -d ProjectDir="$PWD" -d SourceDir="$PWD\target\release" -o dist/Planora-0.1.0-x64.msi
```

### Linux Ubuntu 24.04 (Debian Package)
```bash
bash packaging/linux/build-deb.sh
# Generates dist/planora_0.1.0_amd64.deb
```

### Linux Fedora 43 (RPM Package)
```bash
bash packaging/linux/build-rpm.sh
# Generates dist/planora-0.1.0-1.x86_64.rpm
```

### macOS Apple Silicon (Application Bundle & PKG)
```bash
bash packaging/macos/build-package.sh
# Generates dist/Planora-0.1.0-arm64.pkg and dist/Planora-0.1.0-arm64-app.zip
```

---

## Verified Hardware Benchmarks & Profiling

Planora's sampling overhead and cache benchmark methodology have been verified on physical hardware:

### Test Machine Configuration
- **Platform**: Windows 11 x86_64
- **Processor**: 11th Gen Intel(R) Core(TM) i7-1165G7 @ 2.80 GHz (4 Physical Cores / 8 Logical CPUs)
- **Cache Topology**:
  - L1: 48 KiB Data + 32 KiB Instruction per core
  - L2: 1280 KiB per core
  - L3: 12288 KiB (12 MiB shared by all logical CPUs)
- **GPU**: Intel(R) Iris(R) Xe Graphics

### Telemetry Sampling Overhead
- **Sample Count**: 30 consecutive intervals (at 50 ms rate)
- **Durations**:
  - `min`: 15.62 ms
  - `median`: 19.46 ms
  - `p95`: 30.90 ms
- *Result*: Background collection completes comfortably within the standard 100 ms polling budget with zero UI thread contention.

### Sattolo Line Probe Results
- **Permutation**: Sattolo cyclic algorithm ($j < i$) ensures a single cycle visiting every 128-byte node once.
- **Duration**: 2.33 seconds total execution time across 14 working set sizes.

| Working Set | Min Latency | Median Latency | P95 Latency | P99 Latency | Architectural Level |
|:------------|:------------|:---------------|:------------|:------------|:--------------------|
| **8 KiB**   | 9.01 ns     | 9.45 ns        | 10.91 ns    | 10.99 ns    | L1 Data Cache       |
| **16 KiB**  | 9.23 ns     | 10.03 ns       | 10.52 ns    | 10.52 ns    | L1 Data Cache       |
| **32 KiB**  | 9.42 ns     | 10.86 ns       | 14.28 ns    | 22.24 ns    | L1 Data Cache       |
| **64 KiB**  | 10.16 ns    | 11.08 ns       | 11.16 ns    | 12.57 ns    | L2 Cache            |
| **256 KiB** | 10.13 ns    | 10.82 ns       | 12.40 ns    | 15.21 ns    | L2 Cache            |
| **1.0 MiB** | 10.47 ns    | 11.06 ns       | 11.99 ns    | 54.00 ns    | L2 Cache            |
| **2.0 MiB** | 13.47 ns    | 14.42 ns       | 16.17 ns    | 19.44 ns    | L3 Cache            |
| **8.0 MiB** | 16.33 ns    | 18.85 ns       | 40.37 ns    | 66.88 ns    | L3 Cache            |
| **16.0 MiB**| 62.43 ns    | 84.97 ns       | 104.30 ns   | 113.22 ns   | Main DRAM           |
| **32.0 MiB**| 90.86 ns    | 100.54 ns      | 107.35 ns   | 120.61 ns   | Main DRAM           |
| **64.0 MiB**| 98.86 ns    | 104.55 ns      | 112.36 ns   | 117.35 ns   | Main DRAM           |

*Observation*: Latency transitions cleanly from L1/L2 (~9–11 ns) to L3 (~14–19 ns), followed by the step to main memory DRAM (~85–104 ns) once the 12 MiB L3 cache boundary is exceeded.

---

## Brand Assets & Design System

The repository includes the full set of Planora brand assets:
- [Interactive brand showcase](assets/brand/showcase.html)
- [Brand guidelines and design token documentation](docs/brand-guidelines.md)
- [Design tokens](assets/design-tokens.json) and [generated CSS](assets/design-tokens.css)
- [Vector logos](assets/brand/logos/) and [transparent PNGs](assets/brand/png/)
- Platform app icons: [Windows ICO](assets/brand/platforms/windows/planora.ico), [Linux PNGs/SVG](assets/brand/platforms/linux/), [macOS ICNS](assets/brand/platforms/macos/Planora.icns)
- 18 vector interface icons on a 24-pixel grid in [assets/brand/ui/](assets/brand/ui/)

To validate and rebuild brand assets:
```sh
npm install
python -m pip install -r scripts/requirements.txt
npm run brand:build
npm run brand:validate
```

---

## License

This project is licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
