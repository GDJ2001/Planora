# Planora v1.0.0 — Official Production Release

**Release Date**: November 8, 2026  
**Tag**: `v1.0.0`  
**License**: Apache License 2.0 / Dual MIT OR Apache-2.0  

Planora v1.0.0 is the first official production release of **Planora**, a high-performance cross-platform desktop hardware visualizer and system resource monitor built in Rust 2024 with an unelevated `egui`/`eframe` native GUI and a privileged background helper daemon.

---

## 🚀 Key Highlights & Architectural Features

- **Responsive Telemetry & Monotone Graphing**: Real-time smooth monotone time series curves with faint min..max variance bands and unit-aware adaptive round axis scaling (KiB/s, MiB/s, GiB/s, MB/s, GB/s).
- **Two-Layer Physical Plausibility Sanitization**: Zero synthetic zeros. Metric availability is modeled with typed enum states (`Metric::Unavailable` with reasons: `Unsupported`, `PermissionDenied`, `WarmingUp`, `Stale`). Frames are validated for percentage bounds, memory total consistency, and finite float properties before publication.
- **Strict Privilege Separation**:
  - The desktop GUI (`planora`) runs **unelevated** with zero administrator requirements.
  - Privileged helper daemon (`planora-helper`) communicates over authenticated local IPC (**Protocol v7**) using Windows SID-specific ACLs and Linux Unix peer credentials (`SO_PEERCRED`).
  - Request buffer ceiling: 64 KiB; response buffer ceiling: 8 MiB; strict 2s handshake and 5s request deadlines.
- **Hardware Topology & Multi-Package Visualizer**: Complete CPU topology inspection via CPUID and OS APIs, mapping physical sockets, NUMA nodes, core types (P-cores, E-cores), and L1/L2/L3 cache hierarchies.
- **Safe Process Affinity Editing**: Windows process affinity mutation revalidates target process creation identity (`FILETIME`) through the opened handle to prevent PID-reuse race conditions.
- **Cross-Platform Sensor Subsystem**: Integrated support for Windows PDH counters, hardware-filtered `GetIfTable2` networking, `GlobalMemoryStatusEx` physical/commit accounting, AMD sensor probes, Linux RAPL energy metrics, and an isolated user-mode LibreHardwareMonitor broker.
- **Local Recording & Replay (Format v5)**: Streamline performance profiling by recording diagnostic sessions to `.planora.jsonl` with deterministic playback and CSV/JSON export.
- **Seamless System Appearance**: Automatic synchronization with OS light/dark mode, high-contrast themes, system text scaling, and animation preferences.

---

## 📦 Downloads & Platform Packages

All binary releases are compiled with the pinned Rust 1.99.1 toolchain and verified against automated cryptographic manifests.

### Windows (Windows 10 / 11 x64)
- **Burn Setup Bundle (Recommended)**: [Planora-1.0.0-x64-Setup.exe](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64-Setup.exe)  
  *All-in-one branded installer with desktop shortcuts, helper service registration, and optional advanced sensor broker.*
- **Enterprise MSI Installer**: [Planora-1.0.0-x64.msi](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.msi)  
  *Silent deployment package for system administrators.*
- **Packaging Manifest**: [Planora-1.0.0-x64.manifest.json](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.manifest.json)

### Linux (x86_64)
- **Ubuntu / Debian (.deb)**: [planora_1.0.0-1_amd64.deb](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/planora_1.0.0-1_amd64.deb)  
  *Compatible with Ubuntu 24.04 LTS, 26.04 LTS, and Debian 12/13.*
- **Fedora / Red Hat (.rpm)**: [planora-1.0.0-1.fc44.x86_64.rpm](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/planora-1.0.0-1.fc44.x86_64.rpm)  
  *Compatible with Fedora 43/44 and Enterprise Linux 9/10.*

### macOS (Apple Silicon arm64)
- **macOS Installer (.pkg)**: [Planora-1.0.0-arm64.pkg](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-arm64.pkg)  
  *Self-contained macOS package with SMAppService privileged helper registration.*
- **macOS App Bundle (.zip)**: [Planora-1.0.0-arm64-app.zip](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-arm64-app.zip)

### Android Companion (Google Play Early Release & Direct Sideload)
- **Google Play Early Access (Closed Testing Track)**:  
  *Opt in to the official Google Play closed testing track to receive automatic updates and test the mobile telemetry companion:*  
  👉 **[Join Closed Testing on Google Play](https://play.google.com/apps/testing/org.planora.android)**  
  *(See the [Closed Testing Guide](https://github.com/GDJ2001/Planora/blob/main/doc/release/CLOSED_TESTING_AND_PRODUCTION_ACCESS_GUIDE.md) for full tester opt-in steps).*
- **Direct APK Sideload (GitHub Releases)**: [Planora-1.0.0-universal.apk](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-universal.apk)  
  *(For de-Googled Android devices or regions without Google Play Services).*

---

## 🔒 Verification & Cryptographic Checksums

Verify your downloaded packages using `sha256sum` or PowerShell `Get-FileHash`:

```sh
sha256sum -c SHA256SUMS.txt
```

Download the full checksum manifest: [SHA256SUMS.txt](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/SHA256SUMS.txt)  
Complete dependency attribution: [dependency-inventory.json](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/dependency-inventory.json)  
Build provenance record: [provenance.json](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/provenance.json)

---

## 🛠️ Verification Gates & Source Code Parity

The v1.0.0 source release has passed 100% of the repository verification gates:
- `cargo fmt --all -- --check` :white_check_mark:
- `cargo clippy --workspace --all-targets --locked -- -D warnings` :white_check_mark:
- `cargo test --workspace --locked` (200+ unit, integration, and IPC resilience tests) :white_check_mark:
- GitHub Actions CI matrix across Windows, Ubuntu, Fedora, macOS, and cargo-audit :white_check_mark:
