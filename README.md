# Planora

![Planora logo](assets/brand/logos/planora-horizontal-light.svg)

**Your PC, clearly.** See how processes use your computer's resources.

Planora is a Rust 2024 desktop resource monitor and hardware visualizer. Its native interface uses `eframe`/`egui` 0.37 and the [Planora design tokens](assets/design-tokens.json). The workspace targets Windows 10/11 x86-64, Linux x86-64, and Apple Silicon macOS 13+; it does not include a browser edition.

The current 0.2.0-beta.1 source includes system and process telemetry, topology discovery, resource charts and device details, explicit cancellable probes, Windows process-affinity editing, optional local recording/replay, and CSV/JSON exports. Monitoring requires a locally authorized privileged helper. Measurements that lack a supported source are shown as **Unavailable** with a reason. See the [beta capability and verification matrix](docs/beta-readiness.md) for current release gates; the [implementation audit](docs/implementation-status.md) is historical.

## Build and test

Use the pinned [Rust 1.95 toolchain](rust-toolchain.toml). Install the native dependencies for `eframe` on the target platform, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --bins --locked
```

The binaries are `planora` and `planora-helper`. The UI connects to the helper using **Protocol v3** (`PROTOCOL_VERSION = 3`) over an authenticated local IPC channel with a 64 KiB request ceiling, 8 MiB response ceiling, a 16-connection ceiling (max 4 per caller), and strict 2s handshake and 5s request deadlines. The local endpoint retains its `planora-v2` name for upgrade detection; a mismatched helper receives an incompatibility screen. Monitoring requires an authorized local privileged helper; the GUI remains unelevated.

The helper runs a dedicated background sampling worker. The default 100 ms collection cadence can be set to 100, 250, 500, 1,000, or 2,000 ms. The fastest active client's request wins; with no clients it samples once per second. CPU utilization refreshes no faster than `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL` and preserves its original sample timestamp. Invalid frames are rejected before publication. Unavailable measurements remain visibly marked as `Unavailable` with typed reasons (e.g., `Unsupported`, `PermissionDenied`, `TemporarilyFailed`, `WarmingUp`, `Stale`) and never appear as synthetic zeros.

The in-memory UI smoke test runs with `cargo test --workspace --locked`. The optional `planora --headless-smoke-test` command writes labeled, simulated SVG/Edge fixtures under `dist/fixture-smoke`; these are diagnostic renders, not native-window screenshots. Use [capture_native.ps1](scripts/capture_native.ps1) on Windows for native evidence.

### Hardened Privileged Actions & Bounds
- **Windows Affinity Mutation:** Revalidates process creation identity (`FILETIME` / sequence) through the *same* process handle before applying proposed CPU affinity sets, preventing PID-reuse race conditions.
- **Linux Affinity Limitation:** Affinity writes are disabled in v1 (`ErrorCode::Unsupported`). In Linux, thread affinity is managed per-TID via `sched_setaffinity`; safely mutating affinity for an entire multi-threaded process without races requires enumerating and synchronizing every thread under `/proc/[pid]/task`, which is deferred until a reviewed implementation can guarantee atomic identity.
- **Micro-Architectural Probes:** Memory-budgeted to a strict 64 MiB working set (72 MiB total allocation budget) and time-limited (12s for cache line/page probes, 20s for core matrix). All probes check for cancellation before and during sampling, and report partial results if cancelled.
- **Diagnostic Logging:** The helper maintains a thread-safe 256-event in-memory ring buffer of diagnostic events (`DiagnosticEntry`) with operation IDs, monotonic timestamps, error codes, and redacted summaries (process names, arguments, and command lines are excluded by default). Logs can be fetched and copied to the clipboard directly from the UI Settings view.

## Packages and authorization

- **Windows:** After committing the source, run [Build-Planora.ps1](packaging/windows/Build-Planora.ps1). It builds both release binaries, uses numeric MSI ProductVersion `0.2.0` for the `0.2.0-beta.1` prerelease, validates the MSI, and writes a SHA-256 manifest. An administrator can [authorize](packaging/windows/Authorize-Planora.ps1) or [revoke](packaging/windows/Revoke-Planora.ps1) the intended user. The named pipe retains the `planora-v2` endpoint name, restricted by a SID-specific ACL (`FILE_GENERIC_READ | FILE_GENERIC_WRITE`). Authorization is stored under administrator-owned `HKLM\SOFTWARE\Planora`; MSI migration reads the 0.1.5 service value before replacing that service. The 0.1.5 migration and a clean install/uninstall cycle have been exercised on Windows 11. Use [capture_native.ps1](scripts/capture_native.ps1) for installed UI evidence. The unsigned installer is for internal testing only.
- **Ubuntu 24.04:** After a release build, run [build-deb.sh](packaging/linux/build-deb.sh). The package installs the systemd service and polkit authorization action. The local socket is `/run/planora/planora-v2.sock` with mode `0660` restricted to the `planora-users` group; the helper runs with `CAP_PERFMON` rather than broad `CAP_SYS_ADMIN`.
- **Fedora:** After a release build, run [build-rpm.sh](packaging/linux/build-rpm.sh). The RPM uses the same helper and authorization model.
- **Apple Silicon macOS:** Run [build-package.sh](packaging/macos/build-package.sh) on a Mac. The package includes an SMAppService LaunchDaemon in the app bundle. Its installer records the exact console-user UID for local IPC access; the user must still approve daemon registration through macOS. The app ZIP alone has no authorized helper. The package is unsigned by default and has signing/notarization hooks.

Windows 0.1.5 is the historical upgrade baseline, with [release evidence](docs/release-evidence-0.1.5.md). The installed 0.2.0-beta.1 build has [its own verification record](docs/release-evidence-0.2.0-beta.1.md), but it is not approved for public distribution; see [beta readiness](docs/beta-readiness.md). Linux and macOS remain previews until native tests pass.

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
