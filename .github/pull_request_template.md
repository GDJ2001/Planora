## Description

Briefly describe the change and its rationale. Reference any relevant GitHub issues.

Fixes #(issue)

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Performance optimization / Refactoring
- [ ] Documentation update

## Affected Modules

- [ ] `crates/planora-core` (IPC contracts, validation, availability states)
- [ ] `crates/planora-topology` (CPUID, OS topology)
- [ ] `crates/planora-telemetry` (Sampling, PDH, sysfs, eBPF)
- [ ] `crates/planora-bench` (Micro-architectural probes)
- [ ] `crates/planora-helper` (Privileged daemon, local IPC authorization)
- [ ] `crates/planora-ui` (Native egui/eframe GUI)
- [ ] `crates/planora-sensor-broker` (Windows C#/.NET sensor broker)
- [ ] `packaging/` or CI workflows

## Verification Checklist

Please run and confirm all verification gates pass locally before submitting:

- [ ] `cargo fmt --all -- --check` passes cleanly
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` produces zero warnings
- [ ] `cargo test --workspace --locked` passes all unit and integration tests
- [ ] `cargo build --release --bins --locked` builds successfully
- [ ] `python scripts/export-icons.py --check` passes icon validation (if assets modified)
- [ ] No synthetic zero metrics introduced (`Metric::Unavailable` used for unavailable measurements)
- [ ] GUI remains strictly unelevated
