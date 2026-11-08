# Planora v1.0.0 Release Catalog & Artifact Verification Matrix

**Release Version**: 1.0.0  
**Tag**: [`v1.0.0`](https://github.com/GDJ2001/Planora/releases/tag/v1.0.0)  
**Source Commit**: `586f35e9e8f2372b604f170f0b6b82778b59615b`  
**Working Tree Clean**: Yes (`source_clean: true`)  
**Publication Date**: November 8, 2026  

---

## 1. Verified Release Packages & Cryptographic Hashes

All release packages have been compiled with the pinned Rust 1.99.1 toolchain and verified for cryptographic parity.

| Artifact Name | Platform / Target | Size (Bytes) | SHA-256 Digest | Direct Download URL |
| :--- | :--- | :---: | :--- | :--- |
| **`Planora-1.0.0-x64-Setup.exe`** | Windows 10/11 x86-64 | 43,089,955 | `ed6852a044a59ed06cfe687fa1ed621fd82864f244087308e9e0b305a6345e81` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64-Setup.exe) |
| **`Planora-1.0.0-x64.msi`** | Windows 10/11 x86-64 | 39,190,528 | `8f5e2d2f3b1f7432308185873cd683d3c56adc2562668d768d63f75d8b54ad70` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.msi) |
| **`Planora-1.0.0-x64.manifest.json`** | Windows 10/11 x86-64 | 1,058 | `b245b99a9d46390345300d7d766ee92e278fa877ad3c5a11e08158170a3ee32e` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.manifest.json) |
| **`dependency-inventory.json`** | Multi-Platform (413 crates) | 91,701 | `2e4a6f1b9ec2d2c7d4506f93d007ffbe822c4a53d56bd3eac537827c634aeb4c` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/dependency-inventory.json) |
| **`SHA256SUMS.txt`** | Multi-Platform Manifest | 376 | (Self-referential manifest) | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/SHA256SUMS.txt) |

---

## 2. Embedded Binary Verification (Windows x64)

The Windows MSI image contains four internal binaries, validated directly against their embedded build identities:

| Binary | Subsystem | SHA-256 Digest |
| :--- | :--- | :--- |
| `planora.exe` | Windows GUI (Subsystem 2) | `06a2c78e4483280cfef870a57e682565dc2c104007d581a9117fc97a06d3a922` |
| `planora-helper.exe` | Console / Service (Subsystem 3) | `6b0e2edd70f3359014b0c7ec073b066e90403b8173c3a4636a7e0739a692a33a` |
| `planora-sensor-broker.exe` | Console / Worker (Subsystem 3) | `ddfd60cf44aa662286ec558fc9f10f38d5fcbe8a711c6fb42fc43d29707d1ff5` |
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
