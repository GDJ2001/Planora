# Contributing to Planora

Thank you for your interest in contributing to **Planora**! Planora is an open-source, high-performance desktop hardware visualizer and system resource monitor built in Rust (2024 edition) with an unelevated native `egui`/`eframe` GUI and a privileged background helper daemon.

---

## 1. Environment Prerequisites

Before contributing, ensure your development environment satisfies the required toolchains:

### 1.1 Toolchains
- **Rust Toolchain**: Pinned to **`1.99.1`** (configured automatically by `rust-toolchain.toml`). Components: `clippy`, `rustfmt`.
- **Operating Systems**: Windows 10/11 x86-64, Linux (Ubuntu 24.04/26.04 or Fedora 44 x86-64), or macOS 13+ (Apple Silicon).
- **.NET SDK**: .NET 9.0 or 10.0 (required for building the Windows hardware sensor broker `planora-sensor-broker`).
- **Python**: Python 3.10+ (for brand asset validation and release inventories).

### 1.2 Linux Dependencies (Ubuntu / Debian)
```sh
sudo apt-get update && sudo apt-get install -y \
  libx11-dev libxkbcommon-dev libwayland-dev \
  libpolkit-gobject-1-dev libbpf-dev libelf-dev zlib1g-dev \
  clang linux-libc-dev pkg-config
```

### 1.3 Windows Dependencies
For Windows MSI/Setup installer packaging:
```powershell
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2 WixToolset.Util.wixext/5.0.2
```

---

## 2. Module Architecture Boundaries

Planora adheres to strict architectural isolation between modules:

| Crate | Responsibility | Privilege Level |
| :--- | :--- | :--- |
| **`crates/planora-core`** | Portable types, IPC protocol contracts (Protocol v7), metric availability states (`Metric<T>`), and physical plausibility validation. | Unelevated / Generic |
| **`crates/planora-topology`** | Hardware, CPU, cache topology discovery via CPUID and OS APIs. | Unelevated / Generic |
| **`crates/planora-telemetry`** | Cross-platform system sampling (`sysinfo`, Windows PDH/`GetIfTable2`/`GlobalMemoryStatusEx`, Linux sysfs/block/eBPF). | Unelevated & Helper |
| **`crates/planora-bench`** | Bounded, cancellable micro-architectural probes (64 MiB working set cap, 72 MiB budget). | Unelevated / Worker |
| **`crates/planora-helper`** | Privileged daemon enforcing local IPC authorization (Windows SID ACLs / Linux polkit and UID checks) and process sampling. | Elevated (`LocalSystem` / root) |
| **`crates/planora-ui`** | Unelevated native GUI (`eframe`/`egui` 0.37 with `wgpu`), user settings (Schema v8), history ingestion, and recording v5. | Unelevated (User) |
| **`crates/planora-sensor-broker`** | Windows C#/.NET worker managing LibreHardwareMonitor and optional signed PawnIO driver in an isolated user-mode child. | Unelevated Child |

### Core Architectural Rules:
1. **The GUI binary (`planora`) must remain unelevated**. It communicates solely via local IPC with `planora-helper`.
2. **The helper daemon never accesses user-owned files or recordings**. User recordings belong exclusively in the user process.
3. **Telemetry truthfulness**: Unavailable measurements must remain visibly `Unavailable` with an explicit reason (`AvailabilityReason::Unsupported`, `PermissionDenied`, `WarmingUp`, `Stale`). Never emit synthetic zeros.
4. **Physical Plausibility**: Telemetry frames must pass `validate()` before publication (checking percentage bounds, memory total constraints, and finite floats).

---

## 3. Local Verification Gates

Before submitting code, all local verification gates must pass without warnings:

```sh
# 1. Check code formatting
cargo fmt --all -- --check

# 2. Strict workspace linting (zero warnings allowed)
cargo clippy --workspace --all-targets --locked -- -D warnings

# 3. Run all workspace unit and integration tests
cargo test --workspace --locked

# 4. Verify release binary compilation
cargo build --release --bins --locked

# 5. Check brand icon packaging integrity
python scripts/export-icons.py --check
```

On Windows (to test MSI and Burn Setup packaging):
```powershell
./packaging/windows/Build-Planora.ps1
```

---

## 4. Development & Pull Request Workflow

1. **Fork and Branch**:
   - Fork the repository and create a feature branch off `main`:
     ```sh
     git checkout -b feature/my-enhancement
     ```
2. **Commit Conventions**:
   - Follow [Conventional Commits](https://www.conventionalcommits.org/):
     - `feat:` New features
     - `fix:` Bug fixes
     - `docs:` Documentation improvements
     - `refactor:` Code refactoring without behavior change
     - `test:` Adding or refining tests
     - `ci:` CI workflow changes
3. **Keep Diffs Focused**: Avoid unrelated refactorings or reformatting lines outside your feature scope.
4. **Do Not Weaken Tests**: Never skip, weaken, or delete existing tests to force a pass.
5. **Open a Pull Request**:
   - Push your branch to GitHub and open a pull request against `main`.
   - Fill out the PR template completely, referencing any related issue tickets.
   - Ensure all automated GitHub Actions CI matrix checks (Windows, Ubuntu, Fedora, macOS, Security Advisory) turn green.
