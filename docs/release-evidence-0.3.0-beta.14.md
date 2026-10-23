# Planora 0.3.0-beta.14 internal package evidence

Beta.14 polishes the Overview resource summary. It gives the table a stable scroll ID, fills the available panel width, aligns values by column, formats byte rates into readable units, adds the detected GPU adapter to the summary, and shows history coverage as a labeled bar. The Overview card grid uses no more than four columns so a fifth resource cannot overflow a wide window. This is an unsigned internal build from committed revision `cc3632d`, with helper protocol v5 and MSI ProductVersion `0.2.1014`.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.14-x64-Setup.exe` | `5D9BCB10C09D084289016F5A597EF0E42F18C1D2A6FBA65996993B8573CA0961` |
| `Planora-0.3.0-beta.14-x64.msi` | `F2EC579DA651181FADB2EFD28BEBBB6F8E11872FA6DB161A3EB85CFE044D9D8A` |
| `planora.exe` | `17F964DACD93FC3F570AC0D16FC5A76CEA72520F497A3A804545D69A073DFF8D` |
| `planora-helper.exe` | `A02B858E119BC2DBD3D801DC85FF1E8EFCA7A59481C42E784848A1BDD521359A` |
| `planora-sensor-broker.exe` | `BC304097B6206B92B05D2DA2929424F79EC8A589F53D9A6478B0B34455504B43` |

The Setup and MSI hashes were independently compared with the generated manifest. The package script validated WiX output, the GUI subsystem, and the pinned PawnIO signature/hash. Formatting, strict workspace Clippy, and 52 focused UI binary tests passed; two unrelated slow UI tests were excluded. The [dark 1536×864 fixture](screenshots/native/overview-summary-dark-1536x864-fixture.png) and [light 1024×640 fixture](screenshots/native/overview-summary-light-1024x640-fixture.png) are native egui captures with **simulated sample data**, not live telemetry. Visual review found no red widget-ID diagnostic in the new summary layout. The compact capture shows the upper part of the summary because the page is vertically scrollable.

Installed upgrade verification is pending. Beta.13 remains the installed rollback package until beta.14 Setup completes and the installed UI/helper identities, settings, authorization, live snapshots, and Overview capture are checked. The wider Windows acceptance, accessibility, installer maintenance, signing, and CI execution gates remain open.
