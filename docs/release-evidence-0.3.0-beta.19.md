# Planora 0.3.0-beta.19 resource selector follow-up

Beta.19 addresses the fresh-launch issue found during beta.18 installed acceptance. A persisted list offset could open Performance among sensor rows while CPU remained selected in the detail pane. The live machine has 46 available sensors, so one card per sensor also made the selector unnecessarily long.

The selected card is now revealed once when the wide selector opens. Individual sensors are collapsed under a count-labeled section by default; searching still reveals matching sensor rows. A restored sensor selection expands its section, and a selection made in compact mode is revealed when the wide selector returns. User scrolling after that first reveal is respected.

This is a UI-only follow-up. Helper protocol v5, recording format v4, settings schema v6, sensor providers, and installation behavior are unchanged. Application version is `0.3.0-beta.19`; MSI version is `0.2.1019`. The installed beta.18 package is retained for rollback.

## Source and live development verification

`cargo fmt --all --check`, strict workspace Clippy, and all workspace tests passed (129 active tests, two existing ignored tests). A normal, non-fixture launch of the beta.19 development build connected to the installed helper and rendered the selected CPU card at the top of the resource selector. The 46 sensor readings appeared as one collapsed Sensors section, while the detail pane remained on CPU. The development window closed through its normal shutdown flow. The resource filter implementation reveals matching sensors even when the section is collapsed; an interactive search check remains in installed acceptance.

## Package build

Built from clean committed revision `edbd756`. The packaging script verified the Windows GUI subsystem, matching release UI/helper protocol-v5 identities, the pinned signed PawnIO dependency, MSI version `0.2.1019`, and WiX MSI validation. Independent SHA-256 checks matched the generated manifest:

| Artifact | SHA-256 |
|---|---|
| Setup EXE | `64EE2FB01A8078F000E39E686F56EF0C89A589A28CC7E8A4A334A0482818B951` |
| MSI | `DF6384FFA7CD6A636FD1720EBA5197D6FDBD534F7D3FD4119C0E0C36FB842F61` |
| `planora.exe` | `E45DF8EA9D515366FF0CD6E3D8063D1832AA8EE7E5190B65542504709313C745` |
| `planora-helper.exe` | `EC762C4A5D322A63AE6A5C5C814B438F4931162D18233DBA1EC850C208FCF351` |
| `planora-sensor-broker.exe` | `D30F8FC4EAEBE65BA1C1BABB047BDE11F20D62D0A90F902FC358BE38968EE669` |

Before upgrade, beta.18's three installed binary hashes matched its manifest, PlanoraHelper was running, authorization was present, and the settings SHA-256 remained `DAC6D1CCE00B596273145C55E59A5D40E17DD89F948F916E3706D361D412E6A1`. The authorization value was not read or recorded.

## Installed verification — 2026-10-25

The beta.19 Setup displayed “Planora is installed.” Its Burn log ended with `Apply complete, result: 0x0, restart: None`; the MSI log recorded installation success and ProductVersion `0.2.1019`. Installed UI, helper, and broker SHA-256 values match the manifest. All three report beta.19 revision `edbd756`; UI and helper report protocol v5. PlanoraHelper is running, authorization remains present, and the user settings hash matches the pre-upgrade value above.

The installed helper returned 20 distinct fresh protocol-v5 snapshots, with a maximum CPU reading age of 674 ms. The final snapshot included one measured Intel Iris Xe adapter, 46 available advanced-sensor records, CPU package power, and 11 CPU-temperature records. These confirm availability, not independent sensor accuracy.

A normal installed launch opened the full-size Performance workspace without a terminal. CPU was selected and visible in the resource selector; 46 sensors were collapsed under one section. Expanding the section exposed their cards, and searching for `package` filtered it to the three matching CPU package readings. After clearing search, scrolling deep into the sensor cards, closing normally, and relaunching, the selected CPU card returned to view. The UI worker and owned sensor-worker process both exited on close. The unsigned beta.19 package remains an internal build; broader visual sizes, accessibility, performance, and Windows 10/11 acceptance remain open.
