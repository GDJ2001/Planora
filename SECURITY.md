# Planora Security Policy

The Planora development team takes security and privilege isolation very seriously. Planora is designed with strict privilege separation between the unelevated graphical user interface and its elevated background monitoring helper daemon.

---

## 1. Supported Versions

Security updates and patches are provided for the following versions:

| Version | Supported | Notes |
| :--- | :---: | :--- |
| **1.0.x** | :white_check_mark: | Current official production release track. |
| < 1.0.0 | :x: | Prerelease betas are unsupported. Users must upgrade to 1.0.0+. |

---

## 2. Core Security & Privilege Architecture

Planora enforces strict architectural security boundaries:

1. **Unelevated GUI Execution**:
   - The user-facing application (`planora`) **never requests or requires elevation**. It runs under standard user credentials.
   - User recording files, JSON exports, and local settings are accessed strictly from the user's local profile (`%APPDATA%` on Windows, `~/.config` on Linux).

2. **Privileged Helper Daemon (`planora-helper`)**:
   - Runs as `LocalSystem` (Windows) or root / dedicated system user (Linux/macOS).
   - **Never opens user-owned files or recordings**.
   - Enforces SID-specific ACLs on Windows named pipes (`FILE_GENERIC_READ | FILE_GENERIC_WRITE` granted solely to authorized user SIDs).
   - Validates Unix socket peer credentials (`SO_PEERCRED` / `euid`) against administrator-authorized entries in `/var/lib/planora/authorized-uids/`.

3. **IPC Protocol v7 Defenses**:
   - Strict 64 KiB incoming request size cap; 8 MiB response frame ceiling.
   - Enforced 2-second handshake deadlines and 5-second request timeouts to prevent client or server hanging.
   - Max 16 concurrent IPC connections (max 4 per unique caller).
   - Frames exceeding limits or containing non-finite/malformed floats are immediately dropped and rejected before memory allocation.

4. **PID-Reuse Mitigation on Affinity Mutation**:
   - On Windows, process affinity mutations revalidate the target process creation timestamp (`FILETIME`) through the opened handle before applying masks, preventing PID-reuse race conditions.
   - On Linux, cross-process affinity mutation remains disabled (`ErrorCode::Unsupported`) due to thread-level `sched_setaffinity` race hazards.

---

## 3. Reporting a Vulnerability

If you discover a security vulnerability in Planora, please **do not open a public GitHub issue**. Instead, follow our responsible disclosure procedure:

1. **Contact**: Email details to **[security@planora.org](mailto:security@planora.org)** or submit a private security advisory directly through GitHub via [Planora Security Advisories](https://github.com/GDJ2001/Planora/security/advisories/new).
2. **Details to Include**:
   - Type of issue (e.g., privilege escalation, IPC authorization bypass, denial of service, memory corruption).
   - Affected platform and operating system version (Windows, Ubuntu, Fedora, macOS).
   - Step-by-step reproduction instructions, Proof-of-Concept (PoC) code, or crash trace.
   - Potential impact and threat model assessment.

### Response Timeline & SLA
- **Initial Acknowledgment**: Within 48 hours of receipt.
- **Triage & Assessment**: Within 5 business days with confirmation of severity.
- **Remediation & Patching**: Security patches are prioritized and released within 14 to 30 days depending on complexity.
- **Public Disclosure**: Coordinated disclosure after the security release is published and available to users.

---

## 4. Out of Scope

The following scenarios are considered out of scope for security vulnerability reports:
- Attacks requiring physical access to an unlocked developer workstation.
- Attacking an unelevated GUI process from an account that already possesses full `Administrator` / `LocalSystem` / `root` privileges.
- Standard hardware sensor limits when physical hardware sensors or vendors do not supply MSRs or thermal data.
- Theoretical vulnerabilities without demonstrable exploitability or memory corruption.
