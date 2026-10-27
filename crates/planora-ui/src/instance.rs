//! Keep one normal UI per Windows logon session. Command-line tools and native
//! fixtures never acquire this lock.
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
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{GetCurrentProcessId, OpenProcess, TerminateProcess, PROCESS_TERMINATE},
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
