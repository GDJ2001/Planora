# Planora v1.0.0 Release Procedure Runbook

**Document**: `doc/release/RELEASE_PROCEDURE_V1.md`  
**Target Release**: Planora v1.0.0 (Official Production Release)  
**Standard**: Reproducible, Cryptographically Auditable Multi-Platform Publication  

---

## 1. Overview & Release Pre-Flight Gates

This runbook outlines the exact step-by-step procedure for building, verifying, signing, packaging, cataloging, and publishing **Planora v1.0.0** to GitHub Releases and packaging ecosystems.

### 1.1 Pre-Flight Gate Checklist
Every gate must pass cleanly before cutting a Git release tag:
1. **Toolchain Version**: Rust `1.99.1` (`rustc 1.99.1`), .NET SDK 9.0/10.0, WiX Toolset `5.0.2`.
2. **Clean Working Tree**: `git status --porcelain` returns 0 output.
3. **Format**: `cargo fmt --all -- --check` returns exit code 0.
4. **Clippy Lints**: `cargo clippy --workspace --all-targets --locked -- -D warnings` returns exit code 0.
5. **Unit & Integration Tests**: `cargo test --workspace --locked` passes 100%.
6. **Brand Asset Check**: `python scripts/export-icons.py --check` passes.

---

## 2. Multi-Platform Build & Packaging Procedures

### 2.1 Windows Build Procedure (MSI & Burn Setup Bundle)
Executed on Windows 10/11 x86-64 using PowerShell:
```powershell
# 1. Ensure WiX v5 and extensions are installed
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2 WixToolset.Util.wixext/5.0.2

# 2. Execute release packaging script
./packaging/windows/Build-Planora.ps1

# 3. Generate release inventory and SHA256 checksums
python scripts/release_inventory.py
```
This produces:
- `dist/Planora-1.0.0-x64-Setup.exe` (Self-extracting branded installer bundle)
- `dist/Planora-1.0.0-x64.msi` (Enterprise MSI with authenticated helper service)
- `dist/Planora-1.0.0-x64.manifest.json` (Binary cryptographic manifest)
- `dist/dependency-inventory.json`
- `dist/SHA256SUMS.txt`
- `dist/provenance.json`

### 2.2 Linux Debian / Ubuntu Package Procedure
Executed on Ubuntu 24.04 or 26.04:
```sh
# 1. Install build requirements
sudo apt-get update && sudo apt-get install -y \
  libx11-dev libxkbcommon-dev libwayland-dev \
  libpolkit-gobject-1-dev libbpf-dev libelf-dev zlib1g-dev clang

# 2. Build binaries and Debian package
cargo build --release --bins --locked
bash packaging/linux/build-deb.sh
python3 scripts/release_inventory.py
```
This produces:
- `dist/planora_1.0.0-1_amd64.deb`

### 2.3 Linux Fedora / RPM Package Procedure
Executed in Fedora 44 container or host:
```sh
# 1. Install build requirements
dnf install -y gcc clang cmake pkgconf-pkg-config libX11-devel libxkbcommon-devel \
  wayland-devel rpm-build curl tar gzip python3 libbpf-devel elfutils-libelf-devel zlib-devel

# 2. Build binaries and RPM package
cargo build --release --bins --locked
bash packaging/linux/build-rpm.sh
python3 scripts/release_inventory.py
```
This produces:
- `dist/planora-1.0.0-1.fc44.x86_64.rpm`

### 2.4 macOS Package Procedure (Apple Silicon)
Executed on macOS 13+ (Apple Silicon runner `macos-15`):
```sh
cargo build --release --bins --target aarch64-apple-darwin --locked
bash packaging/macos/build-package.sh
python3 scripts/release_inventory.py
```
This produces:
- `dist/Planora-1.0.0-arm64.pkg`
- `dist/Planora-1.0.0-arm64-app.zip`

---

## 3. Cryptographic Verification & Manifest Cataloging

Before publication, compute and verify SHA-256 digests:
```powershell
Get-FileHash -Algorithm SHA256 dist/*.*
```
Ensure all artifacts match entries in `RELEASE_MANIFEST.json`, `RELEASE_CATALOG.md`, and `dist/SHA256SUMS.txt`.

---

## 4. GitHub Release Publication Procedure

Once all artifacts are compiled and cryptographically verified:

```sh
# 1. Commit and push any release preparation changes
git add .
git commit -m "chore(release): prepare Planora v1.0.0 release artifacts and documentation"
git push origin main

# 2. Create signed Git release tag
git tag -a v1.0.0 -m "Planora v1.0.0 Official Release"
git push origin v1.0.0

# 3. Publish GitHub Release via GitHub CLI
gh release create v1.0.0 \
  --title "Planora v1.0.0 — Official Production Release" \
  --notes-file doc/release/V1_0_0_RELEASE_NOTES.md \
  dist/Planora-1.0.0-x64-Setup.exe \
  dist/Planora-1.0.0-x64.msi \
  dist/Planora-1.0.0-x64.manifest.json \
  dist/SHA256SUMS.txt \
  dist/dependency-inventory.json \
  dist/provenance.json
```

---

## 5. Post-Release Verification & CI Remediation

1. Monitor triggered GitHub Actions workflows:
   ```sh
   gh run list --branch main -L 5
   gh run watch <run-id>
   ```
2. Download CI build artifacts from Linux and macOS runners and verify checksum parity with local digests.
3. Confirm all GitHub Release asset download links resolve (HTTP 200) without 404 dead links.
4. Verify repository is clean and ready for public visibility.
