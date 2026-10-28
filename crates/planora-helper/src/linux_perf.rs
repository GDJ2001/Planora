//! Linux sampled PMU collection. Only the two fixed hardware events below are
//! accepted; event names and process identities never become paths or commands.
use planora_core::{
    ErrorCode, IdentitySource, PmuProfileRecord, PmuSourceInfo, ProcessIdentity, ProfileResult,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const RING_PAGES: usize = 8;
const MAX_THREADS: usize = 64;
const SAMPLE_HZ: u64 = 99;
const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_RECORD_LOST: u32 = 2;
const PERF_RECORD_SAMPLE: u32 = 9;
const PERF_RECORD_LOST_SAMPLES: u32 = 13;

#[repr(C)]
struct PerfAttr {
    event_type: u32,
    size: u32,
    config: u64,
    sample_freq: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    config1: u64,
    config2: u64,
}

fn source_config(name: &str) -> Option<u64> {
    match name {
        "cpu-cycles" => Some(0),
        "instructions" => Some(1),
        _ => None,
    }
}

pub(super) fn enumerate_sources() -> Vec<PmuSourceInfo> {
    // The generic hardware events are offered only when the kernel exposes
    // its CPU PMU. An actual open still checks the current security policy.
    let root = std::path::Path::new("/sys/bus/event_source/devices/cpu");
    if !root.join("type").is_file() {
        return Vec::new();
    }
    ["cpu-cycles", "instructions"]
        .into_iter()
        .filter(|name| root.join("events").join(name).is_file())
        .map(|name| PmuSourceInfo {
            id: source_config(name).unwrap_or_default() as u32,
            name: name.to_owned(),
            // Linux uses an adaptive sample frequency, not an exact counter
            // interval. These fields are retained for the v5 wire structure.
            min_interval: 1,
            max_interval: SAMPLE_HZ as u32,
            description: Some("Adaptive sampled hardware event for one process; values are sample counts, not exact event totals".to_owned()),
            sampling_unit: Some("samples".to_owned()),
        })
        .collect()
}

pub(super) fn probe() -> Result<(), (ErrorCode, String)> {
    let source = enumerate_sources().into_iter().next().ok_or((
        ErrorCode::Unsupported,
        "the kernel exposes no supported CPU PMU source".to_owned(),
    ))?;
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return Err((ErrorCode::Internal, "invalid system page size".to_owned()));
    }
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as u32;
    let ring = open_ring(
        tid,
        process_start_ticks(tid).unwrap_or_default(),
        source.id as u64,
        0,
        page as usize,
    )?;
    drop(ring);
    Ok(())
}

fn start_ticks_at(path: &str) -> Option<u64> {
    let stat = fs::read_to_string(path).ok()?;
    let fields = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    fields.get(19)?.parse().ok() // field 22, after state at field 3
}

fn process_start_ticks(pid: u32) -> Option<u64> {
    start_ticks_at(&format!("/proc/{pid}/stat"))
}

fn identity_valid(identity: &ProcessIdentity) -> bool {
    identity.pid != 0
        && identity.started != 0
        && identity.source == IdentitySource::LinuxProcStartTicks
        && process_start_ticks(identity.pid) == Some(identity.started)
}

fn thread_ids(pid: u32) -> Result<Vec<(u32, u64)>, (ErrorCode, String)> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(format!("/proc/{pid}/task")).map_err(|e| {
        (
            ErrorCode::InvalidInput,
            format!("cannot inspect target threads: {e}"),
        )
    })? {
        let entry = entry.map_err(|e| (ErrorCode::Internal, e.to_string()))?;
        if let Ok(id) = entry.file_name().to_string_lossy().parse::<u32>()
            && let Some(started) = start_ticks_at(&format!("/proc/{pid}/task/{id}/stat"))
        {
            ids.push((id, started));
        }
        if ids.len() > MAX_THREADS {
            return Err((
                ErrorCode::InvalidInput,
                format!("target exceeds the {MAX_THREADS}-thread profiling limit"),
            ));
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

struct Ring {
    fd: OwnedFd,
    mapping: *mut u8,
    length: usize,
    source: usize,
    thread_started: u64,
}

impl Drop for Ring {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mapping.cast(), self.length);
        }
    }
}

