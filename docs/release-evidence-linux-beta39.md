# Planora 0.3.0-beta.39 Linux block-profiling preview

Beta.39 is an unsigned internal Linux package built from clean commit `171d732c0251df9119e8505b822671877f41f344` (Rust 1.99.1, protocol v6). It adds a fixed libbpf CO-RE block collector in a separate systemd service. The profiler has `CAP_BPF` and `CAP_PERFMON`; the ordinary monitoring helper has `CAP_PERFMON` for the existing PMU path but no `CAP_BPF`. The profiler uses its own mode-700 `/run/planora-profile` directory and a mode-600 socket; stopping it leaves the monitoring socket in `/run/planora` available.

## Reproducible packages and installed identity

| Artifact | SHA-256 |
| --- | --- |
| `dist/planora_0.3.0~beta.39-1_amd64.deb` | `b076d0213b8696453fbb5af69a7f15970a6fdb01dc61a6d1aff33ace295d3ba6` |
| `dist/planora-0.3.0-0.beta.39.x86_64.rpm` | `e1b41ca69ed8615d737152c17ce96f5f999abe1fdc0643bd91138fd8456c5cfe` |
| Installed UI | `a87ff0352224635f2a5f6f1609eeb52ba6b645d8dc05ec866cc07c1e0ef0e20d` |
| Installed helper | `386bf2862b8089e87c1552dddf0818b056d311b195fe168f738430201e764b43` |
| Installed collector | `202caf3e103dfa22290144b6bf0e97255933656950fd31f95be5f148e3c2bf1e` |
| Installed BPF object | `17acbd18fb141a9399ddac2038e6ca3b8228a268a03197c571a99546156f7435` |

`dist/provenance.json` records the clean revision and package hashes; the installed binary hashes match the release build. The UI and helper report beta.39, commit `171d732`, target `x86_64-unknown-linux-gnu`, and protocol v6. Both DEB and RPM contain the collector, service, source, and license notices. The RPM was built in Ubuntu WSL but has **not** been installed on Fedora.

## Ubuntu 24.04 WSL observations

- Workspace tests, formatting, strict all-target Clippy, Windows workspace `cargo check`, shell syntax, DEB build, RPM build, and installed service verification passed. The UI suite had 92 passing tests; the existing ignored sustained-service test remains ignored.
- The beta.36-to-beta.38 and beta.38-to-beta.39 upgrades preserved the per-user authorization record and personal files. Both services were active after beta.39 installation. An authenticated helper request reported Disk I/O profiling **Available**.
- A five-second system-wide profile returned 191 completed block events, zero lost events, one `dd` record with 198,180,864 attributed write bytes, and 8,192 unattributed read bytes. A selected-process profile retained PID plus Linux start ticks, returned the selected write data, and reported four lost events as incomplete. Cancellation moved through **Cancelling** to **Cancelled**.
- A beta.39 system-wide profile attributed a 256 MiB direct-write workload to `dd` and reported one lost event. A stopped profiler removed only its own socket; the helper socket remained. The collector returned namespace-local PIDs and process start ticks that exactly matched `/proc` in a separate live fixture.
- The package uninstall stopped both services and removed binaries and system authorization. Configuration and recording markers plus `app.ron` retained their previous SHA-256 hashes. Reinstall started both services, required a new authorization, and accepted it without sign-out. This test invoked the root authorization script with a simulated `PKEXEC_UID`; a graphical polkit approval on native Linux remains unverified.
- With an authenticated 100 ms sampling request, 30 snapshots contained 29 distinct sample timestamps over 4.2 seconds. Fewer fresh timestamps than requests are expected because requesting a cached frame does not create a new observation.

## Limits and remaining gates

The collector counts completed read/write block requests, retains kernel writeback separately, records event loss, and validates process creation identity. A profile may contain partial records when the kernel reports loss; the UI must not treat it as complete. Its 1.4 million event limit, 60-second duration cap, 8 MiB ring, 65,536 pending-request map, 256 MiB worker memory ceiling, and bounded Rust result handoff prevent an unbounded session. Targeted process results exclude activity positively attributed to other processes.

This is **not** full Linux acceptance. Native Ubuntu 26.04 and Fedora 44 installation, GNOME/KDE and Wayland/X11 behavior, physical GPU/fan hardware, kernel and security-policy differences, one-hour performance, real graphical polkit approval, accessibility, signed packages, and redistribution review remain open. Intel/NVIDIA Linux GPU metrics and isolation of the existing PMU sampler into the profiling worker also remain implementation work. Historical Windows release claims are not Linux acceptance evidence.
