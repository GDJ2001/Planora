//! Comprehensive hardware inventory discovery for CPU, GPU, Motherboard, Memory, Storage, Network, and Cooling.

#[cfg(windows)]
use planora_core::MemoryModuleDescriptor;
use planora_core::{
    CoolingInventoryDevice, CpuInventory, GpuInventoryDevice, HardwareInventory, MemoryInventory,
    MotherboardInventory, NetworkInventoryDevice, StorageInventoryDevice,
};

#[cfg(windows)]
unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

pub fn discover_inventory() -> HardwareInventory {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let topo = crate::discover();

    HardwareInventory {
        generation: 1,
        timestamp_ms: now_ms,
        cpu: discover_cpu(&topo),
        gpus: discover_gpus(),
        motherboard: discover_motherboard(),
        memory: discover_memory(),
        storage: discover_storage(),
        network: discover_network(),
        cooling: discover_cooling(),
    }
}

fn discover_cpu(topo: &planora_core::Topology) -> CpuInventory {
    let mut cpu = CpuInventory {
        vendor: topo.vendor.clone(),
        brand: topo.model.clone(),
        architecture: topo.architecture.clone(),
        physical_cores: topo.cores.len() as u16,
        logical_cpus: (topo
            .cores
            .iter()
            .map(|c| c.logical_cpus.len())
            .sum::<usize>()
            .max(1)) as u16,
        ..Default::default()
    };

    let max_package = topo
        .cores
        .iter()
        .filter_map(|c| c.package_id)
        .max()
        .map(|p| p + 1)
        .unwrap_or(1);
    cpu.packages = max_package;

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        let cpuid = raw_cpuid::CpuId::new();
        if let Some(finfo) = cpuid.get_feature_info() {
            let base_family = finfo.family_id();
            let base_model = finfo.model_id();
            let ext_family = finfo.extended_family_id();
            let ext_model = finfo.extended_model_id();

            let mut family = base_family as u32;
            let mut model = base_model as u32;
            if family == 0x0F {
                family += ext_family as u32;
            }
            if family == 0x06 || family == 0x0F {
                model += (ext_model as u32) << 4;
            }

            cpu.family = family;
            cpu.model = model;
            cpu.stepping = finfo.stepping_id() as u32;

            let mut features = Vec::new();
            if finfo.has_sse() {
                features.push("SSE".into());
            }
            if finfo.has_sse2() {
                features.push("SSE2".into());
            }
            if finfo.has_sse3() {
                features.push("SSE3".into());
            }
            if finfo.has_ssse3() {
                features.push("SSSE3".into());
            }
            if finfo.has_sse41() {
                features.push("SSE4.1".into());
            }
            if finfo.has_sse42() {
                features.push("SSE4.2".into());
            }
            if finfo.has_avx() {
                features.push("AVX".into());
            }
            if finfo.has_fma() {
                features.push("FMA".into());
            }
            if finfo.has_aesni() {
                features.push("AES".into());
            }
            if finfo.has_f16c() {
                features.push("F16C".into());
            }

            if let Some(ext) = cpuid.get_extended_feature_info() {
                if ext.has_avx2() {
                    features.push("AVX2".into());
                }
                if ext.has_bmi1() {
                    features.push("BMI1".into());
                }
                if ext.has_bmi2() {
                    features.push("BMI2".into());
                }
                if ext.has_avx512f() {
                    features.push("AVX512F".into());
                }
            }
            cpu.features = features;
        }

        if let Some(freq) = cpuid.get_processor_frequency_info() {
            let base = freq.processor_base_frequency();
            let max = freq.processor_max_frequency();
            if base > 0 {
                cpu.base_frequency_mhz = Some(base as u32);
            }
            if max > 0 {
                cpu.max_frequency_mhz = Some(max as u32);
            }
        }
    }

    // Fallback frequency parse from brand string e.g. "@ 3.60GHz"
    if cpu.base_frequency_mhz.is_none()
        && let Some(pos) = cpu.brand.find('@')
    {
        let freq_part = cpu.brand[pos + 1..].trim();
        if let Some(ghz_pos) = freq_part.find("GHz")
            && let Ok(ghz) = freq_part[..ghz_pos].trim().parse::<f32>()
        {
            cpu.base_frequency_mhz = Some((ghz * 1000.0) as u32);
        }
    }

    cpu
}

