# Planora 0.3.0-beta.20 Overview and navigation polish

The final installed beta.19 review exposed two more visual issues: at a wide window, Overview showed four resource cards on the first row and one isolated card on the second; switching from a scrolled History page could open Overview at the inherited vertical offset, hiding its header. Beta.20 retains beta.19 as rollback.

Overview now uses five cards in one row when the content width permits. At medium widths, three cards occupy the first row and the final two divide the second row evenly. Narrower layouts use two or one columns. The Overview, History, Hardware, Benchmarks, and Settings outer scroll areas have distinct IDs, so changing pages does not carry an unrelated scroll position.

Native fixture review: [dark Overview at 1536](screenshots/native/ui-polish-beta20-overview-dark-1536-fixture.png) shows five aligned cards; [light Overview at 1180](screenshots/native/ui-polish-beta20-overview-light-1180-fixture.png) shows a balanced three-plus-two layout. The fixture images use simulated measurements. A normal beta.20 development launch connected to the installed helper; after scrolling History into its lower charts, switching to Overview displayed the header and cards at the top. This was a live development-build check, not installed beta.20 acceptance.

Helper protocol v5, recording format v4, settings schema v6, telemetry providers, authorization, and installer behavior are unchanged. Application version is `0.3.0-beta.20`; MSI version is `0.2.1020`.

## Verification

Formatting, strict workspace Clippy, and all 130 active workspace tests passed (two existing ignored).

## Package build

Built from clean committed revision `7af6698`. The package script verified the Windows GUI subsystem, matching UI/helper protocol-v5 identities, MSI version `0.2.1020`, and WiX MSI validation. Independent SHA-256 checks matched the manifest:

| Artifact | SHA-256 |
| --- | --- |
| Setup EXE | `6EC6DD87E9DED99CFE51C85BBD1F55DC08D622AD0961DFE8E6088EF59FA681EE` |
| MSI | `CE2EA503C820ABCA943CDEB9AA0AC1CAAEF793C85D6BE2EA7A35B9FF4F5D2058` |
| `planora.exe` | `23943A05B5B30BECABA95CD6DC52E82D4805B80B49CD8DA0A2E39834F74B4732` |
| `planora-helper.exe` | `9CD3AD26456BE882EC74F59EC15BAF91867200DDA2F905E52DFA155C37A9C401` |
| `planora-sensor-broker.exe` | `1F89C74D90552B606FF0491B7C60EF2A697DBA711827C2AAE3CFDACAB7FAB63E` |

Before upgrade, the three installed beta.19 binary hashes matched their manifest, PlanoraHelper was running, authorization was present, and the settings SHA-256 was `DAC6D1CCE00B596273145C55E59A5D40E17DD89F948F916E3706D361D412E6A1`. The authorization value was not read or recorded.

## Installed verification — 2026-10-25

The user reported the beta.20 Setup completed. The Burn log ended `Apply complete, result: 0x0, restart: None`; the MSI log recorded successful installation and ProductVersion `0.2.1020`. Installed UI, helper, and broker hashes match the beta.20 manifest. Their build identities report version `0.3.0-beta.20` and source revision `7af6698`; UI/helper protocol is v5. PlanoraHelper is running, authorization remains present, and the settings hash matches the pre-upgrade value.

The installed helper returned 20 distinct fresh protocol-v5 snapshots, maximum CPU reading age 525 ms. The final snapshot had one measured Intel Iris Xe adapter, 274 GPU engine instances, and 46 available advanced sensors, including CPU package power and 11 CPU-temperature records. This verifies availability, not independent accuracy.

A normal installed launch opened the full-size Performance workspace without a terminal. On the installed Overview at a 1536-pixel-wide desktop, all five resource cards filled one row. After scrolling down through the History charts, returning to Overview displayed its title and cards at the top rather than inheriting the History offset. Native fixture images also cover the dark wide and light medium layouts. Broader native sizes, scaling, Narrator, high contrast, and long-run performance remain open. Beta.20 is unsigned and internal; beta.19 is retained for rollback.
