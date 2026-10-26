# Planora 0.3.0-beta.13 internal package evidence

Beta.13 fixes Windows GPU adapter discovery. The DXGI factory interface ID was incorrect, so `CreateDXGIFactory1` returned `E_NOINTERFACE` and the installed beta.12 helper published no adapters even while PDH published GPU engine counters. The DXGI adapter-memory interface ID was also incorrect. Both now match the Windows SDK; software render adapters are excluded from the physical-GPU list, and the persisted `gpu:none` selection moves to the discovered adapter. Source revision `f077839` uses helper protocol v5 and MSI ProductVersion `0.2.1013`.

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.13-x64-Setup.exe` | `5907672777CC8C5A730A666E36A387CFCC11E573618A4AF6D0AAB28D83E17C16` |
| `Planora-0.3.0-beta.13-x64.msi` | `5D0372A8AEE52CF0D63D1E29536DF99E6965CF0EEFFF69A8A2745DCFE1EDB218` |
| `planora.exe` | `CD65B43404367F5C29C96F44239C3D559523A519DA808DA5E052C34978A15C16` |
| `planora-helper.exe` | `BC50CBE2ED2F0D1C7691861C875179058C01ACFC7692C710268910C122C5E8DD` |
| `planora-sensor-broker.exe` | `9DB73E06F48B7C7DF28A8EBA0B693CD51C67712182F5019FC9CECCF1DDA66CF8` |

The Setup/MSI hashes were independently compared with the package manifest, and all three installed binary hashes match it. The build script validated WiX output, Windows GUI subsystem, and the pinned PawnIO signature/hash. Formatting, strict workspace Clippy, the Windows GPU tests, and workspace library/binary tests passed; two unrelated slow UI tests were excluded from the workspace run.

On this Windows 11 x64 PC, beta.12's installed helper returned **zero GPU adapters** and 244 GPU engine instances in a three-snapshot check. A direct native beta.13 test detected `Intel(R) Iris(R) Xe Graphics`, LUID `55927`, and a matching measured PDH engine. The user completed the beta.13 Setup upgrade with Advanced sensors selected; the Burn log reports `Apply complete, result: 0x0`, with no restart. The installed UI reports beta.13, `f077839`, release profile, and protocol v5. The helper is running, administrator-owned authorization remains present, and the pre-upgrade settings hash `97301B829E6F771FA87BCA1E60E96066495AAEBC7B8023D77FDE4F92F31AB539` was unchanged immediately after upgrade. A subsequent GPU capture intentionally changed the saved resource/theme preference.

The installed LocalSystem helper returned 20 distinct fresh protocol-v5 snapshots. It reported **one Intel Iris Xe adapter**, LUID `55927`, 228 GPU engine instances, 147 engine entries matching that adapter, and one measured adapter. The same check found 46 available advanced-sensor readings, including CPU package power and 11 CPU temperatures. Engine-instance counts vary with workload and time; these numbers are observations, not fixed hardware characteristics.

The [installed dark GPU capture](screenshots/native/beta13_gpu_installed_dark_1180x790.png) is a native egui framebuffer at 1475×988 pixels from a requested 1180×790 logical window at 125% effective scale. It shows the Intel adapter selected, a live 13.3% reading, an observed history segment, and dedicated/shared memory bars. The screenshot was taken during history warm-up (2/60 seconds coverage), so it does not validate a full 60-second trace. Visual review found no clipping or unavailable GPU message in this capture.

This verifies adapter detection and presentation on this machine. Multi-adapter, sleep/removal, other vendor hardware, independent memory-counter comparison, full chart-history review, Windows 10 testing, installer repair/uninstall, accessibility, long-run performance, signing, and CI execution remain open. Beta.13 is unsigned and remains an internal build.
