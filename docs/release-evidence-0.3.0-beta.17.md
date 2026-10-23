# Planora 0.3.0-beta.17 internal package evidence

Beta.17 improves the History page's Resource statistics and Interval detail panels. Both use the available width. Numeric values are right aligned with explicit units; statistics include low-to-peak range bars with average markers and labeled observation coverage. The interval table has a fixed header, virtualized scrolling, a missing-interval filter, and tab-separated copying. Missing values remain distinct from observed zero. Independent scroll IDs fix the duplicate-ID overlays reproduced from the user's screenshot.

This update retains system-managed appearance from beta.15 and graph rendering from beta.16. It does not change measurement providers, helper protocol v5, recording format v4, settings schema v6, or advanced-sensor installation. Application version is `0.3.0-beta.17`, MSI version `0.2.1017`.

## Baseline

On 2026-10-23, installed beta.16 reported revision `881887c`. Installed UI, helper, and sensor-broker SHA-256 values matched its manifest. PlanoraHelper was running and authorization was present (the authorization value was not recorded). The settings file's SHA-256 was `0641C33A0534CEA138C3EAE7C5D41C5AFC7167DAECA3CFEB877BB4C510FF513F`. Beta.16 and beta.14 packages are retained for rollback.

## Verification

The duplicate-scroll-ID regression was observed failing before the change and passing afterward. The History layout regression covers five logical window sizes, both themes, and text multipliers 1, 1.5, and 2, checking width bounds and widget-ID diagnostics. These synthetic checks do not substitute for physical monitor/DPI or Narrator tests. A separate regression preserves zero versus missing interval values and filter semantics.

`cargo fmt --all --check`, `cargo test --workspace --lib --bins --locked` (including all 65 UI tests), and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed on the beta.17 source before packaging.

Native development previews were reviewed at 1536×864 dark and 1024×640 light, rendered at this PC's 125% scale. They contain labeled simulated data. Package hashes, final release captures, and installation results will be recorded after the clean-revision build. Wider native accessibility, mixed-DPI, install/repair/uninstall, Windows 10/11, signing, and CI execution gates remain open. This is an unsigned internal build.
