//! Windows helper authorization before the normal application window opens.
//! The UI stays unelevated; only the installed helper authorization script
//! requests administrator approval when the local helper cannot be reached.

use planora_core::{Command, ErrorCode, PROTOCOL_VERSION, Request, Response, ResponseEnvelope};
use std::{
    ffi::OsString,
    io,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    thread,
    time::{Duration, Instant},
};

const SCRIPT_NAME: &str = "Authorize-Planora.ps1";
const TRUSTED_SCRIPT: &[u8] = include_bytes!("../../../packaging/windows/Authorize-Planora.ps1");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelperProbe {
    Ready,
    NeedsAuthorization,
    Offline,
    Incompatible,
    Unresponsive,
}

fn classify_handshake(envelope: ResponseEnvelope, request_id: u64) -> HelperProbe {
    if envelope.request_id != request_id {
        return HelperProbe::Unresponsive;
    }
    match envelope.response {
        Response::Hello {
            version,
            helper_authorized: true,
            ..
        } if version == PROTOCOL_VERSION => HelperProbe::Ready,
        Response::Hello {
            version,
            helper_authorized: false,
            ..
        } if version == PROTOCOL_VERSION => HelperProbe::NeedsAuthorization,
        Response::Error {
            code: ErrorCode::Unauthorized,
            ..
        } => HelperProbe::NeedsAuthorization,
        Response::Hello { .. }
        | Response::Error {
            code: ErrorCode::VersionMismatch,
            ..
        } => HelperProbe::Incompatible,
        _ => HelperProbe::Unresponsive,
    }
}

fn probe() -> HelperProbe {
    let mut stream = match planora_helper::connect() {
        Ok(stream) => stream,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return HelperProbe::NeedsAuthorization;
        }
        Err(_) => return HelperProbe::Offline,
    };
    let request_id = 1;
    let request = Request {
        request_id,
        command: Command::Hello {
            version: PROTOCOL_VERSION,
        },
    };
    match planora_helper::exchange_request_timeout(
        &mut stream,
        &request,
        planora_helper::HANDSHAKE_TIMEOUT,
    ) {
        Ok(response) => {
            let result = classify_handshake(response, request_id);
            if result == HelperProbe::NeedsAuthorization && legacy_v3_detected() {
                HelperProbe::Incompatible
            } else {
                result
            }
        }
        Err(_) => HelperProbe::Unresponsive,
    }
}

fn legacy_v3_detected() -> bool {
    let Ok(mut stream) = planora_helper::connect() else {
        return false;
    };
    let request = Request {
        request_id: 2,
        command: Command::Hello { version: 3 },
    };
    matches!(
        planora_helper::exchange_request_timeout(
            &mut stream,
            &request,
            planora_helper::HANDSHAKE_TIMEOUT,
        ),
        Ok(ResponseEnvelope {
            request_id: 2,
            response: Response::Hello { version: 3, .. },
        })
    )
}

fn path_is_installed(exe: &Path, program_files: &Path) -> bool {
    exe.file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("planora.exe"))
        && exe.parent().is_some_and(|parent| {
            parent
                .to_string_lossy()
                .eq_ignore_ascii_case(&program_files.join("Planora").to_string_lossy())
        })
}

fn program_files_directory() -> Option<PathBuf> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
    };
    let mut raw = std::ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 32_768 && unsafe { *raw.add(len) } != 0 {
        len += 1;
    }
    let directory = if len < 32_768 {
        Some(PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(raw, len)
        })))
    } else {
        None
    };
    unsafe { CoTaskMemFree(raw.cast()) };
    directory
}

pub fn system_powershell() -> io::Result<PathBuf> {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = [0u16; 1024];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..len]))
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "System PowerShell is unavailable",
        ));
    }
    Ok(path)
}

pub fn trusted_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let program_files = program_files_directory()?;
    if !path_is_installed(&exe, &program_files) {
        return None;
    }
    let script = exe.parent()?.join(SCRIPT_NAME);
    if std::fs::read(&script).ok()?.as_slice() != TRUSTED_SCRIPT {
        crate::startup::record("startup-authorization script-integrity-failed");
        return None;
    }
    Some(script)
}

