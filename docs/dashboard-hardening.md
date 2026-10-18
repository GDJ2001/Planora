# Dashboard correctness hardening

Updated: 2026-10-19. This document describes source changes and local verification, **not a native release acceptance certificate**.

## Corrected issues

- **Telemetry timestamps:** History points now use each metric's capture timestamp projected onto the local monotonic display timeline. Repeated or out-of-order sample timestamps are discarded, including repeated cached CPU readings. Old samples stay in the correct timeline position instead of being presented as freshly measured values.
- **Unavailable measurements:** Disk and network overview cards no longer substitute missing read/write/receive/send rates with zero. Invalid or negative rate inputs also show **Unavailable**.
- **Process identity:** Selected-process trend buffers use the full PID + OS creation identity and reset if the PID is reused or a different process is selected.
- **Display pause:** Both the frame and chart histories are snapshotted as immutable shared handles on pause. The sampler and live histories continue advancing, while the visible tables/charts stay frozen. No affinity action is enabled while paused.
- **Benchmarks:** CPU-pair heatmaps display the same measured result in the two symmetric cells; the diagonal now says `N/A`, not `0.0 ns`.
- **Keyboard/mouse chart inspection:** Click a chart to focus it. Left/Right arrows move its crosshair, Home/End jump to the visible range limits, and Escape clears it. Selected sample readouts are shown below the chart. Leaving a chart clears keyboard-specific selection. Mouse hover remains supported.
- **Long processes and resizing:** Process names are clipped within the table name column, with hover access to the full value. The process inspector has a draggable divider on wide layouts.
- **Dense history performance:** Screen-pixel min/max downsampling retains spikes and discontinuities. Large telemetry frames are passed to rendering through `Arc`, avoiding an extra copy of every process on each repaint.
- **Scrolling and paint:** The overview and benchmark pages scroll at smaller window sizes. The unconditional repaint timer was removed; telemetry and interactions initiate redraws.

## Regression coverage added

Unit tests exercise wall-clock capture/receipt conversion, duplicate timestamp rejection, filtering future points from paused charts, PID reuse resets, dense-history gap/peak retention, symmetric CPU-pair mapping, and invalid throughput formatting.

## Merge review and local verification (2026-10-19)

The merge review fixed the PID-reuse test's obsolete identity initializer, Rust formatting, test-module ordering, and Linux-only lint failures. It also corrected chart Escape focus persistence and non-convex chart area fills. Affinity responses now retain their process identity; selecting a different process clears pending edits and triggers a fresh query. Changes require live, unpaused telemetry and a known nonempty CPU set; queue submission is nonblocking. Build metadata tracks Git HEAD/ref changes so switching to main does not retain an old displayed commit.

| Environment | Fresh verification |
| --- | --- |
| Windows x86-64, Rust 1.95 | Workspace tests: 44 passed; formatting and strict workspace/all-targets Clippy passed; release binaries built. |
| Ubuntu 24.04 under WSL, Rust 1.95 | Workspace tests: 47 passed; strict workspace/all-targets Clippy passed. |
| Headless egui interaction on both environments | Pointer focus, Escape, and subsequent repaint regression passed; pending affinity state resets on process identity change. This does not verify native window appearance. |

GitHub Actions run `39144421102` reports: "The job was not started because an Actions budget is preventing further use." Hosted checks therefore provide no additional platform validation. The merge incorporates locally verified development work; public-release acceptance remains separate.

## Outstanding release validation

1. Run the checks on Fedora and Apple Silicon macOS and restore hosted CI execution.
2. Test the actual binary and screenshots in light/dark modes at 1024×640, 1366×768, and 1920×1080; repeat at 125%, 150%, and 200% scaling.
3. Use deterministic fixtures for unavailable/zero/overflow rates, delayed and repeated telemetry frames, PID reuse, pause longer than five minutes, 10,000 processes, and partially completed CPU-pair results. Check focus/keyboard behavior and chart/inspector clipping.
4. Measure p95 frame time under 16.7 ms **on the specified reference machine**, while sampling and benchmark work occurs. Do not assume source-level optimizations prove that target.
5. Confirm all existing helper/affinity/security tests still pass and that the package executable build identifier matches the reviewed commit.

Any failing native tests or styling issues must be corrected before release. Do not treat an open PR as a shipped build.
