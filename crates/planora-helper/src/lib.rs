//! Narrow, versioned local IPC for the privileged collector.
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerOptions, Stream, prelude::*};
use planora_core::{
    Command, MAX_REQUEST_SIZE, MAX_RESPONSE_SIZE, PROTOCOL_VERSION, Request, Response,
    ResponseEnvelope,
};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

#[cfg(windows)]
const ENDPOINT: &str = "planora-v2";
#[cfg(target_os = "linux")]
const ENDPOINT: &str = "/run/planora/planora-v2.sock";
#[cfg(target_os = "macos")]
const ENDPOINT: &str = "/var/run/planora/planora-v2.sock";

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

fn name() -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        ENDPOINT.to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        ENDPOINT.to_fs_name::<GenericFilePath>()
    }
}

pub fn connect() -> io::Result<Stream> {
    Stream::connect(name()?)
}

/// Send a request with size enforcement (max 64 KiB).
pub fn send_request(stream: &mut impl Write, req: &Request) -> io::Result<()> {
    let data = serde_json::to_vec(req).map_err(io::Error::other)?;
    if data.len() > MAX_REQUEST_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "request size ({} bytes) exceeds limit of {} bytes",
                data.len(),
                MAX_REQUEST_SIZE
            ),
        ));
    }
    stream.write_all(&(data.len() as u32).to_le_bytes())?;
    stream.write_all(&data)?;
    stream.flush()
}

/// Receive a request with size enforcement (max 64 KiB).
pub fn receive_request(stream: &mut impl Read) -> io::Result<Request> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let len = u32::from_le_bytes(length) as usize;
    if len > MAX_REQUEST_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("request size ({len} bytes) exceeds limit of {MAX_REQUEST_SIZE} bytes"),
        ));
    }
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}

/// Send a response with size enforcement (max 8 MiB).
pub fn send_response(stream: &mut impl Write, resp: &ResponseEnvelope) -> io::Result<()> {
    let data = serde_json::to_vec(resp).map_err(io::Error::other)?;
    if data.len() > MAX_RESPONSE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "response size ({} bytes) exceeds limit of {} bytes",
                data.len(),
                MAX_RESPONSE_SIZE
            ),
        ));
    }
    stream.write_all(&(data.len() as u32).to_le_bytes())?;
    stream.write_all(&data)?;
    stream.flush()
}

/// Receive a response with size enforcement (max 8 MiB).
pub fn receive_response(stream: &mut impl Read) -> io::Result<ResponseEnvelope> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let len = u32::from_le_bytes(length) as usize;
    if len > MAX_RESPONSE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("response size ({len} bytes) exceeds limit of {MAX_RESPONSE_SIZE} bytes"),
        ));
    }
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}

/// Read exact bytes with absolute deadline enforcement on Stream without accumulating blocked threads.
#[cfg(windows)]
pub fn read_exact_deadline(
    stream: &mut Stream,
    buf: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    let raw_handle = match stream {
        Stream::NamedPipe(pipe) => pipe.as_handle().as_raw_handle(),
    };

    let mut total_read = 0;
    while total_read < buf.len() {
        let mut avail: u32 = 0;
        let ok = unsafe {
            PeekNamedPipe(
                raw_handle as _,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut avail,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if avail == 0 {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC receive deadline exceeded",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        let to_read = (buf.len() - total_read).min(avail as usize) as u32;
        let mut bytes_read: u32 = 0;
        let ok = unsafe {
            ReadFile(
                raw_handle as _,
                buf[total_read..].as_mut_ptr() as _,
                to_read,
                &mut bytes_read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "IPC peer disconnected",
            ));
        }
        total_read += bytes_read as usize;
    }
    Ok(())
}

/// Write all bytes with absolute deadline enforcement on Stream without accumulating blocked threads.
#[cfg(windows)]
pub fn write_all_deadline(stream: &mut Stream, data: &[u8], deadline: Instant) -> io::Result<()> {
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use windows_sys::Win32::Foundation::{ERROR_NO_DATA, ERROR_PIPE_BUSY};
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Pipes::{PIPE_NOWAIT, PIPE_WAIT, SetNamedPipeHandleState};

    let raw_handle = match stream {
        Stream::NamedPipe(pipe) => pipe.as_handle().as_raw_handle(),
    };

    let nowait_mode = PIPE_NOWAIT;
    let ok = unsafe {
        SetNamedPipeHandleState(
            raw_handle as _,
            &nowait_mode,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    struct RestorePipeMode(isize);
    impl Drop for RestorePipeMode {
        fn drop(&mut self) {
            let wait_mode = PIPE_WAIT;
            unsafe {
                SetNamedPipeHandleState(
                    self.0 as _,
                    &wait_mode,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
    }
    let _restore = RestorePipeMode(raw_handle as isize);

    let mut total_written = 0;
    while total_written < data.len() {
        let chunk_len = (data.len() - total_written).min(65536) as u32;
        let mut written: u32 = 0;
        let ok = unsafe {
            WriteFile(
                raw_handle as _,
                data[total_written..].as_ptr() as _,
                chunk_len,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok != 0 {
            total_written += written as usize;
        } else {
            let err = io::Error::last_os_error();
            let raw_code = err.raw_os_error().unwrap_or(0) as u32;
            if raw_code == ERROR_NO_DATA || raw_code == ERROR_PIPE_BUSY {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "IPC send deadline exceeded",
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            } else {
                return Err(err);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_nonblocking_unix(fd: std::os::unix::io::RawFd, nonblocking: bool) -> io::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        let new_flags = if nonblocking {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        if libc::fcntl(fd, libc::F_SETFL, new_flags) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn read_exact_deadline(
    stream: &mut Stream,
    buf: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    set_nonblocking_unix(fd, true)?;

    struct RestoreSocketMode(std::os::unix::io::RawFd);
    impl Drop for RestoreSocketMode {
        fn drop(&mut self) {
            let _ = set_nonblocking_unix(self.0, false);
        }
    }
    let _restore = RestoreSocketMode(fd);

    let mut total_read = 0;
    while total_read < buf.len() {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IPC receive deadline exceeded",
            ));
        }
        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = (remaining.as_millis().min(i32::MAX as u128) as i32).max(1);

        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        let res = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if res < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if res == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IPC receive deadline exceeded",
            ));
        }

        let n = unsafe {
            libc::read(
                fd,
                buf[total_read..].as_mut_ptr() as *mut libc::c_void,
                buf.len() - total_read,
            )
        };
        if n > 0 {
            total_read += n as usize;
        } else if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "IPC peer disconnected",
            ));
        } else {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn write_all_deadline(stream: &mut Stream, data: &[u8], deadline: Instant) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    set_nonblocking_unix(fd, true)?;

    struct RestoreSocketMode(std::os::unix::io::RawFd);
    impl Drop for RestoreSocketMode {
        fn drop(&mut self) {
            let _ = set_nonblocking_unix(self.0, false);
        }
    }
    let _restore = RestoreSocketMode(fd);

    let mut total_written = 0;
    while total_written < data.len() {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IPC send deadline exceeded",
            ));
        }
        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = (remaining.as_millis().min(i32::MAX as u128) as i32).max(1);

        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLOUT | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        let res = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if res < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if res == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IPC send deadline exceeded",
            ));
        }

        let n = unsafe {
            libc::write(
                fd,
                data[total_written..].as_ptr() as *const libc::c_void,
                data.len() - total_written,
            )
        };
        if n > 0 {
            total_written += n as usize;
        } else if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "IPC write returned zero",
            ));
        } else {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
    }
    Ok(())
}

/// Send a request with deadline enforcement on the Stream.
pub fn send_request_deadline(
    stream: &mut Stream,
    req: &Request,
    deadline: Instant,
) -> io::Result<()> {
    let data = serde_json::to_vec(req).map_err(io::Error::other)?;
    if data.len() > MAX_REQUEST_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "request size ({} bytes) exceeds limit of {} bytes",
                data.len(),
                MAX_REQUEST_SIZE
            ),
        ));
    }
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(&data);
    write_all_deadline(stream, &frame, deadline)
}

