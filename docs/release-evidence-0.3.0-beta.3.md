# Planora 0.3.0-beta.3 Windows internal Setup evidence

**Status on 2026-10-21: installed and exercised, then removed during maintenance testing. A fresh reinstall failed and rolled back.** Beta.3 remains an internal, superseded build. Its source commit is `27fa642`, application version `0.3.0-beta.3`, MSI/Burn version `0.2.1003`, and protocol v4.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.3-x64-Setup.exe` | `BF7AB05DD176994C68437CA03E06DD81DE769AF5179E831FBE2DEFF4D59A67DB` |
| `Planora-0.3.0-beta.3-x64.msi` | `2A90835338F514828E6443F45EB2194392CA453887F332BF6421B9D0A3CD29B1` |
| `planora.exe` | `300339DED453BF3C883460875DF2A961BFAE647102E11E255EFE643ADAA725F6` |
| `planora-helper.exe` | `5A101985F75049370A14F1F1EDAF1081CAC20CFEF8BDF69804D62A86334C30B5` |

The release script built both binaries and the MSI/Setup, checked matching build identities, validated the MSI, and wrote `dist/Planora-0.3.0-beta.3-x64.manifest.json`. `cargo fmt --all -- --check`, `cargo test --workspace --locked`, strict workspace Clippy, and `git diff --check` passed before packaging. Burn extraction confirmed the custom theme, logo/title icon, version and installation-location variables, launch target, and chained `ARPSYSTEMCOMPONENT=1` property.

The upgrade from beta.2 completed successfully. Installed UI and helper hashes matched the manifest; both reported beta.3, commit `27fa642`, and protocol v4. The helper ran with existing authorization, the settings hash remained `8426C689C33A56EED3248FBBD675B1E50BE85D776937D3E4BA272E98F931D5EA`, and the installed-service test observed 21 consecutive fresh snapshots. Windows Settings showed one visible Planora entry; the MSI's separate entry was marked `SystemComponent=1`. Both Start menu shortcuts were present. The successful upgrade Burn log is `%TEMP%\Planora_20261021213734.log`.

The maintenance test exposed two defects. The registered maintenance screen presented Repair and Uninstall side by side. Although Repair was intended, the Burn/MSI logs `%TEMP%\Planora_20261021214706.log` and its MSI log record an **uninstall action**, completed successfully. The evidence does not establish whether this came from the click target or user selection. The custom theme used incorrect Burn action numbers and showed a generic success result, masking what happened. Files, helper service, and system authorization were removed; personal settings remained.

The subsequent fresh Setup failed and rolled back. Its MSI log reports **Error 1920: Planora Helper failed to start**; Burn reported `0x80070643`. The MSI unconditionally started the helper during installation, but the helper requires an authorized SID and exits when none exists. No Planora app or helper remained installed after the rollback. Beta.4 fixes the first-install lifecycle and maintenance messaging. Do not rebuild altered beta.3 contents under MSI version `0.2.1003`.

Still open for beta.3: repair outcome, cancellation/UAC denial, native accessibility/scaling review, and Windows 10/11 acceptance. Beta.3 is unsigned and not approved for public distribution.
