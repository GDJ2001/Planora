# Planora 0.3.0-beta.1 Windows internal build evidence

**Status on 2026-10-21: built and source-checked; installation and public beta acceptance incomplete.** The package was produced from clean commit `69faf79`, with application version `0.3.0-beta.1`, MSI ProductVersion `0.2.1001`, and helper protocol v4. Artifacts are local under `dist/` and are unsigned.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.1-x64-Setup.exe` | `D4536AB3075BBE17B7C57F2A7273F8FF7C99FADA9F1D63B13D6213B569CB72D9` |
| `Planora-0.3.0-beta.1-x64.msi` | `17F55AD7904132BD33E2116C1F7AC87DD29897AA605CC2AE78E4C35D423CF82A` |
| `planora.exe` in the package manifest | `0FE46EC185D0F980893E581C67890FABF4D84D6695FAA87C85A283ECC15A047A` |
| `planora-helper.exe` in the package manifest | `6C2A2FA2FE03AE903E2318E908D9DE54E8EEBD3A6E35BC30812A7B95769F4A4D` |

WiX MSI validation passed during packaging. On this Windows host, `cargo fmt --all -- --check`, `cargo test --workspace --locked --quiet`, and strict workspace Clippy passed. The final release binary reported source commit `69faf79` and protocol v4. Its fixture visual smoke suite generated and verified both themes at five window sizes, plus process, hardware, benchmark, capability, and settings fixtures at 1180×790. Those fixtures are synthetic renders, not proof of installed native UI behavior. A native v4 UI capture against the installed v3 helper correctly displayed a version-mismatch explanation.

Two Setup upgrade attempts stopped at Windows elevation. The latest Burn log, `dist/planora-0.3-install-retry.log`, records `0x800704c7` (elevated child process canceled), then `0x80070642` and exit code 1602. The MSI did not begin. Afterward, Installed Apps still showed Planora 0.2.0, the installed UI reported protocol v3 and source commit `96cebaf`, `PlanoraHelper` remained running, and the current-user settings file's SHA-256 was unchanged (`8426C689C33A56EED3248FBBD675B1E50BE85D776937D3E4BA272E98F931D5EA`). A successful interactive UAC approval and installed v4 smoke test are still required. Do not describe this attempt as an installed upgrade.

[GitHub Actions run 39505160639](https://github.com/GDJ2001/Planora/actions/runs/39505160639) failed on both the original attempt and a rerun before any job steps executed. The Windows check-run annotation states: “The job was not started because an Actions budget is preventing further use.” This is an account-level execution blocker; it is not evidence that the source checks failed or passed in CI. Restore the Actions budget and rerun before acceptance.

Remaining release gates are tracked in [the capability and verification matrix](beta-readiness.md). In particular, AMD metric collection and real disk ETW/PMU profiling are not implemented, Intel/NVIDIA/AMD native sensor comparison is incomplete, installed package lifecycle is untested for 0.3, and distribution signing is absent.