/// Receive a request with deadline enforcement on the Stream.
pub fn receive_request_deadline(stream: &mut Stream, deadline: Instant) -> io::Result<Request> {
    let mut length = [0u8; 4];
    read_exact_deadline(stream, &mut length, deadline)?;
    let len = u32::from_le_bytes(length) as usize;
    if len > MAX_REQUEST_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("request size ({len} bytes) exceeds limit of {MAX_REQUEST_SIZE} bytes"),
        ));
    }
    let mut data = vec![0u8; len];
    read_exact_deadline(stream, &mut data, deadline)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}

/// Send a response with deadline enforcement on the Stream.
pub fn send_response_deadline(
    stream: &mut Stream,
    resp: &ResponseEnvelope,
    deadline: Instant,
) -> io::Result<()> {
    let data = serde_json::to_vec(resp).map_err(io::Error::other)?;
    if data.len() > MAX_RESPONSE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "response size ({} bytes) exceeds limit of {} bytes",
                data.len(),
                MAX_RESPONSE_SIZE
            ),
        ));
    }
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(&data);
    write_all_deadline(stream, &frame, deadline)
}

/// Receive a response with deadline enforcement on the Stream.
pub fn receive_response_deadline(
    stream: &mut Stream,
    deadline: Instant,
) -> io::Result<ResponseEnvelope> {
    let mut length = [0u8; 4];
    read_exact_deadline(stream, &mut length, deadline)?;
    let len = u32::from_le_bytes(length) as usize;
    if len > MAX_RESPONSE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("response size ({len} bytes) exceeds limit of {MAX_RESPONSE_SIZE} bytes"),
        ));
    }
    let mut data = vec![0u8; len];
    read_exact_deadline(stream, &mut data, deadline)?;
    serde_json::from_slice(&data).map_err(io::Error::other)
}

pub fn send_request_timeout(
    stream: &mut Stream,
    req: &Request,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(60));
    send_request_deadline(stream, req, deadline)
}

pub fn receive_request_timeout(stream: &mut Stream, timeout: Duration) -> io::Result<Request> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(60));
    receive_request_deadline(stream, deadline)
}

pub fn send_response_timeout(
    stream: &mut Stream,
    resp: &ResponseEnvelope,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(60));
    send_response_deadline(stream, resp, deadline)
}

pub fn receive_response_timeout(
    stream: &mut Stream,
    timeout: Duration,
) -> io::Result<ResponseEnvelope> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(60));
    receive_response_deadline(stream, deadline)
}

/// Transact a command to the helper over a dedicated local IPC connection with strict deadlines.
pub fn transact(command: &Command) -> io::Result<Response> {
    static REQUEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut stream = connect()?;

    // Perform handshake with HANDSHAKE_TIMEOUT deadline
    let handshake_id = REQUEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    send_request_deadline(
        &mut stream,
        &Request {
            request_id: handshake_id,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        },
        Instant::now() + HANDSHAKE_TIMEOUT,
    )?;

    let handshake_env = receive_response_timeout(&mut stream, HANDSHAKE_TIMEOUT)?;
    if handshake_env.request_id != handshake_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "handshake request ID mismatch: sent {handshake_id}, got {}",
                handshake_env.request_id
            ),
        ));
    }
    match handshake_env.response {
        Response::Hello {
            version,
            helper_authorized: true,
            ..
        } if version == PROTOCOL_VERSION => {}
        Response::Hello { version, .. } => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "protocol version mismatch: helper is v{version}, client is v{PROTOCOL_VERSION}"
                ),
            ));
        }
        Response::Error { code, message } => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("helper handshake error: [{code}] {message}"),
            ));
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "helper handshake failed",
            ));
        }
    }

    // Send payload command with REQUEST_TIMEOUT deadline
    let req_id = REQUEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    send_request_deadline(
        &mut stream,
        &Request {
            request_id: req_id,
            command: command.clone(),
        },
        Instant::now() + REQUEST_TIMEOUT,
    )?;

    let resp_env = receive_response_timeout(&mut stream, REQUEST_TIMEOUT)?;
    if resp_env.request_id != req_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "request ID mismatch: sent {}, got {}",
                req_id, resp_env.request_id
            ),
        ));
    }

    Ok(resp_env.response)
}

