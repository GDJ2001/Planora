# Planora

![Planora logo](assets/brand/logos/planora-horizontal-light.svg)

**Your PC, clearly.** See how processes use your computer's resources.

Planora is a Rust 2024 desktop resource monitor and hardware visualizer. Its native interface uses `eframe`/`egui` 0.37 and the [Planora design tokens](assets/design-tokens.json). The workspace targets Windows 10/11 x86-64, Linux x86-64, and Apple Silicon macOS 13+; it does not include a browser edition.

The current 0.3.0-beta.23 foundation on `main` consolidates all security hardening, telemetry truthfulness, Windows Setup packaging, responsive Performance workspace, visualization polish, and end-to-end sensor validation workstreams. It includes smooth monotone telemetry graphs with faint min..max range bands, conventional 1-second average CPU utilization time series, full-card interactive resource selection across Overview and Performance, adaptive unit-aware round axis scaling (KiB/s, MiB/s, GiB/s), independent per-lane History view state, automatic Windows system appearance synchronization (light/dark mode, high-contrast palette, accessibility text scaling, and animation preferences), responsive independent pane scrolling, saved Tables only mode, two-layer physical plausibility sanitization and IPC frame validation, physical-disk PDH latency/queue-depth/throughput telemetry, hardware-filtered `GetIfTable2` network throughput, atomic `GlobalMemoryStatusEx` physical/commit accounting, topology discovery, explicit cancellable cache probes, Windows process-affinity editing, and optional local recording/replay and exports. The Windows GUI launches without a terminal and has application-owned window placement and single-instance activation. Monitoring requires a locally authorized privileged helper. Advanced sensors are an optional internal Windows component; individual readings remain unavailable when the hardware or signed driver does not supply them. Disk ETW profiling is experimental after an initial installed-native trace; PMU sampling remains unimplemented. See the [capability and verification matrix](docs/beta-readiness.md). The [implementation audit](docs/implementation-status.md) describes an older revision.

## Build and test

