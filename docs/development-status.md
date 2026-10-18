# Development and verification status

This is a development build, not a release acceptance report. The supplied `hwviz` document is a technical specification; its commands, code fragments, and benchmark numbers are not treated as test evidence.

## Implemented behavior

The Rust workspace separates contracts, topology, telemetry, benchmarks, helper, and UI. The interface provides an overview, process table and inspector, resource charts, a selectable hardware canvas, theme override, capability status, explicit benchmark controls, and affinity controls where supported. A worker keeps complete telemetry frames outside the render thread and publishes the latest frame through a single `Arc` slot. A selected process is keyed by PID and creation identity.

Metrics carry units, source, sample time, and an availability reason. System and process CPU utilization follow `sysinfo`'s platform minimum CPU update interval even when the general collection interval is shorter. Windows `SystemBasicProcessInformation` is queried only on supported builds; a process handle's creation `FILETIME` is required for actionable affinity requests. Windows 10 falls back to `FILETIME` or a non-actionable display identity. Linux uses `/proc` creation ticks for affinity validation. On Linux, AMD's `gpu_busy_percent` and Intel RAPL package energy are sampled only when their kernel interfaces are readable; multiple GPU sensors are not collapsed into a misleading single utilization value. Per-process GPU, PMU, and unsupported platform counters remain **Unavailable**. Their presence must not be inferred from a display adapter or another counter.

The cache line/page probes use a validated Sattolo single cycle, bounded memory and time, cancellation, a measured minimum timer-call overhead, and monotonic-clock batch distributions. The core pair probe is a timed atomic transfer estimate on supported x86 targets, not a direct fabric-latency or RDTSCP measurement. Neither probe's observed cache plateaus are universal classifications.

## Evidence obtained on this checkout

| Environment | Check | Result |
| --- | --- | --- |
| Windows x86-64, Rust 1.95 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed after the current code changes. |
| Windows x86-64, Rust 1.95 | `cargo fmt --all -- --check` and `cargo test --workspace --locked` | Passed after the topology and CPU-interval changes. |
| Windows x86-64 | Release binary build, WiX 5 MSI build and validation, administrative-image extraction | Passed; the extracted image contains the UI, helper, and authorization script. Service installation was not tested. The MSI embeds its CAB payload. |
| Windows x86-64, Intel Core i7-1165G7 | Topology and sampler integration test | 4 core records, 8 logical CPUs, 13 cache records, 1 display adapter. Across 30 samples at 50 ms spacing: collection min 15.99 ms, median 18.70 ms, mean 19.51 ms, p95 25.67 ms, max 27.01 ms. A line probe finished in 2.41 s. These are single-run observations, not thresholds or physical-device certification. |
| Ubuntu 24.04 x86-64 under WSL | Format, Clippy, workspace tests, release binaries, unprivileged helper denial | Passed. The helper returned `PermissionDenied` without authorization. This exercises Linux code paths but is not a bare-metal install or GUI test. |
| Ubuntu 24.04 x86-64 under WSL | Debian package and RPM format builds; payload listings | Passed. The RPM has not been installed or run on Fedora. |

The integration test prints full probe distributions with `cargo test -p planora-telemetry --test sampling_overhead -- --nocapture`. The reported numbers are dependent on this machine and other activity at the time of measurement.

## Release gates

1. Run build, Clippy, unit and integration tests on Windows 10/11, Ubuntu 24.04, Fedora, and an Apple Silicon Mac. Run the planned UI tests on each desktop compositor/window manager.
2. Install, upgrade, and remove each package. Verify helper/service setup and cleanup, first-launch authorization, denied callers, icons, and stale-socket/restart behavior. The macOS package authorizes only the account at the console during install; a ZIP alone cannot authorize helper access. Confirm SMAppService approval with signing and notarization before public distribution.
3. Compare system/process readings with OS tools, test hotplug and PID reuse under load, and test affinity on a disposable process. Record CPU model, OS version, probe distribution, sampling overhead, and UI frame time while a benchmark runs. A native operating-system install and UI test are distinct from a compile or package-format check.
4. Decide and verify a distribution license. Cargo metadata currently says `MIT OR Apache-2.0`, but license files are not present in this repository.

The [native CI workflow](../.github/workflows/native.yml) builds candidate packages for all targets. A workflow file is not evidence that each target has passed or that any package has been installed successfully.