fn open_ring(
    tid: u32,
    thread_started: u64,
    config: u64,
    source: usize,
    page: usize,
) -> Result<Ring, (ErrorCode, String)> {
    let attr = PerfAttr {
        event_type: PERF_TYPE_HARDWARE,
        size: std::mem::size_of::<PerfAttr>() as u32,
        config,
        sample_freq: SAMPLE_HZ,
        sample_type: (1 << 1) | (1 << 2), // PERF_SAMPLE_TID | PERF_SAMPLE_TIME
        // PERF_FORMAT_TOTAL_TIME_ENABLED | PERF_FORMAT_TOTAL_TIME_RUNNING:
        // report when the PMU had to multiplex the requested events.
        read_format: 3,
        // disabled, exclude kernel/hypervisor, frequency mode
        flags: 1 | (1 << 5) | (1 << 6) | (1 << 10),
        wakeup_events: 1,
        bp_type: 0,
        config1: 0,
        config2: 0,
    };
    let raw = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            &attr,
            tid as libc::pid_t,
            -1,
            -1,
            0,
        )
    };
    if raw < 0 {
        let error = std::io::Error::last_os_error();
        let code = match error.raw_os_error() {
            Some(libc::EACCES | libc::EPERM) => ErrorCode::Unauthorized,
            Some(libc::ENOENT | libc::EOPNOTSUPP) => ErrorCode::Unsupported,
            _ => ErrorCode::Internal,
        };
        return Err((code, format!("perf_event_open failed: {error}")));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
    let length = (RING_PAGES + 1) * page;
    let mapping = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            length,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    };
    if mapping == libc::MAP_FAILED {
        return Err((
            ErrorCode::Internal,
            format!("perf mmap failed: {}", std::io::Error::last_os_error()),
        ));
    }
    let ring = Ring {
        fd,
        mapping: mapping.cast(),
        length,
        source,
        thread_started,
    };
    // PERF_EVENT_IOC_ENABLE = _IO('$', 0); setting it after mmap avoids losing
    // the first sample before the consumer is ready.
    if unsafe { libc::ioctl(ring.fd.as_raw_fd(), 0x2400) } < 0 {
        return Err((
            ErrorCode::Internal,
            format!("perf enable failed: {}", std::io::Error::last_os_error()),
        ));
    }
    Ok(ring)
}

fn read_u64_at(mapping: *mut u8, offset: usize) -> u64 {
    unsafe { std::ptr::read_volatile(mapping.add(offset).cast::<u64>()) }
}

fn copy_ring_bytes(
    mapping: *mut u8,
    data_offset: usize,
    data_size: usize,
    start: u64,
    out: &mut [u8],
) {
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = unsafe { *mapping.add(data_offset + ((start as usize + index) % data_size)) };
    }
}

