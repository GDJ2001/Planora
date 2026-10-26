# Planora 0.3.0-beta.27 startup authorization evidence

Beta.27 retains beta.26's pre-window helper authorization and resolves the elevated PowerShell second stage from the Windows system directory. This removes reliance on command search paths in an automatically invoked privilege boundary. The application UI stays unelevated; the installed helper owns privileged telemetry.

## Verification

- Built the MSI and Setup from clean commit `5e2c843` in an isolated worktree. The separately edited history/UI files in the primary checkout were not included in this package.
- Focused startup authorization tests, strict workspace Clippy, PowerShell parser validation, GUI-subsystem check, installer theme check, WiX MSI validation, and release identity checks passed. The isolated full workspace run stopped at `broker_cached_readings_expire_to_stale_during_restart_backoff`: with an installed sensor broker present, its test constructor started that broker and returned 43 live readings instead of the single synthetic reading expected by the fixture. This test-environment dependency needs a separate fixture isolation fix; it does not exercise startup authorization.
- The user approved the beta.27 Setup. Burn exited `0x0` without restart. The installed UI, helper, and sensor broker hashes match the release manifest, and the installed authorization script matches the committed source.
- `PlanoraHelper` is running. Authorization remains present. The personal settings SHA-256 remained `639737F0100C46D7B34EFAA61721E5921B91BBA3C672406673C1D6CD08916245` across the beta.26-to-beta.27 upgrade; authorization values were not recorded.
- A normal installed launch logged build `5e2c843`, protocol v5, `startup-authorization helper-ready`, a first painted frame at 1180 × 790 logical pixels and 125% system scale, and `helper-connection result=connected`. UI Automation found a visible Planora window and Live Telemetry.

## Package identity

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.27-x64-Setup.exe` | `F86030355F23D3D648E748C812332D3ACA1FBC317905DEC557770EA975B05590` |
| `Planora-0.3.0-beta.27-x64.msi` | `ACFD457FBF7B69F8EA071140BF14B6704235A60334BB965DACA781DA1042704E` |
| `planora.exe` | `B3D3A3AB3AC2F7E46E90F00018E9CD2C0A1EEABCB60576ACD6E6C0C2E0F976A5` |
| `planora-helper.exe` | `370C8390ADB2C4FEA0CADAE3A32C0C96CC2B5E440D7C61D5157C505D7957AC36` |
| `planora-sensor-broker.exe` | `D8A9844ADC409CF3266B68C674CAFC4CB0FD648870CBB769B056E0CC07D75DBD` |

## Remaining acceptance

The reference Windows account was already authorized at launch. An account without authorization still needs a native end-to-end test of approval, denial, timeout, and recovery, including confirmation that the UAC prompt appears before the Planora window. This unsigned build remains internal.
