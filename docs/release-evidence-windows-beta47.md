# Planora 0.3.0-beta.47 Windows adaptive sampling evidence

Beta.47 was built from clean commit `173186cd068cd42123adaf8034e5c6b7c0d78124` with Rust 1.99.1. It retains helper protocol v6, settings schema 7, recording format v4, the approved UI, and the beta.46 process-acquisition optimization. This unsigned build is internal.

## Regression and change

An installed beta.46 Windows probe confirmed the 100 ms focused collection target: 100 distinct snapshots, collection p95 5.432 ms, compared with beta.45's 62.72 ms. A separate native interaction exposed an adaptive-rate regression: after minimizing Planora, a slower second client still saw the helper's effective interval at 100 ms. The UI-frame callback was the only place that updated the requested interval; minimized windows do not reliably deliver those frames.

Beta.47 calculates the Windows requested interval in the existing telemetry client worker from the native window's minimized, visible, and foreground states. The saved preference remains the focused target, initially 100 ms. Unfocused windows request at least 500 ms after a two-second debounce; minimized or hidden windows request at least 1,000 ms immediately. Returning to foreground requests the saved interval. No new thread was added. A unit regression covers the debounce, minimized state, and saved intervals above the adaptive floors. The 2,000 ms IPC probe now sends uncounted keepalive reads so the helper's idle transaction deadline does not close the probe connection between measured samples.

Windows `cargo fmt --all -- --check`, `cargo test --workspace --locked --quiet`, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. The focused adaptive policy test passed. The Ubuntu 24.04 WSL VHDX could not attach because Windows reported `HCS/ERROR_SHARING_VIOLATION`; no distributions were running, and WSL shutdown did not release the attachment. Beta.47 changes Windows-only application logic and the cross-platform probe script, but a Linux build/check of this exact revision remains open. Beta.46 WSL package and live-helper checks remain historical evidence.

## Package identity

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `Planora-0.3.0-beta.47-x64-Setup.exe` | 43,945,127 | `1081265fbd31435648f33dc6ee082704106d1b86a159c3778b85705a53ac4342` |
| `Planora-0.3.0-beta.47-x64.msi` | 40,046,592 | `dcf11d7520a088e503cf09587f90e757e56ff8b1ccb32a57745e6c365eec0311` |

The MSI ProductVersion is `0.2.1047`. WiX theme and MSI validation passed. An administrative MSI image contained the UI, helper, and sensor broker, and all three hashes matched [`dist/Planora-0.3.0-beta.47-x64.manifest.json`](../dist/Planora-0.3.0-beta.47-x64.manifest.json). [`dist/provenance.json`](../dist/provenance.json) records a clean source commit and the artifact hashes. No beta.47 Linux package was produced while the WSL VHDX was locked.

The Windows reference PC was upgraded from beta.46. Burn recorded `Apply complete, result: 0x0`, no restart, and exit code `0x0`. The installed UI, helper, and sensor broker hashes matched the beta.47 manifest. Both installed Rust binaries reported beta.47, commit `173186c`, and protocol v6. The helper was running, the authorization record remained present, and the settings file hash remained `8D1100B233E5BB729D71565ED199FF78F0EAD7644712749B07A7BBBB671B872D`.

With the installed UI open, a second client requesting 2,000 ms observed the helper's fastest-client effective interval at **100 ms focused**, **500 ms after more than two seconds unfocused**, and **1,000 ms while minimized**. Restoring focus returned it to **100 ms**. A separate 100-snapshot installed-helper probe at 100 ms returned 99 distinct sequences and collection p95 **9.322 ms** (median 5.394 ms). One maximum collection interval reached 138.925 ms, associated with a 133.794 ms network-provider interval; this short run met the p95 target but does not establish long-run tail stability. The redacted report is [`dist/beta47-windows-live-ipc.json`](../dist/beta47-windows-live-ipc.json).

A second 100-snapshot run returned 100 distinct sequences, collection p95 **7.160 ms**, and maximum **9.233 ms**. Its network-provider maximum was 3.180 ms. The first run's long network interval did not recur in this short repeat; longer monitoring is still needed before dismissing tail spikes. See [`dist/beta47-windows-live-ipc-repeat.json`](../dist/beta47-windows-live-ipc-repeat.json).

## Remaining verification

- Measure focused versus minimized CPU against a comparable always-100-ms baseline. Complete the one-hour combined growth run and 10,000-process UI p95 test.
- Restore Ubuntu WSL access for exact-revision Linux checks; repeat physical Ubuntu/Fedora and broader desktop/accessibility acceptance before public distribution.
