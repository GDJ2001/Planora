# Planora 0.3.0-beta.15 internal package evidence

Beta.15 delivers automatic Windows system appearance synchronization and a responsive, explicitly bounded Performance workspace. The manual Appearance & Motion card is removed in favor of Windows-native dark/light mode, high-contrast support, accessibility text size scaling, and system animation preferences. Settings schema migrates from version 5 to 6, discarding legacy appearance overrides while strictly preserving sampling rates, table layouts, resource choices, history preferences, and personal data. The Performance workspace features bounded selector width (240px scalable to 320px with text scale), a 16px gap, independent vertical scrolling panes, 40% height-bounded charts (240–420px), two-column memory and GPU summary cards (stacked when < 656px), and a searchable GPU engine table with human-readable identities.

This is an unsigned internal build from committed revision `44ef0e3`, using helper protocol v5 and MSI ProductVersion `0.2.1015`.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.15-x64-Setup.exe` | `25577403E21BA71B2847359193AF694CA8DDA996FBA41362F4E179259C70C200` |
| `Planora-0.3.0-beta.15-x64.msi` | `F806139CAB6D2F9690627B37F7D1DBD028CD4C82657D75382E903782526E82DF` |
| `planora.exe` | `D345CAB34692B33B185A96E9623E894FFE5C80668577397BC301F9620D8DB030` |
| `planora-helper.exe` | `242CA0DCEBE0CD875E3FFB8B93E30E6944BB773C4BEC986B8DD85ADCF1E1990D` |
| `planora-sensor-broker.exe` | `62C4E2309CA624385731EE4B203B0A9A0481D243627EDF20094CCB3B138A1CF5` |

The Setup and MSI hashes were independently verified with `dist/Planora-0.3.0-beta.15-x64.manifest.json`. The packaging script validated WiX output, the GUI subsystem (`Windows GUI`), and the pinned PawnIO signature/hash.

Code formatting (`cargo fmt --all --check`), strict workspace Clippy (`cargo clippy --workspace --all-targets --locked -- -D warnings`), and the complete unit, integration, and UI test suite (57 tests in `planora-ui` including responsive width verification across 800px, 940px, 1120px, 1328px, 1712px and 1.0x, 1.5x, 2.0x text scaling) passed cleanly.

Visual fixtures were generated natively without helper dependency:
- [Dark 1180×790 Performance Workspace](screenshots/native/performance-workspace-dark-1180x790-fixture.png)
- [Light 1024×640 Performance Workspace](screenshots/native/performance-workspace-light-1024x640-fixture.png)
- [Dark 1180×790 Settings Troubleshooting View](screenshots/native/settings-troubleshooting-dark-fixture.png)

Beta.14 remains preserved in `dist/` as the verified rollback baseline.