Use the pinned [Rust 1.99.1 toolchain](rust-toolchain.toml). Install the native dependencies for `eframe` on the target platform, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --bins --locked
```

The binaries are `planora` and `planora-helper`, plus an optional `planora-sensor-broker.exe` on Windows. The UI connects to the helper using **Protocol v6** (`PROTOCOL_VERSION = 6`) over an authenticated local IPC channel with a 64 KiB request ceiling, 8 MiB response ceiling, a 16-connection ceiling (max 4 per caller), and strict 2s handshake and 5s request deadlines. The local endpoint retains its `planora-v2` name for upgrade detection; a mismatched helper receives an incompatibility screen. Monitoring requires an authorized local privileged helper; the GUI remains unelevated. Vendor libraries run in an isolated user-mode child; when installed, the advanced sensor broker is a bounded, helper-owned child.

The helper runs a dedicated background sampling worker. The default 100 ms collection cadence can be set to 100, 250, 500, 1,000, or 2,000 ms. The fastest active client's request wins; with no clients it samples once per second. CPU utilization refreshes no faster than `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL` and preserves its original sample timestamp. Invalid frames are rejected before publication. Unavailable measurements remain visibly marked as `Unavailable` with typed reasons (e.g., `Unsupported`, `PermissionDenied`, `TemporarilyFailed`, `WarmingUp`, `Stale`) and never appear as synthetic zeros.

The in-memory UI smoke test runs with `cargo test --workspace --locked`. The optional `planora --headless-smoke-test` command writes labeled, simulated SVG/Edge fixtures under `dist/fixture-smoke`. For an actual egui window with simulated data and no helper, use `planora --native-fixture --view performance --resource cpu --theme dark --width 1180 --height 790 --screenshot-out D:\captures\cpu.png`; `--tables-only` previews the alternate mode. Fixture captures are visibly labeled and are not installed-live evidence. Use [capture_native.ps1](scripts/capture_native.ps1) on Windows for installed native evidence.

### Hardened Privileged Actions & Bounds
- **Windows Affinity Mutation:** Revalidates process creation identity (`FILETIME` / sequence) through the *same* process handle before applying proposed CPU affinity sets, preventing PID-reuse race conditions.
- **Linux Affinity Limitation:** Affinity writes are disabled in v1 (`ErrorCode::Unsupported`). In Linux, thread affinity is managed per-TID via `sched_setaffinity`; safely mutating affinity for an entire multi-threaded process without races requires enumerating and synchronizing every thread under `/proc/[pid]/task`, which is deferred until a reviewed implementation can guarantee atomic identity.
- **Micro-Architectural Probes:** Memory-budgeted to a strict 64 MiB working set (72 MiB total allocation budget) and time-limited (12s for cache line/page probes, 20s for core matrix). All probes check for cancellation before and during sampling, and report partial results if cancelled.
- **Diagnostic Logging:** The helper maintains a thread-safe 256-event in-memory ring buffer of diagnostic events (`DiagnosticEntry`) with operation IDs, monotonic timestamps, error codes, and redacted summaries (process names, arguments, and command lines are excluded by default). Logs can be fetched and copied to the clipboard directly from the UI Settings view.

## Packages and authorization

- **Windows:** After committing the source, run [Build-Planora.ps1](packaging/windows/Build-Planora.ps1). It builds a branded Burn Setup EXE and an underlying MSI, uses numeric MSI ProductVersion `0.2.1023` for `0.3.0-beta.23`, validates the MSI, and writes a SHA-256 manifest. Setup shows the fixed administrator-protected Program Files location and presents install, repair, and confirmed uninstall actions. **Advanced sensors** is an explicit opt-in that installs the signed PawnIO dependency when needed; its separate license and redistribution obligations require review before public release. The bundle is the single Installed Apps entry, while direct MSI deployments remain separately manageable. A fresh install leaves the helper stopped until [authorization](packaging/windows/Authorize-Planora.ps1) for the intended user; upgrades with existing authorization start it automatically. [Uninstall Planora](packaging/windows/Uninstall-Planora.ps1) opens registered maintenance; an administrator can [revoke](packaging/windows/Revoke-Planora.ps1) authorization. The named pipe retains the `planora-v2` endpoint name, restricted by a SID-specific ACL (`FILE_GENERIC_READ | FILE_GENERIC_WRITE`). Authorization is stored under administrator-owned `HKLM\SOFTWARE\Planora`; MSI migration reads the 0.1.5 service value before replacing that service. Full native repair, upgrade, and removal acceptance remains open. Use [capture_native.ps1](scripts/capture_native.ps1) for installed UI evidence. Unsigned packages are for internal testing only.
- **Ubuntu 24.04/26.04:** After a release build, run [build-deb.sh](packaging/linux/build-deb.sh). The package installs the unprivileged monitoring helper, an isolated `CAP_PERFMON` PMU worker, a separate block-profiling worker, and polkit authorization and revocation actions. Authorization takes effect without signing out. The local socket is `/run/planora/planora-v2.sock`; every connection is checked against Unix peer credentials and an administrator-owned per-user record before commands are accepted. Explicit disk I/O profiles use the packaged, fixed libbpf collector when the kernel exposes BTF, block tracepoints, and `CAP_BPF`; unattributable kernel writeback remains separate.
- **Fedora:** After a release build, run [build-rpm.sh](packaging/linux/build-rpm.sh). The RPM uses the same per-user helper authorization model. Native Fedora installation and desktop acceptance still need verification.
- **Apple Silicon macOS:** Run [build-package.sh](packaging/macos/build-package.sh) on a Mac. The package includes an SMAppService LaunchDaemon in the app bundle. Its installer records the exact console-user UID for local IPC access; the user must still approve daemon registration through macOS. The app ZIP alone has no authorized helper. The package is unsigned by default and has signing/notarization hooks.

Windows 0.1.5 is a historical upgrade baseline, with [release evidence](docs/release-evidence-0.1.5.md). The installed 0.2.0-beta.1 build has [its own historical verification record](docs/release-evidence-0.2.0-beta.1.md). The current 0.3 source and packages are internal work until the [beta gates](docs/beta-readiness.md) pass. Linux and macOS remain previews until native tests pass.

## Repository

| Path | Purpose |
| --- | --- |
| [planora-core](crates/planora-core) | Versioned contracts and explicit metric availability |
| [planora-topology](crates/planora-topology) | OS and CPU topology discovery |
| [planora-telemetry](crates/planora-telemetry) | System and process sampling |
| [planora-bench](crates/planora-bench) | Bounded, cancellable cache and core probes |
| [planora-helper](crates/planora-helper) | Local IPC, authorization, and affinity operations |
| [planora-ui](crates/planora-ui) | Native Planora interface |
| [Brand showcase](assets/brand/showcase.html) | Light/dark preview and asset comparison |
| [Brand guidelines](docs/brand-guidelines.md) | Logo, typography, color, and icon rules |

The supplied [hardware visualizer development document](docs/hwviz_%20Micro-Architectural%20Hardware%20Visualizer%20%26%20Task%20Manager%20-%20Developer%20Documentation.docx) is design input. Its code snippets and performance claims are evaluated against current platform APIs and measurements rather than assumed to be implemented verbatim.

## License

The source declares `MIT OR Apache-2.0`; see [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE), and [NOTICE](NOTICE). Approval of the public distribution terms and signing credentials remain release gates.
