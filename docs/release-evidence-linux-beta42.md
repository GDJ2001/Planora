# Planora 0.3.0-beta.42 Linux profiling UI preview

Beta.42 is an unsigned internal Linux package built from clean commit `f4301c7` with Rust 1.99.1 and helper protocol v6. It connects the already-implemented Linux disk I/O profiler to the Advanced Profiling UI. The disk action is enabled during a live, idle session; Linux-specific copy replaces the previous claim that disk tracing was unavailable. Profile results now include an optional `ProfileKind`, so a PMU run with no samples remains a PMU result. The field is additive: older results without it still deserialize and existing recording format v4 is unchanged. The disk result table scrolls within its own horizontal region.

## Package and installed identity

| Artifact | SHA-256 |
| --- | --- |
| `dist/planora_0.3.0~beta.42-1_amd64.deb` | `6bcb84d9d46ee020896532e4ba2a543edebc36ef432973e81240ae39a9f73112` |
| `dist/planora-0.3.0-0.beta.42.x86_64.rpm` | `e03f45d1d01415ea9cac3ac05eeb5cf59fcf6768681ca9502f8434a91073b5ec` |
| Installed UI | `55f0dac7f0370b0d588dfc783892febf7da5d117af5226cd0487cc50f50eda1b` |
| Installed helper and PMU worker binary | `b7186238397c2d3c38673960daa8ae66b73b7a91aba456684caa5e0e4a4ebff0` |

`dist/provenance.json` and `dist/SHA256SUMS.txt` record the clean revision and artifact hashes. Installed UI and helper hashes matched the release build. Both binaries report beta.42, commit `f4301c7`, target `x86_64-unknown-linux-gnu`, and protocol v6. The RPM was built and inspected in Ubuntu WSL, but has not been installed on Fedora.

## Ubuntu 24.04 WSL observations

- The beta.41-to-beta.42 DEB upgrade left the monitoring helper, PMU worker, and block profiler active. The authorization record remained present. The user configuration and recording markers retained the beta.41 hashes recorded in [beta.41 evidence](release-evidence-linux-beta41.md).
- An authenticated installed helper returned protocol v6. A two-second selected-process PMU profile returned `kind: PmuSampling`, 161 `cpu-cycles` samples, zero loss, and matching PID plus creation identity. A three-second system-wide disk profile returned `kind: DiskIo`, 66 completed block events, zero reported loss, and `complete: true`.
- Rust formatting, full workspace tests, strict all-target Clippy, and Windows workspace `cargo check` passed. The core compatibility regression checks that legacy results without `kind` deserialize and a new empty PMU result keeps its type.
- The installed UI rendered WSLg X11 fixture captures of the Advanced Profiling page in dark 1180×790 and light 1024×640. Both show the enabled disk action and Linux-specific labels without clipping or overlap. `dist/linux-beta42-captures/manifest.json` records the installed UI hash, source commit, scale, capture hashes, and review. These are simulated fixture screens, not live profiling UI acceptance.

## Remaining gates

The WSL evidence confirms package execution and real kernel profiling on the WSL kernel. It does not establish physical Ubuntu/Fedora acceptance, GNOME/KDE or Wayland behavior, graphical polkit approval, fan/GPU hardware coverage, suspend/hotplug, accessibility, one-hour performance, or signed distribution. Intel and NVIDIA Linux GPU telemetry remain implementation work. Short-lived system-wide disk attribution still requires a physical-machine cross-check; a beta.41 WSL fixture had an unexpected process label while a longer selected-process fixture matched PID and start ticks.
