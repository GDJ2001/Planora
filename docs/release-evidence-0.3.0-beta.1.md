# Planora 0.3.0-beta.1 Windows internal build evidence

**Status on 2026-10-21: installed and live-tested on this Windows 11 host; public beta acceptance incomplete.** The package was produced from clean commit `69faf79`, with application version `0.3.0-beta.1`, MSI ProductVersion `0.2.1001`, and helper protocol v4. Artifacts are local under `dist/` and are unsigned.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.1-x64-Setup.exe` | `D4536AB3075BBE17B7C57F2A7273F8FF7C99FADA9F1D63B13D6213B569CB72D9` |
| `Planora-0.3.0-beta.1-x64.msi` | `17F55AD7904132BD33E2116C1F7AC87DD29897AA605CC2AE78E4C35D423CF82A` |
| `planora.exe` in the package manifest | `0FE46EC185D0F980893E581C67890FABF4D84D6695FAA87C85A283ECC15A047A` |
| `planora-helper.exe` in the package manifest | `6C2A2FA2FE03AE903E2318E908D9DE54E8EEBD3A6E35BC30812A7B95769F4A4D` |

WiX MSI validation passed during packaging. On this Windows host, `cargo fmt --all -- --check`, `cargo test --workspace --locked --quiet`, and strict workspace Clippy passed. The final release binary reported source commit `69faf79` and protocol v4. Its fixture visual smoke suite generated and verified both themes at five window sizes, plus process, hardware, benchmark, capability, and settings fixtures at 1180×790. Those fixtures are synthetic renders, not proof of installed native UI behavior. A native v4 UI capture against the installed v3 helper correctly displayed a version-mismatch explanation.

Two initial Setup attempts stopped at Windows elevation. The latest failed Burn log, `dist/planora-0.3-install-retry.log`, records `0x800704c7` (elevated child process canceled), then `0x80070642` and exit code 1602. The MSI did not begin. The user did not see those prompts. A third attempt launched Setup with Windows' explicit **Run as administrator** verb; `dist/planora-0.3-explicit-runas.log` records exit code 0 and the major upgrade from installed 0.2.0. No reboot was requested.

After the successful upgrade, the installed UI reported version `0.3.0-beta.1`, protocol v4 and source commit `69faf79`. The installed UI and helper hashes matched this manifest. Windows showed one visible Planora Installed Apps entry (the chained MSI entry is hidden). The previous authorization SID remained present without exposing its value, and the current-user settings file retained its pre-upgrade SHA-256 (`8426C689C33A56EED3248FBBD675B1E50BE85D776937D3E4BA272E98F931D5EA`). The helper service was **stopped** after upgrade despite preserved authorization; running the packaged administrator authorization script started it. A 20-second installed-service test then completed a protocol-v4 authorized handshake, capability exchange, and 20 consecutive fresh snapshots. The helper auto-start gap is fixed in the next source candidate and still needs upgrade verification.

The installed native `eframe` framebuffer produced 90 PNGs across nine pages, both themes, and 1024×640, 1180×790, 1366×768, 1536×864 and 1920×1080 logical sizes. The local manifest is `dist/native-installed-0.3/manifest.json`; each image records its binary hash and actual dimensions. The effective capture scale was 125%, despite the system-reported DPI value of 96. Representative images were visually reviewed, while the manifest intentionally leaves unreviewed images marked pending. The 1024×640 and 1180×790 Capabilities images clip the recovery column without horizontal scrolling, and History uses awkward negative-offset interval labels. Source fixes for these are in the next beta candidate; the installed beta.1 still has them.

[GitHub Actions run 39505160639](https://github.com/GDJ2001/Planora/actions/runs/39505160639) failed on both the original attempt and a rerun before any job steps executed. The Windows check-run annotation states: “The job was not started because an Actions budget is preventing further use.” This is an account-level execution blocker; it is not evidence that the source checks failed or passed in CI. Restore the Actions budget and rerun before acceptance.

Remaining release gates are tracked in [the capability and verification matrix](beta-readiness.md). In particular, AMD metric collection and real disk ETW/PMU profiling are not implemented, Intel/NVIDIA/AMD native sensor comparison is incomplete, repair/uninstall and wider Windows acceptance are untested, and distribution signing is absent.
