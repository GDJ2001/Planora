//! Whole-block-device counters from Linux sysfs. Partitions and stacked
//! virtual devices are excluded from system totals to avoid double counting.
use planora_core::{
    AvailabilityReason, Metric, ResourceDeviceSample, UNIT_BYTES_PER_SEC, UNIT_MILLISECONDS,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counters {
    read_ops: u64,
    read_sectors: u64,
    read_ms: u64,
    write_ops: u64,
    write_sectors: u64,
    write_ms: u64,
    in_flight: u64,
}

fn parse_stat(raw: &str) -> Option<Counters> {
    let values = raw
        .split_whitespace()
        .take(9)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 9).then(|| Counters {
        read_ops: values[0],
        read_sectors: values[2],
        read_ms: values[3],
        write_ops: values[4],
        write_sectors: values[6],
        write_ms: values[7],
        in_flight: values[8],
    })
}

fn physical_device(name: &str) -> bool {
    !["loop", "ram", "zram", "dm-", "md", "sr"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn identity(path: &Path, name: &str) -> String {
    let stable = ["device/wwid", "wwid", "device/serial"]
        .iter()
        .find_map(|field| fs::read_to_string(path.join(field)).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            path.join("device")
                .canonicalize()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| name.to_owned());
    let hash = stable.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("linux-block:{hash:016x}")
}

fn byte_delta(current: u64, previous: u64) -> Option<u64> {
    current.checked_sub(previous)?.checked_mul(512)
}

fn rate(bytes: u64, elapsed: Duration) -> u64 {
    let nanos = elapsed.as_nanos().max(1);
    (u128::from(bytes) * 1_000_000_000 / nanos).min(u128::from(u64::MAX)) as u64
}

fn rate_metric(value: Option<u64>, elapsed: Duration, stamp: u64) -> Metric<u64> {
    match value {
        Some(bytes) => Metric::available(
            rate(bytes, elapsed),
            UNIT_BYTES_PER_SEC,
            "Linux block sysfs",
            stamp,
        ),
        None => Metric::unavailable(
            UNIT_BYTES_PER_SEC,
            "Linux block sysfs",
            stamp,
            AvailabilityReason::WarmingUp,
        ),
    }
}

#[derive(Default)]
pub(super) struct BlockCollector {
    previous: HashMap<String, Counters>,
}

impl BlockCollector {
    pub(super) fn sample(
        &mut self,
        elapsed: Duration,
        stamp: u64,
    ) -> (Metric<u64>, Metric<u64>, Vec<ResourceDeviceSample>) {
        self.sample_at(Path::new("/sys/block"), elapsed, stamp)
    }

    fn sample_at(
        &mut self,
        root: &Path,
        elapsed: Duration,
        stamp: u64,
    ) -> (Metric<u64>, Metric<u64>, Vec<ResourceDeviceSample>) {
        let Ok(entries) = fs::read_dir(root) else {
            let unavailable = || {
                Metric::unavailable(
                    UNIT_BYTES_PER_SEC,
                    "Linux block sysfs",
                    stamp,
                    AvailabilityReason::TemporarilyFailed(
                        "block-device statistics are unavailable".into(),
                    ),
                )
            };
            return (unavailable(), unavailable(), Vec::new());
        };
        let prior_ids: HashSet<_> = self.previous.keys().cloned().collect();
        let mut seen = HashSet::new();
        let mut output = Vec::new();
        let mut read_total = 0u64;
        let mut write_total = 0u64;
        let mut all_valid = true;
        for entry in entries.flatten().take(256) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !physical_device(&name) {
                continue;
            }
            let path = entry.path();
            let Some(current) = fs::read_to_string(path.join("stat"))
                .ok()
                .and_then(|s| parse_stat(&s))
            else {
                all_valid = false;
                continue;
            };
            let id = identity(&path, &name);
            if !seen.insert(id.clone()) {
                continue;
            }
            let previous = self.previous.insert(id.clone(), current);
            let read = previous.and_then(|p| byte_delta(current.read_sectors, p.read_sectors));
            let write = previous.and_then(|p| byte_delta(current.write_sectors, p.write_sectors));
            let latency_ms = previous.and_then(|p| {
                let operations = current
                    .read_ops
                    .checked_sub(p.read_ops)?
                    .checked_add(current.write_ops.checked_sub(p.write_ops)?)?;
                let busy_ms = current
                    .read_ms
                    .checked_sub(p.read_ms)?
                    .checked_add(current.write_ms.checked_sub(p.write_ms)?)?;
                (operations > 0).then(|| {
                    Metric::available(
                        busy_ms as f32 / operations as f32,
                        UNIT_MILLISECONDS,
                        "Linux block request completion time",
                        stamp,
                    )
                })
            });
            if let (Some(read), Some(write)) = (read, write) {
                read_total = read_total.saturating_add(read);
                write_total = write_total.saturating_add(write);
            } else {
                all_valid = false;
            }
            output.push(ResourceDeviceSample {
                id,
                label: name,
                read_bytes_per_sec: rate_metric(read, elapsed, stamp),
                write_bytes_per_sec: rate_metric(write, elapsed, stamp),
                latency_ms,
                queue_depth: Some(Metric::available(
                    current.in_flight as f32,
                    "requests",
                    "Linux block in-flight requests",
                    stamp,
                )),
                link_speed_bps: None,
                link_state: None,
                is_hardware_interface: None,
                interface_type: None,
            });
        }
        self.previous.retain(|id, _| seen.contains(id));
        output.sort_by(|a, b| a.label.cmp(&b.label));
        if output.is_empty() || !all_valid || seen != prior_ids {
            let reason = if output.is_empty() {
                "no readable whole-block-device counters"
            } else {
                "device set or counters changed; establishing a new baseline"
            };
            let unavailable = || {
                Metric::unavailable(
                    UNIT_BYTES_PER_SEC,
                    "Linux block sysfs",
                    stamp,
                    AvailabilityReason::TemporarilyFailed(reason.into()),
                )
            };
            (unavailable(), unavailable(), output)
        } else {
            (
                rate_metric(Some(read_total), elapsed, stamp),
                rate_metric(Some(write_total), elapsed, stamp),
                output,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_disk_rates_latency_and_reset_gaps() {
        let root = std::env::temp_dir().join(format!("planora-block-{}", std::process::id()));
        let disk = root.join("nvme0n1");
        let partition = root.join("dm-0");
        fs::create_dir_all(&disk).unwrap();
        fs::create_dir_all(&partition).unwrap();
        fs::write(partition.join("stat"), "1 0 100 4 1 0 100 4 0\n").unwrap();
        fs::write(disk.join("wwid"), "test-device\n").unwrap();
        fs::write(disk.join("stat"), "1 0 100 4 1 0 200 6 0\n").unwrap();
        let mut collector = BlockCollector::default();
        assert!(
            collector
                .sample_at(&root, Duration::from_secs(1), 1)
                .0
                .value
                .is_none()
        );
        fs::write(disk.join("stat"), "3 0 120 14 2 0 220 10 1\n").unwrap();
        let (read, write, devices) = collector.sample_at(&root, Duration::from_secs(1), 2);
        assert_eq!(devices.len(), 1);
        assert_eq!(read.value, Some(20 * 512));
        assert_eq!(write.value, Some(20 * 512));
        assert_eq!(
            devices[0].latency_ms.as_ref().unwrap().value,
            Some(14.0 / 3.0)
        );
        assert_eq!(devices[0].queue_depth.as_ref().unwrap().value, Some(1.0));
        fs::write(disk.join("stat"), "1 0 1 1 1 0 1 1 0\n").unwrap();
        assert!(
            collector
                .sample_at(&root, Duration::from_secs(1), 3)
                .0
                .value
                .is_none()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
