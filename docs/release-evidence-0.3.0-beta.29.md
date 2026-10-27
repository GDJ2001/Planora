# Planora 0.3.0-beta.29 Overview containment

This internal update removes Overview capacity bars and prevents cards and summary cells from expanding the page. Beta.28 already exists at source `634811c`; it is preserved. This update uses MSI `0.2.1029`.

## Changes

- Reused the committed beta.28 bar removal. Cards now have measured, wrapped text, explicit vertical content and bounded chart rectangles. Memory used/total remains text.
- Summary column minimums use actual heading and formatted-value font metrics. Exact cell allocations prevent short values from shifting later columns. If all six columns cannot fit, each resource becomes a labeled section with two value columns or one.
- Now, Average, Low, Peak and readable Coverage remain present. Missing observations remain distinct from zero. Range controls wrap within the panel.
- Shared page headings wrap and move their controls to another row when needed. Fixture-only scale/text/scroll controls support repeatable native review without changing saved preferences.
- Measurement providers, helper protocol v5, recording format v4 and settings semantics are unchanged by this patch.

## Automated verification

`cargo fmt --all -- --check`, `cargo test --workspace --locked`, and strict workspace/all-target Clippy pass. UI suite: 86 passed. Existing hardware-dependent ignored tests remain ignored.

Seven focused Overview tests include actual text galley bounds, non-elided values, chart endpoints, numeric alignment, table/stacked fields, pointer activation, responsive columns, unavailable and empty history. Fixtures cover long names, large formatted values, both themes and text scaling through 200 percent. These checks inspect interior contents, not only the outer panel.

## Native visual review

The development native fixture capture matrix contains 62 captures: Overview at all five target sizes in both themes, enlarged-text scrolled sections, four emulated pixel-per-point scales with normal/enlarged text, and the other eight pages at 1024x640 and 1180x790 in both themes. Inspected screenshots show no Overview capacity bars, overlapping summary fields or horizontal page expansion. The compact Settings and Recordings views also retain their controls inside their panels.

Fixture captures are synthetic data. Pixel-per-point overrides emulate rendering scale; they do not establish Windows DPI-change, monitor-move or Narrator acceptance. Scrolled images intentionally cut content at the vertical viewport boundary; all sections remain vertically reachable.

## Package and installed acceptance

Release package identity, final executable capture evidence and installed acceptance are recorded after the clean-source build. Installation remains pending until Windows approval and installed hash verification complete.

## Remaining acceptance

Real Windows display-scale changes, mixed-monitor moves, Narrator, interactive keyboard/pause/range exercises and dynamic many-device installed sessions were not established by screenshot inspection. Existing broader public-release gates still apply. This package remains unsigned and internal.
