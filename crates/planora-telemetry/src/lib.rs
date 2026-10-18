//! Cross-platform baseline collector. Hardware-specific counters can enrich its frames.
use planora_core::{Metric, PROTOCOL_VERSION, ProcessIdentity, ProcessRecord, TelemetryFrame};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

pub const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn counter_delta(prev: u64, curr: u64) -> u64 {
    curr.wrapping_sub(prev)
}

pub fn rate(delta: u64, elapsed: Duration) -> u64 {
    if elapsed.is_zero() {
        0
    } else {
        (delta as f64 / elapsed.as_secs_f64()).min(u64::MAX as f64) as u64
    }
}

pub fn rate_from_counters(prev: u64, curr: u64, elapsed: Duration) -> u64 {
    rate(counter_delta(prev, curr), elapsed)
}

#[cfg(windows)]
#[repr(C)]
struct UNICODE_STRING {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[cfg(windows)]
#[repr(C)]
struct SYSTEM_BASICPROCESS_INFORMATION {
    next_entry_offset: u32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
    sequence_number: u64,
    image_name: UNICODE_STRING,
}

#[cfg(windows)]
#[derive(Clone, Debug)]
pub struct BasicProcessInfo {
    pub pid: u32,
    pub sequence_number: u64,
    pub image_name: String,
}

#[cfg(windows)]
pub fn query_windows_basic_processes() -> Result<Vec<BasicProcessInfo>, i32> {
    const SYSTEM_BASIC_PROCESS_INFORMATION_CLASS: u32 = 252;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = -1073741820; // 0xC0000004 as i32

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQuerySystemInformation(
            system_information_class: u32,
            system_information: *mut std::ffi::c_void,
            system_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    let mut capacity = 64 * 1024;
    let mut buffer = vec![0u8; capacity];
    let mut return_length: u32 = 0;

    loop {
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_BASIC_PROCESS_INFORMATION_CLASS,
                buffer.as_mut_ptr().cast(),
                capacity as u32,
                &mut return_length,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            capacity = (return_length as usize).max(capacity * 2) + 16_384;
            buffer.resize(capacity, 0);
            continue;
        } else if status < 0 {
            return Err(status);
        } else {
            break;
        }
    }

    let mut out = Vec::new();
    let mut offset = 0usize;
    let limit = (return_length as usize).min(buffer.len());
    while offset + std::mem::size_of::<SYSTEM_BASICPROCESS_INFORMATION>() <= limit {
        let entry =
            unsafe { &*(buffer.as_ptr().add(offset) as *const SYSTEM_BASICPROCESS_INFORMATION) };
        let pid = entry.unique_process_id as u32;
        let mut name = String::new();
        if !entry.image_name.buffer.is_null() && entry.image_name.length > 0 {
            let u16_len = (entry.image_name.length / 2) as usize;
            let slice = unsafe { std::slice::from_raw_parts(entry.image_name.buffer, u16_len) };
            name = String::from_utf16_lossy(slice);
        }
        out.push(BasicProcessInfo {
            pid,
            sequence_number: entry.sequence_number,
            image_name: name,
        });
        if entry.next_entry_offset == 0 {
            break;
        }
        offset += entry.next_entry_offset as usize;
    }
    Ok(out)
}

pub struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    last: Option<Instant>,
    sequence: u64,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last: None,
            sequence: 0,
        }
    }

    pub fn sample(&mut self) -> TelemetryFrame {
        let now = Instant::now();
        let stamp = unix_ms();
        let elapsed = self.last.map(|last| now.duration_since(last));
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        self.networks.refresh(true);
        self.disks.refresh(true);
        self.sequence += 1;
        let first = elapsed.is_none();
        let delta = elapsed.unwrap_or_default();
        let proc_count = self.system.cpus().len().max(1) as f32;

        #[cfg(windows)]
        let basic_processes = query_windows_basic_processes().ok();
        #[cfg(windows)]
        let basic_map: std::collections::HashMap<u32, u64> = basic_processes
            .map(|list| {
                list.into_iter()
                    .map(|p| (p.pid, p.sequence_number))
                    .collect()
            })
            .unwrap_or_default();

        let mut processes = Vec::with_capacity(self.system.processes().len());
        for (pid, p) in self.system.processes() {
            let io = p.disk_usage();
            let cpu = if first {
                Metric::unavailable(
                    "%",
                    "sysinfo process time delta",
                    stamp,
                    "first sample has no elapsed interval",
                )
            } else {
                Metric::available(
                    (p.cpu_usage() / proc_count).clamp(0.0, 100.0),
                    "%",
                    "sysinfo normalized to machine capacity",
                    stamp,
                )
            };
            #[cfg(windows)]
            let (read, write) = (
                Metric::unavailable(
                    "bytes/s",
                    "sysinfo",
                    stamp,
                    "Windows counts all process I/O, not disk I/O",
                ),
                Metric::unavailable(
                    "bytes/s",
                    "sysinfo",
                    stamp,
                    "Windows counts all process I/O, not disk I/O",
                ),
            );
            #[cfg(not(windows))]
            let (read, write) = if first {
                (
                    Metric::unavailable("bytes/s", "sysinfo", stamp, "first sample"),
                    Metric::unavailable("bytes/s", "sysinfo", stamp, "first sample"),
                )
            } else {
                (
                    Metric::available(
                        rate(io.read_bytes, delta),
                        "bytes/s",
                        "sysinfo process disk delta",
                        stamp,
                    ),
                    Metric::available(
                        rate(io.written_bytes, delta),
                        "bytes/s",
                        "sysinfo process disk delta",
                        stamp,
                    ),
                )
            };
            #[cfg(windows)]
            let _ = io;

            #[cfg(target_os = "linux")]
            let identity = linux_start_ticks(pid.as_u32())
                .map(|started| ProcessIdentity {
                    pid: pid.as_u32(),
                    started,
                    source: "linux proc start ticks".into(),
                })
                .unwrap_or_else(|_| ProcessIdentity {
                    pid: pid.as_u32(),
                    started: p.start_time(),
                    source: "sysinfo epoch seconds".into(),
                });

            #[cfg(windows)]
            let identity = if let Some(&seq) = basic_map.get(&pid.as_u32()) {
                ProcessIdentity {
                    pid: pid.as_u32(),
                    started: seq,
                    source: "windows process sequence (SystemBasicProcessInformation)".into(),
                }
            } else {
                windows_start_filetime(pid.as_u32())
                    .map(|started| ProcessIdentity {
                        pid: pid.as_u32(),
                        started,
                        source: "windows process FILETIME".into(),
                    })
                    .unwrap_or_else(|| ProcessIdentity {
                        pid: pid.as_u32(),
                        started: p.start_time(),
                        source: "sysinfo epoch seconds".into(),
                    })
            };

            #[cfg(not(any(target_os = "linux", windows)))]
            let identity = ProcessIdentity {
                pid: pid.as_u32(),
                started: p.start_time(),
                source: "sysinfo epoch seconds".into(),
            };

            processes.push(ProcessRecord {
                identity,
                name: p.name().to_string_lossy().into_owned(),
                cpu_percent: cpu,
                memory_bytes: Metric::available(
                    p.memory(),
                    "bytes",
                    "sysinfo resident memory",
                    stamp,
                ),
                disk_read_bytes_per_sec: read,
                disk_write_bytes_per_sec: write,
                gpu_percent: Metric::unavailable(
                    "%",
                    "OS GPU provider",
                    stamp,
                    "per-process GPU counter unavailable",
                ),
                affinity: Metric::unavailable(
                    "cpus",
                    "OS affinity provider",
                    stamp,
                    "not queried in baseline sampler",
                ),
            });
        }
        processes.sort_by(|a, b| {
            b.cpu_percent
                .value
                .unwrap_or_default()
                .total_cmp(&a.cpu_percent.value.unwrap_or_default())
        });
        let (read, write) = self.disks.list().iter().fold((0u64, 0u64), |(r, w), d| {
            let io = d.usage();
            (
                r.saturating_add(io.read_bytes),
                w.saturating_add(io.written_bytes),
            )
        });
        let (rx, tx) = self.networks.iter().fold((0u64, 0u64), |(r, t), (_, n)| {
            (
                r.saturating_add(n.received()),
                t.saturating_add(n.transmitted()),
            )
        });
        let measured = |value: u64, source: &str| {
            if first {
                Metric::unavailable(
                    "bytes/s",
                    source,
                    stamp,
                    "first sample has no elapsed interval",
                )
            } else {
                Metric::available(rate(value, delta), "bytes/s", source, stamp)
            }
        };
        let mut frame = TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            sequence: self.sequence,
            sampled_at_ms: stamp,
            collection_duration_ms: 0.0,
            cpu_percent: if first {
                Metric::unavailable(
                    "%",
                    "sysinfo",
                    stamp,
                    "first sample has no elapsed interval",
                )
            } else {
                Metric::available(
                    self.system.global_cpu_usage(),
                    "%",
                    "sysinfo total CPU delta",
                    stamp,
                )
            },
            memory_used_bytes: Metric::available(
                self.system.used_memory(),
                "bytes",
                "sysinfo physical memory",
                stamp,
            ),
            memory_total_bytes: Metric::available(
                self.system.total_memory(),
                "bytes",
                "sysinfo physical memory",
                stamp,
            ),
            disk_read_bytes_per_sec: measured(read, "sysinfo disk device delta"),
            disk_write_bytes_per_sec: measured(write, "sysinfo disk device delta"),
            network_rx_bytes_per_sec: measured(rx, "sysinfo interface delta"),
            network_tx_bytes_per_sec: measured(tx, "sysinfo interface delta"),
            gpu_percent: Metric::unavailable(
                "%",
                "OS GPU provider",
                stamp,
                "GPU counter unavailable",
            ),
            package_power_watts: Metric::unavailable(
                "W",
                "OS power provider",
                stamp,
                "power counter unavailable",
            ),
            processes,
        };
        frame.collection_duration_ms = now.elapsed().as_secs_f32() * 1000.0;
        self.last = Some(now);
        frame
    }
}