#[cfg(windows)]
fn discover_gpus() -> Vec<GpuInventoryDevice> {
    let mut gpus = Vec::new();

    // 1. Correlate with Windows display driver registry for driver versions & dates
    let mut reg_drivers: Vec<(u16, u16, String, String, String)> = Vec::new();
    unsafe {
        let class_key = windows_sys::core::w!(
            "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}"
        );
        let mut h_class = std::ptr::null_mut();
        if windows_sys::Win32::System::Registry::RegOpenKeyExW(
            windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            class_key,
            0,
            windows_sys::Win32::System::Registry::KEY_READ,
            &mut h_class,
        ) == 0
        {
            let mut sub_idx = 0u32;
            let mut sub_name = [0u16; 64];
            while windows_sys::Win32::System::Registry::RegEnumKeyW(
                h_class,
                sub_idx,
                sub_name.as_mut_ptr(),
                sub_name.len() as u32,
            ) == 0
            {
                sub_idx += 1;
                let mut h_sub = std::ptr::null_mut();
                if windows_sys::Win32::System::Registry::RegOpenKeyExW(
                    h_class,
                    sub_name.as_ptr(),
                    0,
                    windows_sys::Win32::System::Registry::KEY_READ,
                    &mut h_sub,
                ) == 0
                {
                    let read_reg_str = |val_name: *const u16| -> String {
                        let mut buf = [0u16; 256];
                        let mut cb = (buf.len() * 2) as u32;
                        let mut ty = 0u32;
                        if windows_sys::Win32::System::Registry::RegQueryValueExW(
                            h_sub,
                            val_name,
                            std::ptr::null_mut(),
                            &mut ty,
                            buf.as_mut_ptr() as *mut u8,
                            &mut cb,
                        ) == 0
                        {
                            let chars = (cb / 2) as usize;
                            let end = buf[..chars].iter().position(|&c| c == 0).unwrap_or(chars);
                            String::from_utf16_lossy(&buf[..end]).trim().to_string()
                        } else {
                            String::new()
                        }
                    };

                    let driver_desc = read_reg_str(windows_sys::core::w!("DriverDesc"));
                    let driver_ver = read_reg_str(windows_sys::core::w!("DriverVersion"));
                    let driver_date = read_reg_str(windows_sys::core::w!("DriverDate"));
                    let matching_id = read_reg_str(windows_sys::core::w!("MatchingDeviceId"));

                    let mut ven_id = 0u16;
                    let mut dev_id = 0u16;
                    let upper = matching_id.to_uppercase();
                    if let Some(vpos) = upper.find("VEN_")
                        && vpos + 8 <= upper.len()
                    {
                        ven_id = u16::from_str_radix(&upper[vpos + 4..vpos + 8], 16).unwrap_or(0);
                    }
                    if let Some(dpos) = upper.find("DEV_")
                        && dpos + 8 <= upper.len()
                    {
                        dev_id = u16::from_str_radix(&upper[dpos + 4..dpos + 8], 16).unwrap_or(0);
                    }

                    if ven_id > 0 && dev_id > 0 {
                        reg_drivers.push((ven_id, dev_id, driver_desc, driver_ver, driver_date));
                    }

                    windows_sys::Win32::System::Registry::RegCloseKey(h_sub);
                }
            }
            windows_sys::Win32::System::Registry::RegCloseKey(h_class);
        }
    }

    // 2. Query DXGI Adapters
    #[repr(C)]
    struct DXGI_ADAPTER_DESC1 {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        sub_sys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        adapter_luid: windows_sys::Win32::Foundation::LUID,
        flags: u32,
    }

    let dxgi_path = super::trusted_system_dll("dxgi.dll");
    let mut dxgi_loaded = false;
    if let Some(dll_path) = dxgi_path {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dll_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let h_module = unsafe {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                0x00000800,
            )
        };
        if !h_module.is_null() {
            let proc = unsafe {
                windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                    h_module,
                    c"CreateDXGIFactory1".as_ptr() as *const u8,
                )
            };
            if let Some(pfn) = proc {
                type CreateFactory1Fn = unsafe extern "system" fn(
                    *const windows_sys::core::GUID,
                    *mut *mut std::ffi::c_void,
                ) -> i32;
                let create_factory: CreateFactory1Fn = unsafe { std::mem::transmute(pfn) };

                let iid_idxgi_factory1 = windows_sys::core::GUID {
                    data1: 0x770aae78,
                    data2: 0xf26f,
                    data3: 0x4dba,
                    data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
                };

                let mut factory: *mut std::ffi::c_void = std::ptr::null_mut();
                if unsafe { create_factory(&iid_idxgi_factory1, &mut factory) } == 0
                    && !factory.is_null()
                {
                    dxgi_loaded = true;
                    let factory_vtable = unsafe { *(factory as *mut *mut *mut std::ffi::c_void) };
                    // EnumAdapters1 is at index 12 in IDXGIFactory1 vtable
                    type EnumAdapters1Fn = unsafe extern "system" fn(
                        *mut std::ffi::c_void,
                        u32,
                        *mut *mut std::ffi::c_void,
                    ) -> i32;
                    let enum_adapters1: EnumAdapters1Fn =
                        unsafe { std::mem::transmute(*factory_vtable.add(12)) };

                    let mut i = 0u32;
                    loop {
                        let mut adapter: *mut std::ffi::c_void = std::ptr::null_mut();
                        if unsafe { enum_adapters1(factory, i, &mut adapter) } != 0
                            || adapter.is_null()
                        {
                            break;
                        }
                        i += 1;

                        let adapter_vtable =
                            unsafe { *(adapter as *mut *mut *mut std::ffi::c_void) };
                        // GetDesc1 is at index 10 in IDXGIAdapter1 vtable
                        type GetDesc1Fn = unsafe extern "system" fn(
                            *mut std::ffi::c_void,
                            *mut DXGI_ADAPTER_DESC1,
                        ) -> i32;
                        let get_desc1: GetDesc1Fn =
                            unsafe { std::mem::transmute(*adapter_vtable.add(10)) };

                        let mut desc: DXGI_ADAPTER_DESC1 = unsafe { std::mem::zeroed() };
                        if unsafe { get_desc1(adapter, &mut desc) } == 0 {
                            // Filter software adapter (DXGI_ADAPTER_FLAG_SOFTWARE = 2)
                            if desc.flags & 2 == 0 {
                                let len = desc
                                    .description
                                    .iter()
                                    .position(|&c| c == 0)
                                    .unwrap_or(desc.description.len());
                                let name = String::from_utf16_lossy(&desc.description[..len])
                                    .trim()
                                    .to_string();

                                let pci_ven = desc.vendor_id as u16;
                                let pci_dev = desc.device_id as u16;
                                let vendor_name = match pci_ven {
                                    0x10DE => "NVIDIA",
                                    0x1002 => "AMD",
                                    0x8086 => "Intel",
                                    0x1414 => "Microsoft",
                                    0x106B => "Apple",
                                    _ => "Unknown Vendor",
                                }
                                .to_string();

                                let luid_u64 = ((desc.adapter_luid.HighPart as u64) << 32)
                                    | (desc.adapter_luid.LowPart as u64);

                                let is_discrete = desc.dedicated_video_memory > 256 * 1024 * 1024
                                    && pci_ven != 0x8086;

                                // Correlate registry driver info
                                let reg_match = reg_drivers
                                    .iter()
                                    .find(|(v, d, _, _, _)| *v == pci_ven && *d == pci_dev);

                                let (driver_ver, driver_date) =
                                    if let Some((_, _, _, ver, date)) = reg_match {
                                        (
                                            (!ver.is_empty()).then_some(ver.clone()),
                                            (!date.is_empty()).then_some(date.clone()),
                                        )
                                    } else {
                                        (None, None)
                                    };

                                let display_name = if !name.is_empty() {
                                    name
                                } else if let Some((_, _, ddesc, _, _)) = reg_match {
                                    ddesc.clone()
                                } else {
                                    format!("{vendor_name} Graphics Adapter")
                                };

                                gpus.push(GpuInventoryDevice {
                                    device_id: format!("gpu:pci:{:04x}:{:04x}", pci_ven, pci_dev),
                                    name: display_name,
                                    vendor_name,
                                    pci_vendor_id: pci_ven,
                                    pci_device_id: pci_dev,
                                    pci_subsystem_id: (desc.sub_sys_id > 0)
                                        .then_some(desc.sub_sys_id),
                                    pci_bus_location: None,
                                    adapter_luid: Some(luid_u64),
                                    dedicated_vram_bytes: desc.dedicated_video_memory as u64,
                                    is_discrete,
                                    driver_version: driver_ver,
                                    driver_date,
                                });
                            }
                        }

                        // Release adapter
                        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
                        let release: ReleaseFn =
                            unsafe { std::mem::transmute(*adapter_vtable.add(2)) };
                        unsafe { release(adapter) };
                    }

                    // Release factory
                    type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
                    let release: ReleaseFn = unsafe { std::mem::transmute(*factory_vtable.add(2)) };
                    unsafe { release(factory) };
                }
            }
            unsafe { FreeLibrary(h_module) };
        }
    }

    // Fallback if DXGI was unavailable: build devices from display registry entries
    if !dxgi_loaded && gpus.is_empty() {
        for (ven_id, dev_id, desc, ver, date) in reg_drivers {
            let vendor_name = match ven_id {
                0x10DE => "NVIDIA",
                0x1002 => "AMD",
                0x8086 => "Intel",
                _ => "Unknown Vendor",
            }
            .to_string();

            gpus.push(GpuInventoryDevice {
                device_id: format!("gpu:pci:{:04x}:{:04x}", ven_id, dev_id),
                name: desc,
                vendor_name,
                pci_vendor_id: ven_id,
                pci_device_id: dev_id,
                pci_subsystem_id: None,
                pci_bus_location: None,
                adapter_luid: None,
                dedicated_vram_bytes: 0,
                is_discrete: ven_id != 0x8086,
                driver_version: (!ver.is_empty()).then_some(ver),
                driver_date: (!date.is_empty()).then_some(date),
            });
        }
    }

    gpus
}

