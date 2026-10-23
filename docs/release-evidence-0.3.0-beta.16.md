# Planora 0.3.0-beta.16 internal package evidence

Beta.16 delivers end-to-end telemetry graph smoothing, representation bounds enhancement, and artifact elimination across Performance, History, and Process views. It completely resolves the visual anomalies shown in user feedback screenshots:
1. **Vertical Needle Strokes**: Eliminated by replacing naive polyline points with Fritsch–Carlson monotone cubic Hermite splines (`monotone_path`) sampled at sample midpoints with strictly increasing horizontal coordinates.
2. **Faint Vertical Seam Lines**: Eliminated by replacing sequential per-quad fill segments with a continuous single triangle-strip gradient mesh (`area_mesh`), removing double-drawn anti-aliased seam boundaries.
3. **Honest Extreme Spikes without Distortion**: Added a translucent min..max envelope band (`band_mesh`) behind the mean curve, accurately surfacing telemetry bursts and sub-sample extremes without needle stroke artifacts.
4. **Unit-Aware Round Binary Axis Scaling**: Implemented binary rate quantization (1, 2, 5, 10, 20, 50, 100, 200, 500 binary prefixes in KiB/s, MiB/s, GiB/s) matching `format_rate()`, alongside adaptive percentage ceilings (10%, 25%, 50%, 100% in Fit mode; 0–100% in Full mode).
5. **No Readout Overlap**: Moved dynamic metric readouts and units to a dedicated header row above the plot canvas; cursor vertical line and series marker dots snap cleanly to the nearest `ChartPoint`.
6. **Independent History Lanes**: Upgraded History state to maintain separate `PlotState` instances per lane (`[PlotState; 4]`), eliminating cache thrashing, cursor bleed, and axis hysteresis cross-talk.
7. **Stable Rate Headlines**: Integrated `latest_stable_value()` 1-second average into disk and network selector chips and headline readouts to eliminate burst-induced visual flickering.

This is an unsigned internal build from committed revision `881887c`, using helper protocol v5 and MSI ProductVersion `0.2.1016`.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.16-x64-Setup.exe` | `281EB3A26E54A997E0A39682A83426D2E8C17C55A2AFE03AA17CD6070DFA97C9` |
| `Planora-0.3.0-beta.16-x64.msi` | `AE7CE5D8219E3115048C3534682DB6F6FB0FF896A668ACA2241F84BD1FF871C7` |
| `planora.exe` | `067AAEB56ACEED7EBF09637374DBAD21F171806D68D586D833B70F50C2C84C4B` |
| `planora-helper.exe` | `13A3D715AEDDCAA81695F721475CA3BA3DBF596532E534DA8DB49A5A40AED2C8` |
| `planora-sensor-broker.exe` | `D5E5B04BB30A585086DD2925FA2360A8D49DCF77D78E7010296679C1429BED98` |
| `PawnIO_setup.exe` | `1F519A22E47187F70A1379A48CA604981C4FCF694F4E65B734AAA74A9FBA3032` |

The Setup and MSI hashes were independently verified with `dist/Planora-0.3.0-beta.16-x64.manifest.json`. The packaging script validated WiX output, the GUI subsystem (`Windows GUI`), and the pinned PawnIO signature/hash.

Code formatting (`cargo fmt --all --check`), strict workspace Clippy (`cargo clippy --workspace --all-targets --locked -- -D warnings`), and the complete unit test suite (63 tests in `planora-ui` covering monotonic curves, mesh construction, binary axis division, midpoint aggregation, and independent lane states) passed cleanly.

Visual fixtures were generated natively without helper dependency:
- [Dark 1180×790 CPU Performance Workspace](screenshots/native/performance-workspace-dark-1180x790-fixture.png)
- [Light 1024×640 CPU Performance Workspace](screenshots/native/performance-workspace-light-1024x640-fixture.png)
- [Dark 1180×790 Network Interface Performance Workspace](screenshots/native/performance-network-dark-1180x790-fixture.png)
- [Dark 1180×790 Multi-Lane History View](screenshots/native/history-lanes-dark-1180x790-fixture.png)

Beta.15 remains preserved in `dist/` as the verified rollback baseline.
