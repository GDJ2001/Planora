# Planora 0.3.0-beta.10 internal package evidence

Beta.10 exposes the existing experimental Windows disk ETW profile in the native UI, polls active benchmark/profile status to completion, reports profile elapsed time, and treats a cancelled trace as cancelled after ETW cleanup. It also removes a timing-sensitive telemetry test assumption. The package was built from clean committed revision `0b36271` with MSI ProductVersion `0.2.1010` and helper protocol v5. It is unsigned and for internal testing.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.10-x64-Setup.exe` | `565996EC6105432C5C035F1A80BD32DF83927916738B45305AE04E259960C720` |
| `Planora-0.3.0-beta.10-x64.msi` | `CC7DB9183B97B7424DBBD3765E37C1FFEDF6CAA84C586D6B08F40B545BE2EE00` |
| `planora.exe` | `AB7C74D780E11AC9BD79A7EDAF622ED1F1A9F7192D8C742890D38460E9AB4042` |
| `planora-helper.exe` | `3A7F303DFAB3EC1D7231D69E21B2E929947E8A59CE2461C3FAD69FD0AA0F202F` |
| `planora-sensor-broker.exe` | `B9B4AE25DBDFF1FBD8AEB5BBE991F5E915DFA48EFCC6F9B68E314B80130A6639` |

The Setup, MSI, three binaries, and pinned PawnIO payload were independently rehashed against the manifest. The UI `--build-info --output` reports beta.10, `0b36271`, and protocol v5. Workspace library/binary tests, strict Clippy, formatting, broker build, and WiX package validation passed. The full workspace test run initially had one CPU-cadence assertion fail under load; its corrected focused test and the subsequent full library/binary run passed. Integration IPC resilience tests passed on the earlier run, with no later change to that transport.

The [beta.9 installed evidence](release-evidence-0.3.0-beta.9.md) documents the starting system and its cancellation defect. On this Windows 11 x64 PC, the beta.10 Burn upgrade returned `0x0` with no restart. Installed UI, helper, and sensor broker hashes match the manifest; the UI and broker report beta.10/revision `0b36271` and the UI reports protocol v5. The PlanoraHelper and PawnIO services were running. Administrator-owned authorization remained present, and the settings hash stayed `BE9142FB24191383772F215D895A7426061B23DBC00747AC597933857D943AC8`.

The installed helper returned 20 distinct fresh protocol-v5 snapshots with maximum observed CPU age 423 ms. The helper-owned broker returned 46 available sensor readings, including one CPU package-power reading and 11 CPU temperatures. An explicit three-second installed ETW disk profile completed in 3015 ms with 56 events, zero reported losses, five process records, and 100% attributed bytes in this one trace. A separate 15-second profile acknowledged cancellation and ended in `Cancelled`, rather than beta.9's incorrect `Completed` state. No `PlanoraDiskIo` ETW session remained after cancellation, and the helper service remained running. These results establish one-machine lifecycle behavior, not general counter accuracy or event-loss resilience.

An installed native [Benchmarks capture](screenshots/native/beta10-benchmarks-installed.png) rendered at 1475×988 pixels from a requested 1180×790 logical-pixel window (about 125% effective scale), with live telemetry and no visible clipping on the default probes tab. The **Advanced Profiling** tab has not been visually reviewed yet. The [GitHub Actions run for evidence commit `15d0959`](https://github.com/GDJ2001/Planora/actions/runs/39789228358) marked Windows, Ubuntu, Fedora, and macOS jobs failed before any steps were created; no CI test result can be inferred from it. Normal installed launch, keyboard/Narrator flow, other sizes/scales, independent sensor comparison, repair/uninstall, Windows 10, other vendor hardware, sustained profiling, and redistribution review remain open.
