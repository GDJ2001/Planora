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
    let mut total_read = 0;
    while total_read < buf.len() {
        let end = (total_read + 64 * 1024).min(buf.len());
        let count = overlapped_pipe_io(
            stream,
            &mut buf[total_read..end],
            false,
            deadline,
            "IPC receive deadline exceeded",
        )?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "IPC peer disconnected",
            ));
        }
        total_read += count;
    }
    Ok(())
}

/// Write all bytes with absolute deadline enforcement on Stream without accumulating blocked threads.
#[cfg(windows)]
pub fn write_all_deadline(stream: &mut Stream, data: &[u8], deadline: Instant) -> io::Result<()> {
    let mut total_written = 0;
    while total_written < data.len() {
        let end = (total_written + 4096).min(data.len());
        let mut chunk = data[total_written..end].to_vec();
        let count = overlapped_pipe_io(
            stream,
            &mut chunk,
            true,
            deadline,
            "IPC send deadline exceeded",
        )?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "IPC write returned zero",
            ));
        }
        total_written += count;
    }
    Ok(())
}

/// The interprocess Windows stream owns an overlapped pipe handle. Every operation must
/// supply an OVERLAPPED record and keep its buffer alive until completion or cancellation.
#[cfg(windows)]
fn overlapped_pipe_io(
    stream: &Stream,
    buffer: &mut [u8],
    write: bool,
    deadline: Instant,
    timeout_message: &'static str,
) -> io::Result<usize> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    let now = Instant::now();
    if now >= deadline {
        return Err(io::Error::new(io::ErrorKind::TimedOut, timeout_message));
    }

    struct Event(HANDLE);
    impl Drop for Event {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error());
    }
    let event = Event(event);
    let handle = match stream {
        Stream::NamedPipe(pipe) => pipe.inner().as_raw_handle() as HANDLE,
    };
    let mut overlapped = OVERLAPPED {
        hEvent: event.0,
        ..OVERLAPPED::default()
    };
    let started = unsafe {
        if write {
            WriteFile(
                handle,
                buffer.as_ptr(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        } else {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        }
    };
    if started == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout_ms = remaining.as_millis().min(u32::MAX as u128) as u32;
        let wait = unsafe { WaitForSingleObject(event.0, timeout_ms) };
        if wait != WAIT_OBJECT_0 {
            let wait_error = io::Error::last_os_error();
            // A canceled operation may still complete normally. Wait for its final state
            // before freeing either the OVERLAPPED record or the application buffer.
            unsafe {
                CancelIoEx(handle, &overlapped);
                let mut ignored = 0;
                GetOverlappedResult(handle, &overlapped, &mut ignored, 1);
            }
            if wait == WAIT_TIMEOUT {
                return Err(io::Error::new(io::ErrorKind::TimedOut, timeout_message));
            }
            return Err(wait_error);
        }
    }
    let mut transferred = 0;
    if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(transferred as usize)
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
fn stream_raw_fd(stream: &Stream) -> std::os::unix::io::RawFd {
    use std::os::unix::io::AsRawFd;
    match stream {
        Stream::UdSocket(socket) => socket.inner().as_raw_fd(),
    }
}

#[cfg(unix)]
pub fn read_exact_deadline(
    stream: &mut Stream,
    buf: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    let fd = stream_raw_fd(stream);
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
    let fd = stream_raw_fd(stream);
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

/// Send one request and receive its matching response within one absolute deadline.
/// Callers must discard the stream after any error because a partial frame may remain.
pub fn exchange_request_timeout(
    stream: &mut Stream,
    request: &Request,
    timeout: Duration,
) -> io::Result<ResponseEnvelope> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(60));
    send_request_deadline(stream, request, deadline)?;
    let response = receive_response_deadline(stream, deadline)?;
    if response.request_id != request.request_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "request ID mismatch: sent {}, got {}",
                request.request_id, response.request_id
            ),
        ));
    }
    Ok(response)
}

/// Transact a command to the helper over a dedicated local IPC connection with strict deadlines.
pub fn transact(command: &Command) -> io::Result<Response> {
    static REQUEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut stream = connect()?;

    // Perform handshake with HANDSHAKE_TIMEOUT deadline
    let handshake_id = REQUEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handshake = Request {
        request_id: handshake_id,
        command: Command::Hello {
            version: PROTOCOL_VERSION,
        },
    };
    let handshake_env = exchange_request_timeout(&mut stream, &handshake, HANDSHAKE_TIMEOUT)?;
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
    let request = Request {
        request_id: req_id,
        command: command.clone(),
    };
    let resp_env = exchange_request_timeout(&mut stream, &request, REQUEST_TIMEOUT)?;

    Ok(resp_env.response)
}

