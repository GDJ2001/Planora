//! AMD ADLX and Legacy ADL sensor provider.
use planora_core::{Metric, SensorReading, SensorType};
use std::ptr;

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

const ADL_MAX_PATH: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct AdapterInfo {
    size: i32,
    adapter_index: i32,
    udid: [u8; ADL_MAX_PATH],
    bus_number: i32,
    device_number: i32,
    function_number: i32,
    vendor_id: i32,
    adapter_name: [u8; ADL_MAX_PATH],
    display_name: [u8; ADL_MAX_PATH],
    present: i32,
    exist: i32,
    driver_path: [u8; ADL_MAX_PATH],
    driver_path_ext: [u8; ADL_MAX_PATH],
    pnp_string: [u8; ADL_MAX_PATH],
    os_display_index: i32,
}

#[repr(C)]
struct ADLPMActivity {
    size: i32,
    engine_clock: i32,
    memory_clock: i32,
    vddc: i32,
    activity_percent: i32,
    current_performance_level: i32,
    current_bus_speed: i32,
    current_bus_lanes: i32,
    maximum_bus_lanes: i32,
    reserved: i32,
}

#[repr(C)]
struct ADLTemperature {
    size: i32,
    temperature: i32,
}

#[repr(C)]
struct ADLFanSpeedValue {
    size: i32,
    speed_type: i32,
    fan_speed: i32,
    flags: i32,
}

unsafe extern "C" fn adl_alloc(size: i32) -> *mut std::ffi::c_void {
    if size <= 0 {
        return ptr::null_mut();
    }
    let Ok(layout) = std::alloc::Layout::from_size_align(size as usize, 8) else {
        return ptr::null_mut();
    };
    unsafe { std::alloc::alloc(layout) as _ }
}

#[derive(Clone, Debug)]
struct AdlAdapter {
    adapter_index: i32,
    device_id: String,
    device_name: String,
}

enum AmdBackend {
    Adlx {
        h_module: *mut std::ffi::c_void,
        _adlx_sys: *mut std::ffi::c_void,
    },
    Adl {
        h_module: *mut std::ffi::c_void,
        adapters: Vec<AdlAdapter>,
    },
}

pub struct AmdAdlxProvider {
    backend: AmdBackend,
}

unsafe impl Send for AmdAdlxProvider {}
unsafe impl Sync for AmdAdlxProvider {}

impl AmdAdlxProvider {
    pub fn try_init() -> Result<Self, (planora_core::CapabilityState, String)> {
        #[cfg(windows)]
        if !crate::has_pci_vendor_gpu(0x1002) {
            return Err((
                planora_core::CapabilityState::NotApplicable,
                "No AMD graphics adapter present on this system".into(),
            ));
        }

        // 1. Try modern AMD ADLX first if available and providing measurements
        if let Some(provider) = Self::try_init_adlx()
            && !provider.sample(crate::unix_ms()).is_empty()
        {
            return Ok(provider);
        }

        // 2. Fall back to legacy AMD ADL (atiadlxx.dll) for GCN and older cards
        if let Some(provider) = Self::try_init_adl() {
            return Ok(provider);
        }

        Err((
            planora_core::CapabilityState::Unsupported,
            "Neither AMD ADLX nor legacy ADL could provide hardware sensor readings".into(),
        ))
    }

    fn try_init_adlx() -> Option<Self> {
        #[cfg(not(windows))]
        {
            None
        }
        #[cfg(windows)]
        {
            let dll_path = crate::vendor_sensors::trusted_system_dll("amdadlx64.dll")?;
            if !dll_path.exists() {
                return None;
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
                return None;
            }

            let init_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADLXInitialize".as_ptr() as *const u8,
                )
            }?;

