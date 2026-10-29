# Planora 0.3.0-beta.36 Linux WSL session-lock correction

Beta.36 is an unsigned internal Linux preview built from clean commit `ed5f498c671a6573ca33ca48a066217ab6dd2967`. It keeps protocol v6 and the beta.35 Linux telemetry/profiling behavior. The prior [beta.35 record](release-evidence-linux-beta35.md) documents those features and the remaining native gates.

## Correction

Beta.35 put the Linux instance lock in `XDG_RUNTIME_DIR` normally but moved it to a private temporary directory when WSLg supplied an insecure runtime directory. If that directory's mode changed between launches, the two processes used different locks and opened separate windows. Beta.36 keeps the WSL session lock and activation socket in one verified mode-700, user-owned directory across both states. It redirects `XDG_RUNTIME_DIR` for winit only when the supplied directory is invalid. A new private directory is created with mode 700 atomically; a pre-existing symlink, wrong owner, or permissive directory is rejected.

## Build and package evidence

| Artifact | SHA-256 |
| --- | --- |
| `dist/planora_0.3.0~beta.36-1_amd64.deb` | `4f1e2b688cc0fe1db28dd9b6568007da3391953e0f53c56f44d2ef0630ac7ac7` |
| `dist/planora-0.3.0-0.beta.36.x86_64.rpm` | `5c600fa0a52aeaca5de2a35ce14eb527571dce7d9e90f3433383e06a494646c6` |
| Installed `/usr/bin/planora` | `5e6dd933095e1f5b0f9f7ee8dee817ba89b8d5ba01bd34735a141d08189a2081` |
| Installed `/usr/libexec/planora/planora-helper` | `6c74b9e282cec4ba03fc7f5bbc59ed23f16bf9d06348f57c0474b79eaabf36f5` |

`dist/provenance.json` records the clean source revision and both package hashes. Both installed hashes matched the release binaries. Both installed binaries reported beta.36, commit `ed5f498`, protocol v6, and `x86_64-unknown-linux-gnu`.

Rust 1.99.1 workspace tests, formatting, and strict all-target Clippy passed on Ubuntu 24.04 WSL. The UI suite had 92 passing tests; the two existing ignored integration tests remain ignored. The beta.36 DEB upgraded the installed beta.35 package; the helper service was active, authorization and personal preference/recording markers survived, and AppStream offline validation plus `systemd-analyze verify` passed. Online AppStream validation emitted a homepage URL warning because the GitHub repository URL was not reachable from this environment; public metadata needs review before distribution. The RPM was built but has no Fedora installation evidence.

For the installed lifecycle check, the first normal UI opened while `/run/user/1001` was mode 755. After changing it to mode 700, the second normal launch exited and one 1180×790 Planora window remained. The private lock directory was mode 700. Sending `WM_DELETE_WINDOW` to the remaining window produced exit code 0 and removed its activation socket; only the separately installed helper remained. The runtime directory was restored to mode 700 afterward. [Installed live capture](screenshots/native/linux-beta36/performance-cpu-wsl-x11-live.png) shows CPU telemetry over WSLg X11; its short history reflects a fresh launch.

This correction does not close the [beta.35 remaining gates](release-evidence-linux-beta35.md#remaining-gates): attributed disk profiling, isolated profiling privileges, missing GPU paths, physical Ubuntu/Fedora hardware and desktop acceptance, long-run performance, signing, and public release review.