fn drain(ring: &Ring, samples: &mut [u64], lost: &mut u64) {
    let data_offset = read_u64_at(ring.mapping, 1040) as usize;
    let data_size = read_u64_at(ring.mapping, 1048) as usize;
    if data_offset < 4096 || data_size == 0 || data_offset.saturating_add(data_size) > ring.length {
        return;
    }
    let head = read_u64_at(ring.mapping, 1024);
    std::sync::atomic::fence(Ordering::Acquire);
    let mut tail = read_u64_at(ring.mapping, 1032);
    if head.saturating_sub(tail) > data_size as u64 {
        *lost = lost.saturating_add(1);
        tail = head - data_size as u64;
    }
    while tail + 8 <= head {
        let mut header = [0u8; 8];
        copy_ring_bytes(ring.mapping, data_offset, data_size, tail, &mut header);
        let record_type = u32::from_ne_bytes(header[0..4].try_into().unwrap());
        let size = u16::from_ne_bytes(header[6..8].try_into().unwrap()) as usize;
        if size < 8 || size > data_size || tail + size as u64 > head {
            *lost = lost.saturating_add(1);
            tail = head;
            break;
        }
        match record_type {
            PERF_RECORD_SAMPLE if size >= 24 => {
                samples[ring.source] = samples[ring.source].saturating_add(1)
            }
            PERF_RECORD_LOST if size >= 24 => {
                let mut payload = [0u8; 8];
                copy_ring_bytes(
                    ring.mapping,
                    data_offset,
                    data_size,
                    tail + 16,
                    &mut payload,
                );
                *lost = lost.saturating_add(u64::from_ne_bytes(payload));
            }
            PERF_RECORD_LOST_SAMPLES if size >= 16 => {
                let mut payload = [0u8; 8];
                copy_ring_bytes(ring.mapping, data_offset, data_size, tail + 8, &mut payload);
                *lost = lost.saturating_add(u64::from_ne_bytes(payload));
            }
            _ => {}
        }
        tail += size as u64;
    }
    std::sync::atomic::fence(Ordering::Release);
    unsafe {
        std::ptr::write_volatile(ring.mapping.add(1032).cast::<u64>(), tail);
    }
}

fn scheduled_times(ring: &Ring) -> Option<(u64, u64)> {
    let mut counters = [0u64; 3];
    let bytes = unsafe {
        libc::read(
            ring.fd.as_raw_fd(),
            counters.as_mut_ptr().cast(),
            std::mem::size_of_val(&counters),
        )
    };
    (bytes == std::mem::size_of_val(&counters) as isize).then_some((counters[1], counters[2]))
}

