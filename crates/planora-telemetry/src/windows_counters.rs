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

#[repr(C)]
struct MibIfRow2 {
    interface_luid: u64,
    interface_index: u32,
    interface_guid: [u8; 16],
    alias: [u16; 257],
    description: [u16; 257],
    physical_address_length: u32,
    physical_address: [u8; 32],
    flags: u32,
    interface_type: u32,
    tunnel_type: u32,
    media_type: u32,
    physical_medium_type: u32,
    access_type: u32,
    direction_type: u32,
    interface_and_oper_status_flags: u32,
    oper_status: u32,
    admin_status: u32,
    media_connect_state: u32,
    network_guid: [u8; 16],
    connection_type: u32,
    transmit_link_speed: u64,
    receive_link_speed: u64,
    in_octets: u64,
    in_ucast_pkts: u64,
    in_nucast_pkts: u64,
    in_discards: u64,
    in_errors: u64,
    in_unknown_protos: u64,
    in_ucast_octets: u64,
    in_multicast_octets: u64,
    in_broadcast_octets: u64,
    out_octets: u64,
    out_ucast_pkts: u64,
    out_nucast_pkts: u64,
    out_discards: u64,
    out_errors: u64,
    out_ucast_octets: u64,
    out_multicast_octets: u64,
    out_broadcast_octets: u64,
    out_ql_len: u64,
}

#[repr(C)]
struct MibIfTable2 {
    num_entries: u32,
    // followed by table entries
}

pub fn query_network_links() -> HashMap<String, NetworkLinkInfo> {
    let mut links = HashMap::new();
    unsafe {
        let iphlp = windows_sys::Win32::System::LibraryLoader::GetModuleHandleA(
            c"iphlpapi.dll".as_ptr().cast(),
        );
        let hmod = if iphlp.is_null() {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryA(c"iphlpapi.dll".as_ptr().cast())
        } else {
            iphlp
        };
        if hmod.is_null() {
            return links;
        }
        let get_if_table2 = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            hmod,
            c"GetIfTable2".as_ptr().cast(),
        );
        let free_if_table2 = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            hmod,
            c"FreeMibTable".as_ptr().cast(),
        );
        if let (Some(get_fn), Some(free_fn)) = (get_if_table2, free_if_table2) {
            type GetTableFn = unsafe extern "system" fn(*mut *mut MibIfTable2) -> u32;
            type FreeTableFn = unsafe extern "system" fn(*mut std::ffi::c_void);
            let get: GetTableFn = std::mem::transmute(get_fn);
            let free: FreeTableFn = std::mem::transmute(free_fn);

            let mut table_ptr: *mut MibIfTable2 = std::ptr::null_mut();
            if get(&mut table_ptr) == 0 && !table_ptr.is_null() {
                let num_entries = (*table_ptr).num_entries as usize;
                let rows_ptr = (table_ptr as *const u8).add(8) as *const MibIfRow2;
                let rows = std::slice::from_raw_parts(rows_ptr, num_entries);
                for row in rows {
                    let alias_len = row
                        .alias
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(row.alias.len());
                    let desc_len = row
                        .description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(row.description.len());
                    let alias = String::from_utf16_lossy(&row.alias[..alias_len]);
                    let desc = String::from_utf16_lossy(&row.description[..desc_len]);

                    let state_str = match row.media_connect_state {
                        1 => "Connected",
                        2 => "Disconnected",
                        _ => "Unknown",
                    }
                    .to_string();

                    let speed = row.receive_link_speed.max(row.transmit_link_speed);
                    let info = NetworkLinkInfo {
                        speed_bps: if speed > 0 { Some(speed) } else { None },
                        link_state: Some(state_str),
                    };
                    if !alias.is_empty() {
                        links.insert(alias.clone(), info.clone());
                    }
                    if !desc.is_empty() {
                        links.insert(desc, info);
                    }
                }
                free(table_ptr.cast());
            }
        }
    }
    links
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
        }
    }
}
