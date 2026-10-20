//! Intel Graphics Control Library (IGCL) sensor provider.
use planora_core::{Metric, SensorReading, SensorType};
use std::ptr;

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

#[repr(C)]
struct CtlInitArgs {
    size: u32,
    version: u8,
    reserved: [u8; 3],
    flags: u32,
    supported_version: u32,
    app_version: u32,
}

#[allow(dead_code)]
#[repr(C)]
struct CtlDeviceAdapterProperties {
    size: u32,
    version: u8,
    device_type: u8,
    reserved: [u8; 2],
    device_id: u32,
    rev_id: u32,
    pci_vendor_id: u32,
    pci_device_id: u32,
    name: [u8; 100],
}

#[repr(C)]
struct CtlPowerEnergyCounter {
    size: u32,
    version: u8,
    reserved: [u8; 3],
    energy: u64,
    timestamp: u64,
}

#[repr(C)]
struct CtlTempState {
    size: u32,
    version: u8,
    temp_type: u8,
    reserved: u8,
    temp: f64,
}

#[repr(C)]
struct CtlFreqState {
    size: u32,
    version: u8,
    freq_type: u8,
    reserved: u8,
    current_freq: f64,
    target_freq: f64,
    actual_freq: f64,
}

pub struct IntelIgclProvider {
    h_module: *mut std::ffi::c_void,
    ctl_api: *mut std::ffi::c_void,
    devices: Vec<*mut std::ffi::c_void>,
    device_names: Vec<String>,
    last_energy: Vec<(u64, u64)>, // (energy_uj, timestamp)
}

// Safety: pointers are only accessed within the worker process
unsafe impl Send for IntelIgclProvider {}
unsafe impl Sync for IntelIgclProvider {}

