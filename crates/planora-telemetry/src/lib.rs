//! Cross-platform baseline collector. Hardware-specific counters can enrich its frames.
use planora_core::{
    AvailabilityReason, IdentitySource, Metric, PROTOCOL_VERSION, ProcessIdentity, ProcessRecord,
    TelemetryFrame,
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};

pub mod vendor_sensors;
#[cfg(windows)]
pub mod windows_counters;
#[cfg(windows)]
mod windows_gpu;
#[cfg(windows)]
pub use windows_gpu::counter_available as windows_gpu_counter_available;

pub const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(windows)]
pub fn windows_basic_process_query_supported() -> bool {
    use std::sync::OnceLock;
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        #[repr(C)]
        struct OsVersion {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            service_pack: [u16; 128],
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(version: *mut OsVersion) -> i32;
        }
        let mut version = OsVersion {
            size: std::mem::size_of::<OsVersion>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service_pack: [0; 128],
        };
        if unsafe { RtlGetVersion(&mut version) } < 0 || version.major != 10 {
            return false;
        }
        if version.build > 26100 {
            return true;
        }
        if version.build != 26100 {
            return false;
        }
        use windows_sys::Win32::System::Registry::{
            HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
        };
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let path = wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let key = wide("UBR");
        let mut revision = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                path.as_ptr(),
                key.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut revision as *mut u32).cast(),
                &mut size,
            )
        };
        status == 0 && size == 4 && revision >= 4770
    })
}

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

#[cfg(target_os = "linux")]
fn rapl_delta(previous: u64, current: u64, maximum: u64) -> Option<u64> {
    if maximum == 0 || previous >= maximum || current >= maximum {
        return None;
    }
    Some(if current >= previous {
        current - previous
    } else {
        maximum - previous + current
    })
}

#[cfg(target_os = "linux")]
fn rapl_zones() -> Result<Vec<(String, u64, u64)>, String> {
    let entries = std::fs::read_dir("/sys/class/powercap")
        .map_err(|error| format!("Linux powercap is unavailable: {error}"))?;
    let mut zones = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Root RAPL zones describe packages; subzones would double-count energy.
        if !name.starts_with("intel-rapl:") || name.matches(':').count() != 1 {
            continue;
        }
        let read = |file: &str| -> Result<u64, String> {
            std::fs::read_to_string(entry.path().join(file))
                .map_err(|error| format!("{name}/{file}: {error}"))?
                .trim()
                .parse::<u64>()
                .map_err(|error| format!("{name}/{file}: {error}"))
        };
        let energy = read("energy_uj")?;
        let maximum = read("max_energy_range_uj")?;
        zones.push((name, energy, maximum));
    }
    if zones.is_empty() {
        Err("no Intel RAPL package zones exposed by Linux powercap".into())
    } else {
        zones.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(zones)
    }
}

#[cfg(target_os = "linux")]
pub fn linux_rapl_available() -> bool {
    rapl_zones().is_ok()
}

#[cfg(target_os = "linux")]
fn amd_gpu_busy() -> Result<(f32, String), String> {
    let entries = std::fs::read_dir("/sys/class/drm")
        .map_err(|error| format!("Linux DRM is unavailable: {error}"))?;
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || !name[4..].chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let path = entry.path().join("device/gpu_busy_percent");
        if !path.exists() {
            continue;
        }
        let value = std::fs::read_to_string(&path)
            .map_err(|error| format!("{name} AMD busy counter: {error}"))?
            .trim()
            .parse::<f32>()
            .map_err(|error| format!("{name} AMD busy counter: {error}"))?;
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(format!("{name} AMD busy counter is outside 0–100%"));
        }
        devices.push((value, name));
    }
    match devices.len() {
        1 => Ok(devices.remove(0)),
        0 => Err("no AMD gpu_busy_percent counter exposed by Linux DRM".into()),
        _ => Err("multiple GPU busy counters cannot be represented as one aggregate".into()),
    }
}

#[cfg(target_os = "linux")]
pub fn linux_amd_gpu_busy_available() -> bool {
    amd_gpu_busy().is_ok()
}

