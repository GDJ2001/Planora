# Planora v1.0.0 Release Catalog & Artifact Verification Matrix

**Release Version**: 1.0.0  
**Tag**: [`v1.0.0`](https://github.com/GDJ2001/Planora/releases/tag/v1.0.0)  
**Source Commit**: `7071db32dd65874015256ffe32d0518b34005bcc`  
**Working Tree Clean**: Yes (`source_clean: true`)  
**Publication Date**: November 8, 2026  

---

## 1. Verified Release Packages & Cryptographic Hashes

All release packages have been compiled with the pinned Rust 1.99.1 toolchain and verified for cryptographic parity.

| Artifact Name | Platform / Target | Size (Bytes) | SHA-256 Digest | Direct Download URL |
| :--- | :--- | :---: | :--- | :--- |
| **`Planora-1.0.0-x64-Setup.exe`** | Windows 10/11 x86-64 | 43,089,751 | `e62927f575e65be5cb7c93c2782feef1c509d2bdf8b6f243f2892be9b3215b4b` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64-Setup.exe) |
| **`Planora-1.0.0-x64.msi`** | Windows 10/11 x86-64 | 39,190,528 | `883e0360f3433b47f35339f3d51bf257eca49571ebcce79c7430f8da91dcb7e5` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.msi) |
| **`Planora-1.0.0-x64.manifest.json`** | Windows 10/11 x86-64 | 1,058 | `cad60fc431a0be809d0bae144ab72fdd90a15f4a89f0fa4c892ad2bc9c093927` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-x64.manifest.json) |
| **`planora_1.0.0-1_amd64.deb`** | Linux (Ubuntu / Debian x86-64) | 7,437,760 | `e12db6bdeaa94b953ed9f4aa2a17b57032ed76607b251561d4fb84724187e18a` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/planora_1.0.0-1_amd64.deb) |
| **`planora-1.0.0-1.fc44.x86_64.rpm`** | Linux (Fedora / RHEL x86-64) | 7,427,041 | `146907b257f41959f661363c4ea74d90835274763c3a02bab603b8d96cb911fa` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/planora-1.0.0-1.fc44.x86_64.rpm) |
| **`Planora-1.0.0-arm64.pkg`** | macOS Apple Silicon (arm64) | 6,508,021 | `ecc10c30c6f2382ee44524cb5337c70f3987eca4226a85757037b0cf3ff67b4e` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-arm64.pkg) |
| **`Planora-1.0.0-arm64-app.zip`** | macOS Apple Silicon (arm64) | 6,505,327 | `9d1013185a5fd1550d394f12128657c472247ac2401e047b540e7727fd8b9cfb` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/Planora-1.0.0-arm64-app.zip) |
| **`dependency-inventory.json`** | Multi-Platform (413 crates) | 91,701 | `2e4a6f1b9ec2d2c7d4506f93d007ffbe822c4a53d56bd3eac537827c634aeb4c` | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/dependency-inventory.json) |
| **`SHA256SUMS.txt`** | Multi-Platform Manifest | 754 | (Cryptographic manifest of all 8 release packages) | [Download](https://github.com/GDJ2001/Planora/releases/download/v1.0.0/SHA256SUMS.txt) |

---

## 2. Embedded Binary Verification (Windows x64)

The Windows MSI image contains four internal binaries, validated directly against their embedded build identities:

| Binary | Subsystem | SHA-256 Digest |
| :--- | :--- | :--- |
| `planora.exe` | Windows GUI (Subsystem 2) | `005f68b00b306f4e956c70b510e667c60b5cad7758a7f20712856e73118ac223` |
| `planora-helper.exe` | Console / Service (Subsystem 3) | `089f8588ed3dc899031f97409df34813c60c8c319b1b43eaac67b3b3dcc87888` |
| `planora-sensor-broker.exe` | Console / Worker (Subsystem 3) | `23196e7c71fafd5f5e9998330035edd07db543839d994836c82242dca18a196f` |
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