#[cfg(not(windows))]
fn discover_gpus() -> Vec<GpuInventoryDevice> {
    #[allow(unused_mut)]
    let mut gpus = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with("card") || !name[4..].chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let dev_dir = entry.path().join("device");
                if !dev_dir.exists() {
                    continue;
                }

                let read_hex = |f: &str| -> u16 {
                    std::fs::read_to_string(dev_dir.join(f))
                        .ok()
                        .and_then(|s| {
                            let clean = s.trim().strip_prefix("0x").unwrap_or(s.trim());
                            u16::from_str_radix(clean, 16).ok()
                        })
                        .unwrap_or(0)
                };

                let ven_id = read_hex("vendor");
                let dev_id = read_hex("device");
                if ven_id > 0 && dev_id > 0 {
                    let vendor_name = match ven_id {
                        0x10DE => "NVIDIA",
                        0x1002 => "AMD",
                        0x8086 => "Intel",
                        _ => "PCI GPU",
                    }
                    .to_string();

                    let sub_ven = read_hex("subsystem_vendor");
                    let sub_dev = read_hex("subsystem_device");
                    let pci_subsystem_id = if sub_ven > 0 && sub_dev > 0 {
                        Some(((sub_ven as u32) << 16) | (sub_dev as u32))
                    } else {
                        None
                    };

                    let pci_bus_location = std::fs::read_link(&dev_dir)
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));

                    let dedicated_vram_bytes =
                        std::fs::read_to_string(dev_dir.join("mem_info_vram_total"))
                            .ok()
                            .and_then(|s| s.trim().parse::<u64>().ok())
                            .or_else(|| {
                                std::fs::read_to_string(dev_dir.join("resource"))
                                    .ok()
                                    .and_then(|res| {
                                        for line in res.lines() {
                                            let parts: Vec<&str> =
                                                line.split_whitespace().collect();
                                            if parts.len() >= 3 {
                                                let start = u64::from_str_radix(
                                                    parts[0].trim_start_matches("0x"),
                                                    16,
                                                )
                                                .ok()?;
                                                let end = u64::from_str_radix(
                                                    parts[1].trim_start_matches("0x"),
                                                    16,
                                                )
                                                .ok()?;
                                                let flags = u64::from_str_radix(
                                                    parts[2].trim_start_matches("0x"),
                                                    16,
                                                )
                                                .ok()?;
                                                if end > start && (flags & 0x200 != 0) {
                                                    let size = end - start + 1;
                                                    if size >= 64 * 1024 * 1024 {
                                                        return Some(size);
                                                    }
                                                }
                                            }
                                        }
                                        None
                                    })
                            })
                            .unwrap_or(0);

                    let driver_version = std::fs::read_link(dev_dir.join("driver"))
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .and_then(|drv| {
                            std::fs::read_to_string(format!("/sys/module/{drv}/version"))
                                .or_else(|_| {
                                    std::fs::read_to_string(dev_dir.join("driver/module/version"))
                                })
                                .ok()
                                .map(|s| s.trim().to_string())
                        });

                    let device_id = if let Some(bus) = &pci_bus_location {
                        format!("gpu:pci:{}", bus)
                    } else {
                        format!("gpu:pci:{:04x}:{:04x}", ven_id, dev_id)
                    };

                    gpus.push(GpuInventoryDevice {
                        device_id,
                        name: format!("{vendor_name} GPU ({name})"),
                        vendor_name,
                        pci_vendor_id: ven_id,
                        pci_device_id: dev_id,
                        pci_subsystem_id,
                        pci_bus_location,
                        adapter_luid: None,
                        dedicated_vram_bytes,
                        is_discrete: ven_id != 0x8086,
                        driver_version,
                        driver_date: None,
                    });
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        gpus.push(GpuInventoryDevice {
            device_id: "gpu:apple:integrated".into(),
            name: "Apple Integrated GPU".into(),
            vendor_name: "Apple".into(),
            pci_vendor_id: 0x106B,
            pci_device_id: 0,
            pci_subsystem_id: None,
            pci_bus_location: None,
            adapter_luid: None,
            dedicated_vram_bytes: 0,
            is_discrete: false,
            driver_version: None,
            driver_date: None,
        });
    }
    gpus
}

