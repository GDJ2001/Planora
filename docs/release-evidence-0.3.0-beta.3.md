# Planora 0.3.0-beta.3 Windows internal Setup evidence

**Status on 2026-10-21: built from clean commit `27fa642` and visually previewed; not installed or approved for public distribution.** The installed Windows build remains 0.3.0-beta.2. The application version is `0.3.0-beta.3`, MSI ProductVersion and Burn bundle version are `0.2.1003`, and UI/helper protocol is v4.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.3-x64-Setup.exe` | `BF7AB05DD176994C68437CA03E06DD81DE769AF5179E831FBE2DEFF4D59A67DB` |
| `Planora-0.3.0-beta.3-x64.msi` | `2A90835338F514828E6443F45EB2194392CA453887F332BF6421B9D0A3CD29B1` |
| `planora.exe` | `300339DED453BF3C883460875DF2A961BFAE647102E11E255EFE643ADAA725F6` |
| `planora-helper.exe` | `5A101985F75049370A14F1F1EDAF1081CAC20CFEF8BDF69804D62A86334C30B5` |

The release script built both binaries and the MSI/Setup, checked matching build identities, validated the MSI, and wrote `dist/Planora-0.3.0-beta.3-x64.manifest.json`. `cargo fmt --all -- --check`, `cargo test --workspace --locked`, strict workspace Clippy, and `git diff --check` passed before packaging. Burn extraction confirmed the custom theme, logo/title icon, version and installation-location variables, launch target, and chained `ARPSYSTEMCOMPONENT=1` property.

Native preview of the final Setup showed the Planora icon and wordmark, beta version, install description, resolved `C:\Program Files\Planora` location, administrator-protection explanation, license link and acceptance checkbox, and Install/Cancel controls. The Install action stayed disabled until license acceptance. The preview was canceled before elevation or installation. Custom install paths are intentionally unavailable while a LocalSystem helper executable shares the installation directory; the displayed location is the actual protected MSI destination.

Still open: beta.3 upgrade from beta.2, single Installed Apps entry, helper/authorization preservation, repair and uninstall screens and outcomes, UAC denial and rollback, native accessibility/scaling review, and Windows 10/11 acceptance. Beta.3 is unsigned and remains an internal build.
