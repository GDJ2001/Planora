# Planora 0.3.0-beta.17 internal package evidence

Beta.17 improves the History page's Resource statistics and Interval detail panels. Both use the available width. Numeric values are right aligned with explicit units; statistics include low-to-peak range bars with average markers and labeled observation coverage. The interval table has a fixed header, virtualized scrolling, a missing-interval filter, and tab-separated copying. Missing values remain distinct from observed zero. Independent scroll IDs fix the duplicate-ID overlays reproduced from the user's screenshot.

This update retains system-managed appearance from beta.15 and graph rendering from beta.16. It does not change measurement providers, helper protocol v5, recording format v4, settings schema v6, or advanced-sensor installation. Application version is `0.3.0-beta.17`, MSI version `0.2.1017`.

## Baseline

On 2026-10-23, installed beta.16 reported revision `881887c`. Installed UI, helper, and sensor-broker SHA-256 values matched its manifest. PlanoraHelper was running and authorization was present (the authorization value was not recorded). The settings file's SHA-256 was `0641C33A0534CEA138C3EAE7C5D41C5AFC7167DAECA3CFEB877BB4C510FF513F`. Beta.16 and beta.14 packages are retained for rollback.

## Verification

The duplicate-scroll-ID regression was observed failing before the change and passing afterward. The History layout regression covers five logical window sizes, both themes, and text multipliers 1, 1.5, and 2, checking width bounds and widget-ID diagnostics. These synthetic checks do not substitute for physical monitor/DPI or Narrator tests. A separate regression preserves zero versus missing interval values and filter semantics.

`cargo fmt --all --check`, `cargo test --workspace --lib --bins --locked` (113 passed, one existing ignored test, including all 65 UI tests), and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed on the beta.17 source before packaging.

Native release previews from `d7fee05` were reviewed at [1536×864 dark](screenshots/native/history-tables-beta17-dark-1536x864-fixture.png) and [1024×640 light](screenshots/native/history-tables-beta17-light-1024x640-fixture.png), rendered at this PC's 125% scale (1920×1080 and 1280×800 physical pixels). They contain labeled simulated data. Both show the summary using the available width, readable units, and no red widget-ID diagnostics. The interval table continues below the viewport; horizontal scrolling retains its columns on compact windows. These captures do not establish keyboard, clipboard, or screen-reader acceptance.

## Package

Built from clean committed revision `d7fee05`. The packaging script verified binary identities, Windows GUI subsystem, WiX output, and the pinned PawnIO hash/signature. Setup and MSI hashes were independently compared to the generated manifest.

| Artifact | SHA-256 |
| --- | --- |
| Setup EXE | `820A8842731779FFABA86A5896CAD88EC91E6E1A1C358DA03C86B7D5650A02C1` |
| MSI | `E7656F5E34FC1623EC6B9E4255C5DF70B1B9596E0F6C59C6F18A63A82E49AAD2` |
| planora.exe | `4CF3B1D69AFA2BF5523B94EE70E82F7BE54F00D4AEB956D6F8E511153B99C327` |
| planora-helper.exe | `9D0FB2641DC4706FAE9538A85D98217F7567B82A5B630F3373FA14B1AA3EEF99` |
| planora-sensor-broker.exe | `8DF5F22B87753C5237C2EE5036E736D1574DB1ACBE29EC19E4F843A4FEEF74D4` |

The installed beta.16 rollback baseline also returned 20 distinct fresh protocol-v5 snapshots, including one Intel Iris Xe adapter with a measured utilization value. This is baseline evidence, not beta.17 installed acceptance.

## Installed verification — 2026-10-25

The beta.17 Burn upgrade completed with apply result `0x0`, exit `0x0`, and no restart, recorded in `Planora_20261023225801.log`. The first immediate identity check ran while MSI was still applying and saw beta.16; it was not treated as success. After completion, the installed UI reports beta.17, revision `d7fee05`, protocol v5. Installed UI, helper, and sensor-broker hashes all match the package manifest. PlanoraHelper is running, authorization remains present, and the settings SHA-256 is unchanged from the baseline above.

The installed helper returned 20 distinct fresh protocol-v5 snapshots (maximum CPU reading age 467 ms), one measured Intel Iris Xe adapter, and 46 available sensor records, including one CPU-package-power record and 11 CPU-temperature records. These are provider availability observations, not independent measurement-accuracy comparisons.

A normal installed launch opened the full window without a visible terminal. The live [History review capture](screenshots/native/history-tables-beta17-installed-live.jpg) shows both panels filling the available width, aligned readable values, range/coverage indicators, and no red duplicate-ID overlays. Scrolling the interval rows keeps their headers visible. The missing-interval checkbox was toggled successfully. The accessibility tree exposes text, controls, and labeled coverage progress bars; this is not a Narrator acceptance result. The app was left open for review. The capture is from the installed binary, dark system theme, maximized window; the computer-use tool reports a 1536×864 logical capture and supplies a JPEG. It is live data, not a fixture.

Wider native accessibility, mixed-DPI, clipboard output, install/repair/uninstall, Windows 10/11, signing, and CI execution gates remain open. This is an unsigned internal build.