#[cfg(target_os = "linux")]
struct RaplSample {
    energy_uj: u64,
    maximum_uj: u64,
    at: Instant,
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
    const STATUS_NOT_SUPPORTED: i32 = -1073741637; // 0xC00000BB
    const MAX_BYTES: usize = 16 * 1024 * 1024;

    if !windows_basic_process_query_supported() {
        return Err(STATUS_NOT_SUPPORTED);
    }

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
            capacity = (return_length as usize)
                .max(capacity * 2)
                .saturating_add(16_384);
            if capacity > MAX_BYTES {
                return Err(STATUS_INFO_LENGTH_MISMATCH);
            }
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
    if limit < std::mem::size_of::<SYSTEM_BASICPROCESS_INFORMATION>() {
        return Err(STATUS_INFO_LENGTH_MISMATCH);
    }
    while offset + std::mem::size_of::<SYSTEM_BASICPROCESS_INFORMATION>() <= limit {
        let entry = unsafe {
            (buffer.as_ptr().add(offset) as *const SYSTEM_BASICPROCESS_INFORMATION).read_unaligned()
        };
        let pid = entry.unique_process_id as u32;
        let mut name = String::new();
        if !entry.image_name.buffer.is_null() && entry.image_name.length > 0 {
            let start = entry.image_name.buffer as usize;
            let base = buffer.as_ptr() as usize;
            let end = start.checked_add(entry.image_name.length as usize);
            if entry.image_name.length % 2 != 0
                || start < base
                || end.is_none_or(|end| end > base + limit)
            {
                return Err(STATUS_INFO_LENGTH_MISMATCH);
            }
            let units = (0..entry.image_name.length as usize / 2)
                .map(|i| unsafe { entry.image_name.buffer.add(i).read_unaligned() })
                .collect::<Vec<_>>();
            name = String::from_utf16_lossy(&units);
        }
        out.push(BasicProcessInfo {
            pid,
            sequence_number: entry.sequence_number,
            image_name: name,
        });
        if entry.next_entry_offset == 0 {
            break;
        }
        let next = entry.next_entry_offset as usize;
        if next < std::mem::size_of::<SYSTEM_BASICPROCESS_INFORMATION>()
            || offset.checked_add(next).is_none_or(|end| end > limit)
        {
            return Err(STATUS_INFO_LENGTH_MISMATCH);
        }
        offset += next;
    }
    Ok(out)
}

