# Planora 0.3.0-beta.20 Overview and navigation polish

The final installed beta.19 review exposed two more visual issues: at a wide window, Overview showed four resource cards on the first row and one isolated card on the second; switching from a scrolled History page could open Overview at the inherited vertical offset, hiding its header. Beta.20 retains beta.19 as rollback.

Overview now uses five cards in one row when the content width permits. At medium widths, three cards occupy the first row and the final two divide the second row evenly. Narrower layouts use two or one columns. The Overview, History, Hardware, Benchmarks, and Settings outer scroll areas have distinct IDs, so changing pages does not carry an unrelated scroll position.

Native fixture review: [dark Overview at 1536](screenshots/native/ui-polish-beta20-overview-dark-1536-fixture.png) shows five aligned cards; [light Overview at 1180](screenshots/native/ui-polish-beta20-overview-light-1180-fixture.png) shows a balanced three-plus-two layout. The fixture images use simulated measurements. A normal beta.20 development launch connected to the installed helper; after scrolling History into its lower charts, switching to Overview displayed the header and cards at the top. This was a live development-build check, not installed beta.20 acceptance.

Helper protocol v5, recording format v4, settings schema v6, telemetry providers, authorization, and installer behavior are unchanged. Application version is `0.3.0-beta.20`; MSI version is `0.2.1020`.

## Verification

Formatting, strict workspace Clippy, and all 130 active workspace tests passed (two existing ignored). Clean-revision package build and installed live acceptance are pending.
