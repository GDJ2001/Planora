# Planora 0.3.0-beta.19 resource selector follow-up

Beta.19 addresses the fresh-launch issue found during beta.18 installed acceptance. A persisted list offset could open Performance among sensor rows while CPU remained selected in the detail pane. The live machine has 46 available sensors, so one card per sensor also made the selector unnecessarily long.

The selected card is now revealed once when the wide selector opens. Individual sensors are collapsed under a count-labeled section by default; searching still reveals matching sensor rows. A restored sensor selection expands its section, and a selection made in compact mode is revealed when the wide selector returns. User scrolling after that first reveal is respected.

This is a UI-only follow-up. Helper protocol v5, recording format v4, settings schema v6, sensor providers, and installation behavior are unchanged. Application version is `0.3.0-beta.19`; MSI version is `0.2.1019`. The installed beta.18 package is retained for rollback.

## Source and live development verification

`cargo fmt --all --check`, strict workspace Clippy, and all workspace tests passed (129 active tests, two existing ignored tests). A normal, non-fixture launch of the beta.19 development build connected to the installed helper and rendered the selected CPU card at the top of the resource selector. The 46 sensor readings appeared as one collapsed Sensors section, while the detail pane remained on CPU. The development window closed through its normal shutdown flow. The resource filter implementation reveals matching sensors even when the section is collapsed; an interactive search check remains in installed acceptance.

Pending clean-revision package build and installed live acceptance. Installed checks will compare binary hashes and build identities, confirm the sensor group can expand and be searched, verify the selected card after a fresh relaunch, and collect fresh telemetry snapshots.
