//! Windows platform-specific telemetry queries for memory, disk latency/queue, and network interfaces.
use std::collections::HashMap;

#[repr(C)]
struct MemoryStatusEx {
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
    fn GlobalMemoryStatusEx(lpBuffer: *mut MemoryStatusEx) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsMemoryStatus {
    pub physical_total: u64,
    pub physical_available: u64,
    pub physical_used: u64,
    pub commit_total: u64,
    pub commit_used: u64,
}

pub fn get_memory_status() -> Option<WindowsMemoryStatus> {
    let mut status = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        dw_memory_load: 0,
        ull_total_phys: 0,
        ull_avail_phys: 0,
        ull_total_page_file: 0,
        ull_avail_page_file: 0,
        ull_total_virtual: 0,
        ull_avail_virtual: 0,
        ull_avail_extended_virtual: 0,
    };
    if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
        Some(WindowsMemoryStatus {
            physical_total: status.ull_total_phys,
            physical_available: status.ull_avail_phys,
            physical_used: status.ull_total_phys.saturating_sub(status.ull_avail_phys),
            commit_total: status.ull_total_page_file,
            commit_used: status
                .ull_total_page_file
                .saturating_sub(status.ull_avail_page_file),
        })
    } else {
        None
    }
}

#[derive(Clone, Debug, Default)]
pub struct NetworkLinkInfo {
    pub speed_bps: Option<u64>,
    pub link_state: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WindowsNetIfSnapshot {
    pub interface_luid: u64,
    pub alias: String,
    pub description: String,
    pub if_type: u32,
    pub is_hardware: bool,
    pub in_octets: u64,
    pub out_octets: u64,
    pub oper_status: u32,
    pub speed_bps: Option<u64>,
    pub link_state: Option<String>,
}

use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

pub fn query_network_links() -> HashMap<String, NetworkLinkInfo> {
    let mut links = HashMap::new();
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
                let alias = String::from_utf16_lossy(&row.Alias[..alias_len]);
                let desc = String::from_utf16_lossy(&row.Description[..desc_len]);

                let state_str = match row.MediaConnectState {
                    1 => "Connected",
                    2 => "Disconnected",
                    _ => "Unknown",
                }
                .to_string();

                let speed = row.ReceiveLinkSpeed.max(row.TransmitLinkSpeed);
                let info = NetworkLinkInfo {
                    speed_bps: if speed > 0 && speed < u64::MAX && speed <= 10_000_000_000_000 {
                        Some(speed)
                    } else {
                        None
                    },
                    link_state: Some(state_str),
                };
                if !alias.is_empty() {
                    links.insert(alias.clone(), info.clone());
                }
                if !desc.is_empty() {
                    links.insert(desc, info);
                }
            }
            FreeMibTable(table_ptr.cast());
        }
    }
    links
}

pub fn query_windows_net_interfaces() -> Vec<WindowsNetIfSnapshot> {
    let mut snapshots = Vec::new();
    unsafe {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table_ptr) == 0 && !table_ptr.is_null() {
            let num_entries = (*table_ptr).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*table_ptr).Table.as_ptr(), num_entries);
            for row in rows {
                // Skip filter interfaces (FilterInterface is bit 1: 0x02)
                let bitfield = row.InterfaceAndOperStatusFlags._bitfield;
                if (bitfield & 0x02) != 0 {
                    continue;
                }
                // Skip software loopback (IF_TYPE_SOFTWARE_LOOPBACK = 24)
                if row.Type == 24 {
                    continue;
                }
                // Skip tunnels (IF_TYPE_TUNNEL = 131)
                if row.Type == 131 {
                    continue;
                }

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
                let alias = String::from_utf16_lossy(&row.Alias[..alias_len]);
                let description = String::from_utf16_lossy(&row.Description[..desc_len]);

                let state_str = match row.MediaConnectState {
                    1 => "Connected",
                    2 => "Disconnected",
                    _ => "Unknown",
                }
                .to_string();

                let speed = row.ReceiveLinkSpeed.max(row.TransmitLinkSpeed);
                let speed_bps = if speed > 0 && speed < u64::MAX && speed <= 10_000_000_000_000 {
                    Some(speed)
                } else {
                    None
                };

                let is_hardware = (bitfield & 0x01) != 0;
                let luid = row.InterfaceLuid.Value;

                snapshots.push(WindowsNetIfSnapshot {
                    interface_luid: luid,
                    alias,
                    description,
                    if_type: row.Type,
                    is_hardware,
                    in_octets: row.InOctets,
                    out_octets: row.OutOctets,
                    oper_status: row.OperStatus as u32,
                    speed_bps,
                    link_state: Some(state_str),
                });
            }
            FreeMibTable(table_ptr.cast());
        }
    }
    snapshots
}

