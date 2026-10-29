//! Fixed, local perf-event worker. This entrypoint runs only under its
//! dedicated systemd unit with CAP_PERFMON; it accepts no paths or raw events.
use crate::linux_pmu_wire as wire;
#[path = "linux_perf.rs"]
mod linux_perf;
use planora_core::{ErrorCode, ProfileResult};
use std::{
    fs,
    io::{self, Read},
    os::{
        fd::AsRawFd,
        unix::{
            fs::PermissionsExt,
            net::{UnixListener, UnixStream},
        },
    },
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn peer_is_service_user(stream: &UnixStream) -> bool {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    (unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    }) == 0
        && length as usize == std::mem::size_of::<libc::ucred>()
        && credentials.uid == unsafe { libc::geteuid() }
}

fn serve_client(mut stream: UnixStream) -> io::Result<()> {
    if !peer_is_service_user(&stream) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unauthorized PMU client",
        ));
    }
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let request = wire::read_request(&mut stream)?;
    if request.version != wire::VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "PMU protocol mismatch",
        ));
    }
    let result: Result<Option<ProfileResult>, (ErrorCode, String)> = match request.command {
        wire::Command::Probe => linux_perf::probe().map(|()| None),
        wire::Command::Sample {
            target,
            duration_seconds,
            sources,
        } => {
            if duration_seconds == 0
                || duration_seconds > 60
                || sources.is_empty()
                || sources.len() > 2
            {
                Err((
                    ErrorCode::InvalidInput,
                    "invalid PMU duration or source count".into(),
                ))
            } else {
                let cancel = Arc::new(AtomicBool::new(false));
                let finished = Arc::new(AtomicBool::new(false));
                let mut watcher_stream = stream.try_clone()?;
                watcher_stream.set_read_timeout(Some(Duration::from_millis(100)))?;
                let watcher_cancel = cancel.clone();
                let watcher_finished = finished.clone();
                let watcher = thread::spawn(move || {
                    let mut extra = [0u8; 1];
                    while !watcher_finished.load(Ordering::Acquire) {
                        match watcher_stream.read(&mut extra) {
                            Ok(0) | Ok(_) => {
                                watcher_cancel.store(true, Ordering::Release);
                                break;
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                                ) => {}
                            Err(_) => {
                                watcher_cancel.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }
                });
                let outcome =
                    linux_perf::sample(target, duration_seconds, &sources, &cancel).map(Some);
                finished.store(true, Ordering::Release);
                let _ = watcher.join();
                outcome
            }
        }
    };
    wire::write_frame(
        &mut stream,
        &wire::Reply {
            version: wire::VERSION,
            result,
        },
        wire::MAX_RESPONSE,
    )
}

pub fn serve() -> io::Result<()> {
    let path = std::path::Path::new(wire::SOCKET);
    if path.exists() {
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let _ = serve_client(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
