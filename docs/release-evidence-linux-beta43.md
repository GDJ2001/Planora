# Planora 0.3.0-beta.43 Linux GPU activity preview

Beta.43 is an unsigned internal Linux package built from clean commit `1fda8c3` with Rust 1.99.1 and helper protocol v6. It adds an optional per-adapter AMD `gpu_busy_percent` reading. The UI labels this **Aggregate device busy**, separate from **Busiest engine**; no physical-engine percentage is inferred from the aggregate. GPU selection, history, Overview, Performance, and the adapter table use the same semantic key. Missing, invalid, or unreadable counters retain typed unavailable reasons. Old frames without the optional field still deserialize.

## Package and installed identity

| Artifact | SHA-256 |
| --- | --- |
| `dist/planora_0.3.0~beta.43-1_amd64.deb` | `1cb4456027d5e577ddd0f44603608b6ac4c0016d49f2ad63872de56603b8ae37` |
| `dist/planora-0.3.0-0.beta.43.x86_64.rpm` | `657bd501a910d24a986d7820cedddfced9fc1bdffbd3e6ca4c7cd2e4461ae41b` |
| Installed UI | `75cce2732980d4bb3199f78b93404c55d36129dbc12e6f515e41bc9ea9363340` |
| Installed helper | `2d585d70d3dc9061946efce2a4d380e5e2d6c805dbbc21b09dacd892e08b4423` |

`dist/provenance.json` reports clean source commit `1fda8c3f7cc0844a91f140aa5be4bab97e02303e`; `dist/SHA256SUMS.txt` lists the dependency inventory and both packages. The installed hashes match the release binaries. Both binaries report beta.43, commit `1fda8c3`, target `x86_64-unknown-linux-gnu`, and protocol v6.

## Ubuntu 24.04 WSL observations

- The DEB upgraded beta.42 to beta.43. Monitoring, PMU, and block-profiler services remained active. The authorization record for UID 1001 remained present. The user settings hash stayed `873974781691603d6997ac2c0f6c8319316c68760282b89c62350f26976c66fd` across the upgrade.
- An authenticated helper handshake returned protocol v6 and authorization. At the 100 ms request interval, 30 snapshot requests produced 29 distinct sequence numbers and 30 available CPU values. The WSL DRM adapter has no AMD counter, so the helper correctly reports AMD aggregate busy as Unsupported.
- A normal installed UI launch under WSLg X11 exited successfully after its 3.5-second auto-close and left no UI process. WSLg printed Mesa EGL fallback warnings; it did not prevent the launch.
- Two AMD DRM fixture adapters returned separate 12% and 83% aggregate readings and never claimed a physical-engine measurement. Invalid values were unavailable. Legacy adapter JSON without the new field deserialized. Full Rust workspace tests, formatting, strict all-target Clippy, and Windows workspace `cargo check --locked` passed.

## Remaining gates

The WSL package and fixture evidence does not establish live AMD hardware accuracy, Intel/NVIDIA Linux GPU telemetry, Fedora installation, physical Ubuntu hardware, GNOME/KDE or Wayland behavior, fan visibility, graphical polkit approval, suspend/hotplug, accessibility, one-hour performance, or signing. The RPM was built and inspected in Ubuntu WSL but not installed on Fedora. Beta.43 remains an internal preview.
