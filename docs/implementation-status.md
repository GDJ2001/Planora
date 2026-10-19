# Planora implementation audit and next milestones

**Audited:** 2026-10-18  
**Compared against:** the development plan supplied in the project discussion and the `main` source tree at `6a32804`. The repository includes the hwviz DOCX, but its complete binary text was not independently extracted for this audit.  
**Status:** implementation in progress; **not ready for public release**. This audit is a historical baseline; see the [0.1.5 release evidence](release-evidence-0.1.5.md) for current local checks.

## Overall estimate

Approximately **50% of the requested feature scope has source-code coverage**, but only **20–25% release readiness** is evidenced. These are engineering estimates, not test-derived measurements. In particular, a feature implemented on one OS and untested on the others is *partial*, not complete. The sole CI run observed at audit time, [39123949892](https://github.com/GDJ2001/Planora/actions/runs/39123949892), was **failed** for Windows, Ubuntu, Fedora and macOS with no successful build established.

| Plan workstream | Source status | Remaining acceptance evidence |
| --- | --- | --- |
| Branding and design tokens | **Largely implemented** | Check generated assets in all native windows and packages. |
| Rust 2024 workspace / contracts | **Largely implemented** | Verify exact toolchain and dependency lockfile compile on all targets; expand contract evolution/compatibility tests. |
| CPU/cache/NUMA/GPU topology | **Partial** | Platform-specific validation against native OS topology; hotplug and hardware fixtures; avoid inferred links. |
| CPU, process, disk, network sampling | **Partial** | OS-tool comparisons, sustained 100 ms overhead tests, cache of unchanged metadata, counter reset handling. |
| GPU/power/PMU metrics | **Mostly absent** | Approved providers and correctly explained unavailable states. |
| Native views (overview, processes, canvas, inspector, charts, theme) | **Substantially implemented** | Native usability, accessibility, rendering and selection tests for each platform. |
| Sattolo line/page probes and core matrix | **Partial** | Timer calibration, cancellation under load, timing distribution integrity, hardware comparability and repeatability. |
| Process affinity | **Partial on Windows/Linux; unavailable on macOS** | PID-reuse race review, process handle/pidfd lifetime strategy and disposable-process OS integration tests. |
| Privileged helper / IPC | **Partial, with security blockers** | Strong client identity binding on each OS, authorization install/remove verification, resource-exhaustion tests. |
| Platform packages / CI | **Scripts exist, unverified** | Successful four-platform CI, real install/upgrade/uninstall and service lifecycle tests, signed-release hooks. |
| Test suite / hardware profiling | **Early** | Property tests, recorded fixtures, UI tests, native integration, benchmark trace artifacts. |

## Confirmed high-priority gaps

1. **P0 — Linux helper startup:** The systemd unit specifies `User=planora` and effective `CAP_PERFMON`, but helper startup previously refused every non-root UID. The companion PR changes that gate to accept a root process or Linux effective `CAP_PERFMON` and adds parser tests. **Runtime still needs verification on Ubuntu and Fedora.**
2. **P0 — macOS privileged IPC policy:** macOS socket access is granted to the `staff` group and `peer_authorized` only checks whether peer credentials exist. This does **not** independently prove that the caller is the specifically authorized user. Bind the helper's client identity/authorization to the OS-approved authorization flow before claiming the macOS helper secure.
3. **P0 — No passing CI:** All four jobs in the first workflow run failed, and their returned job step summaries were empty. Investigate the Actions run and runner availability first; then resolve compilation, package and permission errors based on actual logs. There is **no proven native build** at audit time.
4. **P1 — Affinity PID-reuse race:** Linux checks `/proc/<pid>/stat` before and after `sched_setaffinity(pid, ...)`, but this is not an atomic identity-bound handle operation. Review a race-proof OS strategy, explicitly test process termination/reuse and fail closed when it cannot be guaranteed. Windows handle-based sequence validation also needs platform testing.
5. **P1 — Unsupported metric coverage:** GPU/PMU/power and per-process GPU paths are marked unavailable; provide real supported providers and capability-specific reasons without proxy values.
6. **P1 — Probe comparability:** Current pointer chase reports monotonic nanoseconds per load and marks x86-cycle comparability false. Verify timer overhead/calibration, precise CPU affinity support, distribution correctness and interruption behavior on real hardware before publishing architectural classifications.
7. **P1 — Runtime UI behavior:** Telemetry frames use a replaceable `Arc` slot, but there is no evidence yet of sustained frame-time/CPU overhead under benchmarks. The charts previously graphed missing disk/network data as zero; the PR skips unavailable samples.
8. **P1 — Packaging:** Validate service registration/authorization, Linux capabilities, uninstall cleanup and upgrades on each OS. macOS signing/notarization should remain a release gate.
9. **P2 — Documentation accuracy:** Distinguish reference measurements from independently reproduced results; add machine-readable benchmark reports. README CI badges and license claims must match repository facts.

## Work completed in this review branch

- Corrected the Linux capability-aware helper launch gate (unit tests for present/missing/malformed CapEff).
- Prevented false zero latency when the inter-core API receives the same CPU twice (regression test).
- Stopped unavailable disk or network metrics from being converted into plotted zero samples.
- Added this scoped, evidence-based audit and prioritized acceptance plan.

These changes are **unverified until CI and native integration tests pass**.

## Next executable milestones

### M1 — Make a minimal vertical slice run safely on all platforms
- Get `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` green on Windows, Ubuntu, Fedora and Apple Silicon macOS.
- Validate privilege separation. On Linux: packaged `planora` account, effective `CAP_PERFMON`, local socket access granted only after group authorization, service start/stop/restart. On Windows: actual service SID ACL and admin-authorization flow. On macOS: tie caller credentials to explicit local authorization, not `staff` membership alone.
- Test one end-to-end path: UI connects; authorized helper returns topology, snapshot and capability states; unauthorized client is denied.

### M2 — Proven real measurements and safe actions
- Fixture and native tests for topology, creation identity, CPU/time units, cumulative counter changes/resets, unavailable states and hotplug.
- Add a bounded telemetry overhead and UI frame-time reporting harness. Capture OS/version/CPU/clock/interval for each run.
- Harden affinity against PID reuse; confirm old/requested/actual sets for disposable processes.
- Validate Sattolo cycle invariants, timer calibration, cancellation, memory/concurrency ceilings and CPU-pair results using physical machines.

### M3 — Release candidates
- Confirm icon, accessibility and theme consistency and process-selection persistence.
- Install, update and uninstall MSI/DEB/RPM/macOS packages on clean target machines. Check helper and authorization cleanup.
- Emit reproducible CI artifacts and release notes with supported/unsupported hardware table. No claim of public macOS distribution until signing/notarization is functioning.

## Explicit release gates

- **Security:** no unauthenticated process can send privileged operations through local IPC; no hidden privilege escalation; no silent permission changes.
- **Correctness:** no unavailable metric is shown as measured zero; process identity verified before privileged mutations; topology never invents hardware relationships.
- **Performance:** measured sampling and frame-time overhead reported with machine configuration while benchmark work runs.
- **Portability:** real Windows 10, Windows 11, Ubuntu 24.04, Fedora and Apple Silicon macOS integration runs; artifacts install, upgrade and uninstall.
- **Reproducibility:** green CI checks and provenance for any advertised benchmark numbers.