pub fn listener() -> io::Result<interprocess::local_socket::Listener> {
    let options = ListenerOptions::new().name(name()?);
    #[cfg(unix)]
    {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            let directory = std::path::Path::new("/var/run/planora");
            std::fs::create_dir_all(directory)?;
            let name = std::ffi::CString::new(directory.as_os_str().as_bytes())
                .map_err(io::Error::other)?;
            if unsafe { libc::chmod(name.as_ptr(), 0o755) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        #[cfg(target_os = "linux")]
        {
            let directory = std::path::Path::new("/run/planora");
            std::fs::create_dir_all(directory)?;
        }

        let result = options.mode(0o660).create_sync()?;

        #[cfg(target_os = "macos")]
        {
            let name = std::ffi::CString::new(ENDPOINT).map_err(io::Error::other)?;
            let group = std::ffi::CString::new("staff").unwrap();
            let entry = unsafe { libc::getgrnam(group.as_ptr()) };
            if entry.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "staff group unavailable",
                ));
            }
            let gid = unsafe { (*entry).gr_gid };
            if unsafe { libc::chown(name.as_ptr(), u32::MAX, gid) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(result)
    }
    #[cfg(windows)]
    {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sid = std::env::var("PLANORA_ALLOWED_SID").map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "service installer must configure PLANORA_ALLOWED_SID",
            )
        })?;
        if !sid.starts_with("S-1-")
            || !sid
                .chars()
                .all(|c| c.is_ascii_digit() || c == 'S' || c == '-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid authorized user SID",
            ));
        }
        let sddl = widestring::U16CString::from_str(format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{sid})"))
            .map_err(io::Error::other)?;
        let descriptor = SecurityDescriptor::deserialize(&sddl)?;
        options.security_descriptor(descriptor).create_sync()
    }
}

/// Peer credentials check ensuring caller is local and authorized.
#[cfg(unix)]
pub fn peer_authorized(stream: &Stream) -> bool {
    let peer_uid = stream.peer_creds().ok().and_then(|p| p.euid());
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let path = "/Library/Application Support/Planora/authorized-uid";
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        // Protect against symlink attacks and unsafe permissions
        if metadata.file_type().is_symlink() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0
        {
            return false;
        }
        let approved_uid = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        return peer_uid.is_some() && peer_uid == approved_uid;
    }
    #[cfg(not(target_os = "macos"))]
    {
        peer_uid.is_some()
    }
}

#[cfg(windows)]
pub fn peer_authorized(stream: &Stream) -> bool {
    // On Windows, the Security Descriptor restricts access to the authorized SID at pipe creation.
    // In addition, verify that the caller has a valid local PID.
    stream.peer_creds().ok().and_then(|p| p.pid()).is_some()
}

pub fn caller_id(stream: &Stream) -> u64 {
    #[cfg(unix)]
    {
        stream
            .peer_creds()
            .ok()
            .and_then(|p| p.euid())
            .map(|uid| uid as u64)
            .unwrap_or(0)
    }
    #[cfg(windows)]
    {
        stream
            .peer_creds()
            .ok()
            .and_then(|p| p.pid())
            .map(|pid| pid as u64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planora_core::ErrorCode;

    #[test]
    fn rejects_oversize_request() {
        let input = ((MAX_REQUEST_SIZE + 1) as u32).to_le_bytes();
        let result = receive_request(&mut input.as_slice());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_oversize_response() {
        let input = ((MAX_RESPONSE_SIZE + 1) as u32).to_le_bytes();
        let result = receive_response(&mut input.as_slice());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn request_response_serialization_round_trip() {
        let req = Request {
            request_id: 42,
            command: Command::Topology,
        };
        let mut buf = Vec::new();
        send_request(&mut buf, &req).unwrap();
        let decoded = receive_request(&mut buf.as_slice()).unwrap();
        assert_eq!(decoded.request_id, 42);
        assert_eq!(decoded.command, Command::Topology);

        let resp = ResponseEnvelope {
            request_id: 42,
            response: Response::Error {
                code: ErrorCode::Unauthorized,
                message: "access denied".into(),
            },
        };
        let mut resp_buf = Vec::new();
        send_response(&mut resp_buf, &resp).unwrap();
        let decoded_resp = receive_response(&mut resp_buf.as_slice()).unwrap();
        assert_eq!(decoded_resp.request_id, 42);
        match decoded_resp.response {
            Response::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert_eq!(message, "access denied");
            }
            _ => panic!("unexpected response variant"),
        }
    }
}
