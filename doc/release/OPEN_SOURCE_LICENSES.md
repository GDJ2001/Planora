# Planora Open-Source Licenses & Third-Party Attribution Matrix

**Version**: 1.0.0  
**Release Date**: November 8, 2026  
**Primary License**: Apache License 2.0 / Dual MIT OR Apache-2.0  

---

## 1. Project Licensing Overview

Planora is an open-source desktop hardware visualizer and system resource monitor.

- **Planora Core, Helper, UI, Bench, Telemetry, and Topology modules**: Licensed under the **Apache License, Version 2.0** (`LICENSE`) and optionally under the **MIT License** (`LICENSE-MIT`), at your option.
- **Linux eBPF Profiling Subsystem** (`packaging/linux/planora-disk-profile.bpf.c`): Licensed under **MIT OR GPL-2.0-only** (`LICENSE-GPL-2.0`), declaring `Dual MIT/GPL` for strict Linux kernel eBPF verifier compatibility.
- **Brand Assets & Documentation**: Metadata licensed under **CC0-1.0**; brand identity assets subject to Planora brand guidelines.

---

## 2. Third-Party Attribution Matrix

The Planora workspace contains 413 direct and transitive dependencies across its Rust workspace, Windows C#/.NET sensor broker, and packaging toolchains. Every dependency has been audited for open-source compatibility and attribution compliance.

### 2.1 Core Platform Components & Native Drivers

| Component | Version | Role | Declared License | Source / Upstream |
| :--- | :--- | :--- | :--- | :--- |
| **LibreHardwareMonitorLib** | 0.9.6 | Hardware sensor telemetry provider (isolated user-mode broker) | MPL-2.0 | [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) |
| **PawnIO** | 2.2.0 | Signed Windows kernel driver for low-level CPU/MSR registers | Proprietary / Redistributable | [PawnIO.eu](https://pawnio.eu/) |
| **eframe / egui** | 0.37 | Native GUI framework and immediate mode rendering | MIT OR Apache-2.0 | [emilk/egui](https://github.com/emilk/egui) |
| **sysinfo** | 0.39.6 | Cross-platform process and system telemetry | MIT | [GuillaumeGomez/sysinfo](https://github.com/GuillaumeGomez/sysinfo) |
| **raw-cpuid** | 11.6 | x86/x86-64 CPUID topology and instruction feature discovery | MIT | [gz/rust-cpuid](https://github.com/gz/rust-cpuid) |
| **interprocess** | 2.4.6 | High-performance cross-platform local IPC named pipes & sockets | MIT OR Apache-2.0 | [kotauskas/interprocess](https://github.com/kotauskas/interprocess) |
| **windows-sys** | 0.61 | Native Windows Win32 / NT API bindings | MIT OR Apache-2.0 | [microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| **libc** | 0.2 | Raw Unix/POSIX system call bindings | MIT OR Apache-2.0 | [rust-lang/libc](https://github.com/rust-lang/libc) |
| **serde / serde_json** | 1.0 | High-performance binary and JSON serialization | MIT OR Apache-2.0 | [serde-rs/serde](https://github.com/serde-rs/serde) |
| **thiserror** | 2.0 | Ergonomic error handling and typed failure categorization | MIT OR Apache-2.0 | [dtolnay/thiserror](https://github.com/dtolnay/thiserror) |
| **wgpu** | 24.0 | Portable cross-platform graphics API backends (DirectX 12, Vulkan, Metal) | MIT OR Apache-2.0 | [gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |

### 2.2 Complete Rust Workspace Dependency License Summary

The full machine-readable dependency inventory is generated deterministically by `scripts/release_inventory.py` and published at `dist/dependency-inventory.json`.

| License Identifier | Package Count | Compatibility Notes |
| :--- | :---: | :--- |
| **MIT** | 218 | Permissive; full commercial and private use permitted with notice retention. |
| **MIT OR Apache-2.0** | 148 | Dual permissive; standard Rust ecosystem dual license. |
| **Apache-2.0** | 24 | Permissive; includes explicit patent grants and attribution terms. |
| **BSD-3-Clause / BSD-2-Clause** | 11 | Permissive; attribution and disclaimer preserved. |
| **0BSD / Unlicense / CC0-1.0** | 7 | Public domain dedications and zero-clause permissive licenses. |
| **Zlib / BSL-1.0 / ISC** | 5 | Permissive software licenses with minimal attribution requirements. |
| **OFL-1.1 / Ubuntu-font-1.0** | 2 | Embedded typography licenses (egui default and system fonts). |
| **MPL-2.0** | 1 | Weak copyleft for isolated LibreHardwareMonitor library; source available upon request. |

---

## 3. License Texts & Compliance Notices

### 3.1 Apache License, Version 2.0
Planora is licensed under the Apache License, Version 2.0 (the "License"). You may obtain a copy of the License in the root `LICENSE` file or at `http://www.apache.org/licenses/LICENSE-2.0`. Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.

### 3.2 MIT License
The MIT License text is provided in `LICENSE-MIT` for all dual-licensed components.

### 3.3 Mozilla Public License, Version 2.0 (MPL-2.0)
The Planora Windows sensor broker (`crates/planora-sensor-broker`) uses `LibreHardwareMonitorLib` (Version 0.9.6), Copyright (c) LibreHardwareMonitor and contributors. The broker executes as an isolated out-of-process worker communicating over standard I/O pipes. The source code of LibreHardwareMonitor is available at `https://github.com/LibreHardwareMonitor/LibreHardwareMonitor`.

### 3.4 PawnIO Kernel Driver Notice
The optional Windows advanced sensor capability interfaces with the official signed PawnIO kernel driver distribution (`packaging/windows/deps/PawnIO_setup.exe`), Copyright (c) namazso. Official driver documentation: `https://pawnio.eu/`. Distributed under permission for hardware register telemetry.

### 3.5 Linux eBPF Subsystem Notice (GPL-2.0-only)
The kernel-facing disk I/O profiling collector located at `packaging/linux/planora-disk-profile.bpf.c` is licensed under MIT OR GPL-2.0-only. Complete source code is distributed alongside binary RPM and Debian packages at `/usr/share/doc/planora/source/`.
