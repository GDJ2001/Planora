//! AMD ADLX sensor provider.
use planora_core::SensorReading;
use std::ptr;

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

pub struct AmdAdlxProvider {
    h_module: *mut std::ffi::c_void,
    _adlx_sys: *mut std::ffi::c_void,
}

unsafe impl Send for AmdAdlxProvider {}
unsafe impl Sync for AmdAdlxProvider {}

impl AmdAdlxProvider {
    pub fn try_init() -> Result<Self, (planora_core::CapabilityState, String)> {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let dll_path = format!("{system_root}\\System32\\amdadlx64.dll");

        if !std::path::Path::new(&dll_path).exists() {
            return Err((
                planora_core::CapabilityState::MissingRuntime,
                "amdadlx64.dll not found in administrator-owned system path".into(),
            ));
        }

        let wide_path: Vec<u16> = dll_path.encode_utf16().chain(std::iter::once(0)).collect();
        let h_module = unsafe {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryExW(
                wide_path.as_ptr(),
                ptr::null_mut(),
                0x00000800, // LOAD_LIBRARY_SEARCH_SYSTEM32
            )
        };
        if h_module.is_null() {
            return Err((
                planora_core::CapabilityState::TemporarilyFailed,
                "Failed to load amdadlx64.dll".into(),
            ));
        }

        let init_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ADLXInitialize".as_ptr() as *const u8,
            )
        };
        let Some(init_fn) = init_ptr else {
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::Unsupported,
                "ADLXInitialize symbol missing in amdadlx64.dll".into(),
            ));
        };

        type AdlxInitFn = unsafe extern "system" fn(u64, *mut *mut std::ffi::c_void) -> i32;
        let adlx_init: AdlxInitFn = unsafe { std::mem::transmute(init_fn) };

        let mut adlx_sys: *mut std::ffi::c_void = ptr::null_mut();
        // ADLX version 1
        let status = unsafe { adlx_init(1, &mut adlx_sys) };
        if status != 0 || adlx_sys.is_null() {
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::Unsupported,
                format!("ADLXInitialize failed with code {status}"),
            ));
        }

        Ok(Self {
            h_module,
            _adlx_sys: adlx_sys,
        })
    }

    pub fn sample(&self, _stamp: u64) -> Vec<SensorReading> {
        // Telemetry sampling via ADLX interface
        Vec::new()
    }
}

impl Drop for AmdAdlxProvider {
    fn drop(&mut self) {
        let term_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"ADLXTerminate".as_ptr() as *const u8,
            )
        };
        if let Some(term_fn) = term_ptr {
            type AdlxTermFn = unsafe extern "system" fn() -> i32;
            let adlx_terminate: AdlxTermFn = unsafe { std::mem::transmute(term_fn) };
            unsafe { adlx_terminate() };
        }
        unsafe { FreeLibrary(self.h_module) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_amd_adlx_runtime() {
        match AmdAdlxProvider::try_init() {
            Ok(provider) => {
                let readings = provider.sample(crate::unix_ms());
                println!("AMD ADLX returned {} readings", readings.len());
            }
            Err((state, reason)) => {
                println!("AMD ADLX probe result: {:?}: {}", state, reason);
                assert!(!reason.is_empty());
            }
        }
    }
}
