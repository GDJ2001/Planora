//! Unelevated child worker process for vendor sensor reading.
use super::{amd::AmdAdlxProvider, intel::IntelIgclProvider, nvidia::NvidiaNvmlProvider};
use planora_core::{Capability, CapabilityState, SensorReading};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

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

pub fn run_worker(_expected_build_id: Option<&str>) {
    let now = crate::unix_ms();
    let mut intel = IntelIgclProvider::try_init();
    let nvidia = NvidiaNvmlProvider::try_init();
    let amd = AmdAdlxProvider::try_init();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut lines = stdin.lock().lines();

    while let Some(Ok(line)) = lines.next() {
        if line.len() > 64 * 1024 {
            let resp = WorkerResponse::Error {
                message: "line exceeds 64 KiB limit".into(),
            };
            let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
            let _ = stdout.flush();
            continue;
        }

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
                match &intel {
                    Ok(_) => caps.push(make(
                        "Intel IGCL telemetry",
                        "Intel IGCL (Experimental)",
                        CapabilityState::Available,
                        None,
                        None,
                    )),
                    Err((state, reason)) => caps.push(make(
                        "Intel IGCL telemetry",
                        "Intel IGCL (Experimental)",
                        *state,
                        Some(reason.clone()),
                        Some("Install or update Intel graphics driver".into()),
                    )),
                }

                match &nvidia {
                    Ok(_) => caps.push(make(
                        "NVIDIA NVML telemetry",
                        "NVIDIA NVML (Experimental)",
                        CapabilityState::Available,
                        None,
                        None,
                    )),
                    Err((state, reason)) => caps.push(make(
                        "NVIDIA NVML telemetry",
                        "NVIDIA NVML (Experimental)",
                        *state,
                        Some(reason.clone()),
                        Some("Install NVIDIA GPU driver".into()),
                    )),
                }

                match &amd {
                    Ok(_) => caps.push(make(
                        "AMD ADLX telemetry",
                        "AMD ADLX (Experimental)",
                        CapabilityState::Available,
                        None,
                        None,
                    )),
                    Err((state, reason)) => caps.push(make(
                        "AMD ADLX telemetry",
                        "AMD ADLX (Experimental)",
                        *state,
                        Some(reason.clone()),
                        Some("Install AMD Software: Adrenalin Edition".into()),
                    )),
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
