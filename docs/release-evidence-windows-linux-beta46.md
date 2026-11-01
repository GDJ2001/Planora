# Planora 0.3.0-beta.46 optimization evidence

Beta.46 was built from clean commit `8ac3ee0a7bfcc408c0e0555ab2407773911dade0` with Rust 1.99.1. It addresses the installed beta.45 Windows helper collection p95 of 62.72 ms, above the 50 ms target. The helper protocol remains v6, settings schema remains 7, and recording format remains v4. These unsigned packages are internal.

## Implementation and measurements

Windows process inventory and metrics now refresh in an owned background worker. The sampler reads the last completed immutable process snapshot, retaining each metric's original timestamp. Requests to the worker use a bounded one-slot channel; shutdown signals and joins the worker. Linux continues using its existing process path. A direct comparison found that reducing `sysinfo` process refresh fields was slower on this host, so beta.46 keeps the established field set and changes scheduling instead. The `process_refresh_background` diagnostic reports completed worker time separately from the sampler's `collection_total`.

A focused Windows sampler test after this change measured collection p95 **23.69 ms**. This is a local test, not an installed-helper result. The repeatable read-only IPC probe is [`scripts/measure_live_ipc.py`](../scripts/measure_live_ipc.py); it reports aggregate timings and process counts without exporting process names.

The installed beta.46 Windows helper returned **100 distinct snapshots in 100 requests** at requested/effective 100 ms. Collection p95 was **5.432 ms** (p50 4.487 ms, maximum 5.925 ms), below the 50 ms target and 91.3% lower than the installed beta.45 p95 on this PC. The separate background process refresh measured p95 **27.845 ms** and is excluded from `collection_total` because it does not block the sampler. Process counts ranged from 305 to 308. The redacted report is [`dist/beta46-windows-live-ipc.json`](../dist/beta46-windows-live-ipc.json). This is a short live run, not one-hour stability evidence.

Windows and Ubuntu 24.04 WSL passed `cargo fmt --all -- --check`, `cargo test --workspace --locked --quiet`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`. Python syntax and a short WSL probe for the new script passed. The Windows process sampling regression waits for the asynchronous inventory warm-up before checking process presence.

The beta.46 DEB was installed over beta.45 in Ubuntu 24.04 WSL. `planora-helper`, `planora-profiler`, and `planora-pmu` were active, installed UI/helper hashes matched their release binaries, and the existing settings hash remained `cabc4d9661ad082865681cdeea1c801d077ad4c1d55993379067989e9e067a1f`. A 100-snapshot installed-helper probe returned 99 distinct sequences, requested/effective 100 ms, and collection p95 **4.252 ms** (maximum 19.425 ms). The redacted report is [`dist/beta46-wsl-live-ipc.json`](../dist/beta46-wsl-live-ipc.json). WSL results supplement native Linux acceptance.

## Packages and identity

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `Planora-0.3.0-beta.46-x64-Setup.exe` | 43,949,207 | `1c957c6c743c855f29cce71b4251f7c3c990c47288a48bacb89eecd42fab3572` |
| `Planora-0.3.0-beta.46-x64.msi` | 40,050,688 | `a305a6bf42c7db2190fb9eca58d73a1bdb764af1fff4352b89671c0dd5792f64` |
| `planora_0.3.0~beta.46-1_amd64.deb` | 7,337,128 | `f975015bda8c808a4cf77d87fc6a043d1de99ab1bdb4d45ec65ea7666f421070` |
| `planora-0.3.0-0.beta.46.x86_64.rpm` | 9,480,290 | `a2f4749c8bddf3ef60655c01fa63e7981f9bc5d629a496890f48dfffbb369661` |

The Windows MSI ProductVersion is `0.2.1046`. WiX theme validation passed. An administrative MSI image contained `planora.exe`, `planora-helper.exe`, and `planora-sensor-broker.exe`; all three hashes matched the package manifest. `PawnIO_setup.exe` is an optional bootstrapper dependency and is not an MSI-installed application file. [`dist/provenance.json`](../dist/provenance.json) records the clean source commit, and [`dist/SHA256SUMS.txt`](../dist/SHA256SUMS.txt) lists package hashes; every listed artifact was checked after packaging.

Windows Setup upgraded the reference PC and Burn recorded `Apply complete, result: 0x0`, no restart, and exit code `0x0`. The installed UI, helper, and sensor broker SHA-256 values matched the beta.46 manifest. The installed UI and helper both reported beta.46, commit `8ac3ee0`, and protocol v6. The existing authorization registry value remained present. Windows left the helper service stopped after the upgrade; the user approved Planora's helper authorization action, after which the service ran and the installed UI opened with live CPU, memory, GPU, disk, and network readings. The Windows settings file was present after the upgrade; no pre-upgrade hash was recorded, so byte-for-byte preservation was not established.

## Open acceptance gates

- Measure focused versus minimized CPU against an always-100-ms baseline, one-hour combined memory/handle/worker growth, 10,000-process UI frame p95, and native startup/shutdown. No comparable sustained run was completed.
- Repeat desktop and package acceptance on physical Ubuntu/Fedora; WSL and Windows fixture screenshots are supplemental. Review remaining display scales, accessibility, CI execution, signing, and redistribution before public distribution.