pub struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    last: Option<Instant>,
    last_cpu: Option<Instant>,
    system_cpu: Option<Metric<f32>>,
    process_cpu: HashMap<ProcessIdentity, Metric<f32>>,
    prev_disks: HashMap<String, (u64, u64)>,
    prev_networks: HashMap<String, (u64, u64)>,
    #[cfg(target_os = "linux")]
    rapl: HashMap<String, RaplSample>,
    #[cfg(windows)]
    windows_gpu: Option<windows_gpu::WindowsGpu>,
    #[cfg(windows)]
    gpu_last_identities: HashMap<u32, ProcessIdentity>,
    #[cfg(windows)]
    gpu_validated_pids: HashSet<u32>,
    #[cfg(windows)]
    gpu_last_sample_ms: u64,
    sequence: u64,
    session_id: u64,
    topology_generation: u64,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self::new_with_session(1, 1)
    }

    pub fn new_with_session(session_id: u64, topology_generation: u64) -> Self {
        Self {
            system: System::new_all(),
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last: None,
            last_cpu: None,
            system_cpu: None,
            process_cpu: HashMap::new(),
            prev_disks: HashMap::new(),
            prev_networks: HashMap::new(),
            #[cfg(target_os = "linux")]
            rapl: HashMap::new(),
            #[cfg(windows)]
            windows_gpu: windows_gpu::WindowsGpu::new().ok(),
            #[cfg(windows)]
            gpu_last_identities: HashMap::new(),
            #[cfg(windows)]
            gpu_validated_pids: HashSet::new(),
            #[cfg(windows)]
            gpu_last_sample_ms: 0,
            sequence: 0,
            session_id,
            topology_generation,
        }
    }

    pub fn set_topology_generation(&mut self, generation: u64) {
        self.topology_generation = generation;
    }

    pub fn sample(&mut self) -> TelemetryFrame {
        let now = Instant::now();
        let stamp = unix_ms();
        let elapsed = self.last.map(|last| now.duration_since(last));
        let cpu_due = self
            .last_cpu
            .is_some_and(|last| now.duration_since(last) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        if cpu_due {
            self.system.refresh_cpu_usage();
            self.last_cpu = Some(now);
        } else if self.last_cpu.is_none() {
            self.last_cpu = Some(now);
        }
        self.system.refresh_memory();
        let kind = if cpu_due {
            ProcessRefreshKind::everything()
        } else {
            ProcessRefreshKind::everything().without_cpu()
        };
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
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
            #[cfg(windows)]
            let (read, write) = (
                Metric::unavailable(
                    "bytes/s",
                    "sysinfo",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "Windows counts all process I/O, not disk I/O".into(),
                    ),
                ),
                Metric::unavailable(
                    "bytes/s",
                    "sysinfo",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "Windows counts all process I/O, not disk I/O".into(),
                    ),
                ),
            );
            #[cfg(not(windows))]
            let (read, write) = if first {
                (
                    Metric::unavailable("bytes/s", "sysinfo", stamp, AvailabilityReason::WarmingUp),
                    Metric::unavailable("bytes/s", "sysinfo", stamp, AvailabilityReason::WarmingUp),
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
            let (io_read, io_write) = if first {
                (
                    Metric::unavailable(
                        "bytes/s",
                        "sysinfo process I/O",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    ),
                    Metric::unavailable(
                        "bytes/s",
                        "sysinfo process I/O",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    ),
                )
            } else {
                (
                    Metric::available(
                        rate(io.read_bytes, delta),
                        "bytes/s",
                        "Windows process I/O delta",
                        stamp,
                    ),
                    Metric::available(
                        rate(io.written_bytes, delta),
                        "bytes/s",
                        "Windows process I/O delta",
                        stamp,
                    ),
                )
            };
            #[cfg(not(windows))]
            let (io_read, io_write) = (
                Metric::unavailable(
                    "bytes/s",
                    "OS process I/O provider",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "separate process I/O counter unavailable".into(),
                    ),
                ),
                Metric::unavailable(
                    "bytes/s",
                    "OS process I/O provider",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "separate process I/O counter unavailable".into(),
                    ),
                ),
            );

            #[cfg(target_os = "linux")]
            let (started, source) = if let Ok(ticks) = linux_start_ticks(pid.as_u32()) {
                (ticks, IdentitySource::LinuxProcStartTicks)
            } else {
                (p.start_time(), IdentitySource::SysinfoEpochSeconds)
            };
            #[cfg(target_os = "linux")]
            let identity = ProcessIdentity {
                pid: pid.as_u32(),
                started,
                sequence_number: None,
                source,
            };

            #[cfg(windows)]
            let (started, seq, source) = if let Some(ft) = windows_start_filetime(pid.as_u32()) {
                (
                    ft,
                    basic_map.get(&pid.as_u32()).copied(),
                    IdentitySource::WindowsProcessFiletime,
                )
            } else if let Some(&seq) = basic_map.get(&pid.as_u32()) {
                (seq, Some(seq), IdentitySource::WindowsBasicProcessSequence)
            } else {
                (p.start_time(), None, IdentitySource::SysinfoEpochSeconds)
            };
            #[cfg(windows)]
            let identity = ProcessIdentity {
                pid: pid.as_u32(),
                started,
                sequence_number: seq,
                source,
            };

            #[cfg(not(any(target_os = "linux", windows)))]
            let identity = ProcessIdentity {
                pid: pid.as_u32(),
                started: p.start_time(),
                sequence_number: None,
                #[cfg(target_os = "macos")]
                source: IdentitySource::DarwinStartTime,
                #[cfg(not(target_os = "macos"))]
                source: IdentitySource::SysinfoEpochSeconds,
            };

            let cpu = if cpu_due {
                let metric = Metric::available(
                    (p.cpu_usage() / proc_count).clamp(0.0, 100.0),
                    "%",
                    "sysinfo normalized to machine capacity",
                    stamp,
                );
                self.process_cpu.insert(identity.clone(), metric.clone());
                metric
            } else {
                self.process_cpu.get(&identity).cloned().unwrap_or_else(|| {
                    Metric::unavailable(
                        "%",
                        "sysinfo process time delta",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    )
                })
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
                io_read_bytes_per_sec: io_read,
                io_write_bytes_per_sec: io_write,
                gpu_percent: Metric::unavailable(
                    "%",
                    "OS GPU provider",
                    stamp,
                    AvailabilityReason::Unsupported("per-process GPU counter unavailable".into()),
                ),
                affinity: Metric::unavailable(
                    "cpus",
                    "OS affinity provider",
                    stamp,
                    AvailabilityReason::Unsupported("not queried in baseline sampler".into()),
                ),
            });
        }

        processes.sort_by(|a, b| {
            b.cpu_percent
                .value
                .unwrap_or_default()
                .total_cmp(&a.cpu_percent.value.unwrap_or_default())
        });
        let live: HashSet<_> = processes.iter().map(|p| &p.identity).collect();
        self.process_cpu
            .retain(|identity, _| live.contains(identity));

        // Deduplicate disks by name to avoid double-counting partitions on the same device
        let mut disk_read_delta = 0u64;
        let mut disk_write_delta = 0u64;
        let mut seen_disks = HashSet::new();
        let mut disk_devices = Vec::new();
        for disk in self.disks.list() {
            let name = disk.name().to_string_lossy().into_owned();
            if !seen_disks.insert(name.clone()) {
                continue;
            }
            let usage = disk.usage();
            let curr_read = usage.read_bytes;
            let curr_write = usage.written_bytes;

            let mut device_delta = None;
            if let Some(&(prev_read, prev_write)) = self.prev_disks.get(&name) {
                let r_delta = if curr_read >= prev_read {
                    curr_read - prev_read
                } else if curr_read.wrapping_sub(prev_read) < (u64::MAX / 2) {
                    curr_read.wrapping_sub(prev_read)
                } else {
                    0
                };
                let w_delta = if curr_write >= prev_write {
                    curr_write - prev_write
                } else if curr_write.wrapping_sub(prev_write) < (u64::MAX / 2) {
                    curr_write.wrapping_sub(prev_write)
                } else {
                    0
                };
                disk_read_delta = disk_read_delta.saturating_add(r_delta);
                disk_write_delta = disk_write_delta.saturating_add(w_delta);
                device_delta = Some((r_delta, w_delta));
            }
            let source = "sysinfo disk device delta";
            let metric = |value: Option<u64>| match value {
                Some(value) => Metric::available(rate(value, delta), "bytes/s", source, stamp),
                None => {
                    Metric::unavailable("bytes/s", source, stamp, AvailabilityReason::WarmingUp)
                }
            };
            let (lat, q) = {
                #[cfg(windows)]
                {
                    self.windows_gpu
                        .as_ref()
                        .and_then(|gpu| {
                            let metrics = gpu.disk_metrics();
                            metrics
                                .get(&name)
                                .or_else(|| {
                                    metrics
                                        .iter()
                                        .find(|(k, _)| {
                                            k.contains(&name) || name.contains(k.as_str())
                                        })
                                        .map(|(_, v)| v)
                                })
                                .copied()
                        })
                        .unwrap_or((None, None))
                }
                #[cfg(not(windows))]
                (None, None)
            };
            disk_devices.push(planora_core::ResourceDeviceSample {
                id: name.clone(),
                label: name.clone(),
                read_bytes_per_sec: metric(device_delta.map(|d| d.0)),
                write_bytes_per_sec: metric(device_delta.map(|d| d.1)),
                latency_ms: lat
                    .map(|v| Metric::available(v, "ms", "Windows PDH PhysicalDisk", stamp)),
                queue_depth: q
                    .map(|v| Metric::available(v, "queue", "Windows PDH PhysicalDisk", stamp)),
                link_speed_bps: None,
                link_state: None,
            });
            self.prev_disks.insert(name, (curr_read, curr_write));
        }
        self.prev_disks.retain(|name, _| seen_disks.contains(name));

        #[cfg(windows)]
        let net_links = windows_counters::query_network_links();
        #[cfg(not(windows))]
        let net_links = std::collections::HashMap::<String, ()>::new();

        // Deduplicate network interfaces and ignore loopback interfaces
        let mut net_rx_delta = 0u64;
        let mut net_tx_delta = 0u64;
        let mut network_interfaces = Vec::new();
        let mut seen_interfaces = HashSet::new();
        for (name, net) in self.networks.iter() {
            let name_lower = name.to_lowercase();
            if name_lower.contains("loopback") || name == "lo" {
                continue;
            }
            seen_interfaces.insert(name.clone());
            let curr_rx = net.total_received();
            let curr_tx = net.total_transmitted();

            let mut device_delta = None;
            if let Some(&(prev_rx, prev_tx)) = self.prev_networks.get(name) {
                let rx_d = if curr_rx >= prev_rx {
                    curr_rx - prev_rx
                } else if curr_rx.wrapping_sub(prev_rx) < (u64::MAX / 2) {
                    curr_rx.wrapping_sub(prev_rx)
                } else {
                    0
                };
                let tx_d = if curr_tx >= prev_tx {
                    curr_tx - prev_tx
                } else if curr_tx.wrapping_sub(prev_tx) < (u64::MAX / 2) {
                    curr_tx.wrapping_sub(prev_tx)
                } else {
                    0
                };
                net_rx_delta = net_rx_delta.saturating_add(rx_d);
                net_tx_delta = net_tx_delta.saturating_add(tx_d);
                device_delta = Some((rx_d, tx_d));
            }
            let source = "sysinfo interface delta";
            let metric = |value: Option<u64>| match value {
                Some(value) => Metric::available(rate(value, delta), "bytes/s", source, stamp),
                None => {
                    Metric::unavailable("bytes/s", source, stamp, AvailabilityReason::WarmingUp)
                }
            };
            #[cfg(windows)]
            let (link_speed, link_state) = {
                net_links
                    .get(name)
                    .or_else(|| {
                        net_links
                            .iter()
                            .find(|(k, _)| k.contains(name) || name.contains(k.as_str()))
                            .map(|(_, v)| v)
                    })
                    .map(|info| (info.speed_bps, info.link_state.clone()))
                    .unwrap_or((None, None))
            };
            #[cfg(not(windows))]
            let (link_speed, link_state) = (None, None);

            network_interfaces.push(planora_core::ResourceDeviceSample {
                id: name.clone(),
                label: name.clone(),
                read_bytes_per_sec: metric(device_delta.map(|d| d.0)),
                write_bytes_per_sec: metric(device_delta.map(|d| d.1)),
                latency_ms: None,
                queue_depth: None,
                link_speed_bps: link_speed
                    .map(|s| Metric::available(s, "bps", "Windows IP Helper", stamp)),
                link_state,
            });
            self.prev_networks.insert(name.clone(), (curr_rx, curr_tx));
        }
        self.prev_networks
            .retain(|name, _| seen_interfaces.contains(name));

        let measured = |value: u64, source: &str| {
            if first {
                Metric::unavailable("bytes/s", source, stamp, AvailabilityReason::WarmingUp)
            } else {
                Metric::available(rate(value, delta), "bytes/s", source, stamp)
            }
        };

        if cpu_due {
            self.system_cpu = Some(Metric::available(
                self.system.global_cpu_usage().clamp(0.0, 100.0),
                "%",
                "sysinfo total CPU delta",
                stamp,
            ));
        }
        let system_cpu = self.system_cpu.clone().unwrap_or_else(|| {
            Metric::unavailable(
                "%",
                "sysinfo total CPU delta",
                stamp,
                AvailabilityReason::WarmingUp,
            )
        });

        #[cfg(target_os = "linux")]
        let gpu = match amd_gpu_busy() {
            Ok((value, device)) => Metric::available(
                value,
                "%",
                format!("Linux amdgpu {device} gpu_busy_percent"),
                stamp,
            ),
            Err(reason) => Metric::unavailable(
                "%",
                "Linux amdgpu sysfs",
                stamp,
                AvailabilityReason::Unsupported(reason),
            ),
        };
        #[cfg(windows)]
        let (gpu_adapters, gpu_engines, gpu_engine_unavailable_reason) =
            match self.windows_gpu.as_mut() {
                Some(provider) => match provider.sample(stamp) {
                    Ok((adapters, engines)) if !engines.is_empty() => (adapters, engines, None),
                    Ok((adapters, engines)) if !adapters.is_empty() => {
                        (adapters, engines, Some(AvailabilityReason::WarmingUp))
                    }
                    Ok(_) => (
                        Vec::new(),
                        Vec::new(),
                        Some(AvailabilityReason::Unsupported(
                            "no valid GPU counter instances were reported".into(),
                        )),
                    ),
                    Err(error) => (
                        Vec::new(),
                        Vec::new(),
                        Some(AvailabilityReason::TemporarilyFailed(error.to_string())),
                    ),
                },
                None => (
                    Vec::new(),
                    Vec::new(),
                    Some(AvailabilityReason::Unsupported(
                        "GPU performance counter unavailable".into(),
                    )),
                ),
            };
        #[cfg(not(windows))]
        let gpu_adapters = Vec::new();
        #[cfg(not(windows))]
        let gpu_engines = Vec::new();
        #[cfg(not(windows))]
        let gpu_engine_unavailable_reason = Some(AvailabilityReason::Unsupported(
            "GPU Engine counter unavailable on this platform".into(),
        ));
        #[cfg(windows)]
        if let Some(sample_ms) = gpu_engines
            .first()
            .map(|engine| engine.utilization_percent.sampled_at_ms)
        {
            if sample_ms != self.gpu_last_sample_ms {
                self.gpu_validated_pids.clear();
                for process in &processes {
                    if self.gpu_last_identities.get(&process.identity.pid)
                        == Some(&process.identity)
                    {
                        self.gpu_validated_pids.insert(process.identity.pid);
                    }
                }
                self.gpu_last_identities = processes
                    .iter()
                    .map(|p| (p.identity.pid, p.identity.clone()))
                    .collect();
                self.gpu_last_sample_ms = sample_ms;
            }
            for process in &mut processes {
                let busiest = gpu_engines
                    .iter()
                    .filter(|engine| engine.process_pid == Some(process.identity.pid))
                    .filter_map(|engine| engine.utilization_percent.value)
                    .max_by(f32::total_cmp);
                process.gpu_percent = if !self.gpu_validated_pids.contains(&process.identity.pid)
                    || self.gpu_last_identities.get(&process.identity.pid)
                        != Some(&process.identity)
                {
                    Metric::unavailable(
                        "%",
                        "Windows PDH GPU Engine",
                        sample_ms,
                        AvailabilityReason::WarmingUp,
                    )
                } else if let Some(value) = busiest {
                    Metric::available(value, "%", "Windows PDH busiest process engine", sample_ms)
                } else {
                    Metric::unavailable(
                        "%",
                        "Windows PDH GPU Engine",
                        sample_ms,
                        AvailabilityReason::Unsupported(
                            "no engine instance for this process in the current PDH sample".into(),
                        ),
                    )
                };
            }
        } else {
            self.gpu_last_identities.clear();
            self.gpu_validated_pids.clear();
        }
        #[cfg(windows)]
        let gpu = if let Some(busiest) = gpu_adapters
            .iter()
            .filter(|a| a.busiest_engine_percent.value.is_some())
            .max_by(|a, b| {
                a.busiest_engine_percent
                    .value
                    .unwrap()
                    .total_cmp(&b.busiest_engine_percent.value.unwrap())
            }) {
            Metric::available(
                busiest.busiest_engine_percent.value.unwrap(),
                "%",
                "Windows PDH busiest adapter engine",
                busiest.busiest_engine_percent.sampled_at_ms,
            )
        } else {
            Metric::unavailable(
                "%",
                "OS GPU provider",
                stamp,
                AvailabilityReason::Unsupported(
                    "no unambiguous system GPU total; inspect individual engines when available"
                        .into(),
                ),
            )
        };
        #[cfg(all(not(windows), not(target_os = "linux")))]
        let gpu = Metric::unavailable(
            "%",
            "OS GPU provider",
            stamp,
            AvailabilityReason::Unsupported(
                "no unambiguous system GPU total; inspect individual engines when available".into(),
            ),
        );

        #[cfg(target_os = "linux")]
        let package_power = self.sample_rapl(now, stamp);
        #[cfg(not(target_os = "linux"))]
        let package_power = Metric::unavailable(
            "W",
            "OS power provider",
            stamp,
            AvailabilityReason::Unsupported(
                "Package power counter unavailable on this platform".into(),
            ),
        );

        let logical_cpus: Vec<planora_core::LogicalCpuSample> = self
            .system
            .cpus()
            .iter()
            .enumerate()
            .map(|(idx, cpu)| planora_core::LogicalCpuSample {
                cpu_index: (idx % 64) as u16,
                group: (idx / 64) as u16,
                utilization_percent: Metric::available(
                    cpu.cpu_usage().clamp(0.0, 100.0),
                    "%",
                    "sysinfo cpu",
                    stamp,
                ),
            })
            .collect();

        #[cfg(windows)]
        let (memory_avail, commit_used, commit_total) = match windows_counters::get_memory_status()
        {
            Some(status) => (
                Metric::available(
                    status.physical_available,
                    "bytes",
                    "Windows GlobalMemoryStatusEx",
                    stamp,
                ),
                Metric::available(
                    status.commit_used,
                    "bytes",
                    "Windows GlobalMemoryStatusEx",
                    stamp,
                ),
                Metric::available(
                    status.commit_total,
                    "bytes",
                    "Windows GlobalMemoryStatusEx",
                    stamp,
                ),
            ),
            None => (
                Metric::available(
                    self.system.available_memory(),
                    "bytes",
                    "sysinfo available memory",
                    stamp,
                ),
                Metric::unavailable(
                    "bytes",
                    "OS memory provider",
                    stamp,
                    AvailabilityReason::Unsupported("commit unavailable".into()),
                ),
                Metric::unavailable(
                    "bytes",
                    "OS memory provider",
                    stamp,
                    AvailabilityReason::Unsupported("commit unavailable".into()),
                ),
            ),
        };
        #[cfg(not(windows))]
        let (memory_avail, commit_used, commit_total) = (
            Metric::available(
                self.system.available_memory(),
                "bytes",
                "sysinfo available memory",
                stamp,
            ),
            Metric::unavailable(
                "bytes",
                "OS memory provider",
                stamp,
                AvailabilityReason::Unsupported("commit unavailable on platform".into()),
            ),
            Metric::unavailable(
                "bytes",
                "OS memory provider",
                stamp,
                AvailabilityReason::Unsupported("commit unavailable on platform".into()),
            ),
        );

        let sensors = Vec::new();

        let mut frame = TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: self.session_id,
            topology_generation: self.topology_generation,
            sequence: self.sequence,
            sampled_at_ms: stamp,
            collection_duration_ms: 0.0,
            cpu_percent: system_cpu,
            logical_cpus,
            memory_used_bytes: Metric::available(
                self.system.used_memory(),
                "bytes",
                "sysinfo physical memory",
                stamp,
            ),
            memory_available_bytes: memory_avail,
            memory_total_bytes: Metric::available(
                self.system.total_memory(),
                "bytes",
                "sysinfo physical memory",
                stamp,
            ),
            commit_used_bytes: commit_used,
            commit_total_bytes: commit_total,
            disk_read_bytes_per_sec: measured(disk_read_delta, "sysinfo disk device delta"),
            disk_write_bytes_per_sec: measured(disk_write_delta, "sysinfo disk device delta"),
            network_rx_bytes_per_sec: measured(net_rx_delta, "sysinfo interface delta"),
            network_tx_bytes_per_sec: measured(net_tx_delta, "sysinfo interface delta"),
            gpu_percent: gpu,
            gpu_adapters,
            gpu_engines,
            gpu_engine_unavailable_reason,
            disks: disk_devices,
            network_interfaces,
            package_power_watts: package_power,
            sensors,
            processes,
        };
        frame.collection_duration_ms = now.elapsed().as_secs_f32() * 1000.0;
        self.last = Some(now);
        frame
    }

    #[cfg(target_os = "linux")]
    fn sample_rapl(&mut self, now: Instant, stamp: u64) -> Metric<f32> {
        let zones = match rapl_zones() {
            Ok(zones) => zones,
            Err(reason) => {
                self.rapl.clear();
                return Metric::unavailable(
                    "W",
                    "Linux powercap RAPL",
                    stamp,
                    AvailabilityReason::Unsupported(reason),
                );
            }
        };
        let mut watts = 0.0f64;
        let mut ready = true;
        let mut live = HashSet::new();
        for (name, energy_uj, maximum_uj) in zones {
            live.insert(name.clone());
            let previous = self.rapl.insert(
                name,
                RaplSample {
                    energy_uj,
                    maximum_uj,
                    at: now,
                },
            );
            match previous.and_then(|previous| {
                (previous.maximum_uj == maximum_uj)
                    .then(|| rapl_delta(previous.energy_uj, energy_uj, maximum_uj))
                    .flatten()
                    .map(|delta| (delta, now.duration_since(previous.at)))
            }) {
                Some((delta, elapsed)) if !elapsed.is_zero() => {
                    watts += delta as f64 / 1_000_000.0 / elapsed.as_secs_f64();
                }
                _ => ready = false,
            }
        }
        self.rapl.retain(|name, _| live.contains(name));
        if ready && watts.is_finite() && watts <= f32::MAX as f64 {
            Metric::available(
                watts as f32,
                "W",
                "Linux powercap Intel RAPL package energy delta",
                stamp,
            )
        } else {
            Metric::unavailable(
                "W",
                "Linux powercap Intel RAPL",
                stamp,
                AvailabilityReason::WarmingUp,
            )
        }
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
    parse_linux_start_ticks(&text).ok_or_else(|| std::io::Error::other("invalid start ticks"))
}

