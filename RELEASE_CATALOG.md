# Planora v1.0.0 Release Catalog & Artifact Verification Matrix

**Release Version**: 1.0.0  
**Tag**: [`v1.0.0`](https://github.com/GDJ2001/Planora/releases/tag/v1.0.0)  
**Source Commit**: `d54306eeb9172aad8e8eaa30ce6e31dc61a9395e`  
**Working Tree Clean**: Yes (`source_clean: true`)  
**Publication Date**: November 8, 2026  

---

## 1. Verified Release Packages & Cryptographic Hashes

All release packages have been compiled with the pinned Rust 1.99.1 toolchain and verified for cryptographic parity.

| Artifact Name | Platform / Target | Size (Bytes) | SHA-256 Digest | Direct Download URL |
| :--- | :--- | :---: | :--- | :--- |
| **`Planora-1.0.0-x64-Setup.exe`** | Windows 10/11 x86-64 | 43,087,047 | `215c9319bf68db357360bbd35ec72e3fda267d1ce0732648ed76b2a0de191fdf` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64-Setup.exe) |
| **`Planora-1.0.0-x64.msi`** | Windows 10/11 x86-64 | 39,186,432 | `e807462c089c37e8d85f54177e345021e3bd6b48372a264b72c6064476e85e1d` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.msi) |
| **`Planora-1.0.0-x64.manifest.json`** | Windows 10/11 x86-64 | 1,058 | `35437829cc3832a46719f39a2c5173c39238e151352a84a779ebd34176e4d527` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.manifest.json) |
| **`dependency-inventory.json`** | Multi-Platform (413 crates) | 91,701 | `2e4a6f1b9ec2d2c7d4506f93d007ffbe822c4a53d56bd3eac537827c634aeb4c` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/dependency-inventory.json) |
| **`SHA256SUMS.txt`** | Multi-Platform Manifest | 376 | (Self-referential manifest) | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/SHA256SUMS.txt) |

---

## 2. Embedded Binary Verification (Windows x64)

The Windows MSI image contains four internal binaries, validated directly against their embedded build identities:

| Binary | Subsystem | SHA-256 Digest |
| :--- | :--- | :--- |
| `planora.exe` | Windows GUI (Subsystem 2) | `ad4be8dbba98605a3f3f53e8efbf61129e1c0bd60dea514f8eb5e46f023459af` |
| `planora-helper.exe` | Console / Service (Subsystem 3) | `ec41b115cec2d2d47b4eb9b565097998103a6d4aed5c57b5483b49ed3f7b5932` |
| `planora-sensor-broker.exe` | Console / Worker (Subsystem 3) | `39e3ccdd7f5b09fdf38bac9213093117e54ce8aca18e7651563154355b61e31d` |
| `PawnIO_setup.exe` | Signed Driver Setup (namazso) | `1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032` |

---

## 3. Cryptographic Verification Instructions

To verify downloaded files against the official manifest:

### Linux / macOS
```sh
curl -LO https://github.com/GDJ2001/Planora/releases/download/v1.0.0/SHA256SUMS.txt
sha256sum -c SHA256SUMS.txt --ignore-missing
```

### Windows (PowerShell)
```powershell
Invoke-WebRequest -Uri "https://github.com/GDJ2001/Planora/releases/download/v1.0.0/SHA256SUMS.txt" -OutFile "SHA256SUMS.txt"
Get-FileHash -Algorithm SHA256 Planora-1.0.0-x64-Setup.exe, Planora-1.0.0-x64.msi
```
