# Planora 0.3.0-beta.24 release evidence

Beta.24 fixes history loss under detail-series pressure and improves range switching.

## Root cause and change

- On machines reporting hundreds of GPU engine instances, detail history reached its 100,000-point budget. The worker evicted active sensor series and immediately re-created them at the next poll. This repeatedly erased their graphs and summaries, including after switching back to a short range.
- The selected resource and selected sensor trend group now receive retention priority. GPU engine histories are evicted before ordinary device and sensor histories. Evicted background series stay evicted until explicitly selected, avoiding repeated erase/re-create cycles. System histories remain outside the detail budget.
- The history worker publishes the newest complete immutable snapshot through a replaceable one-slot handoff. A slow UI no longer makes it clone the full history on every poll while an older snapshot occupies the slot.
- Selecting a sensor chooses its compatible trend group so a temperature reading does not initially display an unrelated empty power chart.

## Verification

- Focused regressions cover selected sensor retention under GPU engine pressure, recovery of an explicitly selected evicted engine, and 15-minute chart/table observations after changing ranges.
- `cargo fmt --all --check`, strict workspace Clippy, and workspace tests passed on Windows x86-64. Two pre-existing hardware-dependent tests remained ignored.
- Installed on the reference Windows PC over beta.22 on 2026-10-26. The Burn log reports `Apply complete, result: 0x0` and no restart. The installed UI, helper, and sensor broker hashes match the beta.24 manifest; UI and helper report commit `bdfa77d` and protocol v5.
- The `PlanoraHelper` service is running. An authorized protocol-v5 client received 20 distinct fresh snapshots; the oldest CPU reading was 603 ms. The final snapshot reported one measured Intel Iris Xe adapter, 279 GPU engine instances, and 49 sensor records (48 available). The installed UI and its sensor worker opened normally.
- The personal settings file remains present. A continuous installed 15-minute range interaction was not observed in this verification; range switching and budget pressure are covered by focused regressions.

## Package identity

The Windows package was built from commit `bdfa77d` with MSI ProductVersion `0.2.1024`. WiX validation and PE GUI-subsystem checks passed. Manifest comparisons were repeated independently:

| Artifact | SHA-256 |
| --- | --- |
| `Planora-0.3.0-beta.24-x64-Setup.exe` | `A2FE264C17D7ECE8F238CD4C3AD855491F76C0DA748DA0F33914BE7168B0CE09` |
| `Planora-0.3.0-beta.24-x64.msi` | `F2345237B29DC63BE1E5B3AE64EAD2F27AD5C7B4BC078EE522A6F5EFAC3A7BD2` |
| `planora.exe` | `DC3B5EFD3AD7B8AEB3544E938EB87F1BA56BE364BD233F91C64CD2BD578D782B` |
| `planora-helper.exe` | `ECA6DCDEB79503CAF74DCE099F927EA24FD5E7BB0981C16A7B6E899B2D0492C7` |
| `planora-sensor-broker.exe` | `F32C71AD5B89E1AE87C41E54975F1CF7C348C403C52B298E970B7A0D70F5B89E` |

The helper protocol and recording format are unchanged. This is an unsigned internal build.
