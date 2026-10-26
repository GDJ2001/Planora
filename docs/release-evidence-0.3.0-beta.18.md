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

Built from clean committed revision `50c92c3`. The packaging script verified matching release UI/helper protocol-v5 identities, Windows GUI subsystem, the pinned signed PawnIO dependency, MSI version `0.2.1018`, and WiX MSI validation. Independent SHA-256 checks matched every entry in the generated manifest:

| Artifact | SHA-256 |
| --- | --- |
| Setup EXE | `E3A4512BD1676D67F8B636E849DE9C54318F9D02426F76274BF4BED976878013` |
| MSI | `5C82486D7EC64ABB676BEFD3692E07C709B9777D65048D45176A0CD7F1C8A06A` |
| planora.exe | `2BD8724E19B4EE366A6968F0BA7A1CDF3A316A9DD12C899D3B65A462ECE356D6` |
| planora-helper.exe | `4A94581C31BB3ABC044345FEE4D2084C1FBC7B20BD955A99D601F5F27D8F6EEC` |
| planora-sensor-broker.exe | `22CDF1A819C27DB7163E23CDF1443C85637A7CB9A46C70BF72BCC371C7E99685` |

Before upgrade, the installed beta.17 UI reported revision `d7fee05` and protocol v5; PlanoraHelper was running and authorization was present. The current user's settings SHA-256 was `DAC6D1CCE00B596273145C55E59A5D40E17DD89F948F916E3706D361D412E6A1`. The installed Planora window closed through its normal shutdown before Setup was launched. The authorization value was never read or recorded.

## Installed verification — 2026-10-25

The Burn Setup displayed “Planora is installed.” The installed UI, helper, and broker SHA-256 values match the package manifest. All three report beta.18 revision `50c92c3`; UI and helper report protocol v5. PlanoraHelper is running, authorization remains present, and the user settings hash matches the pre-upgrade value above.

The installed helper returned 20 distinct fresh protocol-v5 snapshots, with a maximum CPU reading age of 1028 ms. The final snapshot included one Intel Iris Xe adapter with measured engine utilization and 46 available advanced-sensor records, including CPU package power and 11 CPU-temperature records. These are availability observations, not independent sensor-accuracy comparisons.

A normal installed launch opened a full-size window without a terminal and showed live telemetry. A fresh relaunch exposed one additional usability issue: the saved resource-selector scroll offset can hide the selected CPU card among many sensor cards. That is tracked in beta.19 rather than changing the already-built beta.18 artifact. Native fixture review remains separate from installed live acceptance. Beta.18 is an unsigned internal rollback build.
