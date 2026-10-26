# Planora 0.3.0-beta.4 Windows internal Setup evidence

**Status on 2026-10-21: package built; native installation verification in progress.** This version addresses the [beta.3 fresh-install failure](release-evidence-0.3.0-beta.3.md). It is unsigned and remains internal.

| Identity | Value |
| --- | --- |
| Source commit | `428e6d4` |
| Application version | `0.3.0-beta.4` |
| MSI and Burn version | `0.2.1004` |
| Helper protocol | v4 |

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.4-x64-Setup.exe` | `F93DC8B97FAE289576DEFFE2B8855D287ED7E978F4C596B0F174282AD18F5407` |
| `Planora-0.3.0-beta.4-x64.msi` | `C66F369A4EC2FF42706D9B5F9471C44A3339D6F9302797E8E5031A38BA0981AB` |
| `planora.exe` | `3A3C8068C1E2072E8229A88EC7E46CB99954C4A4E978B1D14E0D4AC4C0FB4EBC` |
| `planora-helper.exe` | `FD9F3338B394E95AC8DE8B4A256E792A4C3A78DB147D78095C9E018BA59D6FB5` |

The release script built the UI and helper from a clean committed revision, checked both build identities, validated the MSI, and produced `dist/Planora-0.3.0-beta.4-x64.manifest.json`. The Setup and MSI SHA-256 values match that manifest. The MSI `Component` and `ServiceControl` tables place helper startup in `StartAuthorizedHelper`, conditioned on `PLANORA_PREVIOUS_SID`; the unconditional helper component only stops/removes the service. This prevents a first install from waiting for an unauthorized service to start. The UI presents authorization after a bounded connection failure.

`cargo fmt --all -- --check`, `cargo test --workspace --locked`, strict workspace Clippy, the first-install UI regression, `git diff --check`, WiX build, and MSI validation passed. The first native Setup attempt ended before installation with Burn exit `0x642` and a Windows dialog stating “User cancelled installation”; no application files or service were present afterward. This does not test the MSI fix. Native install, helper authorization, telemetry, maintenance, and removal outcomes will be recorded after those actions finish.
