use std::time::Duration;

/// Format a CPU or GPU percentage (e.g., "63.9%").
pub fn format_percent(val: Option<f32>) -> String {
    match val {
        Some(pct) if pct.is_finite() => format!("{:.1}%", pct.clamp(0.0, 100.0)),
        _ => "Unavailable".to_string(),
    }
}

/// Format memory usage as ratio: "11.0 / 15.7 GiB" and compute percentage.
pub fn format_memory_ratio(used: Option<u64>, total: Option<u64>) -> (String, Option<f32>) {
    match (used, total) {
        (Some(u), Some(t)) if t > 0 => {
            let u_gib = u as f64 / (1024.0 * 1024.0 * 1024.0);
            let t_gib = t as f64 / (1024.0 * 1024.0 * 1024.0);
            let pct = ((u as f64 / t as f64) * 100.0) as f32;
            (
                format!("{u_gib:.1} / {t_gib:.1} GiB"),
                Some(pct.clamp(0.0, 100.0)),
            )
        }
        (Some(u), _) => {
            let u_gib = u as f64 / (1024.0 * 1024.0 * 1024.0);
            (format!("{u_gib:.1} GiB"), None)
        }
        _ => ("Unavailable".to_string(), None),
    }
}

/// Format byte throughput rate in B/s, KiB/s, MiB/s, or GiB/s.
pub fn format_rate(rate_bytes_per_sec: Option<f32>) -> String {
    let Some(r) = rate_bytes_per_sec else {
        return "Unavailable".to_string();
    };
    if !r.is_finite() || r < 0.0 {
        return "Unavailable".to_string();
    }
    const KIB: f32 = 1024.0;
    const MIB: f32 = 1024.0 * 1024.0;
    const GIB: f32 = 1024.0 * 1024.0 * 1024.0;

    if r >= GIB {
        format!("{:.2} GiB/s", r / GIB)
    } else if r >= MIB {
        format!("{:.1} MiB/s", r / MIB)
    } else if r >= KIB {
        format!("{:.1} KiB/s", r / KIB)
    } else {
        format!("{:.0} B/s", r)
    }
}

/// Format a byte count into B, KiB, MiB, or GiB.
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Format power consumption in Watts.
pub fn format_watts(watts: Option<f32>) -> String {
    match watts {
        Some(w) if w.is_finite() && w >= 0.0 => format!("{w:.1} W"),
        _ => "Unavailable".to_string(),
    }
}

/// Format latency in nanoseconds.
pub fn format_latency_ns(ns: f64) -> String {
    if !ns.is_finite() || ns < 0.0 {
        "N/A".to_string()
    } else {
        format!("{ns:.2} ns")
    }
}

/// Format duration into a human-readable string.
pub fn format_duration(dur: Duration) -> String {
    let total_secs = dur.as_secs_f32();
    if total_secs < 1.0 {
        format!("{:.0} ms", total_secs * 1000.0)
    } else if total_secs < 60.0 {
        format!("{total_secs:.1}s")
    } else {
        let mins = dur.as_secs() / 60;
        let secs = dur.as_secs() % 60;
        format!("{mins}m {secs}s")
    }
}