#[cfg(windows)]
fn discover_motherboard() -> MotherboardInventory {
    let mut mobo = MotherboardInventory::default();
    unsafe {
        let bios_key = windows_sys::core::w!("HARDWARE\\DESCRIPTION\\System\\BIOS");
        let mut h_key = std::ptr::null_mut();
        if windows_sys::Win32::System::Registry::RegOpenKeyExW(
            windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            bios_key,
            0,
            windows_sys::Win32::System::Registry::KEY_READ,
            &mut h_key,
        ) == 0
        {
            let read_val = |val_name: *const u16| -> String {
                let mut buf = [0u16; 256];
                let mut cb = (buf.len() * 2) as u32;
                let mut ty = 0u32;
                if windows_sys::Win32::System::Registry::RegQueryValueExW(
                    h_key,
                    val_name,
                    std::ptr::null_mut(),
                    &mut ty,
                    buf.as_mut_ptr() as *mut u8,
                    &mut cb,
                ) == 0
                {
                    let chars = (cb / 2) as usize;
                    let end = buf[..chars].iter().position(|&c| c == 0).unwrap_or(chars);
                    String::from_utf16_lossy(&buf[..end]).trim().to_string()
                } else {
                    String::new()
                }
            };

            mobo.manufacturer = read_val(windows_sys::core::w!("BaseBoardManufacturer"));
            mobo.product_name = read_val(windows_sys::core::w!("BaseBoardProduct"));
            mobo.version = read_val(windows_sys::core::w!("BaseBoardVersion"));
            mobo.bios_vendor = read_val(windows_sys::core::w!("BIOSVendor"));
            mobo.bios_version = read_val(windows_sys::core::w!("BIOSVersion"));
            mobo.bios_release_date = read_val(windows_sys::core::w!("BIOSReleaseDate"));

            windows_sys::Win32::System::Registry::RegCloseKey(h_key);
        }
    }
    mobo
}