impl IntelIgclProvider {
    pub fn try_init() -> Result<Self, (planora_core::CapabilityState, String)> {
        // Enforce documented, administrator-owned system path: System32 only
        let system_dir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let dll_path = format!("{system_dir}\\System32\\ControlLib.dll");
        if !std::path::Path::new(&dll_path).exists() {
            return Err((
                planora_core::CapabilityState::MissingRuntime,
                "ControlLib.dll not found in System32".into(),
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
                "Failed to load ControlLib.dll from System32".into(),
            ));
        }

        let ctl_init_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlInit".as_ptr() as *const u8,
            )
        };
        let Some(ctl_init) = ctl_init_ptr else {
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::Unsupported,
                "ctlInit symbol missing in ControlLib.dll".into(),
            ));
        };

        type CtlInitFn =
            unsafe extern "system" fn(*mut CtlInitArgs, *mut *mut std::ffi::c_void) -> i32;
        let ctl_init: CtlInitFn = unsafe { std::mem::transmute(ctl_init) };

        let mut init_args = CtlInitArgs {
            size: std::mem::size_of::<CtlInitArgs>() as u32,
            version: 0,
            reserved: [0; 3],
            flags: 0,
            supported_version: (1 << 16),
            app_version: (1 << 16),
        };
        let mut ctl_api: *mut std::ffi::c_void = ptr::null_mut();
        let status = unsafe { ctl_init(&mut init_args, &mut ctl_api) };
        if status != 0 || ctl_api.is_null() {
            unsafe { FreeLibrary(h_module) };
            return Err((
                planora_core::CapabilityState::Unsupported,
                format!("ctlInit failed with code 0x{status:08x}"),
            ));
        }

        let ctl_enum_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlEnumerateDevices".as_ptr() as *const u8,
            )
        };
        let mut devices = Vec::new();
        let mut device_names = Vec::new();
        if let Some(ctl_enum) = ctl_enum_ptr {
            type CtlEnumFn = unsafe extern "system" fn(
                *mut std::ffi::c_void,
                *mut u32,
                *mut *mut std::ffi::c_void,
            ) -> i32;
            let ctl_enum: CtlEnumFn = unsafe { std::mem::transmute(ctl_enum) };

            let mut count = 0u32;
            let res = unsafe { ctl_enum(ctl_api, &mut count, ptr::null_mut()) };
            if res == 0 && count > 0 {
                let mut dev_ptrs: Vec<*mut std::ffi::c_void> =
                    vec![ptr::null_mut(); count as usize];
                if unsafe { ctl_enum(ctl_api, &mut count, dev_ptrs.as_mut_ptr()) } == 0 {
                    for dev in dev_ptrs.into_iter().take(count as usize) {
                        if !dev.is_null() {
                            devices.push(dev);
                            device_names.push("Intel Graphics".into());
                        }
                    }
                }
            }
        }

        let last_energy = vec![(0, 0); devices.len()];

        Ok(Self {
            h_module,
            ctl_api,
            devices,
            device_names,
            last_energy,
        })
    }

    pub fn sample(&mut self, stamp: u64) -> Vec<SensorReading> {
        let mut readings = Vec::new();

        let ctl_power_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"ctlPowerGetEnergyCounter".as_ptr() as *const u8,
            )
        };
        let ctl_temp_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"ctlTemperatureGetState".as_ptr() as *const u8,
            )
        };
        let ctl_freq_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"ctlFrequencyGetState".as_ptr() as *const u8,
            )
        };

        for (idx, &dev) in self.devices.iter().enumerate() {
            let scope = format!(
                "GPU {} ({})",
                idx,
                self.device_names
                    .get(idx)
                    .map(|s| s.as_str())
                    .unwrap_or("Intel")
            );

            // 1. Power energy counter -> Watts
            if let Some(ctl_power) = ctl_power_ptr {
                type CtlPowerFn = unsafe extern "system" fn(
                    *mut std::ffi::c_void,
                    u32,
                    *mut CtlPowerEnergyCounter,
                ) -> i32;
                let ctl_power: CtlPowerFn = unsafe { std::mem::transmute(ctl_power) };

                let mut counter = CtlPowerEnergyCounter {
                    size: std::mem::size_of::<CtlPowerEnergyCounter>() as u32,
                    version: 0,
                    reserved: [0; 3],
                    energy: 0,
                    timestamp: 0,
                };
                // 0 = package/card power
                if unsafe { ctl_power(dev, 0, &mut counter) } == 0 && counter.timestamp > 0 {
                    if let Some(&(prev_energy, prev_time)) = self.last_energy.get(idx)
                        && prev_time > 0
                        && counter.timestamp > prev_time
                    {
                        let delta_e = counter.energy.saturating_sub(prev_energy); // in microjoules
                        let delta_t = counter.timestamp.saturating_sub(prev_time); // in microseconds
                        if delta_t > 0 {
                            let watts = (delta_e as f64 / delta_t as f64) as f32;
                            readings.push(SensorReading {
                                name: "GPU Package Power".into(),
                                sensor_type: SensorType::GpuChipPowerWatts,
                                scope: scope.clone(),
                                provider: "Intel IGCL".into(),
                                value: Metric::available(watts, "W", "Intel IGCL", stamp),
                            });
                        }
                    }
                    if idx < self.last_energy.len() {
                        self.last_energy[idx] = (counter.energy, counter.timestamp);
                    }
                }
            }

            // 2. Temperature
            if let Some(ctl_temp) = ctl_temp_ptr {
                type CtlTempFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, *mut CtlTempState) -> i32;
                let ctl_temp: CtlTempFn = unsafe { std::mem::transmute(ctl_temp) };

                let mut temp_state = CtlTempState {
                    size: std::mem::size_of::<CtlTempState>() as u32,
                    version: 0,
                    temp_type: 0,
                    reserved: 0,
                    temp: 0.0,
                };
                if unsafe { ctl_temp(dev, &mut temp_state) } == 0
                    && temp_state.temp.is_finite()
                    && temp_state.temp > 0.0
                {
                    readings.push(SensorReading {
                        name: "GPU Temperature".into(),
                        sensor_type: SensorType::GpuTemperatureCelsius,
                        scope: scope.clone(),
                        provider: "Intel IGCL".into(),
                        value: Metric::available(temp_state.temp as f32, "°C", "Intel IGCL", stamp),
                    });
                }
            }

            // 3. Frequency / Clock
            if let Some(ctl_freq) = ctl_freq_ptr {
                type CtlFreqFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, *mut CtlFreqState) -> i32;
                let ctl_freq: CtlFreqFn = unsafe { std::mem::transmute(ctl_freq) };

                let mut freq_state = CtlFreqState {
                    size: std::mem::size_of::<CtlFreqState>() as u32,
                    version: 0,
                    freq_type: 0,
                    reserved: 0,
                    current_freq: 0.0,
                    target_freq: 0.0,
                    actual_freq: 0.0,
                };
                if unsafe { ctl_freq(dev, &mut freq_state) } == 0
                    && freq_state.actual_freq.is_finite()
                    && freq_state.actual_freq > 0.0
                {
                    readings.push(SensorReading {
                        name: "GPU Core Clock".into(),
                        sensor_type: SensorType::ClockMhz,
                        scope: scope.clone(),
                        provider: "Intel IGCL".into(),
                        value: Metric::available(
                            freq_state.actual_freq as f32,
                            "MHz",
                            "Intel IGCL",
                            stamp,
                        ),
                    });
                }
            }
        }

        readings
    }
}

impl Drop for IntelIgclProvider {
    fn drop(&mut self) {
        if !self.ctl_api.is_null() {
            let ctl_close_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    self.h_module,
                    c"ctlClose".as_ptr() as *const u8,
                )
            };
            if let Some(ctl_close) = ctl_close_ptr {
                type CtlCloseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
                let ctl_close: CtlCloseFn = unsafe { std::mem::transmute(ctl_close) };
                unsafe { ctl_close(self.ctl_api) };
            }
        }
        if !self.h_module.is_null() {
            unsafe { FreeLibrary(self.h_module) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_intel_igcl_runtime() {
        match IntelIgclProvider::try_init() {
            Ok(mut provider) => {
                let readings = provider.sample(crate::unix_ms());
                println!("Intel IGCL returned {} sensor readings", readings.len());
            }
            Err((state, reason)) => {
                println!("Intel IGCL probe result: {:?}: {}", state, reason);
                assert!(!reason.is_empty());
            }
        }
    }
}
