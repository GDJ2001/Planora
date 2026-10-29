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
    adapters_at(Path::new("/sys/class/drm"), stamp)
}

fn aggregate_busy_metric(
    device: &Path,
    vendor: &str,
    driver: &str,
    stamp: u64,
) -> Option<Metric<f32>> {
    if vendor != "0x1002" || driver != "amdgpu" {
        return None;
    }
    let path = device.join("gpu_busy_percent");
    let reading = match fs::read_to_string(path) {
        Ok(raw) => match raw.trim().parse::<f32>() {
            Ok(value) if value.is_finite() && (0.0..=100.0).contains(&value) => {
                return Some(Metric::available(
                    value,
                    UNIT_PERCENT,
                    "Linux amdgpu gpu_busy_percent",
                    stamp,
                ));
            }
            _ => AvailabilityReason::TemporarilyFailed(
                "The AMD driver returned an invalid aggregate busy reading".into(),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            AvailabilityReason::Unsupported(
                "This AMD driver exposes no aggregate busy counter".into(),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            AvailabilityReason::PermissionDenied(
                "Cannot read the AMD aggregate busy counter".into(),
            )
        }
        Err(_) => AvailabilityReason::TemporarilyFailed(
            "The AMD aggregate busy counter could not be read".into(),
        ),
    };
    Some(Metric::unavailable(
        UNIT_PERCENT,
        "Linux amdgpu gpu_busy_percent",
        stamp,
        reading,
    ))
}

fn adapters_at(root: &Path, stamp: u64) -> Vec<GpuAdapterSample> {
    let Ok(entries) = fs::read_dir(root) else {
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
            aggregate_busy_percent: aggregate_busy_metric(&device, &vendor, &driver, stamp),
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

    #[test]
    fn amd_busy_is_per_adapter_and_never_called_an_engine() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "planora-gpu-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (index, busy) in [(0, "12"), (1, "83")] {
            let device = root.join(format!("card{index}/device"));
            fs::create_dir_all(&device).unwrap();
            fs::write(device.join("vendor"), "0x1002\n").unwrap();
            fs::write(device.join("gpu_busy_percent"), busy).unwrap();
            symlink("/sys/bus/pci/drivers/amdgpu", device.join("driver")).unwrap();
        }
        let adapters = adapters_at(&root, 42);
        assert_eq!(adapters.len(), 2);
        let values: Vec<_> = adapters
            .iter()
            .map(|adapter| adapter.activity_percent().0.value.unwrap())
            .collect();
        assert_eq!(values, [12.0, 83.0]);
        assert!(adapters.iter().all(|adapter| {
            adapter.busiest_engine_percent.value.is_none()
                && adapter.activity_percent().1 == "Aggregate device busy"
                && adapter.activity_history_key().ends_with(":aggregate")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_busy_is_unavailable_and_other_vendors_are_not_mislabeled() {
        let root = std::env::temp_dir().join(format!("planora-gpu-invalid-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("gpu_busy_percent"), "101").unwrap();
        let invalid = aggregate_busy_metric(&root, "0x1002", "amdgpu", 42).unwrap();
        assert!(invalid.value.is_none());
        assert!(aggregate_busy_metric(&root, "0x8086", "i915", 42).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
