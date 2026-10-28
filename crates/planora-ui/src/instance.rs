//! Keep one normal UI per graphical user session. Command-line tools and
//! native fixtures never acquire this lock.
#[cfg(unix)]
pub struct InstanceGuard {
    file: std::fs::File,
    socket_path: std::path::PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
static ACTIVATION_PENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
pub fn take_activation() -> bool {
    ACTIVATION_PENDING.swap(false, std::sync::atomic::Ordering::AcqRel)
}

#[cfg(unix)]
impl InstanceGuard {
    pub fn acquire_or_replace(_replace: bool) -> std::io::Result<Option<Self>> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let mut runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "XDG_RUNTIME_DIR is unavailable",
                )
            })?;
        let mut metadata = std::fs::symlink_metadata(&runtime)?;
        // WSLg may supply a runtime directory owned by its compositor user
        // or with permissions unsuitable for an application socket. Keep
        // both winit and the lock in one private local directory.
        if (metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0)
            && std::env::var_os("WSL_DISTRO_NAME").is_some()
        {
            runtime = std::env::temp_dir()
                .join(format!("planora-runtime-{}", unsafe { libc::geteuid() }));
            match std::fs::create_dir(&runtime) {
                Ok(()) => {
                    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700))?
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            metadata = std::fs::symlink_metadata(&runtime)?;
            // Normal launch reaches this before creating any application
            // workers, so updating the process environment is safe here.
            unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) };
        }
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "XDG_RUNTIME_DIR must belong to this user and be private",
            ));
        }
        let session = std::env::var("XDG_SESSION_ID").unwrap_or_else(|_| "default".into());
        let session: String = session
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
            .take(32)
            .collect();
        let session = if session.is_empty() {
            "default"
        } else {
            &session
        };
        let path = runtime.join(format!("planora-ui-{session}.lock"));
        let socket_path = runtime.join(format!("planora-ui-{session}.sock"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            use std::os::unix::net::UnixDatagram;
            let _ = std::fs::remove_file(&socket_path);
            let socket = UnixDatagram::bind(&socket_path)?;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
            socket.set_read_timeout(Some(std::time::Duration::from_millis(250)))?;
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_stop = stop.clone();
            let worker = std::thread::Builder::new()
                .name("planora-activation".into())
                .spawn(move || {
                    let mut message = [0u8; 16];
                    while !worker_stop.load(std::sync::atomic::Ordering::Acquire) {
                        if let Ok((count, _)) = socket.recv_from(&mut message)
                            && &message[..count] == b"activate"
                        {
                            ACTIVATION_PENDING.store(true, std::sync::atomic::Ordering::Release);
                        }
                    }
                })?;
            Ok(Some(Self {
                file,
                socket_path,
                stop,
                worker: Some(worker),
            }))
        } else if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
            use std::os::unix::net::UnixDatagram;
            let sender = UnixDatagram::unbound()?;
            for _ in 0..4 {
                if sender.send_to(b"activate", &socket_path).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None)
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(unix)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.socket_path);
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(windows)]
pub struct InstanceGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl InstanceGuard {
    /// `None` means another normal window was activated.
    pub fn acquire() -> std::io::Result<Option<Self>> {
        Self::acquire_or_replace(false)
    }

    /// Acquire the instance lock, optionally closing and replacing any existing running instance.
    pub fn acquire_or_replace(replace: bool) -> std::io::Result<Option<Self>> {
        use windows_sys::Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
            System::{
                RemoteDesktop::ProcessIdToSessionId,
                Threading::{CreateMutexW, GetCurrentProcessId},
            },
        };
        let mut session = 0u32;
        if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let sid = current_user_sid()?;
        let name: Vec<u16> = format!("Local\\Planora.UI.{sid}.Session.{session}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            if replace {
                terminate_other_planora_processes();
            } else if activate_existing() {
                return Ok(None);
            } else {
                // If another process held the mutex but has NO visible window after waiting,
                // it is a stranded background process. Terminate it to recover normal UI.
                terminate_other_planora_processes();
            }

            // Retry acquiring the mutex after cleaning up the existing instance
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let retry_handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
                if !retry_handle.is_null() {
                    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
                        return Ok(Some(Self(retry_handle)));
                    }
                    unsafe { windows_sys::Win32::Foundation::CloseHandle(retry_handle) };
                }
            }
            Ok(None)
        } else {
            Ok(Some(Self(handle)))
        }
    }
}

#[cfg(windows)]
fn current_user_sid() -> std::io::Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = (|| -> std::io::Result<String> {
        let mut len = 0u32;
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        if len == 0 || len > 4096 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid token SID length",
            ));
        }
        let mut storage = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(token, TokenUser, storage.as_mut_ptr().cast(), len, &mut len)
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let user = unsafe { &*(storage.as_ptr().cast::<TOKEN_USER>()) };
        let mut wide: *mut u16 = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut wide) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let count = (0..256).find(|i| unsafe { *wide.add(*i) } == 0);
        let sid =
            count.map(|n| String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide, n) }));
        unsafe { LocalFree(wide.cast()) };
        sid.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "SID too long"))
    })();
    unsafe { CloseHandle(token) };
    result
}

#[cfg(windows)]
fn terminate_other_planora_processes() {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{GetCurrentProcessId, OpenProcess, PROCESS_TERMINATE, TerminateProcess},
        },
    };
    let my_pid = unsafe { GetCurrentProcessId() };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() {
        return;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
        loop {
            let name_len = (0..entry.szExeFile.len())
                .find(|&i| entry.szExeFile[i] == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
            if name.eq_ignore_ascii_case("planora.exe") && entry.th32ProcessID != my_pid {
                let proc = unsafe { OpenProcess(PROCESS_TERMINATE, 0, entry.th32ProcessID) };
                if !proc.is_null() {
                    unsafe {
                        TerminateProcess(proc, 0);
                        CloseHandle(proc);
                    }
                }
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }
    unsafe { CloseHandle(snapshot) };
}

#[cfg(windows)]
fn activate_existing() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    let title: Vec<u16> = "Planora".encode_utf16().chain(std::iter::once(0)).collect();
    // The first process may still be constructing its viewport. A bounded wait
    // covers that race while leaving its event loop and UI thread untouched.
    for _ in 0..30 {
        let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !hwnd.is_null() {
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}