#[cfg(not(windows))]
fn discover_motherboard() -> MotherboardInventory {
    #[cfg(target_os = "linux")]
    let mut mobo = MotherboardInventory::default();
    #[cfg(not(target_os = "linux"))]
    let mobo = MotherboardInventory::default();
    #[cfg(target_os = "linux")]
    {
        let read = |path: &str| -> String {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        mobo.manufacturer = read("/sys/class/dmi/id/board_vendor");
        mobo.product_name = read("/sys/class/dmi/id/board_name");
        mobo.version = read("/sys/class/dmi/id/board_version");
        mobo.bios_vendor = read("/sys/class/dmi/id/bios_vendor");
        mobo.bios_version = read("/sys/class/dmi/id/bios_version");
        mobo.bios_release_date = read("/sys/class/dmi/id/bios_date");
    }
    mobo
}

#[cfg(windows)]
fn discover_memory() -> MemoryInventory {
    let mut mem = MemoryInventory::default();

    // 1. Total physical bytes via GlobalMemoryStatusEx
    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct MEMORYSTATUSEX {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    unsafe extern "system" {
        fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
        fn GetSystemFirmwareTable(
            FirmwareTableProviderSignature: u32,
            FirmwareTableID: u32,
            pFirmwareTableBuffer: *mut u8,
            BufferSize: u32,
        ) -> u32;
    }

    let mut state: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    state.dw_length = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut state) } != 0 {
        mem.total_physical_bytes = state.ull_total_phys;
    }

    // 2. SMBIOS Type 17 Memory Device parsing via GetSystemFirmwareTable('RSMB')
    let rsmb_sig = 0x52534D42; // 'RSMB' in ASCII
    let buf_size = unsafe { GetSystemFirmwareTable(rsmb_sig, 0, std::ptr::null_mut(), 0) };
    if buf_size > 8 {
        let mut buffer = vec![0u8; buf_size as usize];
        let read_bytes = unsafe {
            GetSystemFirmwareTable(rsmb_sig, 0, buffer.as_mut_ptr(), buffer.len() as u32)
        };
        if read_bytes as usize == buffer.len() {
            let mut offset = 8usize;
            let size = buffer.len();

            while offset + 4 <= size {
                let smbios_type = buffer[offset];
                let formatted_len = buffer[offset + 1] as usize;
                if formatted_len < 4 || offset + formatted_len > size {
                    break;
                }

                if smbios_type == 17 && offset + 0x14 <= size {
                    let mem_size =
                        u16::from_le_bytes([buffer[offset + 0x0C], buffer[offset + 0x0D]]);
                    let loc_idx = buffer[offset + 0x10];
                    let bank_idx = buffer[offset + 0x11];
                    let mem_type_code = buffer[offset + 0x12];
                    let base_speed =
                        u16::from_le_bytes([buffer[offset + 0x14], buffer[offset + 0x15]]);

                    let configured_speed = if formatted_len >= 0x22 && offset + 0x22 <= size {
                        let cs = u16::from_le_bytes([buffer[offset + 0x20], buffer[offset + 0x21]]);
                        if cs > 0 && cs != 0xFFFF {
                            cs
                        } else {
                            base_speed
                        }
                    } else {
                        base_speed
                    };

                    let mfg_idx = if offset + 0x18 <= size {
                        buffer[offset + 0x17]
                    } else {
                        0
                    };
                    let part_idx = if offset + 0x1B <= size {
                        buffer[offset + 0x1A]
                    } else {
                        0
                    };

                    // Extract strings table following the formatted structure
                    let mut str_offset = offset + formatted_len;
                    let mut str_index = 1u8;
                    let mut slot_loc = String::new();
                    let mut bank_loc = String::new();
                    let mut mfg = String::new();
                    let mut part = String::new();

                    while str_offset < size && buffer[str_offset] != 0 {
                        let mut end = str_offset;
                        while end < size && buffer[end] != 0 {
                            end += 1;
                        }
                        if let Ok(s) = std::str::from_utf8(&buffer[str_offset..end]) {
                            let trimmed = s.trim();
                            if str_index == loc_idx {
                                slot_loc = trimmed.to_string();
                            }
                            if str_index == bank_idx {
                                bank_loc = trimmed.to_string();
                            }
                            if str_index == mfg_idx {
                                mfg = trimmed.to_string();
                            }
                            if str_index == part_idx {
                                part = trimmed.to_string();
                            }
                        }
                        str_index = str_index.saturating_add(1);
                        str_offset = end + 1;
                    }

                    if mem_size > 0 && mem_size != 0xFFFF {
                        let cap_bytes = if mem_size & 0x8000 != 0 {
                            (mem_size & 0x7FFF) as u64 * 1024
                        } else {
                            mem_size as u64 * 1024 * 1024
                        };

                        let mem_type_str = match mem_type_code {
                            0x12 => "DDR",
                            0x13 => "DDR2",
                            0x18 => "DDR3",
                            0x1A => "DDR4",
                            0x1B => "LPDDR4",
                            0x22 => "DDR5",
                            0x23 => "LPDDR5",
                            _ => "DRAM",
                        }
                        .to_string();

                        mem.modules.push(MemoryModuleDescriptor {
                            slot_locator: slot_loc,
                            bank_locator: bank_loc,
                            capacity_bytes: cap_bytes,
                            speed_mts: configured_speed as u32,
                            manufacturer: mfg,
                            part_number: part,
                            memory_type: mem_type_str,
                        });
                    }
                }

                // Advance past strings (double null \0\0)
                let mut next = offset + formatted_len;
                while next + 1 < size && !(buffer[next] == 0 && buffer[next + 1] == 0) {
                    next += 1;
                }
                offset = next + 2;
            }
        }
    }

    mem
}

