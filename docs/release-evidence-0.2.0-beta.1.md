# Planora 0.2.0-beta.1 Windows verification record

**Status: internal, unsigned beta candidate. Public distribution is not approved.** This record covers one Windows 11 Pro x86-64 machine (build 26300, Intel Core i7-1165G7, 4 cores / 8 logical CPUs). It does not establish Windows 10, Linux, or macOS acceptance.

## Exact package and installed identity

The MSI was built from clean commit `96cebaf209e2b6fb0edfaae8d0c5dd6f487746e5` with the pinned Rust 1.95 toolchain and WiX validation. The MSI ProductVersion is numeric `0.2.0`; the binary and artifact version is `0.2.0-beta.1`.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.2.0-beta.1-x64.msi` | `DFE210CB114B158F5A1AC39228C0BDA9BC73DD4FA65D49759CD87A129A16C740` |
| Installed `planora.exe` | `D129312D40E93360FA0333146ECF3767D62718721F64299F47B203F67714E55D` |
| Installed `planora-helper.exe` | `47B8DAB6028F52D68DC72507DA0CD2FAB38E3735697273437CAC81EED9553A6E` |

Both installed binary hashes matched the package manifest. `planora.exe --build-info` reported release profile, `x86_64-pc-windows-msvc`, protocol v3, version `0.2.0-beta.1`, and commit `96cebaf`. The dependency inventory, package hashes, and clean-source provenance are in ignored local `dist/` outputs. Those logs and native captures are not release artifacts and may contain local machine or process details.

## Native behavior exercised

- An upgrade from an authorized 0.1.5 fixture to an earlier beta candidate preserved authorization through MSI migration without running authorization again. That candidate started the helper and returned eight fresh protocol-v3 snapshots. The final MSI uses the same migration logic; a second 0.1.5-to-final upgrade was not run.
- The final MSI was installed after full removal of the preceding internal candidate. Uninstall removed its files, service, and `HKLM\SOFTWARE\Planora` key. The final package's installed authorize, revoke, and reauthorize scripts were exercised; the helper ended running with `AuthorizedSid` present and the legacy service environment value absent.
- The final installed helper completed a protocol-v3 authorized handshake and returned 301 consecutive fresh snapshots over 305.71 seconds (5.1 minutes). A separate shorter check returned 12 fresh snapshots. This verifies the exact installed binary, but is not the required one-hour combined profile.
- Workspace tests, format checking, and strict workspace Clippy passed after the final UI change. The final chart header was visually checked in dark and light at 1024×640.
- A release-mode telemetry probe on this machine measured 30 collections after warm-up: 16.39 ms minimum, 18.35 ms median, and 34.61 ms p95 when run without concurrent tests. A run overlapping workspace tests measured 315.87 ms p95, so collection performance remains load-sensitive. These are short probes, not the required one-hour combined profile.

The local native capture set contains 70 installed-app images: seven pages, dark and light themes, and logical sizes 1024×640, 1180×790, 1366×768, 1536×864, and 1920×1080. The actual framebuffer scale was 125%; Windows reported system DPI 96. The capture manifest records the installed binary hash, source revision, dimensions, individual PNG hashes, and contact-sheet review status. All 70 files were present and matched their recorded hashes. A compact chart-header collision found in the preceding capture set was corrected and checked in the final build. The captures do not test Narrator, high contrast, reduced motion, or other display scales.

## Open public-beta gates

- Run the native Windows 10 and Windows 11 acceptance matrix, OS-tool comparisons, 10,000-process fixture, benchmark cancellation under load, affinity changes on a disposable process, and a one-hour CPU/memory/handle/frame-time profile.
- Test future MSI upgrades from this beta and define a numeric ProductVersion path from prerelease `0.2.0` to a stable `0.2.0`; equal-version major upgrades are not established by this package.
- Complete keyboard-only, Narrator, high-contrast, reduced-motion, 100%/150%/200% scaling, and long-label reviews. The native captures alone do not certify accessibility.
- Restore GitHub Actions execution; the last observed runs failed before any job steps. Complete dependency advisory/license review and obtain signing credentials and explicit distribution-license approval. Unsigned builds remain internal.
- Run dedicated Ubuntu, Fedora, and Apple Silicon installation, helper authorization, lifecycle, visual, and uninstall tests before promoting those preview targets.

See the [capability and verification matrix](beta-readiness.md) for feature-specific gaps. The [0.1.5 record](release-evidence-0.1.5.md) is historical baseline evidence.
