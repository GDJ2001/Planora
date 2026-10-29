# Planora 0.3.0-beta.41 Linux profiling isolation preview

Beta.41 is an unsigned internal Linux package built from clean commit `afe2cd5626d87f5e63a1ab60a2d8b172022b9a1a` with Rust 1.99.1 and helper protocol v6. It moves Linux PMU sampling into a dedicated systemd worker with `CAP_PERFMON`. The ordinary monitoring helper now has an empty capability bounding set and zero effective capabilities; the existing block profiler retains `CAP_PERFMON` and `CAP_BPF`. The three services have independent runtime directories and private profiler sockets.

Beta.40 was built and installed as an internal test, but its ordinary helper failed its startup guard after removal of `CAP_PERFMON`. Beta.41 corrects the guard: only the dedicated `planora` service identity with zero effective capabilities is accepted. Beta.40 is a failed artifact and must not be distributed or reused as a rollback package.

## Package and installed identity

| Artifact | SHA-256 |
| --- | --- |
| `dist/planora_0.3.0~beta.41-1_amd64.deb` | `fe054a41bd24b04ccb66798720134d070572db526806046f57b9e857fef04015` |
| `dist/planora-0.3.0-0.beta.41.x86_64.rpm` | `673c84d00ee35240051004e4e53ee60e4b2c053f2e6172309cac551e64736456` |
| Installed UI | `df40c8dba912f068b4b98493e354d68e73183413e49a7acfb79c1ceff6a6aad3` |
| Installed helper and PMU worker binary | `eb748409ce2c634ff5c7ae27f921455fd2e0f55bb9491507ee6b0991671dfacd` |
| Installed block collector | `202caf3e103dfa22290144b6bf0e97255933656950fd31f95be5f148e3c2bf1e` |
| Installed BPF object | `17acbd18fb141a9399ddac2038e6ca3b8228a268a03197c571a99546156f7435` |

`dist/provenance.json` and `dist/SHA256SUMS.txt` record the clean revision and package hashes. Installed UI and helper hashes matched the release build. Both installed binaries report beta.41, commit `afe2cd5`, and protocol v6. The DEB was installed in Ubuntu 24.04 WSL; the RPM was built but not installed on Fedora.

## Ubuntu 24.04 WSL observations

- The DEB upgrade preserved the existing authorization record. The UI configuration and recording test markers retained their SHA-256 (`c1e4881bba7cd8206c795610cf5ecccdee287ffd1babb945b9f990d18e49c20e` for `app.ron`, `6ca7ea2feefc88ecb5ed6356ed963f47dc9137f82526fdd25d618ea626d0803f` for each marker). All three packaged services are active.
- The normal helper's effective capability mask was `0000000000000000`; systemd reports an empty capability bounding set. The PMU worker unit has only `cap_perfmon`; the block profiler unit has `cap_perfmon cap_bpf`.
- An authenticated protocol-v6 connection reported both PMU and disk profiling **Available**. A three-second selected-process `cpu-cycles` profile returned 276 samples, zero loss, 100% PMU scheduled time, and the matching PID plus creation identity. Cancelling a second PMU profile moved through **Cancelling** to **Cancelled** in approximately 101 ms.
- A four-second system-wide block profile observed a 64 MiB direct-write workload, 66 completed block events, and zero reported loss. It attributed the bytes to a Python process in this short-lived WSL fixture, so its process label is **not** treated as validated attribution.
- A seven-second selected-process direct-write fixture retained the `dd` PID and start ticks and attributed 166,551,552 bytes to that identity. It reported two lost events and 118,784 unattributed bytes, correctly marking the result incomplete. The fixture completed 256 MiB of direct writes; the selected profile covered only its seven-second window.
- Stopping the PMU worker left the helper's snapshot endpoint and disk capability available while PMU became unavailable. Stopping the block profiler left snapshots and PMU available while disk profiling became unavailable. Both workers restarted successfully.
- Removing the beta.41 DEB stopped all three services and removed its UI, helper, collector, sockets, and system authorization record. Personal settings and recording markers retained the hashes above. Reinstalling the same DEB started all three services but rejected the user connection until a new grant. A root invocation of the packaged authorization script with simulated `PKEXEC_UID=1001` restored access without sign-out; authenticated snapshots and both profiling capabilities then worked. This is a package lifecycle test, not a graphical polkit approval test.
- Formatting, full workspace tests, strict all-target Clippy, and the Windows workspace check passed. The existing five-minute live-service test remains ignored by default.

## Remaining gates

This is package and service evidence from WSL, not physical Linux acceptance. Native Ubuntu 24.04/26.04 and Fedora 44 package installation, graphical polkit, GNOME/KDE, Wayland/X11, physical GPU and fan providers, suspend/hotplug, accessibility, one-hour performance, signing, and redistribution review remain open. Intel/NVIDIA Linux GPU telemetry remains implementation work. A WSL short-lived disk fixture produced an unexpected process label; native attribution and asynchronous writeback behavior need further validation before claiming per-process disk accuracy on supported hardware.