#[cfg(not(windows))]
fn discover_memory() -> MemoryInventory {
    #[cfg(target_os = "linux")]
    let mut mem = MemoryInventory::default();
    #[cfg(not(target_os = "linux"))]
    let mem = MemoryInventory::default();
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kb = rest
                        .trim()
                        .strip_suffix("kB")
                        .unwrap_or(rest)
                        .trim()
                        .parse::<u64>()
                        .unwrap_or(0);
                    mem.total_physical_bytes = kb * 1024;
                    break;
                }
            }
        }
    }
    mem
}

#[cfg(windows)]
fn discover_storage() -> Vec<StorageInventoryDevice> {
    let mut drives = Vec::new();

    unsafe extern "system" {
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *mut std::ffi::c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn DeviceIoControl(
            hDevice: *mut std::ffi::c_void,
            dwIoControlCode: u32,
            lpInBuffer: *const std::ffi::c_void,
            nInBufferSize: u32,
            lpOutBuffer: *mut std::ffi::c_void,
            nOutBufferSize: u32,
            lpBytesReturned: *mut u32,
            lpOverlapped: *mut std::ffi::c_void,
        ) -> i32;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }

    for idx in 0..16 {
        let drive_path = format!("\\\\.\\PhysicalDrive{idx}");
        let wide_path: Vec<u16> = drive_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // Unelevated query access: dwDesiredAccess = 0, dwShareMode = FILE_SHARE_READ | FILE_SHARE_WRITE (3), OPEN_EXISTING = 3
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                0,
                3,
                std::ptr::null_mut(),
                3,
                0,
                std::ptr::null_mut(),
            )
        };

        if handle.is_null() || handle == -1isize as *mut std::ffi::c_void {
            continue;
        }

        // 1. StorageDeviceProperty (IOCTL 0x002D1400)
        #[repr(C)]
        struct STORAGE_PROPERTY_QUERY {
            property_id: u32,
            query_type: u32,
            additional_parameters: [u8; 1],
        }

        let query = STORAGE_PROPERTY_QUERY {
            property_id: 0, // StorageDeviceProperty
            query_type: 0,  // PropertyStandardQuery
            additional_parameters: [0],
        };

        let mut out_buf = [0u8; 1024];
        let mut returned = 0u32;
        let mut model_name = format!("PhysicalDrive{idx}");
        let mut bus_type_str = "Unknown".to_string();
        let mut fw_rev = None;

        if unsafe {
            DeviceIoControl(
                handle,
                0x002D1400, // IOCTL_STORAGE_QUERY_PROPERTY
                &query as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                out_buf.as_mut_ptr() as *mut std::ffi::c_void,
                out_buf.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        } != 0
        {
            let prod_offset =
                u32::from_le_bytes([out_buf[16], out_buf[17], out_buf[18], out_buf[19]]) as usize;
            let rev_offset =
                u32::from_le_bytes([out_buf[20], out_buf[21], out_buf[22], out_buf[23]]) as usize;
            let bus_type = out_buf[28];

            bus_type_str = match bus_type {
                3 => "ATAPI",
                7 => "USB",
                8 => "RAID",
                11 => "SATA",
                12 => "SAS",
                17 => "NVMe",
                _ => "Block",
            }
            .to_string();

            if prod_offset > 0 && prod_offset < out_buf.len() {
                let end = out_buf[prod_offset..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| prod_offset + p)
                    .unwrap_or(out_buf.len());
                if let Ok(s) = std::str::from_utf8(&out_buf[prod_offset..end]) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        model_name = trimmed.to_string();
                    }
                }
            }

            if rev_offset > 0 && rev_offset < out_buf.len() {
                let end = out_buf[rev_offset..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| rev_offset + p)
                    .unwrap_or(out_buf.len());
                if let Ok(s) = std::str::from_utf8(&out_buf[rev_offset..end]) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        fw_rev = Some(trimmed.to_string());
                    }
                }
            }
        }

        // 2. Capacity via IOCTL_DISK_GET_DRIVE_GEOMETRY_EX (0x000700A0)
        let mut geom_buf = [0u8; 256];
        let mut capacity_bytes = 0u64;
        if unsafe {
            DeviceIoControl(
                handle,
                0x000700A0,
                std::ptr::null(),
                0,
                geom_buf.as_mut_ptr() as *mut std::ffi::c_void,
                geom_buf.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        } != 0
        {
            // DiskSize is at offset 24 in DISK_GEOMETRY_EX
            capacity_bytes = u64::from_le_bytes([
                geom_buf[24],
                geom_buf[25],
                geom_buf[26],
                geom_buf[27],
                geom_buf[28],
                geom_buf[29],
                geom_buf[30],
                geom_buf[31],
            ]);
        }

        // 3. Media type (SSD vs HDD) via StorageDeviceSeekPenaltyProperty (PropertyId = 7)
        let seek_query = STORAGE_PROPERTY_QUERY {
            property_id: 7, // StorageDeviceSeekPenaltyProperty
            query_type: 0,
            additional_parameters: [0],
        };
        let mut seek_buf = [0u8; 16];
        let mut media_type = "Drive".to_string();
        if unsafe {
            DeviceIoControl(
                handle,
                0x002D1400,
                &seek_query as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                seek_buf.as_mut_ptr() as *mut std::ffi::c_void,
                seek_buf.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        } != 0
        {
            // IncursSeekPenalty is a BOOLEAN at offset 8 (after DWORD Version and DWORD Size)
            let incurs_seek_penalty = seek_buf[8] != 0;
            media_type = if incurs_seek_penalty {
                "HDD".to_string()
            } else {
                "SSD".to_string()
            };
        } else if bus_type_str == "NVMe" {
            media_type = "SSD".to_string();
        }

        unsafe { CloseHandle(handle) };

        drives.push(StorageInventoryDevice {
            device_id: format!("storage:drive{idx}"),
            model: model_name,
            serial_number_redacted: true,
            bus_type: bus_type_str,
            media_type,
            capacity_bytes,
            firmware_revision: fw_rev,
        });
    }

    drives
}

