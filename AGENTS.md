# Planora AI Agent Development Guidelines

Planora is a high-performance cross-platform desktop hardware visualizer and system resource monitor written in Rust (2024 edition, pinned toolchain `1.99.1`) with an `egui`/`eframe` native GUI and a privileged background helper daemon.

---

## 1. Architectural Boundaries

- **`crates/planora-core`**: Portable types, IPC protocol contracts (Protocol v6, 64 KiB request / 8 MiB response limits), physical plausibility validation, and explicit metric availability states (`Metric<T>`).
- **`crates/planora-topology`**: Hardware and CPU/cache topology discovery via CPUID and OS APIs.
- **`crates/planora-telemetry`**: Cross-platform system sampling (`sysinfo`, Windows PDH/`GetIfTable2`/`GlobalMemoryStatusEx`, Linux sysfs/block/eBPF, and vendor sensor integrations).
- **`crates/planora-bench`**: Bounded, cancellable micro-architectural probes (64 MiB working set cap, 72 MiB total budget, strict timeouts).
- **`crates/planora-helper`**: Privileged daemon/service enforcing local IPC authorization (Windows SID ACLs / Linux polkit and UID checks), process sampling worker, and safe process affinity mutation.
- **`crates/planora-ui`**: Unelevated native GUI (`eframe`/`egui` 0.37 with `wgpu`), user settings (Schema v7), background history ingestion, and recording format v4.
- **`crates/planora-sensor-broker`**: Windows C#/.NET worker managing LibreHardwareMonitor and optional signed PawnIO driver in an isolated user-mode child.

---

## 2. Core Security & Privilege Guardrails

1. **Privilege Separation**:
   - The GUI binary (`planora`) **must remain unelevated**. It communicates solely via local IPC with `planora-helper`.
   - The helper daemon runs as `LocalSystem` (Windows) or root / dedicated system user (Linux/macOS), but **never opens user-owned files** or recordings. User recordings and exports belong exclusively in the user process (`planora-ui`).
2. **IPC Authorization & Lifecycles**:
   - Named pipe (`planora-v2` on Windows) is protected with a strict SID-specific ACL (`FILE_GENERIC_READ | FILE_GENERIC_WRITE`).
   - Unix socket (`/run/planora/planora-v2.sock`) validates peer credentials (`euid`) against root-owned `/var/lib/planora/authorized-uids/<uid>`.
   - All IPC exchanges have strict absolute deadlines (`HANDSHAKE_TIMEOUT` = 2s, `REQUEST_TIMEOUT` = 5s) using overlapped I/O on Windows and nonblocking `poll` on Unix.
3. **Affinity Mutation Safety**:
   - Windows process affinity mutations (`SetAffinity`) must re-validate the target process creation identity (`FILETIME`) on the opened process handle before applying masks, preventing PID-reuse race conditions.
   - Linux process-wide affinity mutation remains **disabled** (`ErrorCode::Unsupported`) because `sched_setaffinity` applies per-thread and cannot be applied race-free without atomic task-tree synchronization.
4. **Data Plausibility & Telemetry Truthfulness**:
   - Unavailable measurements must remain visibly `Unavailable` with an explicit typed reason (`AvailabilityReason::Unsupported`, `PermissionDenied`, `WarmingUp`, `Stale`). Never emit synthetic zeros.
   - Telemetry frames must pass `validate()` before publication (checking percentage bounds, memory total constraints, finite floats, and non-contradictory availability states).

---

## 3. Verified Verification Commands

Run these from the workspace root with Rust `1.99.1`:

```sh
# Code formatting check
cargo fmt --all -- --check

# Strict workspace linting
cargo clippy --workspace --all-targets --locked -- -D warnings

# Run all workspace unit and integration tests
cargo test --workspace --locked

# Build release binaries
cargo build --release --bins --locked

# Verify brand icon packaging
python scripts/export-icons.py --check
```

On Windows (when packaging with WiX Toolset v5):
```powershell
./packaging/windows/Build-Planora.ps1
```

---

## 4. Key Conventions

- **Protocol Version**: Current protocol is `PROTOCOL_VERSION = 6`. Handshake requires `Command::Hello { version: 6 }`.
- **Settings Version**: Settings schema is `schema_version: 7`. Stored at `%APPDATA%\Planora\settings.json` (Windows) or `~/.config/Planora/settings.json` (Linux).
- **Recording Format**: Current recording format is `v4` JSON Lines (`.planora.jsonl`). Max file size 256 MiB; max duration 2 hours.
- **PE Subsystem**: Windows `planora.exe` is a GUI subsystem executable (subsystem 2) built with `#![cfg_attr(windows, windows_subsystem = "windows")]`.
- **Search Before Create**: Before adding any UI component, card, chart, metric, or IPC command, inspect existing patterns in `crates/planora-core/src/lib.rs` and `crates/planora-ui/src/components/`.