pub fn windows_processor_group_map() -> Vec<(u16, u16)> {
    let mut map = Vec::new();
    unsafe {
        let kernel32 = windows_sys::Win32::System::LibraryLoader::GetModuleHandleA(
            c"kernel32.dll".as_ptr().cast(),
        );
        if !kernel32.is_null() {
            let get_group_count_ptr = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                kernel32,
                c"GetActiveProcessorGroupCount".as_ptr().cast(),
            );
            let get_proc_count_ptr = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                kernel32,
                c"GetActiveProcessorCount".as_ptr().cast(),
            );
            if let (Some(get_group_count), Some(get_proc_count)) =
                (get_group_count_ptr, get_proc_count_ptr)
            {
                type GetGroupCountFn = unsafe extern "system" fn() -> u16;
                type GetProcCountFn = unsafe extern "system" fn(u16) -> u32;
                let get_groups: GetGroupCountFn = std::mem::transmute(get_group_count);
                let get_procs: GetProcCountFn = std::mem::transmute(get_proc_count);
                let num_groups = get_groups();
                for g in 0..num_groups {
                    let procs_in_group = get_procs(g);
                    for p in 0..procs_in_group {
                        map.push((g, p as u16));
                    }
                }
            }
        }
    }
    map
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PROCESSOR_POWER_INFORMATION {
    pub number: u32,
    pub max_mhz: u32,
    pub current_mhz: u32,
    pub mhz_limit: u32,
    pub max_idle_state: u32,
    pub current_idle_state: u32,
}

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

pub fn query_processor_power_information(cpu_count: usize) -> Vec<PROCESSOR_POWER_INFORMATION> {
    let sys_procs = unsafe {
        let mut sys_info: windows_sys::Win32::System::SystemInformation::SYSTEM_INFO =
            std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut sys_info);
        sys_info.dwNumberOfProcessors as usize
    };
    let count = cpu_count.max(sys_procs);
    if count == 0 {
        return Vec::new();
    }
    let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); count];
    let buf_size = (info.len() * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32;

    unsafe {
        let powrprof = windows_sys::Win32::System::LibraryLoader::LoadLibraryA(
            c"powrprof.dll".as_ptr().cast(),
        );
        if !powrprof.is_null() {
            let proc_addr = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                powrprof,
                c"CallNtPowerInformation".as_ptr().cast(),
            );
            if let Some(call_nt_power) = proc_addr {
                type CallNtPowerInfoFn = unsafe extern "system" fn(
                    i32,
                    *const std::ffi::c_void,
                    u32,
                    *mut std::ffi::c_void,
                    u32,
                ) -> i32;
                let func: CallNtPowerInfoFn = std::mem::transmute(call_nt_power);
                // ProcessorInformation = 11
                let status = func(
                    11,
                    std::ptr::null(),
                    0,
                    info.as_mut_ptr() as *mut std::ffi::c_void,
                    buf_size,
                );
                FreeLibrary(powrprof);
                if status == 0 {
                    return info;
                }
            } else {
                FreeLibrary(powrprof);
            }
        }
    }
    Vec::new()
}

#[derive(Clone, Debug)]
pub struct ThermalZoneSample {
    pub name: String,
    pub temperature_c: f32,
}

