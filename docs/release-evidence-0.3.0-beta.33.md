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

## Package and native acceptance

Pending build and installation verification. The source changes above do not establish Windows 10/11 coverage, Narrator acceptance, long-run performance, or fan RPM support. Unsigned packages remain internal.
