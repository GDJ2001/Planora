# Planora 0.3.0-beta.25 installer evidence

Beta.25 fixes the overlapping text and controls shown after removing Planora and refreshes the Windows Setup layout. It does not change helper protocol v5 or recording format v4.

## Cause and change

- The previous theme introduced a `ConfirmUninstall` page outside WixStdBA's supported page set. During Burn's transition to `Success`, controls from that page could remain visible under the success content. This explains the stacked headings and the simultaneous Back, Uninstall, and Close buttons in the reported screenshot.
- Uninstall now runs from WixStdBA's supported `Modify` page. The maintenance page explains the removal effects before the action. `Success` shows operation-specific copy without retaining another page's controls.
- Install, progress, maintenance, success, and failure screens now use a wider layout, consistent margins, larger headings, blue stage labels, and separated footer buttons. The install screen states the location and explains Advanced sensors.
- `packaging/windows/Test-Theme.ps1` rejects unsupported pages, page navigation, duplicate control names, or an uninstall button outside `Modify`. The package build runs this check.

## Verification on the reference Windows PC

- The theme structure check, `cargo fmt --all -- --check`, strict workspace Clippy, and workspace tests passed. A CPU sampler test was made timing-safe after a full process scan crossed the platform's minimum interval during the first test run.
- The MSI passed `wix msi validate`. Setup and MSI were built from clean commit `bcd9c1f` with application version `0.3.0-beta.25` and MSI ProductVersion `0.2.1025`.
- Native WiX windows were inspected through UI Automation and `PrintWindow` at this PC's current display scale: [install](screenshots/native/installer-install-beta25.png), [maintenance](screenshots/native/installer-maintenance-beta25.png), and [install success](screenshots/native/installer-success-beta25.png). Those three screens show separated text and controls without the reported overlap. The uninstall success screen was not captured; its Burn uninstall transaction completed successfully.
- The beta.25 upgrade from beta.24 completed with Burn result `0x0` and no restart. A subsequent beta.25 uninstall completed with result `0x0`; a beta.25 reinstall completed with result `0x0` and no restart. The current installed UI, helper, and sensor broker match their release hashes. The helper is running, authorization is present, and the personal settings file remains present.
- The two registry entries are the visible Burn bundle and its hidden MSI (`ARPSYSTEMCOMPONENT=1`). This registry inspection does not replace a visual check in Windows Installed Apps.

## Package identity

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.25-x64-Setup.exe` | `320CE6219E40054C7A61967EF170B630F1FC4AE5933E564881973D5B62B9FCDD` |
| `Planora-0.3.0-beta.25-x64.msi` | `D976F3EDA1E839FBDE3B7B1CF636DA7AB81299AE20FCFE5DF85414DB63211F40` |
| `planora.exe` | `D8AD62D6083AE81C352ACD90871E5E05DE26E43DC8A16EDE7FB8F25DBA938D9F` |
| `planora-helper.exe` | `701C271F09B4AC09915379B062ACE40AFB2A798BA8E3B89C45B7226395AE9730` |
| `planora-sensor-broker.exe` | `BAFBC60931E43C59927EFA8F28062952EDB13FA343CB873605D547CBDB0514EE` |

This unsigned package remains an internal build.
