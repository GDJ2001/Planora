# Planora

![Planora logo](assets/brand/logos/planora-horizontal-light.svg)

**Your PC, clearly.** See how processes use your computer's resources.

Planora is a Rust 2024 desktop resource monitor and hardware visualizer. Its native interface uses `eframe`/`egui` 0.37 and the [Planora design tokens](assets/design-tokens.json). The workspace targets Windows 10/11 x86-64, Linux x86-64, and Apple Silicon macOS 13+; it does not include a browser edition.

The current implementation includes system and process telemetry, topology discovery, an overview, process search/sort and inspector, a hardware canvas, resource charts, explicit cancellable probes, and affinity editing on Windows and Linux. Monitoring requires a locally authorized privileged helper. Measurements that lack a supported source are shown as **Unavailable** with a reason. See [development status and verification](docs/development-status.md) and [implementation audit status](docs/implementation-status.md) for platform-specific limits and release gates.

## Build and test

Use the pinned [Rust 1.95 toolchain](rust-toolchain.toml). Install the native dependencies for `eframe` on the target platform, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --bins --locked
```

The binaries are `planora` and `planora-helper`. The UI connects to the helper using **Protocol v2** (`PROTOCOL_VERSION = 2`) over an authenticated local IPC channel with a 64 KiB request ceiling, 8 MiB response ceiling, a 16-connection ceiling (max 4 per caller), and strict 2s handshake and 5s request deadlines. Monitoring requires an authorized local privileged helper; the GUI remains unelevated.

The helper runs a dedicated background sampling worker that publishes an atomic `Arc<TelemetryFrame>`, preventing competing client requests from triggering redundant sampling cycles. The default 100 ms collection cadence is configurable from 50 ms to 1000 ms; CPU utilization refreshes no faster than `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL` and preserves its original sample timestamp. Unavailable measurements remain visibly marked as `Unavailable` with typed reasons (e.g., `Unsupported`, `PermissionDenied`, `TemporarilyFailed`, `WarmingUp`, `Stale`) and never appear as synthetic zeros.

### Hardened Privileged Actions & Bounds
- **Windows Affinity Mutation:** Revalidates process creation identity (`FILETIME` / sequence) through the *same* process handle before applying proposed CPU affinity sets, preventing PID-reuse race conditions.
- **Linux Affinity Limitation:** Affinity writes are disabled in v1 (`ErrorCode::Unsupported`). In Linux, thread affinity is managed per-TID via `sched_setaffinity`; safely mutating affinity for an entire multi-threaded process without races requires enumerating and synchronizing every thread under `/proc/[pid]/task`, which is deferred until a reviewed implementation can guarantee atomic identity.
- **Micro-Architectural Probes:** Memory-budgeted to a strict 64 MiB working set (72 MiB total allocation budget) and time-limited (12s for cache line/page probes, 20s for core matrix). All probes check for cancellation before and during sampling, and report partial results if cancelled.
- **Diagnostic Logging:** The helper maintains a thread-safe 256-event in-memory ring buffer of diagnostic events (`DiagnosticEntry`) with operation IDs, monotonic timestamps, error codes, and redacted summaries (process names, arguments, and command lines are excluded by default). Logs can be fetched and copied to the clipboard directly from the UI Settings view.

## Packages and authorization

- **Windows:** Build an MSI with WiX 5 using `wix build packaging/windows/Planora.wxs -arch x64 -d ProjectDir="$PWD" -d SourceDir="$PWD\target\release" -o dist/Planora-0.1.0-x64.msi`, then run `wix msi validate dist/Planora-0.1.0-x64.msi`. The installed `PlanoraHelper` service starts on demand after an administrator runs [Authorize-Planora.ps1](packaging/windows/Authorize-Planora.ps1) for the intended user. Its named pipe is `planora-v2`, restricted by a SID-specific ACL (`FILE_GENERIC_READ | FILE_GENERIC_WRITE`).
- **Ubuntu 24.04:** After a release build, run [build-deb.sh](packaging/linux/build-deb.sh). The package installs the systemd service and polkit authorization action. The local socket is `/run/planora/planora.sock` with mode `0660` restricted to the `planora-users` group; the helper runs with `CAP_PERFMON` rather than broad `CAP_SYS_ADMIN`.
- **Fedora:** After a release build, run [build-rpm.sh](packaging/linux/build-rpm.sh). The RPM uses the same helper and authorization model.
- **Apple Silicon macOS:** Run [build-package.sh](packaging/macos/build-package.sh) on a Mac. The package includes an SMAppService LaunchDaemon in the app bundle. Its installer records the exact console-user UID for local IPC access; the user must still approve daemon registration through macOS. The app ZIP alone has no authorized helper. The package is unsigned by default and has signing/notarization hooks.

These package recipes produce development artifacts. Installation, upgrade, removal, first launch, and helper cleanup still need native-platform verification. The Windows MSI passed WiX validation and administrative-image extraction; Debian and RPM payloads were built under Ubuntu WSL. Those checks are not install tests.

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

A distribution license has not yet been added to this repository. Do not assume an open-source license until the project owner selects one and commits the corresponding license file.
