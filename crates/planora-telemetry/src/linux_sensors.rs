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
        "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" | "fam15h_power" => {
            HardwareCategory::Cpu
        }
        "amdgpu" | "nouveau" | "i915" | "xe" | "radeon" => HardwareCategory::Gpu,
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
            let (kind, unit, divisor, prefix) =
                if filename.starts_with("fan") && filename.ends_with("_input") {
                    (
                        SensorType::FanRpm,
                        planora_core::UNIT_RPM,
                        1.0,
                        filename.trim_end_matches("_input").to_owned(),
                    )
                } else if filename.starts_with("temp") && filename.ends_with("_input") {
                    let kind = match category {
                        HardwareCategory::Cpu => SensorType::CpuTemperatureCelsius,
                        HardwareCategory::Gpu => SensorType::GpuTemperatureCelsius,
                        HardwareCategory::Storage => SensorType::StorageTemperatureCelsius,
                        _ => SensorType::ThermalZoneCelsius,
                    };
                    (
                        kind,
                        planora_core::UNIT_CELSIUS,
                        1000.0,
                        filename.trim_end_matches("_input").to_owned(),
                    )
                } else if filename.starts_with("power")
                    && (filename.ends_with("_average") || filename.ends_with("_input"))
                {
                    let kind = match category {
                        HardwareCategory::Gpu => SensorType::GpuBoardPowerWatts,
                        _ => SensorType::CpuPackagePowerWatts,
                    };
                    let pref = if filename.ends_with("_average") {
                        filename.trim_end_matches("_average")
                    } else {
                        filename.trim_end_matches("_input")
                    };
                    (kind, planora_core::UNIT_WATTS, 1_000_000.0, pref.to_owned())
                } else if filename.starts_with("freq") && filename.ends_with("_input") {
                    // hwmon clocks in Hertz -> MHz
                    (
                        SensorType::ClockMhz,
                        planora_core::UNIT_MHZ,
                        1_000_000.0,
                        filename.trim_end_matches("_input").to_owned(),
                    )
                } else if filename.starts_with("in") && filename.ends_with("_input") {
                    // hwmon voltages in millivolts -> Volts across CPU, Super I/O, and GPU
                    (
                        SensorType::VoltageVolts,
                        planora_core::UNIT_VOLTS,
                        1000.0,
                        filename.trim_end_matches("_input").to_owned(),
                    )
                } else if filename.starts_with("curr") && filename.ends_with("_input") {
                    // hwmon current in milliamperes -> Amperes
                    (
                        SensorType::CurrentAmperes,
                        planora_core::UNIT_AMPERES,
                        1000.0,
                        filename.trim_end_matches("_input").to_owned(),
                    )
                } else if filename.starts_with("pwm")
                    && (filename.ends_with("_input") || !filename.contains('_'))
                {
                    // hwmon PWM duty cycle: raw 0..255 -> 0..100%
                    let pref = filename.trim_end_matches("_input").to_owned();
                    (
                        SensorType::LoadPercent,
                        planora_core::UNIT_PERCENT,
                        2.55,
                        pref,
                    )
                } else {
                    continue;
                };

            let label = read_text(&path.join(format!("{prefix}_label")))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| prefix.clone());
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

            let lower_label = label.to_ascii_lowercase();
            let hardware_category = if (kind == SensorType::FanRpm
                && (lower_label.contains("cpu") || lower_label.contains("pump")))
                || (kind == SensorType::VoltageVolts
                    && (lower_label.contains("vcore") || lower_label.contains("cpu")))
            {
                HardwareCategory::Cpu
            } else {
                category
            };

            let scope = if kind == SensorType::LoadPercent {
                "Cooling".to_string()
            } else if hardware_category == HardwareCategory::Cpu {
                "CPU".to_string()
            } else if hardware_category == HardwareCategory::Gpu {
                "GPU".to_string()
            } else {
                device_id.clone()
            };

            output.push(SensorReading {
                id: format!("hwmon:{device_id}:{prefix}"),
                device_id: device_id.clone(),
                device_name: driver.clone(),
                name: label,
                hardware_category,
                sensor_type: kind,
                scope,
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

    #[test]
    fn amd_gpu_clocks_and_voltage_keep_units_and_gpu_scope() {
        let root = std::env::temp_dir().join(format!("planora-amd-hwmon-{}", std::process::id()));
        let chip = root.join("hwmon0");
        std::fs::create_dir_all(&chip).unwrap();
        std::fs::write(chip.join("name"), "amdgpu\n").unwrap();
        std::fs::write(chip.join("freq1_input"), "1800000000\n").unwrap();
        std::fs::write(chip.join("in0_input"), "925\n").unwrap();
        let readings = sample_at(&root, 77);
        assert_eq!(readings.len(), 2);
        let clock = readings
            .iter()
            .find(|s| s.sensor_type == SensorType::ClockMhz)
            .unwrap();
        let voltage = readings
            .iter()
            .find(|s| s.sensor_type == SensorType::VoltageVolts)
            .unwrap();
        assert_eq!(clock.value.value, Some(1800.0));
        assert_eq!(voltage.value.value, Some(0.925));
        assert_eq!(clock.hardware_category, HardwareCategory::Gpu);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hwmon_power_current_pwm_and_cpu_voltage() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("planora-hwmon-pwr-{}-{nonce}", std::process::id()));
        let chip = root.join("hwmon1");
        std::fs::create_dir_all(&chip).unwrap();
        std::fs::write(chip.join("name"), "nct6791\n").unwrap();
        std::fs::write(chip.join("power1_average"), "45000000\n").unwrap(); // 45 W
        std::fs::write(chip.join("in0_input"), "1044\n").unwrap(); // 1.044 V Vcore
        std::fs::write(chip.join("in0_label"), "Vcore\n").unwrap();
        std::fs::write(chip.join("curr1_input"), "35000\n").unwrap(); // 35 A
        std::fs::write(chip.join("pwm1"), "128\n").unwrap(); // ~50%
        std::fs::write(chip.join("pwm1_label"), "CPU Fan Duty\n").unwrap();

        let readings = sample_at(&root, 99);
        assert_eq!(readings.len(), 4);

        let power = readings
            .iter()
            .find(|s| s.sensor_type == SensorType::CpuPackagePowerWatts)
            .unwrap();
        assert_eq!(power.value.value, Some(45.0));

        let vcore = readings
            .iter()
            .find(|s| s.sensor_type == SensorType::VoltageVolts)
            .unwrap();
        assert_eq!(vcore.value.value, Some(1.044));
        assert_eq!(vcore.hardware_category, HardwareCategory::Cpu);

        let curr = readings
            .iter()
            .find(|s| s.sensor_type == SensorType::CurrentAmperes)
            .unwrap();
        assert_eq!(curr.value.value, Some(35.0));

        let pwm = readings
            .iter()
            .find(|s| s.sensor_type == SensorType::LoadPercent)
            .unwrap();
        assert!(pwm.value.value.unwrap() > 50.0 && pwm.value.value.unwrap() < 51.0);
        assert_eq!(pwm.scope, "Cooling");

        std::fs::remove_dir_all(root).unwrap();
    }
}
