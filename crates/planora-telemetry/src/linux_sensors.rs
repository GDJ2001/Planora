//! Read-only Linux hwmon readings. A missing tachometer is never a zero RPM.
use planora_core::{AvailabilityReason, HardwareCategory, Metric, SensorReading, SensorType};
use std::path::{Path, PathBuf};

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
}

fn owner(driver: &str) -> HardwareCategory {
    match driver {
        "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" => HardwareCategory::Cpu,
        "amdgpu" | "nouveau" | "i915" | "xe" => HardwareCategory::Gpu,
        "nvme" | "drivetemp" => HardwareCategory::Storage,
        _ => HardwareCategory::MotherboardCooling,
    }
}

fn stable_device_path(hwmon: &Path) -> PathBuf {
    hwmon.join("device").canonicalize().unwrap_or_else(|_| {
        hwmon
            .canonicalize()
            .unwrap_or_else(|_| hwmon.to_owned())
            .parent()
            .and_then(Path::parent)
            .unwrap_or(hwmon)
            .to_owned()
    })
}

pub fn sample_at(root: &Path, stamp: u64) -> Vec<SensorReading> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for entry in entries.flatten().take(256) {
        let path = entry.path();
        let driver = read_text(&path.join("name")).unwrap_or_else(|| "hwmon".into());
        let device_id = stable_device_path(&path).to_string_lossy().into_owned();
        let category = owner(&driver);
        let Ok(inputs) = std::fs::read_dir(&path) else {
            continue;
        };
        for input in inputs.flatten().take(256) {
            let filename = input.file_name().to_string_lossy().into_owned();
            let (kind, unit, divisor) =
                if filename.starts_with("fan") && filename.ends_with("_input") {
                    (SensorType::FanRpm, planora_core::UNIT_RPM, 1.0)
                } else if filename.starts_with("temp") && filename.ends_with("_input") {
                    let kind = match category {
                        HardwareCategory::Cpu => SensorType::CpuTemperatureCelsius,
                        HardwareCategory::Gpu => SensorType::GpuTemperatureCelsius,
                        HardwareCategory::Storage => SensorType::StorageTemperatureCelsius,
                        _ => SensorType::ThermalZoneCelsius,
                    };
                    (kind, planora_core::UNIT_CELSIUS, 1000.0)
                } else {
                    continue;
                };
            let prefix = filename.trim_end_matches("_input");
            let label = read_text(&path.join(format!("{prefix}_label")))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| prefix.to_owned());
            let fault = read_text(&path.join(format!("{prefix}_fault"))).as_deref() == Some("1");
            let raw = std::fs::read_to_string(input.path());
            let value = match raw.as_ref().ok().and_then(|s| s.trim().parse::<f32>().ok()) {
                Some(raw) if !fault && kind.is_plausible_value(raw / divisor) => {
                    Metric::available(raw / divisor, unit, "Linux hwmon", stamp)
                }
                Some(_) if fault => Metric::unavailable(
                    unit,
                    "Linux hwmon",
                    stamp,
                    AvailabilityReason::TemporarilyFailed("sensor reports a fault".into()),
                ),
                Some(_) => Metric::unavailable(
                    unit,
                    "Linux hwmon",
                    stamp,
                    AvailabilityReason::TemporarilyFailed(
                        "sensor returned an invalid value".into(),
                    ),
                ),
                None if raw
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied) =>
                {
                    Metric::unavailable(
                        unit,
                        "Linux hwmon",
                        stamp,
                        AvailabilityReason::PermissionDenied(
                            "sensor file is not readable by the helper".into(),
                        ),
                    )
                }
                None => Metric::unavailable(
                    unit,
                    "Linux hwmon",
                    stamp,
                    AvailabilityReason::TemporarilyFailed("sensor could not be read".into()),
                ),
            };
            // A fan connected to a motherboard controller is not necessarily the CPU fan.
            let hardware_category =
                if kind == SensorType::FanRpm && label.to_ascii_lowercase().contains("cpu") {
                    HardwareCategory::Cpu
                } else {
                    category
                };
            output.push(SensorReading {
                id: format!("hwmon:{device_id}:{prefix}"),
                device_id: device_id.clone(),
                device_name: driver.clone(),
                name: label,
                hardware_category,
                sensor_type: kind,
                scope: device_id.clone(),
                provider: "Linux hwmon".into(),
                value,
            });
        }
    }
    output.sort_by(|a, b| a.id.cmp(&b.id));
    output
}

pub fn sample(stamp: u64) -> Vec<SensorReading> {
    sample_at(Path::new("/sys/class/hwmon"), stamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_requires_real_tachometer_and_fault_is_not_zero() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("planora-hwmon-{}-{nonce}", std::process::id()));
        let chip = root.join("hwmon0");
        std::fs::create_dir_all(&chip).unwrap();
        std::fs::write(chip.join("name"), "nct6798\n").unwrap();
        std::fs::write(chip.join("fan1_label"), "CPU Fan\n").unwrap();
        assert!(sample_at(&root, 123).is_empty());
        std::fs::write(chip.join("fan1_input"), "1470\n").unwrap();
        let readings = sample_at(&root, 123);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].value.value, Some(1470.0));
        assert_eq!(readings[0].hardware_category, HardwareCategory::Cpu);
        std::fs::write(chip.join("fan1_fault"), "1\n").unwrap();
        assert!(sample_at(&root, 124)[0].value.value.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
