# Planora 0.3.0-beta.45 optimization evidence

Beta.45 completes the code changes left open by the beta.44 optimization pass. The packages below were built from clean commit `98b6318390a267265efe607ee04d7abc2b0c0304` with Rust 1.99.1. The helper protocol remains v6, settings schema remains 7, and recording format remains v4. These unsigned packages are internal builds.

## Changes

| Area | Beta.45 implementation | Measurement semantics |
| --- | --- | --- |
| Process table | A bounded worker filters and sorts large process sets and remaps PID plus creation identity after provider vector changes. Rendering uses the last consistent immutable frame while a new index is prepared. | Selection, scroll, focus, integer sorting, unavailable values, and the one-second reorder limit remain unchanged. |
| Charts | A bounded worker prepares downsampled geometry for dense histories. The cache includes series identity, generation, range, width, scale, source, and gap state. | Interval extrema, gaps, source changes, timestamps, chart inspection, and exact table/export values remain in the source history. |
| History | The pending-frame queue is capped at four frames and 8 MiB. An explicit stop flag wakes a saturated worker, and publication is limited to once per second. | Overload drops coverage and leaves gaps; it does not synthesize zero values. |
| Diagnostics | Bounded IPC JSON encode/decode timing and a counting Rust allocator are included in the on-request performance report. | Allocation totals describe Rust process allocations, not operating-system working set. Codec timing measures this process's request/response JSON path. |
| Build | `serde` explicitly enables its `rc` feature for isolated core builds. | No wire or persistence format change. |

The prior beta.44 pass already introduced shared `Arc<TelemetryFrame>` snapshots, immutable history generation sharing, provider timing, first-paint/frame counters, recording timing, and focused/unfocused/minimized sampling requests. Beta.45 retains those behaviors.

## Repeatable checks

Windows and Ubuntu 24.04 WSL both passed `cargo fmt --all -- --check`, `cargo test --workspace --locked --quiet`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`. An isolated `cargo check -p planora-core --locked` passed after the Serde feature correction. Focused tests cover 10,000 process rows, PID identity remapping after vector reorder, 900-sample chart preparation with a gap and spike, and shutdown with the history queue full. The 10,000-row test asserts a steady-frame **mean** below 33.3 ms; it does not establish a p95 result.

The installed Ubuntu WSL DEB reports beta.45, commit `98b6318`, and protocol v6 in both UI and helper. The `planora-helper`, `planora-profiler`, and `planora-pmu` services were active after the upgrade. The installed UI and helper hashes matched their newly built Linux release binaries. The settings file hash and two test data markers were unchanged; one existing authorization record remained present. An IPC probe returned 20 snapshots at a requested 100 ms, including 19 distinct sequences. The helper reported a 100 ms effective interval, 6.48 ms p95 collection duration, and 6.70 ms maximum. A two-client probe verified fastest-client arbitration (100 ms) and return to 250 ms after that client disconnected. Four samples appeared during 3.2 seconds with no clients, consistent with the one-second idle rate. These WSL observations are supplemental Linux evidence, not physical Ubuntu or Fedora acceptance.

A 15-second Windows release-build fixture run produced 882 UI frames with p95 1.08 ms and p99 1.28 ms. It reported 1,311,877 Rust allocations, 3.82 MiB live and 24.6 MiB peak live at report time, and 367.5 MiB resident memory. This fixture does not run the live helper, recording, or 10,000-process UI load. Its `first_paint_ms` is absent because fixture capture bypasses normal startup. The fixture report is [`dist/beta45-performance-dark-perf.json`](../dist/beta45-performance-dark-perf.json). Windows Performance Recorder was present, but CPU tracing could not start: policy error `0xc5585011` (“Failed to enable the policy to profile system performance”). WPR was confirmed stopped afterward.

Eighteen Windows fixture screenshots cover Performance in both themes at 1024×640, 1180×790, 1366×768, 1536×864, and 1920×1080 logical sizes, plus Processes and History at compact and wide sizes. The system captured them at 125% effective scale. The [capture manifest](../dist/beta45-captures/manifest.json) records dimensions and hashes. A visual review found no visible panel clipping in those captures; gap and spike rendering remained visible. The captures use simulated data and do not establish installed live appearance, Narrator behavior, or the other display scales.

## Packages and identity

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `Planora-0.3.0-beta.45-x64-Setup.exe` | 43,937,315 | `af56f8d1fb7139ebf603a3767cfe5ec78fba97def54001de0d028751f4a81eb9` |
| `Planora-0.3.0-beta.45-x64.msi` | 40,038,400 | `886475a4780cf703fb377af9260da564c3c6a1ea6e58e22670ee811c2e96ba11` |
| `planora_0.3.0~beta.45-1_amd64.deb` | 7,337,172 | `4490ae55aa59ff3be9802a9043631a7df69c02330e315843cc6ff3f70fa404db` |
| `planora-0.3.0-0.beta.45.x86_64.rpm` | 9,480,325 | `76791abe9bdc6211b35f6976f8e4d17f8378cf8c88c6ec7a9de55183e65038c9` |
| `Planora-0.3.0-beta.45-x64.manifest.json` | 1,085 | `b9192718f118fdb63edd67ca7b52bcff80cbaa54886d1aee15a378343241e4a5` |

The Windows MSI has ProductVersion `0.2.1045`. WiX validation passed. An administrative MSI image extracted the UI, helper, and sensor broker; each file hash matched the package manifest. The installed beta.45 UI, helper, and sensor broker were subsequently checked against those same hashes on the Windows reference PC. The helper started, and the application showed live telemetry. A 100-snapshot installed-helper probe at the requested 100 ms interval measured collection p95 **62.72 ms** (refresh p95 **53.0 ms**), missing the 50 ms collection target. This prompted the beta.46 process-refresh change. The current `dist/provenance.json` and `dist/SHA256SUMS.txt` describe beta.46; the beta.45 package hashes remain in the table above.

## Open acceptance gates

- Measure live focused/unfocused/minimized CPU use, native startup/shutdown, recording, benchmark, multi-client IPC, and 10,000-process **p95** frame time. The installed beta.45 collection p95 missed its target as recorded above. The 25% minimized CPU improvement needs a measured always-100-ms baseline; no comparable measurement was completed here.
- Run the one-hour combined monitoring, recording, benchmark, and profiling scenario, checking sustained memory, allocations, handles/file descriptors, and worker counts. The 15-second fixture and short WSL probe cannot establish this gate.
- Review remaining pages, 100%, 150%, and 200% scaling, enlarged text, Narrator/Orca, and native Windows/Linux desktop behavior. Physical Ubuntu/Fedora package, PMU, disk profiling, and performance acceptance remain open. WSL is not a substitute.
- Recheck CI execution, signing, redistribution notices, and public-release approval before distribution.
