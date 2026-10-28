//! Read-only DRM adapter discovery and supported per-device memory counters.
//! Aggregate DRM busy is kept separate from physical-engine utilization.
use planora_core::{AvailabilityReason, GpuAdapterSample, Metric, UNIT_BYTES, UNIT_PERCENT};
use std::{fs, path::Path};

fn is_card(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()))
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn text(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

fn bytes_metric(path: &Path, stamp: u64) -> Metric<u64> {
    match text(path).and_then(|raw| raw.parse::<u64>().ok()) {
        Some(value) => Metric::available(value, UNIT_BYTES, "Linux DRM sysfs", stamp),
        None => Metric::unavailable(
            UNIT_BYTES,
            "Linux DRM sysfs",
            stamp,
            AvailabilityReason::Unsupported(format!(
                "{} is not exposed by this DRM driver",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        ),
    }
}

pub(super) fn adapters(stamp: u64) -> Vec<GpuAdapterSample> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_card(&name) {
            continue;
        }
        let device = entry.path().join("device");
        if !device.is_dir() {
            continue;
        }
        let canonical = fs::canonicalize(&device).unwrap_or(device.clone());
        let identity = canonical.to_string_lossy();
        let vendor = text(&device.join("vendor")).unwrap_or_default();
        let driver = fs::read_link(device.join("driver"))
            .ok()
            .and_then(|link| {
                link.file_name()
                    .map(|part| part.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "DRM".to_owned());
        let label = match vendor.as_str() {
            "0x8086" => "Intel GPU",
            "0x1002" => "AMD GPU",
            "0x10de" => "NVIDIA GPU",
            _ => "Graphics adapter",
        };
        found.push(GpuAdapterSample {
            adapter_luid: stable_hash(&identity),
            physical_adapter: None,
            name: format!("{label} · {identity} ({driver})"),
            busiest_engine_percent: Metric::unavailable(
                UNIT_PERCENT, "Linux DRM", stamp,
                AvailabilityReason::Unsupported("Physical-engine load is not exposed by this provider; aggregate device busy is a different measurement".into()),
            ),
            dedicated_used_bytes: bytes_metric(&device.join("mem_info_vram_used"), stamp),
            dedicated_total_bytes: bytes_metric(&device.join("mem_info_vram_total"), stamp),
            shared_used_bytes: bytes_metric(&device.join("mem_info_gtt_used"), stamp),
            shared_total_bytes: bytes_metric(&device.join("mem_info_gtt_total"), stamp),
            budget_bytes: None,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.dedup_by_key(|adapter| adapter.adapter_luid);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_primary_cards_are_adapters() {
        assert!(is_card("card0"));
        assert!(!is_card("card0-DP-1"));
        assert!(!is_card("renderD128"));
        assert!(!is_card("card"));
    }
    #[test]
    fn identity_hash_is_stable_and_distinguishes_devices() {
        assert_eq!(
            stable_hash("/sys/devices/pci0000:00/0000:00:02.0"),
            stable_hash("/sys/devices/pci0000:00/0000:00:02.0")
        );
        assert_ne!(
            stable_hash("/sys/devices/pci0000:00/0000:00:02.0"),
            stable_hash("/sys/devices/pci0000:00/0000:01:00.0")
        );
    }
}
