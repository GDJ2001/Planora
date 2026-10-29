//! Unprivileged client for the dedicated CAP_PERFMON worker.
use crate::linux_pmu_wire as wire;
use planora_core::{ErrorCode, PmuSourceInfo, ProcessIdentity, ProfileResult};
use std::{
    fs,
    io::{self, Read},
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

type ProfileError = (ErrorCode, String);

pub(super) fn enumerate_sources() -> Vec<PmuSourceInfo> {
    let root = std::path::Path::new("/sys/bus/event_source/devices/cpu");
    if !root.join("type").is_file() {
        return Vec::new();
    }
    [("cpu-cycles", 0), ("instructions", 1)]
        .into_iter()
        .filter(|(name, _)| fs::metadata(root.join("events").join(name)).is_ok())
        .map(|(name, id)| PmuSourceInfo {
            id,
            name: name.to_owned(),
            min_interval: 1,
            max_interval: 99,
            description: Some("Adaptive sampled hardware event for one process; values are sample counts, not exact event totals".into()),
            sampling_unit: Some("samples".into()),
        })
        .collect()
}

fn read_exact_until(
    stream: &mut UnixStream,
    buffer: &mut [u8],
    cancel: Option<&AtomicBool>,
    deadline: Instant,
) -> Result<(), ProfileError> {
    let mut offset = 0;
    let mut cancelling_since = None;
    while offset < buffer.len() {
        if cancel.is_some_and(|flag| flag.load(Ordering::Acquire)) && cancelling_since.is_none() {
            cancelling_since = Some(Instant::now());
            let _ = stream.shutdown(Shutdown::Write);
        }
        if Instant::now() >= deadline
            || cancelling_since.is_some_and(|at: Instant| at.elapsed() >= Duration::from_secs(2))
        {
            return Err((
                ErrorCode::Internal,
                "PMU worker did not finish within its deadline".into(),
            ));
        }
        match stream.read(&mut buffer[offset..]) {
            Ok(0) => return Err((ErrorCode::Internal, "PMU worker disconnected".into())),
            Ok(count) => offset += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => {
                return Err((
                    ErrorCode::Internal,
                    format!("PMU worker read failed: {error}"),
                ));
            }
        }
    }
    Ok(())
}

fn request(
    command: wire::Command,
    cancel: Option<&AtomicBool>,
    timeout_seconds: u32,
) -> Result<wire::Reply, ProfileError> {
    let mut stream = UnixStream::connect(wire::SOCKET).map_err(|error| {
        let code = if error.kind() == io::ErrorKind::PermissionDenied {
            ErrorCode::PermissionDenied
        } else {
            ErrorCode::Unsupported
        };
        (code, format!("Linux PMU worker is unavailable: {error}"))
    })?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    wire::write_frame(
        &mut stream,
        &wire::Request {
            version: wire::VERSION,
            command,
        },
        wire::MAX_REQUEST,
    )
    .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
    let mut length = [0u8; 4];
    read_exact_until(&mut stream, &mut length, cancel, deadline)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > wire::MAX_RESPONSE {
        return Err((ErrorCode::Internal, "invalid PMU reply size".into()));
    }
    let mut payload = vec![0u8; length];
    read_exact_until(&mut stream, &mut payload, cancel, deadline)?;
    let reply: wire::Reply = serde_json::from_slice(&payload)
        .map_err(|error| (ErrorCode::Internal, format!("invalid PMU reply: {error}")))?;
    if reply.version != wire::VERSION {
        return Err((
            ErrorCode::Unsupported,
            "PMU worker protocol mismatch".into(),
        ));
    }
    Ok(reply)
}

pub(super) fn probe() -> Result<(), ProfileError> {
    request(wire::Command::Probe, None, 2)?.result.map(|_| ())
}

pub(super) fn sample(
    target: Option<ProcessIdentity>,
    duration_seconds: u32,
    sources: &[String],
    cancel: &Arc<AtomicBool>,
) -> Result<ProfileResult, ProfileError> {
    let duration_seconds = if duration_seconds == 0 {
        15
    } else {
        duration_seconds
    };
    if duration_seconds > 60 || sources.is_empty() || sources.len() > 2 {
        return Err((ErrorCode::InvalidInput, "invalid PMU profile limits".into()));
    }
    let reply = request(
        wire::Command::Sample {
            target,
            duration_seconds,
            sources: sources.to_vec(),
        },
        Some(cancel),
        duration_seconds + 5,
    )?;
    if cancel.load(Ordering::Acquire) {
        return Err((ErrorCode::Cancelled, "PMU sampling cancelled".into()));
    }
    reply.result?.ok_or((
        ErrorCode::Internal,
        "PMU worker returned no sample result".into(),
    ))
}
