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
        return "0.0 B/s".to_string();
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
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(1024 * 1024 * 500), "500.0 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 2), "2.00 GiB");
    }
}
