# Planora 0.3.0-beta.12 internal package evidence

Beta.12 replaces the CPU Performance sawtooth trace with one-second activity columns, short-peak markers, a compact utilization ring, and concise chart readouts. Empty time remains blank. The beta.11 installed capture exposed a cursor and tooltip over unobserved warm-up space; beta.12 suppresses them. Other resource plots and the Tables only mode remain available. This is an unsigned internal build from clean committed revision `85ba99e`, with MSI ProductVersion `0.2.1012` and helper protocol v5.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.12-x64-Setup.exe` | `9B10541706B0AAD3B1B31C567B5D4275FFF9BA2F22BD5C4A4131AE3619D696A8` |
| `Planora-0.3.0-beta.12-x64.msi` | `6AF2BF1C3C6AEC2D18F374FA5FB7E5C2E383F19136C9F259A120049536395D5C` |
| `planora.exe` | `77668FA9579DE52B51314C53C54005B214A2985BB4B16ECB716C3035BE30FDBF` |
| `planora-helper.exe` | `D5F895186D74A2BB6A3CB411C9FAEA6DE26AFB6C9AF98FD316EEF3AADF941982` |
| `planora-sensor-broker.exe` | `5B229E3737C62EE18FBD53AA493B5E8949CC28B5CE0790D74BDDB536B3056B99` |

The Setup and MSI were independently rehashed against the generated manifest. The build script verified WiX MSI validity, pinned PawnIO signature/hash, and Windows GUI subsystem. Formatting, strict workspace Clippy, and 52 focused UI binary tests passed; two unrelated slow UI tests were excluded from that run. Workspace library tests passed on the beta.11 source before the beta.12 cursor-only change. The broker was published as a self-contained Windows x64 executable during package creation.

On this Windows 11 x64 PC, the user completed the beta.11 and then beta.12 Setup upgrades. The beta.12 Burn log ended with `0x0` and no restart. Installed UI `--build-info --output` reports beta.12, revision `85ba99e`, release profile, and protocol v5. Installed UI, helper, and broker hashes all match the beta.12 manifest. The helper remains running, administrator-owned authorization remains present, and the current-user settings hash stayed `2BE3164F4AD86EC2A9B684B6F6CDD0479CBEA7D2415CF5F916854B6F2CC6A8DC` across both upgrades.

The installed helper returned 20 distinct fresh protocol-v5 snapshots, with maximum observed CPU age 418 ms. The advanced-sensor broker returned 46 available readings, including one CPU package-power reading and 11 CPU temperatures. These observations verify the local installation and data flow; they do not establish general sensor accuracy or coverage on other hardware.

The [installed beta.12 CPU capture](screenshots/native/beta12_cpu_activity_installed_light_1180x790.png) is a live egui framebuffer at 1475×988 pixels from a requested 1180×790 logical window (125% effective scale). It shows the clean warm-up state and no cursor or tooltip in unobserved space. The [dark populated fixture](screenshots/native/beta11_cpu_activity_dark_1024x640_fixture.png) is 1280×800 pixels from 1024×640 logical; it is clearly marked simulated data and shows the column shape, missing interval, and spike marker. Visual review found no clipping in these captures. The installed capture is during warm-up, so a fully populated installed chart, keyboard/Narrator, other sizes/scales, reduced motion, high contrast, and p95 frame-time review remain open.

Windows 10 native acceptance, installer repair/uninstall and UAC-denial testing, one-hour profiling, CI execution, signing, and redistribution approval remain release gates. Beta.12 must remain internal.
