# Planora Repository File System Hygiene & Codebase Audit Report

**Date**: November 8, 2026  
**Target Release**: Planora v1.0.0  
**Scope**: Entire repository file tree, git-tracked inventory, build pipelines, package configurations, and dependency definitions.

---

## 1. Executive Summary

In preparation for the v1.0.0 official release and transitioning the Planora repository to open-source public visibility, a complete repository-wide audit was conducted. The audit verified:
1. Absence of stray build artifacts, compiled bytecode, intermediate object files, or debug symbols in Git tracking.
2. Absence of hardcoded developer local machine absolute paths or environment-specific usernames in public configuration and test artifacts.
3. Zero accidentally tracked secrets, private keys, certificates, or credentials.
4. Reinforcement of `.gitignore` rules with strict anchoring and protection for build wrappers.
5. Legal compliance with Apache License 2.0 and attribution parity across all supported desktop platforms (Windows, Linux, macOS) and mobile distribution paths.

---

## 2. File Tree & Tracking Audit

### 2.1 Git-Tracked File Inventory
- **Total Tracked Files**: 273 files.
- **Bytecode & Binaries**: Verified zero `.pyc`, `.pyo`, `.class`, `.obj`, `.o`, `.pdb`, `.wixpdb`, `.dll`, or `.exe` files tracked in Git history (with the sole exception of the official, signed PawnIO kernel driver installer `packaging/windows/deps/PawnIO_setup.exe`, validated cryptographically against SHA-256 `1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032` and verified by Authenticode signature).
- **Stray Outputs & Intermediate Directories**: Confirmed that `target/`, `dist/`, `bin/`, `obj/`, `build/`, `.wix/`, and `node_modules/` are untracked and strictly ignored.

### 2.2 Local Machine Path Sanitization
- **Audit Query**: Executed regex search across all tracked files for developer home directories (`C:\Users\...`, `/home/...`).
- **Remediated Finding**: In `docs/screenshots/native/beta29/manifest.json`, the field `"binary"` previously contained a local developer worktree path (`C:\Users\GDJ\.codex\worktrees\startup-auth-beta27\Planora\target\release\planora.exe`).
- **Resolution**: Sanitized the path to a portable relative path (`target\release\planora.exe`).
- **Zero Developer Usernames**: Confirmed 0 occurrences of developer usernames or private machine identities across all source files, documentation, and workflows.

### 2.3 Secret & Credential Scanning
- Scanned tracked files for standard secret formats, private key headers (`BEGIN RSA PRIVATE KEY`, `BEGIN PRIVATE KEY`), AWS/GitHub API tokens, `.env` files, and keystore binaries.
- **Result**: Zero secret leaks detected. All CI workflows utilize standard GitHub Actions secrets or unauthenticated public package endpoints.

---

## 3. Gitignore Rules Reinforcement

The root `.gitignore` was reinforced and anchored to prevent regressions:
- **Build & Packaging Outputs**: Anchored `/target/`, `/dist/`, `/build/`, `/bin/`, `/obj/`, `/.wix/`, and `*.wixpdb`.
- **Node & Dependencies**: Anchored `/node_modules/`.
- **Python Bytecode & Virtual Environments**: Anchored `__pycache__/`, `*.py[cod]`, `*$py.class`, `.venv/`, `env/`, `venv/`.
- **Operating System & Editor Files**: Added `.DS_Store`, `Thumbs.db`, `desktop.ini`, `.vscode/`, `.idea/`, `*.swp`, `*.swo`, `*~`.
- **Local State & Logs**: Anchored `/state/`, `*.log`, `*.tmp`.
- **Fuzzing Artifacts**: Anchored `/fuzz/artifacts/` and `/fuzz/corpus/`.
- **Credential & Key Guards**: Anchored `*.pem`, `*.key`, `*.pfx`, `*.p12`, `*.keystore`, `.env`, `.env.*`.
- **Preserved Build Wrappers**: Explicitly allowed `!gradle/wrapper/gradle-wrapper.jar`, `!gradlew`, `!gradlew.bat` to guarantee cross-platform build wrapper functionality.

---

## 4. Architectural Boundaries & Recommendations

1. **Privilege Separation (GUI vs Helper)**:
   - `planora` UI binary runs unelevated (standard user privileges) and renders via `eframe`/`egui` 0.37 with `wgpu`.
   - `planora-helper` runs elevated (`LocalSystem` on Windows, root / system service on Linux/macOS) and communicates strictly over authenticated local IPC (`planora-v2` named pipe or `/run/planora/planora-v2.sock`).
   - The helper daemon enforces SID-specific ACLs (Windows) and UID credential verification against `/var/lib/planora/authorized-uids/<uid>` (Linux).

2. **IPC Protocol v7 & Safety Bounds**:
   - Request buffer cap: 64 KiB.
   - Response buffer cap: 8 MiB.
   - Deadlines: 2-second handshake deadline, 5-second request deadline.
   - Rejection of oversized or malformed frames before deserialization.

3. **Telemetry Truthfulness & Physical Plausibility**:
   - Explicit `Metric<T>` representation preserving unavailability (`AvailabilityReason::Unsupported`, `PermissionDenied`, `WarmingUp`, `Stale`).
   - Two-layer plausibility sanitization (verifying finite floats, percentage bounds [0.0..100.0], memory total consistency, non-contradictory metrics). Zero synthetic zero fabrication.

4. **Process Affinity Mutation Safety**:
   - Windows affinity updates re-validate `FILETIME` process creation identity on the open handle to eliminate PID-reuse race conditions.
   - Linux process affinity writing remains disabled (`ErrorCode::Unsupported`) until atomic task-tree synchronization can be guaranteed.

---

## 5. Verification Sign-Off

- **Code Formatting Check**: `cargo fmt --all -- --check` — Passed (0 issues).
- **Workspace Linting**: `cargo clippy --workspace --all-targets --locked -- -D warnings` — Passed (0 warnings).
- **Workspace Unit & Integration Test Suite**: `cargo test --workspace --locked` — Passed (103/103 tests in planora-ui, all crates green).
- **Brand & Icon Verification**: `python scripts/export-icons.py --check` — Passed (all icon frames and alpha channels valid).
- **Status**: Repository hygiene is verified and production-ready for public v1.0.0 release.