#[cfg(windows)]
pub fn windows_start_filetime(pid: u32) -> Option<u64> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(handle);
        ok.then_some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }
}

#[cfg(target_os = "linux")]
pub fn linux_start_ticks(pid: u32) -> std::io::Result<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close = text
        .rfind(')')
        .ok_or_else(|| std::io::Error::other("invalid stat"))?;
    // Field 22, with state as field 3 after the final parenthesis.
    text[close + 1..]
        .split_whitespace()
        .nth(19)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| std::io::Error::other("invalid start ticks"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_does_not_invent_cpu_or_io_rate() {
        let mut s = Sampler::new();
        let f = s.sample();
        assert!(f.cpu_percent.value.is_none());
        assert!(f.disk_read_bytes_per_sec.value.is_none());
        assert!(f.network_rx_bytes_per_sec.value.is_none());
        assert_eq!(f.cpu_percent.unit, "%");
        assert_eq!(f.disk_read_bytes_per_sec.unit, "bytes/s");
    }

    #[test]
    fn delta_rate_is_in_bytes_per_second() {
        assert_eq!(rate(200, Duration::from_millis(200)), 1000);
        assert_eq!(rate(100, Duration::from_millis(100)), 1000);
        assert_eq!(rate(0, Duration::from_millis(100)), 0);
    }

    #[test]
    fn counter_wraparound_calculation() {
        // Monotonic 64-bit rollover
        let prev = u64::MAX - 99;
        let curr = 50u64;
        let delta = counter_delta(prev, curr);
        assert_eq!(delta, 150);
        let rate_val = rate_from_counters(prev, curr, Duration::from_secs(1));
        assert_eq!(rate_val, 150);
    }

    #[cfg(windows)]
    #[test]
    fn windows_basic_or_fallback_query() {
        let result = query_windows_basic_processes();
        // On Windows 11 build 26100+ this returns Ok, on earlier builds it returns an error.
        // In either case, it doesn't crash or violate memory safety.
        if let Ok(procs) = result {
            assert!(!procs.is_empty());
            assert!(procs.iter().any(|p| p.pid == std::process::id()));
        }
    }
}
