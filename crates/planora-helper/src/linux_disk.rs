//! Client for the fixed libbpf block collector running in a separate service.
//! Its private stream contains only bounded binary events and a final loss
//! count. Kernel writeback without a user-process issue identity stays separate.
use planora_core::{
    DiskIoProfileRecord, ErrorCode, IdentitySource, ProcessIdentity, ProfileResult,
};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const SOCKET: &str = "/run/planora/planora-profile.sock";
const MAX_EVENTS: u64 = 1_400_000;
const MAX_PROCESSES: usize = 10_000;
const EVENT_SIZE: usize = 72;

type ProfileError = (ErrorCode, String);

fn connect() -> Result<UnixStream, ProfileError> {
    let stream = UnixStream::connect(SOCKET).map_err(|error| {
        let code = match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ErrorCode::Unsupported,
            io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            _ => ErrorCode::Internal,
        };
        (
            code,
            format!("Linux disk profiling worker is unavailable: {error}"),
        )
    })?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    Ok(stream)
}

pub(super) fn probe() -> Result<(), ProfileError> {
    let mut stream = connect()?;
    stream
        .write_all(&0u32.to_le_bytes())
        .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    let mut result = [0u8; 1];
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match stream.read(&mut result) {
            Ok(1) if result[0] == 0 => return Ok(()),
            Ok(0) => break,
            Ok(_) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => break,
        }
    }
    Err((
        ErrorCode::Unsupported,
        "the Linux disk collector could not load or attach its packaged BPF programs".into(),
    ))
}