/// Returns a visible fallback error if approval fails. Called only for a normal
/// installed launch, after the single-instance guard and before creating egui.
pub fn prepare_before_window() -> Option<String> {
    let script = trusted_script()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        let result = probe();
        if result != HelperProbe::Offline || Instant::now() >= deadline {
            break result;
        }
        thread::sleep(Duration::from_millis(200));
    };
    match result {
        HelperProbe::Ready => {
            crate::startup::record("startup-authorization helper-ready");
            return None;
        }
        HelperProbe::Incompatible => {
            crate::startup::record("startup-authorization helper-incompatible");
            return None;
        }
        HelperProbe::Unresponsive => {
            crate::startup::record("startup-authorization helper-unresponsive");
            return None;
        }
        HelperProbe::NeedsAuthorization | HelperProbe::Offline => {
            crate::startup::record("startup-authorization requesting-windows-approval");
        }
    }

    use std::os::windows::process::CommandExt;
    let powershell = match system_powershell() {
        Ok(path) => path,
        Err(error) => return Some(format!("Could not find system PowerShell: {error}")),
    };
    let child = ProcessCommand::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW; Windows still displays UAC.
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            crate::startup::record("startup-authorization launch-failed");
            return Some(format!("Could not request administrator approval: {error}"));
        }
    };
    let deadline = Instant::now() + Duration::from_secs(125);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                crate::startup::record("startup-authorization approved");
                let ready_deadline = Instant::now() + Duration::from_secs(4);
                while Instant::now() < ready_deadline {
                    if probe() == HelperProbe::Ready {
                        crate::startup::record("startup-authorization helper-ready-after-approval");
                        return None;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                crate::startup::record("startup-authorization helper-still-unavailable");
                return Some(
                    "Administrator approval completed, but the helper did not connect. Check the Planora Helper service and retry."
                        .into(),
                );
            }
            Ok(Some(status)) => {
                crate::startup::record(&format!(
                    "startup-authorization failed exit-code={:?}",
                    status.code()
                ));
                return Some(
                    "Windows administrator approval was declined or helper authorization failed. Retry from the connection screen."
                        .into(),
                );
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(200)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                crate::startup::record("startup-authorization timed-out");
                return Some(
                    "Windows administrator approval timed out. Retry from the connection screen."
                        .into(),
                );
            }
            Err(error) => {
                crate::startup::record("startup-authorization wait-failed");
                return Some(format!("Could not complete helper authorization: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_path_gate_excludes_user_writable_copies() {
        assert!(path_is_installed(
            Path::new(r"C:\Program Files\Planora\planora.exe"),
            Path::new(r"C:\Program Files")
        ));
        assert!(!path_is_installed(
            Path::new(r"C:\Users\Test\Downloads\planora.exe"),
            Path::new(r"C:\Program Files")
        ));
    }

    #[test]
    fn windows_system_locations_resolve_without_path_search() {
        assert!(program_files_directory().is_some_and(|path| path.is_dir()));
        assert!(system_powershell().is_ok_and(|path| path.is_file()));
    }

    #[test]
    fn handshake_requests_approval_only_for_authorization_failures() {
        let envelope = |response| ResponseEnvelope {
            request_id: 1,
            response,
        };
        assert_eq!(
            classify_handshake(
                envelope(Response::Hello {
                    version: PROTOCOL_VERSION,
                    session_id: 1,
                    helper_authorized: true,
                }),
                1
            ),
            HelperProbe::Ready
        );
        assert_eq!(
            classify_handshake(
                envelope(Response::Error {
                    code: ErrorCode::Unauthorized,
                    message: String::new(),
                }),
                1
            ),
            HelperProbe::NeedsAuthorization
        );
        assert_eq!(
            classify_handshake(
                envelope(Response::Error {
                    code: ErrorCode::VersionMismatch,
                    message: String::new(),
                }),
                1
            ),
            HelperProbe::Incompatible
        );
    }
}
