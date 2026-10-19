# Planora remaining-issues completion plan

Baseline: `main` at `97a1b6d` (2026-10-19). The Windows 0.1.2 MSI validates and is installed; its UI and helper match the release binaries. The helper returned 301 fresh snapshots over 5.1 minutes. `cargo fmt --all -- --check`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` passed. These results establish a baseline, not acceptance of the issues below.

## 1. Make IPC deadlines real for complete transactions (P1)

`crates/planora-helper/src/lib.rs` polls Unix streams for *some* readable data, then uses blocking `read_exact` for the four-byte header and full body. A peer that sends only part of either can outlive the stated deadline. `send_request` and the helper's `receive_request` are also blocking. The existing stall test covers a peer that sends no response, not partial frames or stalled writes.

- Apply one absolute deadline across each framed request/response, including partial header and body reads and bounded writes. Use platform-appropriate cancellable/nonblocking I/O; do not solve hangs by accumulating detached reader threads.
- Bound idle or incomplete requests on helper connections so authorized but stalled clients cannot occupy all connection slots indefinitely. Preserve size limits, authentication, request IDs, and reconnection behavior.
- Add deterministic tests for a one-byte header, truncated body, delayed chunks crossing a deadline, blocked or stalled write, disconnect, oversized frame, mismatched ID, and recovery over repeated failures. Test Windows and Unix implementations on their respective hosts.

**Accept when:** every operation returns within its documented deadline plus a small scheduler allowance; a failed transaction discards its stream and reconnects cleanly; repeated stalls leave no growing thread or handle count; the existing five-minute installed-service test still passes after rebuilding.

## 2. Finish responsive native layouts (P1)

The installed Overview still clips the Network card and right chart column at a maximized 1536 × 864 window. Inspect `crates/planora-ui/src/components/cards.rs`, `components/charts.rs`, and `views/overview.rs`. In the four-column card row, the rendered content behaves as an unbounded horizontal line despite computed card widths. Calculated-width unit tests did not detect the native overflow.

- Give each summary card a bounded vertical content layout, with a primary measurement, subordinate read/write or receive/transmit detail, and a sparkline that uses only the card's actual inner width. Wrap, abbreviate, or move secondary text when needed; never cut off values without a tooltip or other full-value route.
- Size chart panels from the *actual* remaining content rectangle after sidebar, padding, frame margins, and gaps. Keep titles, range controls, axes, legends, and table alternatives inside the visible panel. A horizontal scrollbar should not be required to see the Overview.
- Add rectangle-containment assertions to the headless layout test, and inspect the actual installed native window in dark and light themes at 1024 × 640, 1180 × 790, 1366 × 768, 1536 × 864, and 1920 × 1080. Check 100%, 125%, 150%, and 200% Windows scaling where available. Verify live and unavailable values, not only fixture values.

**Accept when:** every card and chart is fully visible and readable at each tested size and scale, with no overlap, clipped legend, or horizontal overflow. Record any unavailable scaling configuration as an explicit test gap.

## 3. Correct the hardware canvas (P2)

In the installed native Hardware view, the package label overlaps the first core row and the topology occupies a small part of a large canvas. Inspect `crates/planora-ui/src/views/hardware.rs`.

- Reserve a measured header band for the package/model label and separate it from core and cache nodes. Fit the initial graph to the available canvas while retaining clear node spacing and a sensible maximum zoom; make Reset View restore this fit.
- Keep displayed package, core, cache, and NUMA relationships tied to topology evidence. Do not add decorative or inferred hardware links. Preserve pan, zoom, selection, and the accessible tree alternative.
- Test both a small topology and a denser multi-package or many-core fixture, including long model names, narrow windows, and light/dark themes.

**Accept when:** labels and nodes do not overlap or clip, the default view is legible without manual zoom, and the accessible tree conveys the same supported relationships.

## 4. Separate fixture renders from native visual evidence (P1)

`crates/planora-ui/src/smoke_test.rs` renders custom SVGs, then asks headless Edge to turn them into PNGs. These are useful fixture previews, but they are not screenshots of the native `eframe` window. `crates/planora-ui/src/main.rs` currently ignores `run_smoke_suite` errors. The committed fixture Settings images still show 0.1.1 and the fixture pages exhibit text overlap.

- Return a nonzero process exit status when the requested smoke suite fails. Report a missing Edge executable or failed capture explicitly rather than printing success; keep the fast, browser-independent SVG test available in CI.
- Extend fixture checks beyond file size, dimensions, and nonblack pixels to assert content bounds and detect obvious overlap. Clearly mark fixture imagery as **sample data / simulated render**. Regenerate it from final source and version, or remove stale images from release-facing documentation.
- Capture a separate native Windows evidence set from the installed binary. For each image, record view, theme, window size, Windows scaling, binary SHA-256, displayed build identity, and pass/fail notes. Do not label SVG/Edge output as native evidence.

**Accept when:** an intentionally failed capture fails the command/test; fixture images cannot be mistaken for native captures; native images prove Overview, Processes, Hardware, Benchmarks, Capabilities, and Settings at the required Windows sizes and both themes.

## 5. Rebuild and align release provenance (P1)

The installed 0.1.2 app displays source commit `5494dee`, while current `main` is `97a1b6d`. The UI binary was built before the final commit. `README.md` and `.github/workflows/native.yml` still name a 0.1.1 MSI. Source edits after this plan will change the final commit again.

- Make the MSI filename derive from the workspace/package version in CI and documentation; remove stale 0.1.1 instructions and fixture version labels.
- Commit the fixes, then build the release binaries and MSI from that exact commit. Verify the displayed commit, package version, installed registry version, MSI ProductVersion, and SHA-256 hashes of built and installed binaries agree. Preserve a manifest with those values.
- Run WiX validation and test install or upgrade, first launch, helper authorization, service operation, and uninstall/cleanup on Windows. Restore the final installed build after the uninstall test. A UAC prompt may need the user at the desktop. Re-run the five-minute service test against the newly installed helper.
- For Linux and Apple Silicon macOS, run native build, packaging, install, and visual checks on actual target hosts when available. Otherwise keep them as explicit unverified release gates; Windows results do not close them.

**Accept when:** one committed source revision maps to every release artifact and the installed app reports it; CI and README generate the same versioned filename; native verification evidence is recorded per platform and missing hosts are not reported as passes.

## Completion record

Finish with a short evidence table for each issue: changed files, tests or native steps run, observed result, artifact/hash, and remaining limitation. Run format, workspace tests, and Clippy once after code changes; repeat only when later edits justify it. Keep the working tree clean or list intentional changes. Do not claim the release is complete solely because compilation, SVG previews, or package-format validation passed.