#[cfg(windows)]
fn registry_string(name: &str) -> io::Result<String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_MULTI_SZ, RRF_RT_REG_SZ, RegGetValueW,
    };
    let key: Vec<u16> = "SOFTWARE\\Planora"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let value: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let value_kind = if name == "AuthorizedEnvironment" {
        RRF_RT_REG_MULTI_SZ
    } else {
        RRF_RT_REG_SZ
    };
    let mut bytes = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            value_kind,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    if bytes == 0 || bytes > 8_192 || !bytes.is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid authorization registry value size",
        ));
    }
    let mut data = vec![0u16; bytes as usize / 2];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            value_kind,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let length = data.iter().position(|&c| c == 0).unwrap_or(data.len());
    String::from_utf16(&data[..length]).map_err(io::Error::other)
}

#[cfg(windows)]
fn parse_authorized_sid(value: &str) -> Option<String> {
    let candidate = value
        .split_once("PLANORA_ALLOWED_SID=")
        .map_or(value, |(_, after)| after);
    let sid: String = candidate
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == 'S' || *c == '-')
        .collect();
    let parts: Vec<_> = sid.split('-').collect();
    if parts.len() >= 4
        && parts[0] == "S"
        && parts[1] == "1"
        && parts[2..]
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
    {
        Some(sid)
    } else {
        None
    }
}

#[cfg(windows)]
fn authorized_sid() -> io::Result<String> {
    registry_string("AuthorizedSid")
        .ok()
        .and_then(|value| parse_authorized_sid(&value))
        .or_else(|| {
            registry_string("AuthorizedEnvironment")
                .ok()
                .and_then(|value| parse_authorized_sid(&value))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Planora helper is not authorized for a local user",
            )
        })
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

        #[cfg(target_os = "linux")]
        let result = options.mode(0o666).create_sync()?;
        #[cfg(not(target_os = "linux"))]
        let result = options.mode(0o660).create_sync()?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(ENDPOINT, std::fs::Permissions::from_mode(0o666))?;
        }

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
        let sid = authorized_sid()?;
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
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let Some(uid) = peer_uid.filter(|uid| *uid != 0) else {
            return false;
        };
        let directory = std::path::Path::new("/var/lib/planora/authorized-uids");
        let Ok(parent) = std::fs::symlink_metadata(directory) else {
            return false;
        };
        if !parent.file_type().is_dir() || parent.uid() != 0 || parent.mode() & 0o022 != 0 {
            return false;
        }
        let record = directory.join(uid.to_string());
        let Ok(metadata) = std::fs::symlink_metadata(&record) else {
            return false;
        };
        metadata.file_type().is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.len() <= 32
            && std::fs::read_to_string(&record)
                .ok()
                .is_some_and(|text| text.trim() == uid.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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

    #[test]
    fn malformed_framed_requests_are_rejected_without_panics() {
        let mut seed = 0x7357_a12c_4135_d817u64;
        for length in 0..512usize {
            let mut framed = Vec::with_capacity(length + 4);
            framed.extend_from_slice(&(length as u32).to_le_bytes());
            for _ in 0..length {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                framed.push(seed as u8);
            }
            let _ = receive_request(&mut framed.as_slice());
        }
    }

    #[cfg(windows)]
    #[test]
    fn sid_parser_accepts_migrated_multistring_and_rejects_invalid_values() {
        assert_eq!(
            parse_authorized_sid("S-1-5-21-123"),
            Some("S-1-5-21-123".into())
        );
        assert_eq!(
            parse_authorized_sid("[~]PLANORA_ALLOWED_SID=S-1-5-21-123[~]"),
            Some("S-1-5-21-123".into())
        );
        assert_eq!(parse_authorized_sid("S-1-"), None);
        assert_eq!(parse_authorized_sid(""), None);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an installed Planora authorization registry value"]
    fn installed_authorization_registry_value_is_readable() {
        assert!(authorized_sid().is_ok());
    }
}