fn read_field(
    stream: &mut UnixStream,
    buffer: &mut [u8],
    cancel: &AtomicBool,
    cancelling_since: &mut Option<Instant>,
    deadline: Instant,
) -> Result<(), ProfileError> {
    let mut offset = 0;
    while offset < buffer.len() {
        if Instant::now() >= deadline {
            return Err((
                ErrorCode::Internal,
                "disk collector exceeded its session deadline".into(),
            ));
        }
        if cancel.load(Ordering::Acquire) && cancelling_since.is_none() {
            *cancelling_since = Some(Instant::now());
            let _ = stream.shutdown(Shutdown::Write);
        }
        if cancelling_since.is_some_and(|started| started.elapsed() >= Duration::from_secs(2)) {
            return Err((
                ErrorCode::Cancelled,
                "disk collector did not acknowledge cancellation within two seconds".into(),
            ));
        }
        match stream.read(&mut buffer[offset..]) {
            Ok(0) => {
                return Err((
                    ErrorCode::Internal,
                    "disk collector closed before its result footer".into(),
                ));
            }
            Ok(count) => offset += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => {
                return Err((
                    ErrorCode::Internal,
                    format!("disk collector stream failed: {error}"),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Event {
    issue_ns: u64,
    complete_ns: u64,
    process_start_ns: u64,
    thread_start_ns: u64,
    bytes: u64,
    pid: u32,
    tid: u32,
    direction: u32,
    request_complete: bool,
    name: String,
}

fn parse_event(raw: &[u8; EVENT_SIZE]) -> Option<Event> {
    let word = |at| u64::from_le_bytes(raw[at..at + 8].try_into().expect("fixed event ABI"));
    let dword = |at| u32::from_le_bytes(raw[at..at + 4].try_into().expect("fixed event ABI"));
    let bytes = word(32);
    let issue_ns = word(0);
    let complete_ns = word(8);
    let direction = dword(48);
    if bytes == 0 || bytes > 64 * 1024 * 1024 || complete_ns < issue_ns || direction > 1 {
        return None;
    }
    let name_end = raw[56..72].iter().position(|byte| *byte == 0).unwrap_or(16);
    // The full comm field is 16 bytes and starts at byte 56. A fixed-size
    // frame is checked before reading, so malformed names remain contained.
    let name = String::from_utf8_lossy(&raw[56..56 + name_end]).into_owned();
    Some(Event {
        issue_ns,
        complete_ns,
        process_start_ns: word(16),
        thread_start_ns: word(24),
        bytes,
        pid: dword(40),
        tid: dword(44),
        direction,
        request_complete: dword(52) != 0,
        name,
    })
}

fn start_ticks(start_ns: u64, hertz: u64) -> Option<u64> {
    let ticks = u128::from(start_ns)
        .checked_mul(u128::from(hertz))?
        .checked_div(1_000_000_000)?;
    u64::try_from(ticks).ok().filter(|ticks| *ticks > 0)
}

pub(super) fn sample(
    target: Option<ProcessIdentity>,
    duration_seconds: u32,
    cancel: &AtomicBool,
) -> Result<ProfileResult, ProfileError> {
    if !(1..=60).contains(&duration_seconds) {
        return Err((
            ErrorCode::InvalidInput,
            "disk profile duration must be 1–60 seconds".into(),
        ));
    }
    if let Some(identity) = &target
        && (identity.pid == 0
            || identity.started == 0
            || identity.source != IdentitySource::LinuxProcStartTicks
            || super::linux_perf::process_start_ticks(identity.pid) != Some(identity.started))
    {
        return Err((
            ErrorCode::PidReused,
            "the selected process creation identity is no longer valid".into(),
        ));
    }
    let hertz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hertz <= 0 {
        return Err((ErrorCode::Internal, "invalid kernel clock tick rate".into()));
    }
    let mut stream = connect()?;
    stream
        .write_all(&duration_seconds.to_le_bytes())
        .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    let mut cancelling_since = None;
    let deadline = Instant::now() + Duration::from_secs(u64::from(duration_seconds) + 5);
    let mut events_traced = 0u64;
    let mut records: HashMap<(u32, u64), DiskIoProfileRecord> = HashMap::new();
    let mut unattributed_read = 0u64;
    let mut unattributed_write = 0u64;
    let mut unattributed_ops = 0u64;
    let mut attributed_bytes = 0u64;
    let (lost, elapsed_ms, cancelled) = loop {
        let mut kind = [0u8; 1];
        read_field(
            &mut stream,
            &mut kind,
            cancel,
            &mut cancelling_since,
            deadline,
        )?;
        match kind[0] {
            1 => {
                events_traced += 1;
                if events_traced > MAX_EVENTS {
                    return Err((
                        ErrorCode::Internal,
                        "disk collector exceeded its event limit".into(),
                    ));
                }
                let mut raw = [0u8; EVENT_SIZE];
                read_field(
                    &mut stream,
                    &mut raw,
                    cancel,
                    &mut cancelling_since,
                    deadline,
                )?;
                let event = parse_event(&raw).ok_or((
                    ErrorCode::Internal,
                    "invalid disk collector event".to_owned(),
                ))?;
                let _ = (event.issue_ns, event.complete_ns);
                let started = start_ticks(event.process_start_ns, hertz as u64);
                let has_identity = event.pid != 0
                    && event.tid != 0
                    && event.thread_start_ns != 0
                    && started.is_some();
                if has_identity
                    && target.as_ref().is_some_and(|identity| {
                        identity.pid != event.pid || Some(identity.started) != started
                    })
                {
                    // The selected-process result excludes I/O that belongs
                    // to a different, positively identified process.
                    continue;
                }
                if has_identity
                    && (records.len() < MAX_PROCESSES
                        || records.contains_key(&(event.pid, started.unwrap_or_default())))
                {
                    let started = started.unwrap_or_default();
                    let record = records.entry((event.pid, started)).or_insert_with(|| {
                        DiskIoProfileRecord {
                            pid: event.pid,
                            identity: Some(ProcessIdentity {
                                pid: event.pid,
                                started,
                                sequence_number: None,
                                source: IdentitySource::LinuxProcStartTicks,
                            }),
                            process_name: event.name.clone(),
                            read_bytes: 0,
                            write_bytes: 0,
                            read_operations: 0,
                            write_operations: 0,
                        }
                    });
                    if event.direction == 0 {
                        record.read_bytes = record.read_bytes.saturating_add(event.bytes);
                        record.read_operations += u64::from(event.request_complete);
                    } else {
                        record.write_bytes = record.write_bytes.saturating_add(event.bytes);
                        record.write_operations += u64::from(event.request_complete);
                    }
                    attributed_bytes = attributed_bytes.saturating_add(event.bytes);
                } else {
                    if event.direction == 0 {
                        unattributed_read = unattributed_read.saturating_add(event.bytes);
                    } else {
                        unattributed_write = unattributed_write.saturating_add(event.bytes);
                    }
                    unattributed_ops += u64::from(event.request_complete);
                }
            }
            2 => {
                let mut footer = [0u8; 17];
                read_field(
                    &mut stream,
                    &mut footer,
                    cancel,
                    &mut cancelling_since,
                    deadline,
                )?;
                break (
                    u64::from_le_bytes(footer[..8].try_into().unwrap()),
                    u64::from_le_bytes(footer[8..16].try_into().unwrap()),
                    footer[16] != 0,
                );
            }
            _ => {
                return Err((
                    ErrorCode::Internal,
                    "invalid disk collector frame type".into(),
                ));
            }
        }
    };
    if cancelled || cancel.load(Ordering::Acquire) {
        return Err((ErrorCode::Cancelled, "disk profile cancelled".into()));
    }
    let total_bytes = attributed_bytes
        .saturating_add(unattributed_read)
        .saturating_add(unattributed_write);
    let coverage = if total_bytes == 0 {
        0.0
    } else {
        100.0 * attributed_bytes as f32 / total_bytes as f32
    };
    let mut disk_records: Vec<_> = records.into_values().collect();
    disk_records.sort_by_key(|record| {
        (
            record.pid,
            record.identity.as_ref().map_or(0, |id| id.started),
        )
    });
    Ok(ProfileResult {
        target_desc: target.map_or_else(|| "System-wide disk I/O profile".into(), |id| format!("Disk I/O profile (PID {})", id.pid)),
        provider: Some("Linux libbpf CO-RE block tracing".into()),
        sampling_semantics: Some("Completed physical block requests; kernel writeback and unverified process identities remain unattributed".into()),
        effective_sample_hz: None,
        pmu_scheduled_percent: None,
        complete: Some(lost == 0),
        duration_seconds,
        elapsed_ms,
        events_traced,
        events_lost: lost,
        attribution_coverage_percent: coverage,
        disk_records,
        pmu_records: Vec::new(),
        unattributed_read_bytes: unattributed_read,
        unattributed_write_bytes: unattributed_write,
        unattributed_operations: unattributed_ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixed_event_layout_and_rejects_invalid_operation() {
        let mut raw = [0u8; EVENT_SIZE];
        raw[8..16].copy_from_slice(&10u64.to_le_bytes());
        raw[32..40].copy_from_slice(&4096u64.to_le_bytes());
        raw[40..44].copy_from_slice(&731u32.to_le_bytes());
        raw[44..48].copy_from_slice(&734u32.to_le_bytes());
        raw[48..52].copy_from_slice(&1u32.to_le_bytes());
        raw[56..58].copy_from_slice(b"dd");
        let event = parse_event(&raw).unwrap();
        assert_eq!(
            (event.pid, event.tid, event.name.as_str()),
            (731, 734, "dd")
        );
        assert_eq!(event.bytes, 4096);
        raw[48..52].copy_from_slice(&2u32.to_le_bytes());
        assert!(parse_event(&raw).is_none());
    }

    #[test]
    fn nanoseconds_to_start_ticks_are_bounded() {
        assert_eq!(start_ticks(144_667_000, 100), Some(14));
        assert_eq!(start_ticks(0, 100), None);
    }
}
