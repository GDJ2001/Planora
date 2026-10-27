# Planora 0.3.0-beta.33: charts and categorized sensors

Beta.33 keeps helper protocol v5 and recording format v4. It is an unsigned internal Windows build. MSI ProductVersion is `0.2.1033`. Beta.32 remains the rollback artifact.

## Changes

- Shared time-series presentation uses straight interval-average segments. A single series has a restrained fill; paired and multi-series charts have separate lines without overlapping fills or a minimum/maximum background band. The observation and gap policy remains separate from drawing. The shared inspection reports the interval average, minimum, maximum, source, timestamp, and availability.
- Performance retains prominent CPU, memory, GPU, storage, and connected physical network cards. Detailed readings are grouped under CPU sensors, GPU sensors, memory sensors, storage details, network details, motherboard and cooling, and other hardware. Device and measurement-family rows remain compact. Network prominence uses Windows hardware-interface and link-state metadata, not traffic or interface names. Any interface can be pinned.
- Settings schema 7 preserves existing preferences and adds category expansion and pinned interface IDs. Optional sensor hardware-category and network-interface metadata default safely for older protocol-v5 records. Stable sensor/device/history identities and recording format v4 are unchanged.

## Verification

- `cargo fmt --all -- --check`, `cargo test --workspace --locked -q`, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. The native fixture captures in [beta33](screenshots/native/beta33/) show CPU, GPU, disk, network, and sensor views, including a light compact layout. These captures use simulated data and do not establish installed live behavior.
- Chart regressions cover the absence of the range envelope, single versus paired fills, and observed gaps. Navigation and migration tests cover conservative sensor grouping, physical-interface classification, pinning, and preserved settings.

## CPU fan on the reference laptop

The physical cooling fan is not the same as an exposed tachometer. On the ASUS ExpertBook B1500CEAEY with an i7-1165G7, the installed beta.32 LibreHardwareMonitor broker returned 43 CPU/Intel-GPU readings and **zero Fan readings** from its enabled read-only providers. Windows `Win32_Fan` and `CIM_Tachometer` returned no instances. The broker deliberately leaves broad motherboard/controller/embedded-controller probing disabled. Beta.33 shows “CPU fan RPM unavailable” in the CPU sensor section when there is no actual tachometer reading; it never displays an invented zero or labels the fan absent. Obtaining actual RPM on this model remains open until a documented, read-only OEM or OS source can be validated.

## Package and installed acceptance

The clean committed source was `decc3db`. The Windows build script validated the installer theme, Windows GUI subsystem, broker build identity, signed pinned PawnIO dependency, and MSI. The Setup and MSI independently match the [package manifest](screenshots/native/beta33/package-manifest.json): Setup SHA-256 `194BF9CA5D2F07139872E4F0E9032B167B11769B9F4A0FF6453193920735E5B5`; MSI SHA-256 `CF250B13DE0EBF4A08D665F47A2399D1C43F55EB82AECAB0FAC6F462A9C4045E`.

The user completed Setup with Advanced sensors selected. Burn reported `Apply complete, result: 0x0` and `Exit code: 0x0`, with no restart; MSI reported installation success. A check during the upgrade briefly found no installed files while the old package was being removed. The completed-state check found all three binaries, and each installed SHA-256 matched the manifest. Startup diagnostics recorded beta.33/`decc3db`, a full maximized first frame, and a connected helper. The `PlanoraHelper` service was running. An installed-service protocol-v5 test completed an authorized handshake and 25 consecutive fresh snapshots. The public desktop shortcut remained present. [Installed verification](screenshots/native/beta33/installed-verification.json).

The settings file survived and migrated to schema 7. Its hash changed from `3D7B65362B567C721B4076E46B115012B0917606C0816C2C2F0D367E5BBBC2CD` to `F80A7D793D1733C03AB3D4851EB575C64964DCDFF6AAAA92FECC2A0E2D837C72`; existing sampling, graph, range, resource, and process-table fields were present after migration. The installed beta.33 broker again returned 43 CPU/Intel-GPU readings and zero fan RPM readings.

Native installed-window capture could not be completed because the computer-use runtime failed to initialize (`failed to write kernel assets: The system cannot find the path specified`). The fixture captures therefore remain the visual evidence and are explicitly marked simulated. Windows 10/11 coverage, Narrator acceptance, long-run performance, and fan RPM support remain open. Unsigned packages remain internal.