#[cfg(target_os = "linux")]
fn parse_linux_start_ticks(text: &str) -> Option<u64> {
    let close = text.rfind(')')?;
    // Field 22, with state as field 3 after the final parenthesis.
    text[close + 1..]
        .split_whitespace()
        .nth(19)
        .and_then(|v| v.parse().ok())
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
        assert_eq!(
            f.cpu_percent.unavailable_reason,
            Some(AvailabilityReason::WarmingUp)
        );
        assert!(f.validate().is_ok());
    }

    #[test]
    fn cpu_waits_for_the_platform_minimum_interval() {
        let mut sampler = Sampler::new();
        let first = sampler.sample();
        let immediate = sampler.sample();
        assert!(first.cpu_percent.value.is_none());
        if sysinfo::MINIMUM_CPU_UPDATE_INTERVAL > Duration::from_millis(5) {
            assert!(immediate.cpu_percent.value.is_none());
        }
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(20));
        let later = sampler.sample();
        assert!(later.cpu_percent.value.is_some());
        let cached = sampler.sample();
        assert_eq!(
            cached.cpu_percent.sampled_at_ms,
            later.cpu_percent.sampled_at_ms
        );
        assert!(later.validate().is_ok());
    }

    #[test]
    fn delta_rate_is_in_bytes_per_second() {
        assert_eq!(rate(200, Duration::from_millis(200)), 1000);
        assert_eq!(rate(100, Duration::from_millis(100)), 1000);
        assert_eq!(rate(0, Duration::from_millis(100)), 0);
    }

    #[test]
    fn counter_wraparound_calculation() {
        let prev = u64::MAX - 99;
        let curr = 50u64;
        let delta = counter_delta(prev, curr);
        assert_eq!(delta, 150);
        let rate_val = rate_from_counters(prev, curr, Duration::from_secs(1));
        assert_eq!(rate_val, 150);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_stat_fixture_handles_parentheses_in_name() {
        let prior = std::iter::once("S".to_owned())
            .chain((4..=21).map(|field| field.to_string()))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            parse_linux_start_ticks(&format!("123 (odd ) worker) {prior} 12345")),
            Some(12345)
        );
        assert_eq!(parse_linux_start_ticks("malformed"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rapl_energy_wrap_uses_reported_range() {
        assert_eq!(rapl_delta(90, 10, 100), Some(20));
        assert_eq!(rapl_delta(20, 30, 100), Some(10));
        assert_eq!(rapl_delta(20, 100, 100), None);
        assert_eq!(rapl_delta(1, 2, 0), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_basic_or_fallback_query() {
        let result = query_windows_basic_processes();
        if let Ok(procs) = result {
            assert!(!procs.is_empty());
            assert!(procs.iter().any(|p| p.pid == std::process::id()));
        }
    }
}
