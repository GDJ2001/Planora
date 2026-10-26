# Planora 0.3.0-beta.7 installed Windows evidence

Beta.7 is the startup and lifecycle hotfix, built from `fa80d15` with helper protocol v4 and MSI ProductVersion `0.2.1007`. The package remains an internal unsigned build.

On the current Windows 11 x64 test PC, the branded Setup completed with Burn apply result `0x0`. The installed GUI and helper SHA-256 values matched the package manifest:

| Installed file | SHA-256 |
| --- | --- |
| `planora.exe` | `F5685B20052CB4D01BB818235D25580C8179079AEBD54E6E9B98E5155541935B` |
| `planora-helper.exe` | `E85992640D17BE5DE6603E737E932FE27E452D81E7B971CE8767916476741365` |

The UI build identity reported `0.3.0-beta.7`, revision `fa80d15`, and protocol v4. The helper service was running after upgrade; administrator-owned authorization remained present. A normal installed launch opened at a 1180×790 client area at 125% scale, with no console window. A second normal launch activated the existing app; closing it completed UI shutdown in approximately 277 ms and removed its sensor child within approximately 600 ms. The independently installed helper remained running. The startup log recorded renderer initialization, first painted frame, and a connected helper.

An authenticated installed-service check observed 20 distinct fresh protocol-v4 telemetry snapshots after requesting the 100 ms collection interval. This is a short live smoke test, not a sustained performance run. The check script is [verify-installed-snapshots.ps1](../scripts/verify-installed-snapshots.ps1).

Beta.7 does not include the subsequent sensor/profiling changes. Repair, uninstall, Windows 10, accessibility, multi-scale, and long-run acceptance remain open. The beta.7 Setup and MSI are retained as rollback artifacts while beta.9 is evaluated. The initially built beta.8 package is superseded for internal testing because its profiling and optional-driver installation paths failed source review.
