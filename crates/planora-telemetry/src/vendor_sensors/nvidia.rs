//! NVIDIA NVML sensor provider.
use planora_core::{Metric, SensorReading, SensorType};
use std::ptr;

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

pub struct NvidiaNvmlProvider {
    h_module: *mut std::ffi::c_void,
    device_count: u32,
    devices: Vec<*mut std::ffi::c_void>,
    device_names: Vec<String>,
}

unsafe impl Send for NvidiaNvmlProvider {}
unsafe impl Sync for NvidiaNvmlProvider {}

impl NvidiaNvmlProvider {
    pub fn try_init() -> Result<Self, (planora_core::CapabilityState, String)> {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let prog_files =
            std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());

        let candidates = [
            format!("{system_root}\\System32\\nvml.dll"),
            format!("{prog_files}\\NVIDIA Corporation\\NVSMI\\nvml.dll"),
        ];

        let valid_path = candidates
            .into_iter()
            .find(|p| std::path::Path::new(p).exists());
        let Some(dll_path) = valid_path else {
            return Err((
                planora_core::CapabilityState::MissingRuntime,
                "nvml.dll not found in administrator-owned system path".into(),
            ));
        };

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
                "Failed to load nvml.dll".into(),
            ));
        }

        let init_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"nvmlInit_v2".as_ptr() as *const u8,
            )
        };
        let Some(init_fn) = init_ptr else {
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::Unsupported,
                "nvmlInit_v2 not found in nvml.dll".into(),
            ));
        };

        type NvmlInitFn = unsafe extern "system" fn() -> i32;
        let nvml_init: NvmlInitFn = unsafe { std::mem::transmute(init_fn) };
        let status = unsafe { nvml_init() };
        if status != 0 {
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::TemporarilyFailed,
                format!("nvmlInit_v2 failed with error code {status}"),
            ));
        }

        let mut devices = Vec::new();
        let mut device_names = Vec::new();
        let get_count_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"nvmlDeviceGetCount_v2".as_ptr() as *const u8,
            )
        };
        let get_handle_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"nvmlDeviceGetHandleByIndex_v2".as_ptr() as *const u8,
            )
        };
        let get_name_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"nvmlDeviceGetName".as_ptr() as *const u8,
            )
        };

        if let (Some(get_count), Some(get_handle)) = (get_count_ptr, get_handle_ptr) {
            type GetCountFn = unsafe extern "system" fn(*mut u32) -> i32;
            type GetHandleFn = unsafe extern "system" fn(u32, *mut *mut std::ffi::c_void) -> i32;
            let get_count: GetCountFn = unsafe { std::mem::transmute(get_count) };
            let get_handle: GetHandleFn = unsafe { std::mem::transmute(get_handle) };

            let mut count = 0u32;
            if unsafe { get_count(&mut count) } == 0 {
                for i in 0..count {
                    let mut dev: *mut std::ffi::c_void = ptr::null_mut();
                    if unsafe { get_handle(i, &mut dev) } == 0 && !dev.is_null() {
                        let mut name = format!("NVIDIA GPU {i}");
                        if let Some(get_name) = get_name_ptr {
                            type GetNameFn = unsafe extern "system" fn(
                                *mut std::ffi::c_void,
                                *mut u8,
                                u32,
                            )
                                -> i32;
                            let get_name: GetNameFn = unsafe { std::mem::transmute(get_name) };
                            let mut buf = [0u8; 96];
                            if unsafe { get_name(dev, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
                                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                                if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                                    name = s.to_string();
                                }
                            }
                        }
                        devices.push(dev);
                        device_names.push(name);
                    }
                }
            }
        }

        let device_count = devices.len() as u32;
        Ok(Self {
            h_module,
            device_count,
            devices,
            device_names,
        })
    }

    pub fn sample(&self, stamp: u64) -> Vec<SensorReading> {
        let mut readings = Vec::new();
        if self.device_count == 0 {
            return readings;
        }

        let power_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlDeviceGetPowerUsage".as_ptr() as *const u8,
            )
        };
        let temp_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlDeviceGetTemperature".as_ptr() as *const u8,
            )
        };
        let clock_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlDeviceGetClockInfo".as_ptr() as *const u8,
            )
        };
        let fan_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlDeviceGetFanSpeed".as_ptr() as *const u8,
            )
        };

        for (idx, &dev) in self.devices.iter().enumerate() {
            let scope = format!(
                "GPU {} ({})",
                idx,
                self.device_names
                    .get(idx)
                    .map(|s| s.as_str())
                    .unwrap_or("NVIDIA")
            );

            // 1. Power (milliwatts -> watts)
            if let Some(pfn) = power_ptr {
                type PowerFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> i32;
                let get_power: PowerFn = unsafe { std::mem::transmute(pfn) };
                let mut mw = 0u32;
                if unsafe { get_power(dev, &mut mw) } == 0 {
                    readings.push(SensorReading {
                        name: "GPU Board Power".into(),
                        sensor_type: SensorType::GpuBoardPowerWatts,
                        scope: scope.clone(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(mw as f32 / 1000.0, "W", "NVIDIA NVML", stamp),
                    });
                }
            }

            // 2. Temperature (0 = NVML_TEMPERATURE_GPU)
            if let Some(tfn) = temp_ptr {
                type TempFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;
                let get_temp: TempFn = unsafe { std::mem::transmute(tfn) };
                let mut temp = 0u32;
                if unsafe { get_temp(dev, 0, &mut temp) } == 0 {
                    readings.push(SensorReading {
                        name: "GPU Temperature".into(),
                        sensor_type: SensorType::GpuTemperatureCelsius,
                        scope: scope.clone(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(temp as f32, "°C", "NVIDIA NVML", stamp),
                    });
                }
            }

            // 3. Clock (0 = Graphics clock)
            if let Some(cfn) = clock_ptr {
                type ClockFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;
                let get_clock: ClockFn = unsafe { std::mem::transmute(cfn) };
                let mut mhz = 0u32;
                if unsafe { get_clock(dev, 0, &mut mhz) } == 0 {
                    readings.push(SensorReading {
                        name: "GPU Graphics Clock".into(),
                        sensor_type: SensorType::ClockMhz,
                        scope: scope.clone(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(mhz as f32, "MHz", "NVIDIA NVML", stamp),
                    });
                }
            }

            // 4. Fan speed percent
            if let Some(ffn) = fan_ptr {
                type FanFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> i32;
                let get_fan: FanFn = unsafe { std::mem::transmute(ffn) };
                let mut pct = 0u32;
                if unsafe { get_fan(dev, &mut pct) } == 0 {
                    readings.push(SensorReading {
                        name: "GPU Fan Speed".into(),
                        sensor_type: SensorType::FanRpm,
                        scope: scope.clone(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(pct as f32, "%", "NVIDIA NVML", stamp),
                    });
                }
            }
        }

        readings
    }
}

impl Drop for NvidiaNvmlProvider {
    fn drop(&mut self) {
        let shutdown_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlShutdown".as_ptr() as *const u8,
            )
        };
        if let Some(shutdown_fn) = shutdown_ptr {
            type NvmlShutdownFn = unsafe extern "system" fn() -> i32;
            let nvml_shutdown: NvmlShutdownFn = unsafe { std::mem::transmute(shutdown_fn) };
            unsafe { nvml_shutdown() };
        }
        unsafe { FreeLibrary(self.h_module) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_nvidia_nvml_runtime() {
        match NvidiaNvmlProvider::try_init() {
            Ok(provider) => {
                let readings = provider.sample(crate::unix_ms());
                println!("NVIDIA NVML returned {} readings", readings.len());
            }
            Err((state, reason)) => {
                println!("NVIDIA NVML probe result: {:?}: {}", state, reason);
                assert!(!reason.is_empty());
            }
        }
    }
}
