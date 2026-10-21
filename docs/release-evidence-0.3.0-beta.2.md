# Planora 0.3.0-beta.2 Windows internal build evidence

**Status on 2026-10-21: built from clean commit `dd21302` and installed for internal testing; not approved for public distribution.** Beta.2 uses MSI ProductVersion `0.2.1002` and helper protocol v4.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.2-x64-Setup.exe` | `BD75EAFFF590DBAA07D1548B21A369D97C3FC6557A797A84C808DB7C1F95FDCE` |
| `Planora-0.3.0-beta.2-x64.msi` | `8FB28FFC1E0CEE6892E94D4E992345B90D49539FE39BC5128E1FC80DBB96B81C` |
| `planora.exe` in the package manifest | `061D8F1035D20807F4C40C1E93B626CF831D15BE1BAD816F363020E15ED2F5B2` |
| `planora-helper.exe` in the package manifest | `1E0F22745F70D272D5CF75AA7A7D8CBB2603140F043A074B326F3BDBFF4D2EB0` |

The package build validated the WiX MSI. `cargo fmt --all -- --check`, locked workspace tests, and strict workspace Clippy passed on Windows. Beta.2 makes the Capabilities recovery column horizontally reachable in compact windows, gives capability states readable labels, and renders History gaps as dashes with positive “seconds ago” intervals. Its installer config starts the helper service during installation, addressing the manual-start gap observed after the beta.1 upgrade. Those UI changes still need installed-native visual verification.

An initial **Run as administrator** beta.2 upgrade attempt ended at the Windows secure-desktop prompt before the bundle began. Windows returned a cancellation error and no Burn log was created. A later desktop-launched Setup completed: the installed UI and helper both report `0.3.0-beta.2`, commit `dd21302`, protocol v4, and their SHA-256 hashes match the package manifest. The helper was running when inspected, and the current-user settings SHA-256 remained unchanged (`8426C689C33A56EED3248FBBD675B1E50BE85D776937D3E4BA272E98F931D5EA`). These checks establish installed binary identity and settings retention; they do not yet establish the full live telemetry, visual, repair, or uninstall acceptance.

Registry inspection found both a Burn bundle and an MSI `Planora` Installed Apps entry at version `0.2.1002`. `Visible="no"` on `MsiPackage` did not hide the MSI entry. Beta.3 passes `ARPSYSTEMCOMPONENT=1` to the chained MSI so the bundle can be the sole entry; its native upgrade still needs verification.

GitHub Actions cannot start jobs because of the [account's Actions budget restriction](https://github.com/GDJ2001/Planora/actions/runs/39505160639). AMD sensor sampling, disk ETW and PMU profiling, full visual review, accessibility, repair/uninstall acceptance, multi-hardware Windows 10/11 testing, and signing remain open as described in [the readiness matrix](beta-readiness.md).
