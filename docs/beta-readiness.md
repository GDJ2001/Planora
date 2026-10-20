# Planora 0.2.0-beta.1 capability and verification matrix

**Status: internal beta candidate, not approved for public distribution.** The installed Windows 0.1.5 release is the native baseline in [its historical verification record](release-evidence-0.1.5.md). Source checks for 0.2.0-beta.1 do not establish installed behavior. The older [implementation audit](implementation-status.md) is historical and should not be read as current release evidence.

| Area | 0.2.0-beta.1 source | Automated evidence | Native acceptance still required |
| --- | --- | --- | --- |
| Sampling and contracts | Protocol v3; per-client interval arbitration; supported 100/250/500/1000/2000 ms rates; invalid frames rejected; original metric timestamps retained | Unit tests for arbitration, frame validation, stale and duplicate frames | Windows 10/11 OS-tool comparisons, 100 ms p95 collection and sustained load |
| Benchmark lifecycle | Per-job cancellation, `Cancelling` state, no restart until worker exits; service shutdown signals and waits up to 2 seconds | Cancellation, bounds, and worker-shutdown tests | Stop/restart/service shutdown under active line, page, and matrix probes |
| Processes and affinity | Stable PID plus creation identity; Windows process I/O separated from disk I/O; selected/pinned filters and delayed reordering | Identity and selection tests | 10,000-process interaction, keyboard focus and scroll, disposable-process affinity; persisted column widths/order remain open |
| Resource providers | Disk devices, network interfaces, individual Windows GPU Engine PDH instances; errors typed rather than a fabricated GPU total | Windows GPU counter test and telemetry tests | Cross-check instance semantics and adapter/device identity; device disappearance gaps and additional API fallbacks remain open |
| Recording, replay, exports | Explicit local `.planora.jsonl` v2 recording, bounded writer and 2h/256 MiB limits, truncated-tail recovery, process/chart CSV and benchmark/diagnostic JSON | Recording recovery, opt-in selection, CSV formula escaping | Long-run cap and write-failure exercise, replay UI without helper, cross-version imports and export inspection |
| Settings and authorization | Versioned user settings; atomic replacement; visible save error; Windows authorization launch deduplicated; explicit revoke script | Workspace tests and script/parser checks | Preserve 0.1.5 authorization across major MSI upgrade; verify revoke/uninstall ACLs and helper identity in an installed 0.2 build |
| Visual and accessibility | Responsive cards/charts, explicit unavailable states, 15-minute history, clearer units and text controls | Headless fixture smoke and layout tests | Complete native visual manifest; Windows Narrator, scaling, high contrast, reduced motion, keyboard-only and long-label checks |
| Packages and supply chain | Numeric MSI ProductVersion for prerelease; dual license texts packaged; pinned CI actions; dependency inventory, hashes and provenance generator; scheduled advisory job | Local script and YAML parsing | Clean-revision release package build, installed hash checks, Actions budget recovery, signed distribution and approved license |
| Linux / macOS | Preview packaging; Linux service retains `CAP_PERFMON` and removes `CAP_SYS_NICE` | Cross-platform source guards; Windows-host checks only in this work | Native install, authorization, lifecycle, benchmarks, visuals and removal on Ubuntu, Fedora and Apple Silicon |

## Public Windows beta gates

1. Complete a clean committed Windows package build and install/upgrade from 0.1.5. Verify UI/helper binary hashes and protocol identity against the package manifest. Test service start/stop, cancellation, authorization preservation/revocation, uninstall and reinstall.
2. Fix authorization preservation across a major MSI upgrade. The current service-owned registry value can be removed during service replacement. Do not claim the gate passed by reauthorizing manually.
3. Complete Windows 10 and 11 native comparisons, accessibility, visual review at the specified window sizes and display scales, and one-hour profiling with recording and benchmarks. Record p95 sampling/UI times, CPU, memory, allocations and handles; treat targets as machine-specific.
4. Restore GitHub Actions execution, review dependency/advisory output, approve distribution license/third-party notices, and obtain signing credentials. Unsigned installers are internal only.

Features explicitly outside this cycle: process termination, priority editing, remote/cloud telemetry, automatic updates, kernel drivers, broad PMU expansion. Unsupported metrics remain unavailable with a reason.