/// Unit-aware sensor value formatting with correct precision per sensor category.
pub fn format_sensor_value(
    value: f32,
    unit: &str,
    sensor_type: planora_core::SensorType,
) -> String {
    if !value.is_finite() {
        return "Unavailable".to_string();
    }
    match sensor_type {
        planora_core::SensorType::VoltageVolts => {
            if unit.is_empty() {
                format!("{value:.3}")
            } else {
                format!("{value:.3} {unit}")
            }
        }
        planora_core::SensorType::CurrentAmperes => {
            if unit.is_empty() {
                format!("{value:.2}")
            } else {
                format!("{value:.2} {unit}")
            }
        }
        planora_core::SensorType::CpuPackagePowerWatts
        | planora_core::SensorType::GpuBoardPowerWatts
        | planora_core::SensorType::GpuChipPowerWatts => {
            if unit.is_empty() {
                format!("{value:.1}")
            } else {
                format!("{value:.1} {unit}")
            }
        }
        planora_core::SensorType::CpuTemperatureCelsius
        | planora_core::SensorType::GpuTemperatureCelsius
        | planora_core::SensorType::ThermalZoneCelsius
        | planora_core::SensorType::StorageTemperatureCelsius => {
            if unit.is_empty() {
                format!("{value:.1}")
            } else {
                format!("{value:.1} {unit}")
            }
        }
        planora_core::SensorType::FanRpm | planora_core::SensorType::ClockMhz => {
            if unit.is_empty() {
                format!("{value:.0}")
            } else {
                format!("{value:.0} {unit}")
            }
        }
        planora_core::SensorType::LoadPercent | planora_core::SensorType::StorageHealthPercent => {
            if unit == "%" || unit.is_empty() {
                format!("{value:.1}%")
            } else {
                format!("{value:.1} {unit}")
            }
        }
        planora_core::SensorType::DataGigabytes => {
            if (0.0..1.0).contains(&value) {
                format!("{:.0} MB", value * 1024.0)
            } else {
                format!("{value:.2} GB")
            }
        }
        _ => {
            if unit.is_empty() {
                format!("{value:.1}")
            } else {
                format!("{value:.1} {unit}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_percent() {
        assert_eq!(format_percent(Some(63.88)), "63.9%");
        assert_eq!(format_percent(Some(0.0)), "0.0%");
        assert_eq!(format_percent(None), "Unavailable");
    }

    #[test]
    fn test_format_memory_ratio() {
        let (s, pct) =
            format_memory_ratio(Some(11 * 1024 * 1024 * 1024), Some(16 * 1024 * 1024 * 1024));
        assert_eq!(s, "11.0 / 16.0 GiB");
        assert!(pct.is_some());
        assert!((pct.unwrap() - 68.75).abs() < 0.1);
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(format_rate(Some(2.6 * 1024.0 * 1024.0)), "2.6 MiB/s");
        assert_eq!(format_rate(Some(450.0 * 1024.0)), "450.0 KiB/s");
        assert_eq!(format_rate(Some(128.0)), "128 B/s");
        assert_eq!(format_rate(None), "Unavailable");
        assert_eq!(format_rate(Some(f32::NAN)), "Unavailable");
        assert_eq!(format_rate(Some(-3.0)), "Unavailable");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(1024 * 1024 * 500), "500.0 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 2), "2.00 GiB");
    }

    #[test]
    fn test_format_sensor_value() {
        use planora_core::SensorType;
        assert_eq!(
            format_sensor_value(1.044, "V", SensorType::VoltageVolts),
            "1.044 V"
        );
        assert_eq!(
            format_sensor_value(18.452, "A", SensorType::CurrentAmperes),
            "18.45 A"
        );
        assert_eq!(
            format_sensor_value(25.12, "W", SensorType::CpuPackagePowerWatts),
            "25.1 W"
        );
        assert_eq!(
            format_sensor_value(58.03, "°C", SensorType::GpuTemperatureCelsius),
            "58.0 °C"
        );
        assert_eq!(
            format_sensor_value(1250.4, "RPM", SensorType::FanRpm),
            "1250 RPM"
        );
        assert_eq!(
            format_sensor_value(832.1, "MHz", SensorType::ClockMhz),
            "832 MHz"
        );
        assert_eq!(
            format_sensor_value(65.02, "%", SensorType::LoadPercent),
            "65.0%"
        );
        // < 1.0 GB shows in MB
        assert_eq!(
            format_sensor_value(277.0 / 1024.0, "GB", SensorType::DataGigabytes),
            "277 MB"
        );
        // >= 1.0 GB shows in GB
        assert_eq!(
            format_sensor_value(2.5, "GB", SensorType::DataGigabytes),
            "2.50 GB"
        );
    }
}
