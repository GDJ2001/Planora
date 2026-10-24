# Planora 0.3.0-beta.18 UI polish evidence

Beta.18 refines the native interface without changing helper protocol v5, recording format v4, settings schema v6, telemetry providers, or authorization. MSI version is `0.2.1018`; beta.17 remains the rollback package.

## Visual audit and changes

The installed beta.17 History screenshot reported by the user showed short, ambiguous range bars and a clipped interval row under a nested table viewport. A native fixture audit covered all nine pages in dark and light themes at 1180 × 790 logical pixels. Affected pages were also captured at 1024 × 640 and Performance at 1536 × 864. Fixture images contain simulated measurements, not live hardware evidence.

| Area | Finding | Beta.18 change |
| --- | --- | --- |
| History | The 0–100% range mapping made a narrow memory range look like one dot; throughput rows used unrelated peak scales. The interval table exposed a partial row below its header. | Each row now has an explicit low-to-peak ruler with an average marker and exact low/peak columns. The table uses page scrolling instead of a second vertical viewport. |
| Overview | Resource cards inherited a horizontal layout, leaving most of each card empty. Coverage percentages crowded the bars. Rate sparklines were capped by a percent scale. | Cards use stacked content and full-width 60-second trends, with two, three, or four columns by available width. Coverage has separate bar and percentage space. Sparklines scale from zero to the observed metric peak. |
| Performance | The long CPU small-multiple toggle pushed Export CSV onto a line by itself. | The toggle sits with the CPU detail section; the toolbar stays coherent at the reviewed widths. |
| Processes, Hardware, Benchmarks, Capabilities, Recordings, Settings | No visible clipping or duplicate-widget-ID overlay in the dark/light 1180 × 790 fixture captures. | No unrelated behavior was changed. |

Reviewed captures: [History dark 1180](screenshots/native/ui-polish-beta18-history-dark-1180-fixture.png), [History light 1024](screenshots/native/ui-polish-beta18-history-light-1024-fixture.png), [Overview dark 1180](screenshots/native/ui-polish-beta18-overview-dark-1180-fixture.png), and [Performance light 1536](screenshots/native/ui-polish-beta18-performance-light-1536-fixture.png). The captures show no red duplicate-widget-ID diagnostics. The compact Performance and Overview captures were also reviewed from the local audit directory; horizontal table scrolling remains available where columns exceed width.

## Automated checks

`cargo fmt --all --check` and strict `cargo clippy --workspace --all-targets --locked -- -D warnings` pass. All 115 active workspace library/binary tests pass; one existing test remains ignored. The UI suite includes range-ruler bounds, byte-rate sparkline scaling, layout bounds, missing values, and the headless smoke suite.

## Package and installed verification

Pending packaging from a clean committed revision. Native fixture review is separate from installed live acceptance. Beta.18 remains an unsigned internal build until package hashes, upgrade, helper health, fresh telemetry, and normal launch are checked.
