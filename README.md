# Planora

![Planora logo](assets/brand/logos/planora-horizontal-light.svg)

**Your PC, clearly.** See how processes use your computer's resources.

Planora is a Rust 2024 desktop resource monitor and hardware visualizer. Its native interface uses `eframe`/`egui` 0.37 and the [Planora design tokens](assets/design-tokens.json). The workspace targets Windows 10/11 x86-64, Linux x86-64, and Apple Silicon macOS 13+; it does not include a browser edition.

The current implementation includes system and process telemetry, topology discovery, an overview, process search/sort and inspector, a hardware canvas, resource charts, explicit cancellable probes, and affinity editing on Windows and Linux. Monitoring requires a locally authorized privileged helper. Measurements that lack a supported source are shown as **Unavailable** with a reason. See [development status and verification](docs/development-status.md) for platform-specific limits and tests still required before release.

## Build and test

Use the pinned [Rust 1.95 toolchain](rust-toolchain.toml). Install the native dependencies for `eframe` on the target platform, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --bins --locked
```

The binaries are `planora` and `planora-helper`. The UI waits for the helper's successful local authorization handshake before it displays monitoring data. The 100 ms collection cadence is configurable down to 50 ms; CPU utilization refreshes no faster than `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL` and retains its original sample timestamp between updates.

## Packages and authorization

- **Windows:** Build an MSI with WiX 5 using `wix build packaging/windows/Planora.wxs -arch x64 -d ProjectDir="$PWD" -d SourceDir="$PWD\target\release" -o dist/Planora-0.1.0-x64.msi`, then run `wix msi validate dist/Planora-0.1.0-x64.msi`. The installed `PlanoraHelper` service starts on demand after an administrator runs [Authorize-Planora.ps1](packaging/windows/Authorize-Planora.ps1) for the intended user. Its named pipe is `planora-v1`, restricted by a SID-specific ACL.
- **Ubuntu 24.04:** After a release build, run [build-deb.sh](packaging/linux/build-deb.sh). The package installs the systemd service and polkit authorization action. The local socket is `/run/planora/planora.sock`; the helper uses `CAP_PERFMON` rather than `CAP_SYS_ADMIN`.
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
