//! Small local startup log for failures that occur before the native UI paints.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn log_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA");
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME");
    base.map(PathBuf::from)
        .map(|base| base.join("Planora").join("logs").join("startup.log"))
}

pub fn record(event: &str) {
    let Some(path) = log_path() else { return };
    let _guard = LOG_LOCK.get_or_init(|| Mutex::new(())).lock();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() >= 256 * 1024) {
        let old = path.with_extension("previous.log");
        let _ = fs::remove_file(&old);
        let _ = fs::rename(&path, old);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {event}", planora_telemetry::unix_ms());
    }
}

#[cfg(windows)]
pub fn show_fatal(error: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let path = log_path().map_or_else(|| "Unavailable".to_owned(), |p| p.display().to_string());
    let message = format!("Planora could not start.\n\n{error}\n\nStartup log: {path}");
    let wide: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "Planora startup error"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}
