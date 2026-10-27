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

Built from clean source `b617d9b`; WiX validation, GUI subsystem, binary identities and signed PawnIO dependency checks passed. Setup/MSI and manifest were copied to the primary checkout dist directory and hashes verified. [Package manifest](screenshots/native/beta29/package-manifest.json).

Thirty additional captures use the final release executable. [Native capture manifest](screenshots/native/beta29/manifest.json) distinguishes full-size review from contact-sheet review and records fixture scales. The final executable shows complete table columns and enlarged-text stacked sections.

The user completed beta.29 installation. Burn exited `0x0`, no restart. Installed UI, helper and broker hashes match the manifest; helper running, authorization present, settings hash unchanged. [Installed verification](screenshots/native/beta29/installed-verification.json). Live interactive Overview acceptance remains separate from these hash/service and fixture checks.

## Remaining acceptance

Real Windows display-scale changes, mixed-monitor moves, Narrator, interactive keyboard/pause/range exercises and dynamic many-device installed sessions were not established by screenshot inspection. Existing broader public-release gates still apply. This package remains unsigned and internal.
