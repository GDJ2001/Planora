//! NVIDIA NVML sensor provider.
use planora_core::{Metric, SensorReading, SensorType};
use std::ptr;

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

#[repr(C)]
struct NvmlFanSpeedInfo {
    version: u32,
    fan: u32,
    speed: u32,
}

const NVML_FAN_SPEED_INFO_V1: u32 = (std::mem::size_of::<NvmlFanSpeedInfo>() as u32) | (1 << 24);

pub struct NvidiaNvmlProvider {
    h_module: *mut std::ffi::c_void,
    _device_count: u32,
    devices: Vec<*mut std::ffi::c_void>,
    device_names: Vec<String>,
}

unsafe impl Send for NvidiaNvmlProvider {}
unsafe impl Sync for NvidiaNvmlProvider {}

impl NvidiaNvmlProvider {
    pub fn try_init() -> Result<Self, (planora_core::CapabilityState, String)> {
        #[cfg(windows)]
        if !crate::has_pci_vendor_gpu(0x10DE) {
            return Err((
                planora_core::CapabilityState::NotApplicable,
                "No NVIDIA graphics adapter present on this system".into(),
            ));
        }

        let dll_path = super::trusted_system_dll("nvml.dll").ok_or((
            planora_core::CapabilityState::TemporarilyFailed,
            "Could not resolve the Windows system directory".into(),
        ))?;
        if !dll_path.exists() {
            return Err((
                planora_core::CapabilityState::MissingRuntime,
                "nvml.dll not found in the Windows system directory".into(),
            ));
        }

        use std::os::windows::ffi::OsStrExt;
        let wide_path: Vec<u16> = dll_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
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
                                    name = s.trim().to_string();
                                }
                            }
                        }
                        devices.push(dev);
                        device_names.push(name);
                    }
                }
            }
        }

        if devices.is_empty() {
            let shutdown_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"nvmlShutdown".as_ptr() as *const u8,
                )
            };
            if let Some(shutdown_fn) = shutdown_ptr {
                type NvmlShutdownFn = unsafe extern "system" fn() -> i32;
                let nvml_shutdown: NvmlShutdownFn = unsafe { std::mem::transmute(shutdown_fn) };
                unsafe { nvml_shutdown() };
            }
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::NotApplicable,
                "No NVIDIA GPU devices found via NVML".into(),
            ));
        }

        let device_count = devices.len() as u32;

        Ok(Self {
            h_module,
            _device_count: device_count,
            devices,
            device_names,
        })
    }

    pub fn sample(&self, stamp: u64) -> Vec<SensorReading> {
        let mut readings = Vec::new();

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
        let fan_speed_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlDeviceGetFanSpeed".as_ptr() as *const u8,
            )
        };
        let fan_rpm_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"nvmlDeviceGetFanSpeedRPM".as_ptr() as *const u8,
            )
        };

        for (idx, &dev) in self.devices.iter().enumerate() {
            let dev_id = format!("nvidia_gpu/{idx}");
            let dev_name = self
                .device_names
                .get(idx)
                .cloned()
                .unwrap_or_else(|| format!("NVIDIA GPU {idx}"));
            let scope = format!("GPU {} ({})", idx, dev_name);

            // 1. Power (milliwatts -> watts)
            if let Some(pfn) = power_ptr {
                type PowerFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> i32;
                let get_power: PowerFn = unsafe { std::mem::transmute(pfn) };
                let mut mw = 0u32;
                if unsafe { get_power(dev, &mut mw) } == 0 {
                    let w = mw as f32 / 1000.0;
                    if (0.0..=1000.0).contains(&w) {
                        readings.push(SensorReading {
                            id: format!("{dev_id}/power"),
                            device_id: dev_id.clone(),
                            device_name: dev_name.clone(),
                            name: "GPU Board Power".into(),
                            sensor_type: SensorType::GpuBoardPowerWatts,
                            scope: scope.clone(),
                            provider: "NVIDIA NVML".into(),
                            value: Metric::available(
                                w,
                                planora_core::UNIT_WATTS,
                                "NVIDIA NVML",
                                stamp,
                            ),
                        });
                    }
                }
            }

            // 2. Temperature (0 = NVML_TEMPERATURE_GPU)
            if let Some(tfn) = temp_ptr {
                type TempFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;
                let get_temp: TempFn = unsafe { std::mem::transmute(tfn) };
                let mut temp = 0u32;
                if unsafe { get_temp(dev, 0, &mut temp) } == 0 && (0..=125).contains(&temp) {
                    readings.push(SensorReading {
                        id: format!("{dev_id}/temperature"),
                        device_id: dev_id.clone(),
                        device_name: dev_name.clone(),
                        name: "GPU Temperature".into(),
                        sensor_type: SensorType::GpuTemperatureCelsius,
                        scope: scope.clone(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(
                            temp as f32,
                            planora_core::UNIT_CELSIUS,
                            "NVIDIA NVML",
                            stamp,
                        ),
                    });
                }
            }

            // 3. Clock (0 = Graphics clock)
            if let Some(cfn) = clock_ptr {
                type ClockFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;
                let get_clock: ClockFn = unsafe { std::mem::transmute(cfn) };
                let mut mhz = 0u32;
                if unsafe { get_clock(dev, 0, &mut mhz) } == 0 && (0..=10_000).contains(&mhz) {
                    readings.push(SensorReading {
                        id: format!("{dev_id}/clock"),
                        device_id: dev_id.clone(),
                        device_name: dev_name.clone(),
                        name: "GPU Graphics Clock".into(),
                        sensor_type: SensorType::ClockMhz,
                        scope: scope.clone(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(
                            mhz as f32,
                            planora_core::UNIT_MHZ,
                            "NVIDIA NVML",
                            stamp,
                        ),
                    });
                }
            }

            // 4. Tachometer Fan RPM (nvmlDeviceGetFanSpeedRPM takes nvmlFanSpeedInfo_t*)
            if let Some(rpm_fn) = fan_rpm_ptr {
                type FanRpmFn =
                    unsafe extern "C" fn(*mut std::ffi::c_void, *mut NvmlFanSpeedInfo) -> i32;
                let get_fan_rpm: FanRpmFn = unsafe { std::mem::transmute(rpm_fn) };
                let mut fan_info = NvmlFanSpeedInfo {
                    version: NVML_FAN_SPEED_INFO_V1,
                    fan: 0,
                    speed: 0,
                };
                if unsafe { get_fan_rpm(dev, &mut fan_info) } == 0
                    && (0..=30_000).contains(&fan_info.speed)
                {
                    readings.push(SensorReading {
                        id: format!("{dev_id}/fan_rpm"),
                        device_id: dev_id.clone(),
                        device_name: dev_name.clone(),
                        name: "GPU Fan Speed".into(),
                        sensor_type: SensorType::FanRpm,
                        scope: "Cooling".into(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(
                            fan_info.speed as f32,
                            planora_core::UNIT_RPM,
                            "NVIDIA NVML",
                            stamp,
                        ),
                    });
                }
            }

            // 5. Fan speed percent (Duty cycle)
            if let Some(ffn) = fan_speed_ptr {
                type FanFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> i32;
                let get_fan: FanFn = unsafe { std::mem::transmute(ffn) };
                let mut pct = 0u32;
                if unsafe { get_fan(dev, &mut pct) } == 0 && (0..=100).contains(&pct) {
                    readings.push(SensorReading {
                        id: format!("{dev_id}/fan_duty"),
                        device_id: dev_id.clone(),
                        device_name: dev_name.clone(),
                        name: "GPU Fan Duty".into(),
                        sensor_type: SensorType::LoadPercent,
                        scope: "Cooling".into(),
                        provider: "NVIDIA NVML".into(),
                        value: Metric::available(
                            pct as f32,
                            planora_core::UNIT_PERCENT,
                            "NVIDIA NVML",
                            stamp,
                        ),
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
    fn nvml_fan_speed_info_v1_abi_layout() {
        assert_eq!(std::mem::size_of::<NvmlFanSpeedInfo>(), 12);
        assert_eq!(NVML_FAN_SPEED_INFO_V1, 0x0100_000c);
    }

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