#[cfg(not(windows))]
fn discover_storage() -> Vec<StorageInventoryDevice> {
    #[cfg(target_os = "linux")]
    let mut drives = Vec::new();
    #[cfg(not(target_os = "linux"))]
    let drives = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/block") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("loop") || name.starts_with("ram") {
                    continue;
                }
                let model = std::fs::read_to_string(entry.path().join("device/model"))
                    .unwrap_or_else(|_| name.clone())
                    .trim()
                    .to_string();
                let size_sectors = std::fs::read_to_string(entry.path().join("size"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                let is_rotational = std::fs::read_to_string(entry.path().join("queue/rotational"))
                    .ok()
                    .map(|s| s.trim() == "1")
                    .unwrap_or(true);

                drives.push(StorageInventoryDevice {
                    device_id: format!("storage:{name}"),
                    model,
                    serial_number_redacted: true,
                    bus_type: if name.starts_with("nvme") {
                        "NVMe".into()
                    } else {
                        "SATA".into()
                    },
                    media_type: if is_rotational {
                        "HDD".into()
                    } else {
                        "SSD".into()
                    },
                    capacity_bytes: size_sectors * 512,
                    firmware_revision: None,
                });
            }
        }
    }
    drives
}

#[cfg(windows)]
fn discover_network() -> Vec<NetworkInventoryDevice> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetIfTable2, MIB_IF_TABLE2,
    };

    let mut ifaces = Vec::new();
    unsafe {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table_ptr) == 0 && !table_ptr.is_null() {
            let num_entries = (*table_ptr).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*table_ptr).Table.as_ptr(), num_entries);
            for row in rows {
                let alias_len = row
                    .Alias
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(row.Alias.len());
                let desc_len = row
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(row.Description.len());
                let alias = String::from_utf16_lossy(&row.Alias[..alias_len])
                    .trim()
                    .to_string();
                let desc = String::from_utf16_lossy(&row.Description[..desc_len])
                    .trim()
                    .to_string();

                if alias.is_empty() && desc.is_empty() {
                    continue;
                }

                // Filter out intermediate NDIS lightweight filters
                if alias.contains("Filter") || alias.contains("Packet Scheduler") {
                    continue;
                }

                // 6 = Ethernet (IF_TYPE_ETHERNET_CSMACD), 71 = Wi-Fi (IF_TYPE_IEEE80211)
                let is_hardware = row.Type == 6 || row.Type == 71;
                let speed = row.ReceiveLinkSpeed.max(row.TransmitLinkSpeed);

                let dev_id = format!("net:{}", row.InterfaceIndex);
                ifaces.push(NetworkInventoryDevice {
                    device_id: dev_id,
                    name: if !alias.is_empty() {
                        alias
                    } else {
                        desc.clone()
                    },
                    description: desc,
                    is_hardware,
                    link_speed_bps: (speed > 0 && speed < u64::MAX).then_some(speed),
                    mac_address_redacted: true,
                });
            }
            FreeMibTable(table_ptr as *const std::ffi::c_void);
        }
    }

    ifaces
}

