# Planora 0.3.0-beta.21 CPU utilization chart

The CPU Performance graph now uses a conventional time-series line and restrained area fill. The prior vertical activity bars and separate peak ticks resembled an audio equalizer. The new line connects only observed one-second CPU averages, breaks when an expected interval is missing or the source changes, and does not smooth between points. The summary and cursor retain exact observed values, within-interval minimum/maximum, timestamps, provenance, and coverage. The 0–100% axis remains the default; Fit scale, time ranges, pause, Tables only, and CSV export are unchanged.

This is a UI-only change. Helper protocol v5, recording format v4, settings schema v6, telemetry providers, authorization, and installer behavior are unchanged. Application version is `0.3.0-beta.21`; MSI version is `0.2.1021`. Beta.20 remains the installed rollback package until upgrade verification finishes.

## Source verification

`cargo fmt --all --check`, strict workspace Clippy, and all 129 active workspace tests passed (two existing ignored). A focused test confirms that a missing one-second CPU observation breaks the line. Native egui fixture captures were reviewed in [dark 1536×864](screenshots/native/performance-cpu-line-beta21-dark-1536-fixture.png) and [light 1180×790](screenshots/native/performance-cpu-line-beta21-light-1180-fixture.png). Both contain labeled simulated data, including an intentional history gap and a short CPU spike; neither shows the former vertical bars or clipped chart controls.

## Package and installed verification

Pending clean-revision build and installed live check.
