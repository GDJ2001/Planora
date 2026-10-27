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
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application_uid: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlFirmwareVersion {
    major_version: u64,
    minor_version: u64,
    build_number: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CtlAdapterBdf {
    bus: u8,
    device: u8,
    function: u8,
}

#[repr(C)]
struct CtlDeviceAdapterProperties {
    size: u32,
    version: u8,
    reserved_pad: [u8; 3],
    p_device_id: *mut std::ffi::c_void,
    device_id_size: u32,
    device_type: u32,
    supported_subfunction_flags: u64,
    driver_version: u64,
    firmware_version: CtlFirmwareVersion,
    pci_vendor_id: u32,
    pci_device_id: u32,
    rev_id: u32,
    num_eus_per_sub_slice: u32,
    num_sub_slices_per_slice: u32,
    num_slices: u32,
    name: [u8; 100],
    graphics_adapter_properties: u32,
    frequency: u32,
    pci_subsys_id: u16,
    pci_subsys_vendor_id: u16,
    adapter_bdf: CtlAdapterBdf,
    reserved_bdf_pad: u8,
    num_xe_cores: u32,
    reserved: [u8; 108],
}

#[repr(C)]
struct CtlPowerEnergyCounter {
    size: u32,
    version: u8,
    reserved: [u8; 3],
    energy: u64,    // microjoules
    timestamp: u64, // microseconds
}

#[repr(C)]
struct CtlFreqState {
    size: u32,
    version: u8,
    reserved: [u8; 3],
    current_voltage: f64,
    request: f64,
    tdp: f64,
    efficient: f64,
    actual: f64,
    throttle_reasons: u32,
}

struct IntelDeviceState {
    #[allow(dead_code)]
    adapter_handle: *mut std::ffi::c_void,
    name: String,
    device_id: String,
    power_handles: Vec<*mut std::ffi::c_void>,
    last_energy: Vec<Option<(u64, u64)>>,
    temp_handles: Vec<*mut std::ffi::c_void>,
    freq_handles: Vec<*mut std::ffi::c_void>,
    fan_handles: Vec<*mut std::ffi::c_void>,
}

pub struct IntelIgclProvider {
    h_module: *mut std::ffi::c_void,
    ctl_api: *mut std::ffi::c_void,
    devices: Vec<IntelDeviceState>,
}

unsafe impl Send for IntelIgclProvider {}
unsafe impl Sync for IntelIgclProvider {}

impl IntelIgclProvider {
    pub fn try_init() -> Result<Self, (planora_core::CapabilityState, String)> {
        let dll_path = super::trusted_system_dll("ControlLib.dll").ok_or((
            planora_core::CapabilityState::TemporarilyFailed,
            "Could not resolve the Windows system directory".into(),
        ))?;
        if !dll_path.exists() {
            return Err((
                planora_core::CapabilityState::MissingRuntime,
                "ControlLib.dll not found in System32".into(),
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

        // Try with Level Zero flag first (flags = 1), fallback to standard (flags = 0)
        let mut ctl_api: *mut std::ffi::c_void = ptr::null_mut();
        let mut init_args = CtlInitArgs {
            size: std::mem::size_of::<CtlInitArgs>() as u32,
            version: 0,
            reserved: [0; 3],
            app_version: (1 << 16),
            flags: 1, // CTL_INIT_FLAG_USE_LEVEL_ZERO
            supported_version: 0,
            application_uid: [0; 16],
        };
        let mut status = unsafe { ctl_init(&mut init_args, &mut ctl_api) };
        if status != 0 || ctl_api.is_null() {
            init_args.flags = 0;
            status = unsafe { ctl_init(&mut init_args, &mut ctl_api) };
        }

        if status != 0 || ctl_api.is_null() {
            unsafe { FreeLibrary(h_module) };
            let reason = if status == 0x40000020 {
                "Platform or driver does not support Intel IGCL Control API (0x40000020)"
                    .to_string()
            } else if status == 0x40000009 || status == 0x40000021 {
                format!("Unsupported Intel IGCL version (0x{status:08x})")
            } else {
                format!("ctlInit failed with code 0x{status:08x}")
            };
            return Err((planora_core::CapabilityState::Unsupported, reason));
        }

        let ctl_enum_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlEnumerateDevices".as_ptr() as *const u8,
            )
        };
        let ctl_props_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlGetDeviceProperties".as_ptr() as *const u8,
            )
        };
        let ctl_enum_pwr_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlEnumPowerDomains".as_ptr() as *const u8,
            )
        };
        let ctl_enum_temp_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlEnumTemperatureSensors".as_ptr() as *const u8,
            )
        };
        let ctl_enum_freq_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlEnumFrequencyDomains".as_ptr() as *const u8,
            )
        };
        let ctl_enum_fan_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                h_module,
                c"ctlEnumFans".as_ptr() as *const u8,
            )
        };

        type CtlEnumDevicesFn = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *mut u32,
            *mut *mut std::ffi::c_void,
        ) -> i32;
        type CtlGetPropsFn = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *mut CtlDeviceAdapterProperties,
        ) -> i32;
        type CtlEnumDomainFn = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *mut u32,
            *mut *mut std::ffi::c_void,
        ) -> i32;

        let mut devices = Vec::new();
        if let Some(ctl_enum) = ctl_enum_ptr {
            let ctl_enum: CtlEnumDevicesFn = unsafe { std::mem::transmute(ctl_enum) };
            let mut count = 0u32;
            if unsafe { ctl_enum(ctl_api, &mut count, ptr::null_mut()) } == 0 && count > 0 {
                let mut dev_ptrs = vec![ptr::null_mut(); count as usize];
                if unsafe { ctl_enum(ctl_api, &mut count, dev_ptrs.as_mut_ptr()) } == 0 {
                    for (idx, dev) in dev_ptrs.into_iter().take(count as usize).enumerate() {
                        if dev.is_null() {
                            continue;
                        }
                        let mut name = format!("Intel Graphics Adapter {idx}");
                        let mut dev_id = format!("intel_gpu/{idx}");
                        if let Some(ctl_props) = ctl_props_ptr {
                            let ctl_props: CtlGetPropsFn =
                                unsafe { std::mem::transmute(ctl_props) };
                            let mut luid_bytes = [0u8; 8];
                            let mut props_ok = false;
                            let mut props = CtlDeviceAdapterProperties {
                                size: std::mem::size_of::<CtlDeviceAdapterProperties>() as u32,
                                version: 2,
                                reserved_pad: [0; 3],
                                p_device_id: luid_bytes.as_mut_ptr().cast(),
                                device_id_size: luid_bytes.len() as u32,
                                device_type: 0,
                                supported_subfunction_flags: 0,
                                driver_version: 0,
                                firmware_version: CtlFirmwareVersion::default(),
                                pci_vendor_id: 0,
                                pci_device_id: 0,
                                rev_id: 0,
                                num_eus_per_sub_slice: 0,
                                num_sub_slices_per_slice: 0,
                                num_slices: 0,
                                name: [0; 100],
                                graphics_adapter_properties: 0,
                                frequency: 0,
                                pci_subsys_id: 0,
                                pci_subsys_vendor_id: 0,
                                adapter_bdf: CtlAdapterBdf::default(),
                                reserved_bdf_pad: 0,
                                num_xe_cores: 0,
                                reserved: [0; 108],
                            };
                            for ver in [2u8, 1, 0] {
                                luid_bytes = [0u8; 8];
                                props.version = ver;
                                props.p_device_id = luid_bytes.as_mut_ptr().cast();
                                props.device_id_size = luid_bytes.len() as u32;
                                if unsafe { ctl_props(dev, &mut props) } == 0 {
                                    props_ok = true;
                                    break;
                                }
                            }
                            if props_ok {
                                let luid = u64::from_le_bytes(luid_bytes);
                                if luid != 0 {
                                    dev_id = format!("gpu_luid:0x{luid:016x}");
                                }
                                let len = props
                                    .name
                                    .iter()
                                    .position(|&b| b == 0)
                                    .unwrap_or(props.name.len());
                                if let Ok(s) = std::str::from_utf8(&props.name[..len]) {
                                    let trimmed = s.trim();
                                    if !trimmed.is_empty() {
                                        name = trimmed.to_string();
                                    }
                                }
                            }
                        }

                        // Enumerate power domains
                        let mut power_handles = Vec::new();
                        if let Some(enum_pwr) = ctl_enum_pwr_ptr {
                            let enum_pwr: CtlEnumDomainFn =
                                unsafe { std::mem::transmute(enum_pwr) };
                            let mut pwr_count = 0u32;
                            if unsafe { enum_pwr(dev, &mut pwr_count, ptr::null_mut()) } == 0
                                && pwr_count > 0
                            {
                                let mut ptrs = vec![ptr::null_mut(); pwr_count as usize];
                                if unsafe { enum_pwr(dev, &mut pwr_count, ptrs.as_mut_ptr()) } == 0
                                {
                                    power_handles.extend(
                                        ptrs.into_iter()
                                            .take(pwr_count as usize)
                                            .filter(|p| !p.is_null()),
                                    );
                                }
                            }
                        }
                        let last_energy = vec![None; power_handles.len()];

                        // Enumerate temperature sensors
                        let mut temp_handles = Vec::new();
                        if let Some(enum_temp) = ctl_enum_temp_ptr {
                            let enum_temp: CtlEnumDomainFn =
                                unsafe { std::mem::transmute(enum_temp) };
                            let mut temp_count = 0u32;
                            if unsafe { enum_temp(dev, &mut temp_count, ptr::null_mut()) } == 0
                                && temp_count > 0
                            {
                                let mut ptrs = vec![ptr::null_mut(); temp_count as usize];
                                if unsafe { enum_temp(dev, &mut temp_count, ptrs.as_mut_ptr()) }
                                    == 0
                                {
                                    temp_handles.extend(
                                        ptrs.into_iter()
                                            .take(temp_count as usize)
                                            .filter(|p| !p.is_null()),
                                    );
                                }
                            }
                        }

                        // Enumerate frequency domains
                        let mut freq_handles = Vec::new();
                        if let Some(enum_freq) = ctl_enum_freq_ptr {
                            let enum_freq: CtlEnumDomainFn =
                                unsafe { std::mem::transmute(enum_freq) };
                            let mut freq_count = 0u32;
                            if unsafe { enum_freq(dev, &mut freq_count, ptr::null_mut()) } == 0
                                && freq_count > 0
                            {
                                let mut ptrs = vec![ptr::null_mut(); freq_count as usize];
                                if unsafe { enum_freq(dev, &mut freq_count, ptrs.as_mut_ptr()) }
                                    == 0
                                {
                                    freq_handles.extend(
                                        ptrs.into_iter()
                                            .take(freq_count as usize)
                                            .filter(|p| !p.is_null()),
                                    );
                                }
                            }
                        }

                        // Enumerate fans
                        let mut fan_handles = Vec::new();
                        if let Some(enum_fan) = ctl_enum_fan_ptr {
                            let enum_fan: CtlEnumDomainFn =
                                unsafe { std::mem::transmute(enum_fan) };
                            let mut fan_count = 0u32;
                            if unsafe { enum_fan(dev, &mut fan_count, ptr::null_mut()) } == 0
                                && fan_count > 0
                            {
                                let mut ptrs = vec![ptr::null_mut(); fan_count as usize];
                                if unsafe { enum_fan(dev, &mut fan_count, ptrs.as_mut_ptr()) } == 0
                                {
                                    fan_handles.extend(
                                        ptrs.into_iter()
                                            .take(fan_count as usize)
                                            .filter(|p| !p.is_null()),
                                    );
                                }
                            }
                        }

                        devices.push(IntelDeviceState {
                            adapter_handle: dev,
                            name,
                            device_id: dev_id,
                            power_handles,
                            last_energy,
                            temp_handles,
                            freq_handles,
                            fan_handles,
                        });
                    }
                }
            }
        }

        if devices.is_empty() {
            return Err((
                planora_core::CapabilityState::NotApplicable,
                "No Intel graphics adapters discovered via IGCL".into(),
            ));
        }

        Ok(Self {
            h_module,
            ctl_api,
            devices,
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
        let ctl_fan_ptr = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.h_module,
                c"ctlFanGetState".as_ptr() as *const u8,
            )
        };

        type CtlPowerFn =
            unsafe extern "system" fn(*mut std::ffi::c_void, *mut CtlPowerEnergyCounter) -> i32;
        type CtlTempFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut f64) -> i32;
        type CtlFreqFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut CtlFreqState) -> i32;
        type CtlFanFn = unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut i32) -> i32;

        for dev_state in &mut self.devices {
            let dev_id = dev_state.device_id.clone();
            let dev_name = dev_state.name.clone();

            // 1. Power domain handles -> Watts from energy counter delta
            if let Some(ctl_power) = ctl_power_ptr {
                let ctl_power: CtlPowerFn = unsafe { std::mem::transmute(ctl_power) };
                for (p_idx, &pwr_handle) in dev_state.power_handles.iter().enumerate() {
                    let mut counter = CtlPowerEnergyCounter {
                        size: std::mem::size_of::<CtlPowerEnergyCounter>() as u32,
                        version: 0,
                        reserved: [0; 3],
                        energy: 0,
                        timestamp: 0,
                    };
                    if unsafe { ctl_power(pwr_handle, &mut counter) } == 0 && counter.timestamp > 0
                    {
                        if let Some(Some((prev_energy, prev_time))) =
                            dev_state.last_energy.get(p_idx)
                            && *prev_time > 0
                            && counter.timestamp > *prev_time
                        {
                            if counter.energy < *prev_energy {
                                if p_idx < dev_state.last_energy.len() {
                                    dev_state.last_energy[p_idx] =
                                        Some((counter.energy, counter.timestamp));
                                }
                                continue;
                            }
                            let delta_e = counter.energy - *prev_energy;
                            let delta_t = counter.timestamp - *prev_time;
                            if delta_t > 0 {
                                let watts = (delta_e as f64 / delta_t as f64) as f32;
                                if watts.is_finite() && (0.0..=1000.0).contains(&watts) {
                                    let sensor_name = if p_idx == 0 {
                                        "GPU Package Power".to_string()
                                    } else {
                                        format!("GPU Power Domain {p_idx}")
                                    };
                                    readings.push(SensorReading {
                                        id: format!("{dev_id}/power/{p_idx}"),
                                        device_id: dev_id.clone(),
                                        device_name: dev_name.clone(),
                                        name: sensor_name,
                                        hardware_category: planora_core::HardwareCategory::Gpu,
                                        sensor_type: SensorType::GpuChipPowerWatts,
                                        scope: "GPU".into(),
                                        provider: "Intel IGCL".into(),
                                        value: Metric::available(
                                            watts,
                                            planora_core::UNIT_WATTS,
                                            "Intel IGCL",
                                            stamp,
                                        ),
                                    });
                                }
                            }
                        }
                        if p_idx < dev_state.last_energy.len() {
                            dev_state.last_energy[p_idx] =
                                Some((counter.energy, counter.timestamp));
                        }
                    }
                }
            }

            // 2. Temperature handles -> °C
            if let Some(ctl_temp) = ctl_temp_ptr {
                let ctl_temp: CtlTempFn = unsafe { std::mem::transmute(ctl_temp) };
                for (t_idx, &temp_handle) in dev_state.temp_handles.iter().enumerate() {
                    let mut temp: f64 = 0.0;
                    if unsafe { ctl_temp(temp_handle, &mut temp) } == 0
                        && temp.is_finite()
                        && (0.0..=125.0).contains(&temp)
                    {
                        let sensor_name = if t_idx == 0 {
                            "GPU Core Temperature".to_string()
                        } else {
                            format!("GPU Temperature {t_idx}")
                        };
                        readings.push(SensorReading {
                            id: format!("{dev_id}/temperature/{t_idx}"),
                            device_id: dev_id.clone(),
                            device_name: dev_name.clone(),
                            name: sensor_name,
                            hardware_category: planora_core::HardwareCategory::Gpu,
                            sensor_type: SensorType::GpuTemperatureCelsius,
                            scope: "GPU".into(),
                            provider: "Intel IGCL".into(),
                            value: Metric::available(
                                temp as f32,
                                planora_core::UNIT_CELSIUS,
                                "Intel IGCL",
                                stamp,
                            ),
                        });
                    }
                }
            }

            // 3. Frequency handles -> MHz
            if let Some(ctl_freq) = ctl_freq_ptr {
                let ctl_freq: CtlFreqFn = unsafe { std::mem::transmute(ctl_freq) };
                for (f_idx, &freq_handle) in dev_state.freq_handles.iter().enumerate() {
                    let mut freq_state = CtlFreqState {
                        size: std::mem::size_of::<CtlFreqState>() as u32,
                        version: 0,
                        reserved: [0; 3],
                        current_voltage: 0.0,
                        request: 0.0,
                        tdp: 0.0,
                        efficient: 0.0,
                        actual: 0.0,
                        throttle_reasons: 0,
                    };
                    if unsafe { ctl_freq(freq_handle, &mut freq_state) } == 0
                        && freq_state.actual.is_finite()
                        && (0.0..=10_000.0).contains(&freq_state.actual)
                    {
                        let sensor_name = if f_idx == 0 {
                            "GPU Core Clock".to_string()
                        } else {
                            format!("GPU Clock {f_idx}")
                        };
                        readings.push(SensorReading {
                            id: format!("{dev_id}/clock/{f_idx}"),
                            device_id: dev_id.clone(),
                            device_name: dev_name.clone(),
                            name: sensor_name,
                            hardware_category: planora_core::HardwareCategory::Gpu,
                            sensor_type: SensorType::ClockMhz,
                            scope: "GPU".into(),
                            provider: "Intel IGCL".into(),
                            value: Metric::available(
                                freq_state.actual as f32,
                                planora_core::UNIT_MHZ,
                                "Intel IGCL",
                                stamp,
                            ),
                        });
                    }
                }
            }

            // 4. Fan handles -> RPM
            if let Some(ctl_fan) = ctl_fan_ptr {
                let ctl_fan: CtlFanFn = unsafe { std::mem::transmute(ctl_fan) };
                for (fan_idx, &fan_handle) in dev_state.fan_handles.iter().enumerate() {
                    let mut speed: i32 = 0;
                    // Units: 1 = RPM
                    if unsafe { ctl_fan(fan_handle, 1, &mut speed) } == 0
                        && (0..=30_000).contains(&speed)
                    {
                        readings.push(SensorReading {
                            id: format!("{dev_id}/fan/{fan_idx}"),
                            device_id: dev_id.clone(),
                            device_name: dev_name.clone(),
                            name: format!("GPU Fan {fan_idx}"),
                            hardware_category: planora_core::HardwareCategory::Gpu,
                            sensor_type: SensorType::FanRpm,
                            scope: "GPU".into(),
                            provider: "Intel IGCL".into(),
                            value: Metric::available(
                                speed as f32,
                                planora_core::UNIT_RPM,
                                "Intel IGCL",
                                stamp,
                            ),
                        });
                    }
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
    fn igcl_struct_abi_layouts_match_header() {
        assert_eq!(std::mem::size_of::<CtlInitArgs>(), 36);
        assert_eq!(std::mem::size_of::<CtlFirmwareVersion>(), 24);
        assert_eq!(std::mem::size_of::<CtlDeviceAdapterProperties>(), 320);
        assert_eq!(
            std::mem::offset_of!(CtlDeviceAdapterProperties, firmware_version),
            40
        );
        assert_eq!(std::mem::offset_of!(CtlDeviceAdapterProperties, name), 88);
        assert_eq!(
            std::mem::offset_of!(CtlDeviceAdapterProperties, adapter_bdf),
            200
        );
        assert_eq!(std::mem::size_of::<CtlPowerEnergyCounter>(), 24);
        assert_eq!(std::mem::size_of::<CtlFreqState>(), 56);
    }

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
