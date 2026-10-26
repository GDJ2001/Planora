# Planora 0.3.0-beta.9 internal package evidence

Beta.9 corrects the beta.8 sensor/profiling package and was built from clean committed revision `f72c418`. Rust 1.95 workspace tests, strict Clippy, `dotnet build` for the broker, and standalone telemetry-crate tests passed on the current Windows x64 host. WiX built and validated the MSI. The Setup and MSI remain unsigned internal artifacts.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.9-x64-Setup.exe` | `61C090E34EBF82BC9FD86CBA2B2E713F302DB105F9E289F570DF690189409421` |
| `Planora-0.3.0-beta.9-x64.msi` | `D7AADF73BF533F474213B8F80DBC991FC0E566B776C784B0E80A672978E17381` |
| `planora.exe` | `2B935955C8240C381B9715F1FD6534419DED1C12C952D6811F7F6A5D154E50D8` |
| `planora-helper.exe` | `3476027D22E370518CC21798FBD3CFFEB756BD2A051B518B7ACA9DC1AD11E7E9` |
| `planora-sensor-broker.exe` | `EC5DDC01D8608C67DE29660D570A37D78497E02FB70A88B9AEF46DC7499F5762` |

Independent SHA-256 recalculation matched every manifest entry. UI and helper build identities match beta.9/revision `f72c418`/protocol v5. The broker's `--build-info` reports beta.9, revision `f72c418`, and LibreHardwareMonitorLib 0.9.6; the helper checks both its version and revision before accepting readings. MSI ProductVersion is `0.2.1009`. The MSI contains an optional `AdvancedSensors` feature at level 2, enabled by `PLANORA_ADVANCED_SENSORS=1`. The bundled PawnIO 2.2.0 payload matches its pinned hash and its Authenticode signature validated as `namazso`. The bundle detects an existing PawnIO service, only chains installation after explicit selection, and does not remove a shared PawnIO installation during Planora uninstall.

On the current Intel Iris Xe PC **without PawnIO installed**, a direct beta.9 broker sample enumerated 43 sensor entries, including four CPU power sensors. It returned no supported CPU power or temperature values. Beta.9 presents those as unavailable rather than zero. This check establishes broker behavior without the driver; it does not establish CPU sensor coverage.

The disk ETW collector now uses a private system-logger session, checks the classic DiskIo provider GUID, parses only complete x64 payloads, and does not stop another application's trace on a name collision. A parser regression passed. **Installed-native ETW event/attribution, cancellation, and loss tests are still required.** PMU sampled profiling is explicitly `Not implemented`; source discovery cannot count as sampled data.

The beta.9 Setup upgraded beta.7 on this Windows PC with Burn apply result `0x0` and no restart. Installed UI, helper, and broker hashes match the manifest; the UI reports revision `f72c418`/protocol v5. The PlanoraHelper and signed PawnIO services are running. Administrator-owned authorization remained present and the personal settings SHA-256 remained `BE9142FB24191383772F215D895A7426061B23DBC00747AC597933857D943AC8`. The installed helper returned 20 distinct fresh protocol-v5 snapshots (maximum observed CPU reading age 690 ms). Its helper-owned broker returned 46 available sensor readings, including one CPU package-power and 11 CPU temperature readings. A direct **unelevated** broker launch on the same machine still returned no CPU power or temperature readings, supporting the required elevated-broker boundary.

An explicit three-second installed ETW disk profile completed with **67 events, zero reported losses, four process records, 10 unattributed operations, and 83.27% attributed bytes**. These are one run's results, not a general accuracy claim. A separate installed 15-second profile acknowledged cancellation but then reported `Completed` with a partial result. Beta.10 corrects that lifecycle defect. The installed Advanced Profiling UI remained hard-coded to say all profiling was unavailable; beta.10 corrects that presentation and adds status polling. Repair/uninstall, independent sensor comparison, Windows 10, other vendor hardware, accessibility, sustained profiling, and redistribution review remain open.
