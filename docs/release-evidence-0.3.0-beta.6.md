# Planora 0.3.0-beta.6 internal visualization evidence

## Build identity and package validation

- Source revision: `6d2c2d0` on `codex/planora-performance-workspace`.
- Windows target: `x86_64-pc-windows-msvc`, Rust workspace version `0.3.0-beta.6`, helper protocol v4, MSI ProductVersion `0.2.1006`.
- The clean-source Windows packaging script completed a release build, WiX MSI validation, Burn Setup build, and UI/helper identity comparison. Both release binaries report revision `6d2c2d0`, version `0.3.0-beta.6`, and protocol v4.
- SHA-256 values were independently recalculated and matched `dist/Planora-0.3.0-beta.6-x64.manifest.json`:

| Artifact | SHA-256 |
| --- | --- |
| `planora.exe` | `9B6C7F20AEC235230E06ED4CF3949FBF44CA76AA45A62F9B82E53DBB46150FAB` |
| `planora-helper.exe` | `C6C31E9EF28DCB75E5D5755905946DC3EC68769104FFC16E61877BA3E0F50819` |
| `Planora-0.3.0-beta.6-x64.msi` | `D5D4CDE543F2F410C363313D862B70E68F5C31A603C56AC51C89D969D109034F` |
| `Planora-0.3.0-beta.6-x64-Setup.exe` | `EFE69E626C90E7B4959E74342F03CE2C19CCE830C55B37F17590C99C347AF509` |

The package and manifest are local ignored build outputs under `dist/`; this record preserves their identities. Beta.5 artifacts were not overwritten. These packages are unsigned and internal.

## Automated and native fixture checks

- `cargo fmt --all --check`, `cargo test --workspace --locked --quiet`, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed on the Windows development host. The five-minute installed-service telemetry test is excluded from the ordinary workspace run; a separate shortened installed-service run is recorded below.
- Actual egui native fixture windows were captured from the release binary using `--native-fixture`. The banner identifies simulated data, and the captures do not assert installed or live-helper behavior.
- [GPU, dark, 1180 × 790 logical pixels](screenshots/beta6/gpu-dark.png): selected Intel Iris Xe adapter, one-second history with a deliberate gap, unsummed engine measurement, unavailable dedicated-memory bar.
- [Memory, light, 1180 × 790](screenshots/beta6/memory-light.png): capacity and commit presentation, gap-aware trend, readable table.
- [Network, dark, compact 1024 × 640](screenshots/beta6/network-dark-compact.png): compact navigation and picker, separate receive/transmit traces, directional units.
- [CPU, light, Tables only, 1180 × 790](screenshots/beta6/cpu-light-tables.png): preserved precise logical-processor table and load bars.
- The host rendered these images at approximately 125% effective scale. Earlier beta.5 fixture review covered both themes for CPU, memory, GPU, and network at multiple widths; a duplicate wide-layout ScrollArea ID discovered there was fixed before this package.

## Installed Windows check — 2026-10-22

- Before Setup, `C:\Program Files\Planora` and the `PlanoraHelper` service were absent. The Setup EXE hash matched the manifest.
- The visible beta.6 Setup installed to `C:\Program Files\Planora`. Its Burn log ended with MSI result `0x0`, apply result `0x0`, and exit code `0x0`, without a restart.
- Installed `planora.exe` and `planora-helper.exe` both match the manifest hashes and report version `0.3.0-beta.6`, source commit `6d2c2d0`, and protocol v4.
- The first-launch authorization script completed for the current Windows user. The administrator-owned authorization record is present (value not recorded), and `PlanoraHelper` is running. Both Start menu shortcuts, `Planora` and `Uninstall Planora`, exist.
- The uninstall registry contains one ordinary Planora entry and one MSI entry hidden with `SystemComponent=1`. This supports a single visible Installed Apps entry, but Windows Settings itself was not visually checked in this run.
- `PLANORA_SERVICE_TEST_DURATION_SECS=22 cargo test -p planora-helper --test verify_installed_service --locked -- --ignored --nocapture` passed: authenticated protocol-v4 handshake, capability response, and **22 consecutive fresh snapshots**. This is a short live check, not the five-minute sustained acceptance test.
- [Installed live Performance capture](screenshots/beta6/installed-live-performance.png) came from `C:\Program Files\Planora\planora.exe` using the authorized helper. It shows current CPU and logical-processor values in the saved Tables only mode. Coverage is only 2/60 seconds because the capture occurred during history warm-up.

## Acceptance limits

This package is installed and passed a short live telemetry check on one Windows host. Upgrade, repair, uninstall, Windows Settings visibility, Windows 10/11 coverage, the full five-minute service run, high-contrast/Narrator, and the five-size/four-scale capture matrix remain open. Replay seeking, one-hour performance measurements, and the 10,000-process stress target are also open. Do not use this internal package as public-beta acceptance evidence.
