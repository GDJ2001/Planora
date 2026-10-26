# Planora 0.3.0-beta.24 release evidence

Beta.24 fixes history loss under detail-series pressure and improves range switching.

## Root cause and change

- On machines reporting hundreds of GPU engine instances, detail history reached its 100,000-point budget. The worker evicted active sensor series and immediately re-created them at the next poll. This repeatedly erased their graphs and summaries, including after switching back to a short range.
- The selected resource and selected sensor trend group now receive retention priority. GPU engine histories are evicted before ordinary device and sensor histories. Evicted background series stay evicted until explicitly selected, avoiding repeated erase/re-create cycles. System histories remain outside the detail budget.
- The history worker publishes the newest complete immutable snapshot through a replaceable one-slot handoff. A slow UI no longer makes it clone the full history on every poll while an older snapshot occupies the slot.
- Selecting a sensor chooses its compatible trend group so a temperature reading does not initially display an unrelated empty power chart.

## Verification

- Focused regressions cover selected sensor retention under GPU engine pressure, recovery of an explicitly selected evicted engine, and 15-minute chart/table observations after changing ranges.
- `cargo fmt --all --check`, strict workspace Clippy, and workspace tests passed on Windows x86-64. Two pre-existing hardware-dependent tests remained ignored.
- Native long-running installed verification is pending. The previous installed package remains the rollback baseline until installation and observation complete.

The helper protocol and recording format are unchanged. This is an unsigned internal build.