#[cfg(not(windows))]
fn discover_network() -> Vec<NetworkInventoryDevice> {
    #[cfg(target_os = "linux")]
    let mut ifaces = Vec::new();
    #[cfg(not(target_os = "linux"))]
    let ifaces = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_hardware = !name.starts_with("lo") && !name.starts_with("docker");
                let speed_mbps = std::fs::read_to_string(entry.path().join("speed"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok());
                ifaces.push(NetworkInventoryDevice {
                    device_id: format!("net:{name}"),
                    name: name.clone(),
                    description: format!("Network interface {name}"),
                    is_hardware,
                    link_speed_bps: speed_mbps.map(|mbps| mbps * 1_000_000),
                    mac_address_redacted: true,
                });
            }
        }
    }
    ifaces
}

fn discover_cooling() -> Vec<CoolingInventoryDevice> {
    #[allow(unused_mut)]
    let mut devices = Vec::new();

    #[cfg(target_os = "linux")]
    {
        // 1. Enumerate hwmon fan and pwm channels
        if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let path = entry.path();
                let hwmon_id = entry.file_name().to_string_lossy().into_owned();
                let name = std::fs::read_to_string(path.join("name"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| hwmon_id.clone());

                let mut channel_count = 0u32;
                if let Ok(files) = std::fs::read_dir(&path) {
                    for f in files.flatten() {
                        let fname = f.file_name().to_string_lossy().into_owned();
                        if (fname.starts_with("fan") && fname.ends_with("_input"))
                            || (fname.starts_with("pwm")
                                && fname.chars().skip(3).all(|c| c.is_ascii_digit()))
                        {
                            channel_count += 1;
                        }
                    }
                }

                if channel_count > 0 {
                    devices.push(CoolingInventoryDevice {
                        device_id: format!("cooling:hwmon:{}", hwmon_id),
                        name: format!("{} Hardware Monitor", name),
                        kind: "Fan/PWM Controller".into(),
                        channel_count,
                    });
                }
            }
        }

        // 2. Enumerate thermal cooling devices
        if let Ok(entries) = std::fs::read_dir("/sys/class/thermal") {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().into_owned();
                if fname.starts_with("cooling_device") {
                    let dev_type = std::fs::read_to_string(entry.path().join("type"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| "Cooling Device".into());
                    devices.push(CoolingInventoryDevice {
                        device_id: format!("cooling:thermal:{}", fname),
                        name: format!("Thermal Cooling ({dev_type})"),
                        kind: "Thermal Governor".into(),
                        channel_count: 1,
                    });
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // On Windows and macOS without elevated drivers, non-privileged ACPI cooling controllers
        // are not exposed via standard user-mode APIs. Privileged fan RPMs are dynamically
        // populated via the sensor broker / SMC.
    }

    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_sub() {
        let topo = crate::discover();
        let cpu = discover_cpu(&topo);
        println!("CPU: {:?}", cpu.brand);
    }

    #[test]
    fn test_motherboard_sub() {
        let mobo = discover_motherboard();
        println!("Mobo: {:?}", mobo.product_name);
    }

    #[test]
    fn test_memory_sub() {
        let mem = discover_memory();
        println!("Mem: {}", mem.total_physical_bytes);
    }

    #[test]
    fn test_storage_sub() {
        let st = discover_storage();
        println!("Storage: {}", st.len());
    }

    #[test]
    fn test_network_sub() {
        let net = discover_network();
        println!("Net: {}", net.len());
    }

    #[test]
    fn test_gpus_sub() {
        let gpus = discover_gpus();
        println!("GPUs: {}", gpus.len());
    }
}