            let query_version_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADLXQueryFullVersion".as_ptr() as *const u8,
                )
            }?;

            type AdlxQueryFullVersionFn = unsafe extern "C" fn(*mut u64) -> i32;
            let query_version: AdlxQueryFullVersionFn =
                unsafe { std::mem::transmute(query_version_ptr) };
            let mut full_version = 0u64;
            if unsafe { query_version(&mut full_version) } != 0 || full_version == 0 {
                unsafe { FreeLibrary(h_module) };
                return None;
            }

            type AdlxInitFn = unsafe extern "C" fn(u64, *mut *mut std::ffi::c_void) -> i32;
            let adlx_init: AdlxInitFn = unsafe { std::mem::transmute(init_ptr) };

            let mut adlx_sys: *mut std::ffi::c_void = ptr::null_mut();
            let status = unsafe { adlx_init(full_version, &mut adlx_sys) };
            if status != 0 || adlx_sys.is_null() {
                unsafe { FreeLibrary(h_module) };
                return None;
            }

            Some(Self {
                backend: AmdBackend::Adlx {
                    h_module,
                    _adlx_sys: adlx_sys,
                },
            })
        }
    }

    fn try_init_adl() -> Option<Self> {
        #[cfg(not(windows))]
        {
            None
        }
        #[cfg(windows)]
        {
            let dll_path = crate::vendor_sensors::trusted_system_dll("atiadlxx.dll")?;
            if !dll_path.exists() {
                return None;
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
                return None;
            }

            let create_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADL_Main_Control_Create".as_ptr() as *const u8,
                )
            }?;
            type CreateFn = unsafe extern "C" fn(
                Option<unsafe extern "C" fn(i32) -> *mut std::ffi::c_void>,
                i32,
            ) -> i32;
            let create: CreateFn = unsafe { std::mem::transmute(create_ptr) };
            if unsafe { create(Some(adl_alloc), 1) } != 0 {
                unsafe { FreeLibrary(h_module) };
                return None;
            }

            let num_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADL_Adapter_NumberOfAdapters_Get".as_ptr() as *const u8,
                )
            }?;
            type NumFn = unsafe extern "C" fn(*mut i32) -> i32;
            let num: NumFn = unsafe { std::mem::transmute(num_ptr) };
            let mut count = 0i32;
            if unsafe { num(&mut count) } != 0 || count <= 0 {
                unsafe { FreeLibrary(h_module) };
                return None;
            }

            let info_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADL_Adapter_AdapterInfo_Get".as_ptr() as *const u8,
                )
            }?;
            type InfoFn = unsafe extern "C" fn(*mut u8, i32) -> i32;
            let get_info: InfoFn = unsafe { std::mem::transmute(info_ptr) };

            let mut infos: Vec<AdapterInfo> = vec![unsafe { std::mem::zeroed() }; count as usize];
            for item in &mut infos {
                item.size = std::mem::size_of::<AdapterInfo>() as i32;
            }
            if unsafe {
                get_info(
                    infos.as_mut_ptr() as *mut u8,
                    (std::mem::size_of::<AdapterInfo>() * count as usize) as i32,
                )
            } != 0
            {
                unsafe { FreeLibrary(h_module) };
                return None;
            }

            let mut adapters = Vec::new();
            let mut seen_buses = std::collections::HashSet::new();

            for info in &infos {
                if info.exist != 0 && info.present != 0 {
                    let bus_key = (info.bus_number, info.device_number, info.function_number);
                    if seen_buses.insert(bus_key) {
                        let name_len = info
                            .adapter_name
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(ADL_MAX_PATH);
                        let device_name = String::from_utf8_lossy(&info.adapter_name[..name_len])
                            .trim()
                            .to_string();
                        let device_id = format!(
                            "gpu:pci:{:04x}:{:02x}:{:02x}.{}",
                            info.vendor_id,
                            info.bus_number,
                            info.device_number,
                            info.function_number
                        );
                        adapters.push(AdlAdapter {
                            adapter_index: info.adapter_index,
                            device_id,
                            device_name: if device_name.is_empty() {
                                "AMD Radeon Graphics".into()
                            } else {
                                device_name
                            },
                        });
                    }
                }
            }

            if adapters.is_empty() {
                unsafe { FreeLibrary(h_module) };
                return None;
            }

            Some(Self {
                backend: AmdBackend::Adl { h_module, adapters },
            })
        }
    }

    pub fn sample(&self, stamp: u64) -> Vec<SensorReading> {
        match &self.backend {
            AmdBackend::Adlx { .. } => {
                // If ADLX is present but metrics service is unlinked, return empty
                Vec::new()
            }
            AmdBackend::Adl { h_module, adapters } => Self::sample_adl(*h_module, adapters, stamp),
        }
    }

    fn sample_adl(
        h_module: *mut std::ffi::c_void,
        adapters: &[AdlAdapter],
        stamp: u64,
    ) -> Vec<SensorReading> {
        let mut readings = Vec::new();

        #[cfg(windows)]
        {
            let act_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADL_Overdrive5_CurrentActivity_Get".as_ptr() as *const u8,
                )
            };
            let temp_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADL_Overdrive5_Temperature_Get".as_ptr() as *const u8,
                )
            };
            let fan_ptr = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"ADL_Overdrive5_FanSpeed_Get".as_ptr() as *const u8,
                )
            };

            for (idx, adapter) in adapters.iter().enumerate() {
                let scope = format!("GPU {} ({})", idx, adapter.device_name);

                // 1. Clocks, Voltage, Activity
                if let Some(act_fn_ptr) = act_ptr {
                    type ActFn = unsafe extern "C" fn(i32, *mut ADLPMActivity) -> i32;
                    let get_act: ActFn = unsafe { std::mem::transmute(act_fn_ptr) };
                    let mut act: ADLPMActivity = unsafe { std::mem::zeroed() };
                    act.size = std::mem::size_of::<ADLPMActivity>() as i32;
                    if unsafe { get_act(adapter.adapter_index, &mut act) } == 0 {
                        let core_mhz = (act.engine_clock as f32) / 100.0;
                        if core_mhz > 0.0 && core_mhz <= 15000.0 {
                            readings.push(SensorReading {
                                id: format!("{}/clock/core", adapter.device_id),
                                device_id: adapter.device_id.clone(),
                                device_name: adapter.device_name.clone(),
                                name: "GPU Core Clock".into(),
                                hardware_category: planora_core::HardwareCategory::Gpu,
                                sensor_type: SensorType::ClockMhz,
                                scope: format!("{scope} Core"),
                                provider: "AMD ADL".into(),
                                value: Metric::available(core_mhz, "MHz", "AMD ADL", stamp),
                            });
                        }

                        let mem_mhz = (act.memory_clock as f32) / 100.0;
                        if mem_mhz > 0.0 && mem_mhz <= 15000.0 {
                            readings.push(SensorReading {
                                id: format!("{}/clock/memory", adapter.device_id),
                                device_id: adapter.device_id.clone(),
                                device_name: adapter.device_name.clone(),
                                name: "GPU Memory Clock".into(),
                                hardware_category: planora_core::HardwareCategory::Gpu,
                                sensor_type: SensorType::ClockMhz,
                                scope: format!("{scope} Memory"),
                                provider: "AMD ADL".into(),
                                value: Metric::available(mem_mhz, "MHz", "AMD ADL", stamp),
                            });
                        }

                        let vddc_v = (act.vddc as f32) / 1000.0;
                        if (0.0..=3.0).contains(&vddc_v) && vddc_v > 0.1 {
                            readings.push(SensorReading {
                                id: format!("{}/voltage/core", adapter.device_id),
                                device_id: adapter.device_id.clone(),
                                device_name: adapter.device_name.clone(),
                                name: "GPU Core Voltage".into(),
                                hardware_category: planora_core::HardwareCategory::Gpu,
                                sensor_type: SensorType::VoltageVolts,
                                scope: format!("{scope} Core"),
                                provider: "AMD ADL".into(),
                                value: Metric::available(vddc_v, "V", "AMD ADL", stamp),
                            });
                        }

                        let act_pct = act.activity_percent as f32;
                        if (0.0..=100.0).contains(&act_pct) {
                            readings.push(SensorReading {
                                id: format!("{}/load/gpu", adapter.device_id),
                                device_id: adapter.device_id.clone(),
                                device_name: adapter.device_name.clone(),
                                name: "GPU Load".into(),
                                hardware_category: planora_core::HardwareCategory::Gpu,
                                sensor_type: SensorType::LoadPercent,
                                scope: scope.clone(),
                                provider: "AMD ADL".into(),
                                value: Metric::available(act_pct, "%", "AMD ADL", stamp),
                            });
                        }
                    }
                }

                // 2. Temperature
                if let Some(temp_fn_ptr) = temp_ptr {
                    type TempFn = unsafe extern "C" fn(i32, i32, *mut ADLTemperature) -> i32;
                    let get_temp: TempFn = unsafe { std::mem::transmute(temp_fn_ptr) };
                    let mut temp: ADLTemperature = unsafe { std::mem::zeroed() };
                    temp.size = std::mem::size_of::<ADLTemperature>() as i32;
                    if unsafe { get_temp(adapter.adapter_index, 0, &mut temp) } == 0 {
                        let temp_c = (temp.temperature as f32) / 1000.0;
                        if SensorType::GpuTemperatureCelsius.is_plausible_value(temp_c) {
                            readings.push(SensorReading {
                                id: format!("{}/temperature/core", adapter.device_id),
                                device_id: adapter.device_id.clone(),
                                device_name: adapter.device_name.clone(),
                                name: "GPU Temperature".into(),
                                hardware_category: planora_core::HardwareCategory::Gpu,
                                sensor_type: SensorType::GpuTemperatureCelsius,
                                scope: format!("{scope} Core"),
                                provider: "AMD ADL".into(),
                                value: Metric::available(temp_c, "°C", "AMD ADL", stamp),
                            });
                        }
                    }
                }

                // 3. Fan Speed RPM
                if let Some(fan_fn_ptr) = fan_ptr {
                    type FanFn = unsafe extern "C" fn(i32, i32, *mut ADLFanSpeedValue) -> i32;
                    let get_fan: FanFn = unsafe { std::mem::transmute(fan_fn_ptr) };
                    let mut fan: ADLFanSpeedValue = unsafe { std::mem::zeroed() };
                    fan.size = std::mem::size_of::<ADLFanSpeedValue>() as i32;
                    fan.speed_type = 2; // RPM
                    if unsafe { get_fan(adapter.adapter_index, 0, &mut fan) } == 0 {
                        let rpm = fan.fan_speed as f32;
                        if SensorType::FanRpm.is_plausible_value(rpm) {
                            readings.push(SensorReading {
                                id: format!("{}/fan/0", adapter.device_id),
                                device_id: adapter.device_id.clone(),
                                device_name: adapter.device_name.clone(),
                                name: "GPU Fan".into(),
                                hardware_category: planora_core::HardwareCategory::Gpu,
                                sensor_type: SensorType::FanRpm,
                                scope: scope.clone(),
                                provider: "AMD ADL".into(),
                                value: Metric::available(rpm, "RPM", "AMD ADL", stamp),
                            });
                        }
                    }
                }
            }
        }

        readings
    }
}

