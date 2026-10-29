# Planora 0.3.0-beta.44 Windows and Linux Optimization Release Evidence

Beta.44 is an internal release built from clean commit `7dcd5ad` with Rust 1.99.1 and helper protocol v6. It delivers the approved Windows and Linux Optimization Plan, establishing zero-copy snapshot ingestion, immutable history sharing, downsampled plot cache re-use, 10,000-process table index sorting and 1Hz update throttling, adaptive background sampling, and automated diagnostic performance reports in JSON format.

## Package and installed identity

| Artifact | Size (Bytes) | SHA-256 |
| --- | --- | --- |
| `dist/Planora-0.3.0-beta.44-x64.msi` | 39,821,312 | `27F7185394C6772CE7D28101DA3778A4C4A2C6CA8ABED57C7B6E4945DC42CC57` |
| `dist/Planora-0.3.0-beta.44-x64-Setup.exe` | 43,718,169 | `F4337852F60E2C1646F9732FE6F86BC999A326E53A0976B07C2A484B64235F1D` |
| `dist/planora_0.3.0~beta.44-1_amd64.deb` | 7,063,686 | `22E5341FDF1B5C2A0D7A894DC8BB8131FAD24808D29E51115BCF1DC740E4FAD1` |
| `dist/planora-0.3.0-0.beta.44.x86_64.rpm` | 9,098,113 | `D4E5C6B597E347B80E39E47F1C5C9A6291ACC6574FB1876720B53EBBA3B1FAEE` |
| `dist/Planora-0.3.0-beta.44-x64.manifest.json` | 1,507 | `77E8DA87627CDCEC33060188D5DF5D687F84CE2E57A3B344076E8BFE03D6531F` |
| `dist/dependency-inventory.json` | 91,749 | `8D5F4FD667A7639F42FA468B8620F94FDB94303B11D0CE182B51236D7A94FE53` |

`dist/provenance.json` reports clean source commit `7dcd5ad22dd1a5c99a7fb1f5f89c7d4d29de59c1`; `dist/SHA256SUMS.txt` lists all 6 artifacts with verified SHA-256 sums.

## Measured Performance Latencies

### 1. UI Frame Times & Rendering
- **Steady-state UI Frame (Windows release)**:
  - count: 93 frames
  - p50: **0.52 ms**
  - p95: **0.92 ms** (Target $< 16.7\text{ ms}$)
  - p99: **1.03 ms**
- **10,000-Process Table Rendering**:
  - Steady UI frame duration: **7.20 ms** (Target $< 33.3\text{ ms}$)
  - Index-based sorting (`sort_process_indices` on `Vec<usize>`): passes cleanly without allocations across consecutive frames.
- **Chart Preparation**:
  - Downsampling generation cache hit: **0.0048 ms** ($4.8\text{ }\mu\text{s}$)

### 2. Telemetry Ingestion & Provider Cadence
- **Windows helper collection duration**: **23.78 ms** (Target $< 50\text{ ms}$ at 100 ms cadence)
  - `cpu`: 0.199 ms
  - `disks`: 0.154 ms
  - `gpu`: 0.572 ms
  - `memory`: 0.003 ms
  - `network`: 1.922 ms
  - `processes`: 3.194 ms
  - `refresh`: 16.89 ms
  - `sensors`: 0.0001 ms
- **Linux helper collection duration (Ubuntu 24.04 WSL)**: **19.37 ms**
  - `gpu`: 5.64 ms
  - `disks`: 5.12 ms
  - `refresh`: 4.77 ms
  - `processes`: 3.73 ms
  - `memory`: 0.032 ms
  - `network`: 0.030 ms
  - `cpu`: 0.022 ms
  - `sensors`: 0.003 ms

### 3. Adaptive Sampling
- **Focused rate**: User setting (100 ms).
- **Unfocused rate**: Debounced for 2 seconds, requests $\ge 500\text{ ms}$.
- **Minimized rate**: Requests $\ge 1,000\text{ ms}$ immediately.
- **Focus restoration**: Prompt return to saved rate without debouncing delay.
- **Staleness check**: Evaluates against `shared_copy.sampling_effective_ms`.