pub(super) fn sample(
    target: Option<ProcessIdentity>,
    duration_seconds: u32,
    sources: &[String],
    cancel: &Arc<AtomicBool>,
) -> Result<ProfileResult, (ErrorCode, String)> {
    let target = target.ok_or((
        ErrorCode::InvalidInput,
        "select one process for PMU sampling".to_owned(),
    ))?;
    if !identity_valid(&target) {
        return Err((
            ErrorCode::InvalidInput,
            "target process creation identity changed or is unsupported".to_owned(),
        ));
    }
    if sources.is_empty() || sources.len() > 2 || sources.iter().any(|s| source_config(s).is_none())
    {
        return Err((
            ErrorCode::InvalidInput,
            "choose one or two discovered PMU sources".to_owned(),
        ));
    }
    let discovered: HashSet<_> = enumerate_sources().into_iter().map(|s| s.name).collect();
    if sources.iter().any(|s| !discovered.contains(s)) {
        return Err((
            ErrorCode::Unsupported,
            "requested PMU source is not exposed by this kernel".to_owned(),
        ));
    }
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return Err((ErrorCode::Internal, "invalid system page size".to_owned()));
    }
    let page = page as usize;
    let started = Instant::now();
    let duration = Duration::from_secs(if duration_seconds == 0 {
        15
    } else {
        duration_seconds.min(60)
    } as u64);
    let mut rings: HashMap<(u32, usize), Ring> = HashMap::new();
    let mut samples = vec![0u64; sources.len()];
    let mut lost = 0u64;
    let mut enabled = 0u128;
    let mut running = 0u128;
    while started.elapsed() < duration {
        if cancel.load(Ordering::Relaxed) {
            return Err((ErrorCode::Cancelled, "PMU sampling cancelled".to_owned()));
        }
        if !identity_valid(&target) {
            break;
        }
        let tids = thread_ids(target.pid)?;
        let live: HashMap<_, _> = tids.iter().copied().collect();
        rings.retain(|(tid, _), ring| {
            if live.get(tid) == Some(&ring.thread_started) {
                true
            } else {
                drain(ring, &mut samples, &mut lost);
                if let Some((event_enabled, event_running)) = scheduled_times(ring) {
                    enabled = enabled.saturating_add(u128::from(event_enabled));
                    running = running.saturating_add(u128::from(event_running));
                }
                false
            }
        });
        for (tid, thread_started) in tids {
            for (index, source) in sources.iter().enumerate() {
                if let std::collections::hash_map::Entry::Vacant(entry) = rings.entry((tid, index))
                {
                    let path = format!("/proc/{}/task/{tid}/stat", target.pid);
                    if start_ticks_at(&path) != Some(thread_started) {
                        continue;
                    }
                    match open_ring(
                        tid,
                        thread_started,
                        source_config(source).unwrap(),
                        index,
                        page,
                    ) {
                        Ok(ring) if start_ticks_at(&path) == Some(thread_started) => {
                            entry.insert(ring);
                        }
                        Ok(_) => {}
                        Err(error) if start_ticks_at(&path) == Some(thread_started) => {
                            return Err(error);
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        for ring in rings.values() {
            drain(ring, &mut samples, &mut lost);
        }
        thread::sleep(Duration::from_millis(50).min(duration.saturating_sub(started.elapsed())));
    }
    for ring in rings.values() {
        drain(ring, &mut samples, &mut lost);
    }
    for ring in rings.values() {
        if let Some((event_enabled, event_running)) = scheduled_times(ring) {
            enabled = enabled.saturating_add(u128::from(event_enabled));
            running = running.saturating_add(u128::from(event_running));
        }
    }
    let pmu_scheduled_percent =
        (enabled > 0).then(|| (100.0 * running as f64 / enabled as f64).clamp(0.0, 100.0) as f32);
    let events_traced: u64 = samples.iter().sum();
    let complete = lost == 0 && identity_valid(&target);
    let coverage = if events_traced.saturating_add(lost) == 0 {
        0.0
    } else {
        100.0 * events_traced as f32 / events_traced.saturating_add(lost) as f32
    };
    Ok(ProfileResult {
        target_desc: format!("Linux perf sampled profile (PID {})", target.pid),
        provider: Some("Linux perf_event_open".into()),
        sampling_semantics: Some(
            "Adaptive per-thread sampled hardware events; counts are not exact event totals".into(),
        ),
        effective_sample_hz: Some(SAMPLE_HZ as u32),
        pmu_scheduled_percent,
        complete: Some(complete),
        duration_seconds: duration.as_secs() as u32,
        elapsed_ms: started.elapsed().as_millis() as u64,
        events_traced,
        events_lost: lost,
        attribution_coverage_percent: coverage,
        disk_records: Vec::new(),
        pmu_records: sources
            .iter()
            .enumerate()
            .map(|(index, source)| PmuProfileRecord {
                source: source.clone(),
                sample_count: samples[index],
                target_pid: Some(target.pid),
                target_identity: Some(target.clone()),
            })
            .collect(),
        unattributed_read_bytes: 0,
        unattributed_write_bytes: 0,
        unattributed_operations: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_allowlist_rejects_paths_and_unknown_events() {
        assert_eq!(source_config("cpu-cycles"), Some(0));
        assert_eq!(source_config("instructions"), Some(1));
        assert_eq!(source_config("../../events/raw"), None);
    }
    #[test]
    fn start_identity_matches_current_process() {
        let pid = std::process::id();
        let ticks = process_start_ticks(pid).unwrap();
        let id = ProcessIdentity {
            pid,
            started: ticks,
            sequence_number: None,
            source: IdentitySource::LinuxProcStartTicks,
        };
        assert!(identity_valid(&id));
        assert!(!identity_valid(&ProcessIdentity {
            started: ticks + 1,
            ..id
        }));
    }
}
