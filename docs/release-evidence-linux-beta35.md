# Planora 0.3.0-beta.35 Linux internal package evidence

This record covers clean source commit `6df283ec0fa053245bf7f8d01738f475df6c4a7a` on branch `codex/linux-parity-beta35`. It is an internal, unsigned Linux preview. Windows beta.33 remains the Windows reference build. WSL 2 Ubuntu 24.04 checks below are useful integration evidence; they are not a substitute for a native Ubuntu/Fedora desktop or Linux hardware acceptance.

## Packages and identities

| Artifact | SHA-256 |
| --- | --- |
| `dist/planora_0.3.0~beta.35-1_amd64.deb` | `7afc99d2d0405f3b3b43fd7c17d64863de78162ee21d06d67bff0526e5d95e58` |
| `dist/planora-0.3.0-0.beta.35.x86_64.rpm` | `280b45042ebaa13f2ebe94a390d819bf0db9bb4ae59950a1d805b4619c0cbc87` |
| Installed `/usr/bin/planora` | `02f51f1abb8d76c8613cabd07c71e28e905d431eb1f75f5eb90afaf568bea52b` |
| Installed `/usr/libexec/planora/planora-helper` | `38d171568382d39fa8bfdae3424833a547400e5773a05de745ba83570e573345` |

`dist/provenance.json` records a clean source tree, both packages, the dependency inventory, and package hashes. The installed binary hashes match the release binaries. Both installed binaries report beta.35, commit `6df283e`, protocol v6, and `x86_64-unknown-linux-gnu`.

## Implementation completed on this branch

- Per-user administrator-owned authorization with immediate effect and revocation, Unix peer credentials on each helper connection, and an unelevated UI. The helper continues to run as a dedicated user.
- Linux CPU/process, memory/commit, whole-block-device throughput/latency/queue, network link/speed, DRM GPU enumeration, supported AMD GPU metrics, hwmon temperatures/fans, and powercap energy-derived readings. Unsupported measurements retain typed availability rather than invented zeroes.
- Sampled PMU profiling through kernel-permitted `perf_event_open` sources, bounded sample rings, target process/thread identity checks, sample-loss accounting, and time-enabled/time-running scheduling metadata. Samples are not exact instruction counts.
- Linux graphical-session single-instance activation, automatic desktop appearance, XDG storage and window state, desktop files/icons/AppStream metadata, and native DEB/RPM service packages.

## Automated and WSL 2 checks

- Rust 1.99.1: workspace tests, formatting, and strict all-target Clippy passed under Ubuntu 24.04 WSL. Windows workspace check passed separately. The UI suite included 92 passing tests; two existing integration tests remained ignored.
- DEB upgrade from beta.34 to beta.35 preserved the existing authorization record and user preference/recording markers. The booted systemd helper remained active. AppStream validation and `systemd-analyze verify` passed. The RPM was built and inspected, but not installed in Fedora.
- An unauthorized helper connection was rejected. Authorization allowed 20 distinct fresh snapshots at a 100 ms requested/effective interval. Revocation closed an already authorized connection. Beta.34 removal stopped the helper and removed system authorization/binaries while preserving personal markers; a reinstall required authorization again.
- The installed beta.35 helper returned 20 fresh snapshots. Linux whole-block-device records used the `Linux block sysfs` source; WSL exposed `sda`, `sdb`, and `sdc`. A temporary 32 MiB file write advanced the corresponding `sdc` kernel counters. A 2-second PMU session returned seven samples, zero reported losses, `100.0%` scheduled time, and a complete result. These are WSL-specific observations, not performance claims for physical hardware.
- The installed UI opened at 1180×790 over WSLg X11 with live helper access. With an intentionally mode-755 `XDG_RUNTIME_DIR`, normal startup used a private mode-700 fallback. Sending the window manager's `WM_DELETE_WINDOW` message closed the UI with exit code 0, removed its activation socket, and left only the independently installed helper running. The runtime directory was restored to mode 700 after this check.
- A follow-up launch test found a beta.35 defect: when WSL's runtime directory changed from mode 755 to mode 700 between launches, each launch used a different lock location and two UI windows opened. The source fix is in beta.36; beta.35 should not be used as single-instance acceptance evidence across runtime changes.
- [Installed live capture](screenshots/native/linux-beta35/performance-cpu-wsl-x11-live.png) shows the 1180×790 Performance view with live CPU telemetry and three observed seconds of history. The earlier beta.34 image in the adjacent folder is fixture data and must not be treated as live evidence.

## Remaining gates

| Area | Status |
| --- | --- |
| Attributed disk profiling | **Not implemented.** No packaged libbpf CO-RE collector or owned BPF links/maps exist yet. The capability reports this explicitly. |
| Profiling privilege isolation | The monitoring helper currently carries `CAP_PERFMON`; the planned separate profiling worker with a narrower capability set is not yet implemented. No `CAP_SYS_ADMIN` is granted. |
| GPU providers | AMD sysfs metrics are implemented where exposed. Intel/NVIDIA physical GPU engine and per-client acceptance remain incomplete; aggregate device busy must not be called a physical engine. |
| Native distributions and desktops | Ubuntu 26.04, Fedora 44, GNOME/KDE Wayland and X11, polkit graphical approval, Orca, scaling, mixed monitors, and actual hardware sensor/GPU comparisons need the Linux PC or suitable native environments. |
| Longevity and performance | One-hour monitoring/recording/profiling run and frame-time/resource-growth targets are unverified. |
| Release | Signing, redistribution review, CI execution, and public-release approval remain open. |

This build does not meet the full Linux completion gates. The DEB is suitable for the user's follow-up native evaluation; the RPM needs Fedora installation evidence before that claim can be made.
