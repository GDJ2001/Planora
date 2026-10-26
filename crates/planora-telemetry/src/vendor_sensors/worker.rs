//! Unelevated child worker process for vendor sensor reading.
use super::{amd::AmdAdlxProvider, intel::IntelIgclProvider, nvidia::NvidiaNvmlProvider};
use planora_core::{Capability, CapabilityState, SensorReading};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum WorkerCommand {
    Probe,
    Poll,
    Exit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum WorkerResponse {
    Capabilities { capabilities: Vec<Capability> },
    Samples { sensors: Vec<SensorReading> },
    Error { message: String },
}

pub fn run_worker(expected_build_id: Option<&str>) {
    if let Some(expected) = expected_build_id {
        let actual = env!("CARGO_PKG_VERSION");
        if expected != actual {
            eprintln!("Sensor worker build ID mismatch: expected {expected}, actual {actual}");
            std::process::exit(1);
        }
    }
    let mut intel = IntelIgclProvider::try_init();
    let nvidia = NvidiaNvmlProvider::try_init();
    let amd = AmdAdlxProvider::try_init();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut input = stdin.lock();

    while let Ok(Some(line)) = super::read_line_bounded(&mut input, 64 * 1024) {
        let cmd: WorkerCommand = match serde_json::from_str(&line) {
            Ok(c) => c,
            Err(e) => {
                let resp = WorkerResponse::Error {
                    message: format!("invalid command JSON: {e}"),
                };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                let _ = stdout.flush();
                continue;
            }
        };

        match cmd {
            WorkerCommand::Probe => {
                let now = crate::unix_ms();
                let make = |name: &str,
                            provider: &str,
                            state: CapabilityState,
                            reason: Option<String>,
                            recovery: Option<String>| Capability {
                    name: name.into(),
                    scope: "GPU and platform sensors".into(),
                    provider: provider.into(),
                    state,
                    last_probed_at_ms: now,
                    reason,
                    recovery_action: recovery,
                };

                let mut caps = Vec::new();
                match &mut intel {
                    Ok(provider) => {
                        let has_reading = provider
                            .sample(crate::unix_ms())
                            .iter()
                            .any(|s| s.value.is_available());
                        caps.push(make(
                            "Intel IGCL telemetry", "Intel IGCL (Experimental)",
                            if has_reading { CapabilityState::Available } else { CapabilityState::Unsupported },
                            (!has_reading).then_some("Runtime initialized but this adapter exposes no supported IGCL measurement".into()),
                            None,
                        ));
                    }
                    Err((state, reason)) => caps.push(make(
                        "Intel IGCL telemetry",
                        "Intel IGCL (Experimental)",
                        *state,
                        Some(reason.clone()),
                        Some("Install or update Intel graphics driver".into()),
                    )),
                }

                match &nvidia {
                    Ok(provider) => {
                        let has_reading = provider
                            .sample(crate::unix_ms())
                            .iter()
                            .any(|s| s.value.is_available());
                        caps.push(make(
                            "NVIDIA NVML telemetry",
                            "NVIDIA NVML (Experimental)",
                            if has_reading {
                                CapabilityState::Available
                            } else {
                                CapabilityState::Unsupported
                            },
                            (!has_reading).then_some(
                                "Runtime initialized but no supported NVML sensor returned a value"
                                    .into(),
                            ),
                            None,
                        ));
                    }
                    Err((state, reason)) => {
                        let recovery = if *state == CapabilityState::NotApplicable {
                            None
                        } else {
                            Some("Install NVIDIA GPU driver".into())
                        };
                        caps.push(make(
                            "NVIDIA NVML telemetry",
                            "NVIDIA NVML (Experimental)",
                            *state,
                            Some(reason.clone()),
                            recovery,
                        ));
                    }
                }

                match &amd {
                    Ok(_) => caps.push(make(
                        "AMD ADLX telemetry",
                        "AMD ADLX (Experimental)",
                        CapabilityState::NotImplemented,
                        Some("ADLX runtime initialized, but metric interface sampling is not implemented in this build".into()),
                        None,
                    )),
                    Err((state, reason)) => {
                        let recovery = if *state == CapabilityState::NotApplicable {
                            None
                        } else {
                            Some("Install AMD Software: Adrenalin Edition".into())
                        };
                        caps.push(make(
                            "AMD ADLX telemetry",
                            "AMD ADLX (Experimental)",
                            *state,
                            Some(reason.clone()),
                            recovery,
                        ));
                    }
                }

                let resp = WorkerResponse::Capabilities { capabilities: caps };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                let _ = stdout.flush();
            }
            WorkerCommand::Poll => {
                let stamp = crate::unix_ms();
                let mut sensors = Vec::new();
                if let Ok(p) = intel.as_mut() {
                    sensors.extend(p.sample(stamp));
                }
                if let Ok(p) = nvidia.as_ref() {
                    sensors.extend(p.sample(stamp));
                }
                if let Ok(p) = amd.as_ref() {
                    sensors.extend(p.sample(stamp));
                }

                let resp = WorkerResponse::Samples { sensors };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                let _ = stdout.flush();
            }
            WorkerCommand::Exit => {
                break;
            }
        }
    }
}