impl Drop for AmdAdlxProvider {
    fn drop(&mut self) {
        match &self.backend {
            AmdBackend::Adlx { h_module, .. } => {
                #[cfg(windows)]
                {
                    let term_ptr = unsafe {
                        windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                            *h_module,
                            c"ADLXTerminate".as_ptr() as *const u8,
                        )
                    };
                    if let Some(term_fn) = term_ptr {
                        type AdlxTermFn = unsafe extern "C" fn() -> i32;
                        let adlx_terminate: AdlxTermFn = unsafe { std::mem::transmute(term_fn) };
                        unsafe { adlx_terminate() };
                    }
                    unsafe { FreeLibrary(*h_module) };
                }
            }
            AmdBackend::Adl { h_module, .. } => {
                #[cfg(windows)]
                {
                    let destroy_ptr = unsafe {
                        windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                            *h_module,
                            c"ADL_Main_Control_Destroy".as_ptr() as *const u8,
                        )
                    };
                    if let Some(destroy_fn_ptr) = destroy_ptr {
                        type DestroyFn = unsafe extern "C" fn() -> i32;
                        let destroy: DestroyFn = unsafe { std::mem::transmute(destroy_fn_ptr) };
                        unsafe { destroy() };
                    }
                    unsafe { FreeLibrary(*h_module) };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_amd_runtime_and_sampling() {
        match AmdAdlxProvider::try_init() {
            Ok(provider) => {
                let readings = provider.sample(crate::unix_ms());
                println!(
                    "AMD provider initialized successfully, returned {} readings:",
                    readings.len()
                );
                for r in &readings {
                    println!("  {} ({}): {:?}", r.name, r.scope, r.value.value);
                }
                assert!(
                    !readings.is_empty(),
                    "AMD provider returned 0 readings on AMD host"
                );
            }
            Err((state, reason)) => {
                println!("AMD probe result: {:?}: {}", state, reason);
                assert!(!reason.is_empty());
            }
        }
    }
}