pub fn query_thermal_zones() -> Vec<ThermalZoneSample> {
    use std::ptr;
    use windows_sys::Win32::System::Performance::{
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW,
    };

    let mut zones = Vec::new();
    let mut query: PDH_HQUERY = ptr::null_mut();
    if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) } != 0 {
        return zones;
    }

    let mut counter: PDH_HCOUNTER = ptr::null_mut();
    let path = windows_sys::core::w!(r"\Thermal Zone Information(*)\Temperature");
    let status = unsafe { PdhAddEnglishCounterW(query, path, 0, &mut counter) };
    if status == 0 && unsafe { PdhCollectQueryData(query) } == 0 {
        let mut buffer_size = 0u32;
        let mut item_count = 0u32;
        let ret = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                ptr::null_mut(),
            )
        };
        if ret == PDH_MORE_DATA && buffer_size > 0 {
            let elem_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let count_elements = (buffer_size as usize).div_ceil(elem_size);
            let mut items: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                vec![unsafe { std::mem::zeroed() }; count_elements];
            if unsafe {
                PdhGetFormattedCounterArrayW(
                    counter,
                    PDH_FMT_DOUBLE,
                    &mut buffer_size,
                    &mut item_count,
                    items.as_mut_ptr(),
                )
            } == 0
            {
                for item in items.iter().take(item_count as usize) {
                    if item.FmtValue.CStatus == 0 {
                        let kelvin = unsafe { item.FmtValue.Anonymous.doubleValue };
                        let celsius = kelvin - 273.15;
                        if (0.0..=125.0).contains(&celsius) {
                            let name = if item.szName.is_null() {
                                "ACPI Thermal Zone".to_string()
                            } else {
                                let len = (0..)
                                    .take_while(|&i| unsafe { *item.szName.add(i) != 0 })
                                    .count();
                                let slice = unsafe { std::slice::from_raw_parts(item.szName, len) };
                                String::from_utf16_lossy(slice)
                            };
                            zones.push(ThermalZoneSample {
                                name,
                                temperature_c: celsius as f32,
                            });
                        }
                    }
                }
            }
        }
    }
    unsafe { PdhCloseQuery(query) };
    zones
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_memory_status_returns_positive_physical_and_commit() {
        let status = get_memory_status().expect("GlobalMemoryStatusEx must succeed on Windows");
        assert!(status.physical_total > 0);
        assert!(status.physical_available > 0);
        assert!(status.physical_used <= status.physical_total);
        assert!(status.commit_total >= status.physical_total);
        assert!(status.commit_used <= status.commit_total);
    }

    #[test]
    fn network_links_query_succeeds_without_panics() {
        let links = query_network_links();
        // Link table may or may not be non-empty depending on network adapters, but must never panic
        for (name, info) in links {
            assert!(!name.is_empty());
            if let Some(state) = info.link_state {
                assert!(["Connected", "Disconnected", "Unknown"].contains(&state.as_str()));
            }
            if let Some(spd) = info.speed_bps {
                assert!(spd > 0 && spd < u64::MAX);
            }
        }

        let ifaces = query_windows_net_interfaces();
        for iface in ifaces {
            assert!(iface.interface_luid > 0);
            if let Some(spd) = iface.speed_bps {
                assert!(spd > 0 && spd < u64::MAX);
            }
        }
    }

    #[test]
    fn processor_group_map_returns_valid_entries() {
        let map = windows_processor_group_map();
        // On any modern Windows machine, at least 1 logical processor exists
        assert!(!map.is_empty());
        let mut prev: Option<(u16, u16)> = None;
        for &(group, cpu) in &map {
            assert!(cpu < 64, "group-local CPU index must be < 64, got {cpu}");
            if let Some(p) = prev {
                assert!(
                    (group, cpu) > p,
                    "processor group map must be strictly increasing: {p:?} -> ({group}, {cpu})"
                );
            }
            prev = Some((group, cpu));
        }
    }
}
