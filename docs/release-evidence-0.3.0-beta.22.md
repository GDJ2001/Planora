# Planora 0.3.0-beta.22 release evidence

Beta.22 delivers full-card interactive resource selection and a conventional, legible CPU utilization time-series graph:
1. **Full-Card Resource Selection (Performance Workspace)**: Each resource card in the Performance selector pane is now a unified single interactive button (`Sense::click()`). Clicking anywhere inside the card—including title, value, status, sparkline, or padding—activates and selects that resource. Includes hover tinting, active press feedback, pointer cursor (`CursorIcon::PointingHand`), full device name hover tooltip, accessible `Role::Button` selection semantics, and keyboard focus/activation (Space / Enter).
2. **Interactive Overview Resource Cards**: All resource cards on the System Overview page (CPU, Memory, GPU, Storage, Network) are now interactive click targets. Clicking any card transitions directly to the Performance workspace with the matching resource selected and revealed.
3. **Conventional CPU Utilization Time Series**: Replaced the former vertical activity bars and audio-equalizer peak ticks with a conventional 1-second average time-series line, rich area gradient, and subtle 10% Task Manager-style gridlines. Preserves exact observed values, within-interval min/max in hover tooltips and readouts, honest missing-data gaps, and zero artificial curve overshoots.

This is an unsigned internal build from committed revision `6e1031d`, using helper protocol v5 and MSI ProductVersion `0.2.1022`.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.22-x64-Setup.exe` | `0D12B99472C865BB84E879B7BBF963BCE298D5AD63AF5FDC901C0BFF980AE472` |
| `Planora-0.3.0-beta.22-x64.msi` | `9744D190291DA176EE51C135D50F12094FA6B22569EAE1B2EEB59D47E9981CD7` |
| `planora.exe` | `D8B2980AC7AD8A7B95423F63AF8940713B9E282D10930423927AC4F8CFE69837` |
| `planora-helper.exe` | `766341E062296F6830395A4754DF4AEDC5CA7D6F5ACD94EFCB1EE234933577CA` |
| `planora-sensor-broker.exe` | `0AA74E0D1F7DB85282401FC5426D7ED32B5DCC2B78C6D9E64924227FFE7C7305` |
| `PawnIO_setup.exe` | `1F519A22E47187F70A1379A48CA604981C4FCF694F4E65B734AAA74A9FBA3032` |

The Setup and MSI hashes were independently verified with `dist/Planora-0.3.0-beta.22-x64.manifest.json`. The packaging script validated WiX output, the GUI subsystem (`Windows GUI`), and the pinned PawnIO signature/hash.

Code formatting (`cargo fmt --all --check`), strict workspace Clippy (`cargo clippy --workspace --all-targets --locked -- -D warnings`), and the complete unit test suite (70 tests in `planora-ui`, 131 tests across workspace) passed cleanly.

Visual fixtures were generated natively without helper dependency:
- [Dark 1536×864 Performance Workspace](screenshots/native/performance-cards-beta22-dark-1536-fixture.png)
- [Light 1180×790 Performance Workspace](screenshots/native/performance-cards-beta22-light-1180-fixture.png)
- [Dark 1536×864 Overview Workspace](screenshots/native/overview-clickable-cards-beta22-dark-1536-fixture.png)

Beta.20 remains preserved as the rollback baseline.
