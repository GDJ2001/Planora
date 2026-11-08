# Planora

![Planora logo](assets/brand/logos/planora-horizontal-light.svg)

**Your PC, clearly.** See how processes and workloads use your computer's resources in real time.

Planora is an open-source, high-performance desktop hardware visualizer and system resource monitor written in Rust (2024 edition, pinned toolchain `1.99.1`). It features a native immediate-mode interface using `eframe`/`egui` 0.37 with `wgpu`, adhering strictly to the [Planora design tokens](assets/design-tokens.json) and security privilege separation guardrails.

---

## 🌟 Key Features

- **Smooth Monotone Telemetry Graphs**: Real-time monotone cubic time series charts with faint min..max variance bands and unit-aware adaptive round axis scaling (KiB/s, MiB/s, GiB/s, MB/s, GB/s).
- **Two-Layer Telemetry Truthfulness**: Unavailable sensors remain visibly `Unavailable` with typed reasons (`Unsupported`, `PermissionDenied`, `WarmingUp`, `Stale`). Zero synthetic zeros are ever fabricated.
- **Micro-Architectural Topology Visualizer**: CPUID and OS topology inspection mapping physical CPU sockets, NUMA nodes, core types (P-cores / E-cores), and L1/L2/L3 cache hierarchies.
- **Strict Privilege Separation**: Unelevated native GUI process communicating over authenticated local IPC (**Protocol v7**) with a privileged monitoring daemon (`planora-helper`).
- **Safe Process Affinity Editing**: Windows process affinity mutation re-validates target process creation identity (`FILETIME`) before applying masks to eliminate PID-reuse race conditions.
- **Local Recording & Replay (Format v5)**: Capture performance diagnostic sessions to `.planora.jsonl` with deterministic playback and export to CSV or JSON.
- **Automatic System Appearance Synchronization**: Instant synchronization with OS light/dark modes, high-contrast themes, system font scaling, and animation preferences.

---

## 💻 Supported Platforms & Installation

Planora distributes production-ready native packages for desktop and mobile ecosystems:

| Platform | Architecture | Primary Package | Sideload / Direct Download |
| :--- | :--- | :--- | :--- |
| **Windows** | x86-64 | [Setup Bundle (.exe)](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64-Setup.exe) | [Enterprise MSI (.msi)](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.msi) |
| **Ubuntu / Debian** | x86-64 | [Debian Package (.deb)](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/planora_1.0.0-1_amd64.deb) | GitHub Releases |
| **Fedora / RHEL** | x86-64 | [RPM Package (.rpm)](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/planora-1.0.0-1.fc44.x86_64.rpm) | GitHub Releases |
| **macOS** | Apple Silicon (arm64) | [macOS Package (.pkg)](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-arm64.pkg) | [App Bundle (.zip)](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-arm64-app.zip) |
| **Android Companion** | ARM64 / Universal | [Google Play Early Access](https://play.google.com/apps/testing/org.planora.android) | [Sideloading Guide](doc/release/GOOGLE_PLAY_PUBLISHING_GUIDE.md#4-direct-apk-sideloading-instructions-github-releases) |

### Windows Installation
Download and run `Planora-1.0.0-x64-Setup.exe`. The branded Burn installer registers the application under Program Files, installs desktop shortcuts, and registers the helper daemon with Windows Service Control Manager. Advanced hardware sensor monitoring is an explicit opt-in that utilizes the signed PawnIO kernel driver.

### Linux Installation
- **Ubuntu 24.04 / 26.04**:
  ```sh
  sudo dpkg -i planora_1.0.0-1_amd64.deb
  sudo apt-get install -f
  ```
- **Fedora 43 / 44**:
  ```sh
  sudo dnf install ./planora-1.0.0-1.fc44.x86_64.rpm
  ```
Helper authorization is managed through polkit (`pkexec`) and standard systemd user socket activation (`/run/planora/planora-v2.sock`).

### Android Early Access & Sideloading
- **Google Play Early Access**: Opt into our closed testing group via [Join Closed Testing on Google Play](https://play.google.com/apps/testing/org.planora.android) to receive automated companion updates. See the [Closed Testing Runbook](doc/release/CLOSED_TESTING_AND_PRODUCTION_ACCESS_GUIDE.md).
- **Direct Sideloading**: Sideloading instructions and offline installation procedures are documented in the [Google Play Publishing Guide](doc/release/GOOGLE_PLAY_PUBLISHING_GUIDE.md#4-direct-apk-sideloading-instructions-github-releases).

---

## 🏗️ Architecture & Security Model

```
+-------------------------------------------------------------+
|                 Planora GUI (Unelevated)                     |
|            eframe / egui 0.37 + wgpu | User Session         |
+-------------------------------------------------------------+
                              |
                     Authenticated Local IPC
              (Protocol v7 | 64 KiB Req / 8 MiB Resp)
               Named Pipe (Windows) / Unix Socket (Linux)
                              |
                              v
+-------------------------------------------------------------+
|              Planora Helper (Elevated Daemon)               |
|      LocalSystem (Windows) / Root (Linux) | No User Files   |
+-------------------------------------------------------------+
        |                     |                     |
        v                     v                     v
+---------------+     +---------------+     +---------------+
| System Telemetry|   |  CPU Topology  |   | Sensor Broker  |
| PDH / sysfs   |     | raw-cpuid     |     | Isolated Child |
+---------------+     +---------------+     +---------------+
```

1. **Privilege Isolation**: The GUI process (`planora`) **never elevates**. All elevated sampling and affinity actions are handled by `planora-helper`. The helper never reads or writes user-owned files, recordings, or export paths.
2. **IPC Authorization & Safety**: Windows named pipe uses strict SID-specific ACLs (`FILE_GENERIC_READ | FILE_GENERIC_WRITE`). Linux sockets validate peer UID credentials against `/var/lib/planora/authorized-uids/`. Absolute timeouts: 2s handshake deadline, 5s request deadline.
3. **Bounded Probes**: Micro-architectural cache and memory probes are strictly memory-capped (64 MiB working set cap, 72 MiB total budget) and time-limited with cooperative cancellation checks.

---

## 🛠️ Building & Verification

Planora requires the pinned **Rust 1.99.1** toolchain. Install prerequisites for your platform, then run the standard verification pipeline:

```sh
# Format check
cargo fmt --all -- --check

# Strict workspace linting
cargo clippy --workspace --all-targets --locked -- -D warnings

# Run all unit and integration tests
cargo test --workspace --locked

# Build release binaries
cargo build --release --bins --locked

# Verify brand icon assets
python scripts/export-icons.py --check
```

### Packaging from Source
- **Windows**: Run `powershell ./packaging/windows/Build-Planora.ps1` (requires WiX Toolset v5).
- **Debian / Ubuntu**: Run `bash packaging/linux/build-deb.sh`.
- **Fedora**: Run `bash packaging/linux/build-rpm.sh`.
- **macOS**: Run `bash packaging/macos/build-package.sh`.

---

## 📁 Repository Structure

| Directory | Description |
| :--- | :--- |
| [`crates/planora-core`](crates/planora-core) | Versioned IPC contracts (Protocol v7), metric availability states, physical validation |
| [`crates/planora-topology`](crates/planora-topology) | CPU and OS topology discovery via CPUID and OS APIs |
| [`crates/planora-telemetry`](crates/planora-telemetry) | System and process sampling workers (Windows PDH, Linux sysfs/eBPF) |
| [`crates/planora-bench`](crates/planora-bench) | Memory-budgeted, cancellable cache latency and bandwidth probes |
| [`crates/planora-helper`](crates/planora-helper) | Privileged background service, IPC authorization, affinity mutation |
| [`crates/planora-ui`](crates/planora-ui) | Unelevated native egui GUI application and view renderers |
| [`crates/planora-sensor-broker`](crates/planora-sensor-broker) | Windows C#/.NET worker managing LibreHardwareMonitor and PawnIO |
| [`packaging/`](packaging) | Windows WiX installer, Linux DEB/RPM specs, macOS bundle scripts |
| [`doc/release/`](doc/release) | Release runbooks, Google Play publishing guides, license matrix, and release notes |

---

## 🤝 Community & Contributing

We welcome community contributions, bug reports, and suggestions!
- Read our [Contributing Guidelines](CONTRIBUTING.md) for environment setup and PR workflows.
- Review our [Code of Conduct](CODE_OF_CONDUCT.md) (Contributor Covenant v2.1).
- Report security vulnerabilities responsibly following our [Security Policy](SECURITY.md).
- Browse the [Planora Brand Guidelines](docs/brand-guidelines.md) and [Showcase](assets/brand/showcase.html).

---

## 📄 License

Planora is licensed under the **Apache License, Version 2.0** ([`LICENSE`](LICENSE)), and optionally under the **MIT License** ([`LICENSE-MIT`](LICENSE-MIT)), at your option. See [`NOTICE`](NOTICE) and the complete [Third-Party Attribution Matrix](doc/release/OPEN_SOURCE_LICENSES.md) for details on embedded dependencies and optional components.
