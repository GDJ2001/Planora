# Planora 0.3.0-beta.26 startup authorization evidence

Beta.26 moves the Windows helper authorization attempt before the normal application window is created. The UI keeps its `asInvoker` manifest: only the installed, integrity-checked authorization script requests administrator approval. An already authorized account starts without a prompt. Protocol v5 and recording format v4 are unchanged.

## Implementation and checks

- The pre-window probe distinguishes a ready helper, an authorization failure, an unavailable service, an incompatible protocol, and an unresponsive helper. It requests approval only for authorization or service availability. A declined or failed request opens the app with a specific recovery message instead of treating it as success.
- Automatic elevation is limited to the copy in the Windows Program Files Planora directory. The script must match the source embedded in the binary. PowerShell is resolved from the Windows system directory, and the pre-window wait is bounded.
- The GUI and sensor workers remain unelevated. The LocalSystem helper retains the privileged telemetry boundary and the existing per-user authorization check.
- Formatting, strict workspace Clippy, full workspace tests, and focused startup authorization tests passed. The MSI passed WiX validation; the UI binary has the Windows GUI subsystem.

## Installed Windows result

- Source commit: `eaa1332`; application version `0.3.0-beta.26`; MSI ProductVersion `0.2.1026`; protocol v5.
- The first Setup elevation attempt timed out with `0x800704c7` before installation. The user approved a subsequent administrator prompt. The same Burn transaction then completed with `0x0`, no restart. The beta.25 bundle was removed by the upgrade transaction.
- Installed UI, helper, and sensor broker SHA-256 values match the release manifest. The installed authorization script matches its source. The helper service is running and the authorization registry key and personal settings file are present. Authorization values were not read into this report.
- Normal installed launch logged `startup-authorization helper-ready`, build `eaa1332`, a first painted frame, and `helper-connection result=connected`. UI Automation found the full Planora window, Live Telemetry, and current resource readings. The window opened at the saved maximized placement on this monitor at 125% scale.
- A second normal launch exited while the first window remained open. Closing the first window logged shutdown completion; its owned UI/sensor processes exited and the independently installed helper stayed running. A later normal launch reopened the UI and was left available.

## Package identity

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.26-x64-Setup.exe` | `613A16C33F1743BB45B4428865041AD19D2C057EC84C8264792DD0151DB5671B` |
| `Planora-0.3.0-beta.26-x64.msi` | `8DE3008B907907343C759067A7EC3B868757B33FFB9BD2BB871785E3437F0AA5` |
| `planora.exe` | `62C81F8B25F43D86F86B632266E472BCE22270C54EDB087F651E0BFCC544FE0B` |
| `planora-helper.exe` | `915B37AA58A77EE89414012791436C5A42EE3CD45130D880FB9A41B7BF3278F9` |
| `planora-sensor-broker.exe` | `40B4754D7F8A038E5D96A7E78369EA8691BA5B88454EEDCA9CEF2F5DE5B95F49` |

## Remaining native check

The current Windows account was already authorized when beta.26 launched, so the new first-authorization UAC path has unit coverage but has not been exercised end to end on an unauthorized account. That check should verify the visible UAC prompt appears before any Planora window, denial produces the recovery screen, approval connects the helper, and only the intended account gains access. This unsigned package remains internal.
