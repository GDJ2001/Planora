//! Windows GPU and system counter telemetry via DXGI and PDH.
use planora_core::{AvailabilityReason, GpuAdapterSample, GpuEngineSample, Metric};
use std::{
    collections::HashMap,
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};

unsafe extern "system" {
    fn FreeLibrary(hLibModule: *mut std::ffi::c_void) -> i32;
}

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

// Values from the Windows SDK DXGI headers. Keep the interface identities in
// one place; an incorrect IID makes CreateDXGIFactory1 return E_NOINTERFACE.
const IID_IDXGI_FACTORY1: Guid = Guid {
    data1: 0x770aae78,
    data2: 0xf26f,
    data3: 0x4dba,
    data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
};
const IID_IDXGI_ADAPTER3: Guid = Guid {
    data1: 0x645967a4,
    data2: 0x1392,
    data3: 0x4310,
    data4: [0xa7, 0x98, 0x80, 0x53, 0xce, 0x3e, 0x93, 0xfd],
};
const DXGI_ADAPTER_FLAG_SOFTWARE: u32 = 0x2;

#[repr(C)]
struct Luid {
    low_part: u32,
    high_part: i32,
}

#[repr(C)]
struct DxgiAdapterDesc1 {
    description: [u16; 128],
    vendor_id: u32,
    device_id: u32,
    sub_sys_id: u32,
    revision: u32,
    dedicated_video_memory: usize,
    dedicated_system_memory: usize,
    shared_system_memory: usize,
    adapter_luid: Luid,
    flags: u32,
}

#[repr(C)]
struct DxgiQueryVideoMemoryInfo {
    budget: u64,
    current_usage: u64,
    available_for_reservation: u64,
    current_reservation: u64,
}

#[derive(Clone, Debug)]
pub struct DxgiAdapterInfo {
    pub luid: u64,
    pub vendor_id: u32,
    pub name: String,
    pub dedicated_video_bytes: u64,
    pub _dedicated_system_bytes: u64,
    pub shared_system_bytes: u64,
    pub budget_bytes: Option<u64>,
    pub current_usage_bytes: Option<u64>,
}

pub fn query_dxgi_adapters() -> Vec<DxgiAdapterInfo> {
    let mut results = Vec::new();
    unsafe {
        let dxgi = windows_sys::Win32::System::LibraryLoader::LoadLibraryExA(
            c"dxgi.dll".as_ptr().cast(),
            ptr::null_mut(),
            windows_sys::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if dxgi.is_null() {
            return results;
        }
        let create_factory_ptr = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            dxgi,
            c"CreateDXGIFactory1".as_ptr().cast(),
        );
        let Some(create_factory) = create_factory_ptr else {
            FreeLibrary(dxgi);
            return results;
        };
        type CreateFactoryFn =
            unsafe extern "system" fn(*const Guid, *mut *mut std::ffi::c_void) -> i32;
        let create_factory: CreateFactoryFn = std::mem::transmute(create_factory);

        let mut factory: *mut std::ffi::c_void = ptr::null_mut();
        if create_factory(&IID_IDXGI_FACTORY1, &mut factory) != 0 || factory.is_null() {
            FreeLibrary(dxgi);
            return results;
        }

        let factory_vtbl = *(factory as *mut *mut usize);
        type EnumAdapters1Fn = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            u32,
            *mut *mut std::ffi::c_void,
        ) -> i32;
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let enum_adapters1: EnumAdapters1Fn = std::mem::transmute(*factory_vtbl.add(12));
        let release_factory: ReleaseFn = std::mem::transmute(*factory_vtbl.add(2));

        let mut idx = 0u32;
        loop {
            let mut adapter: *mut std::ffi::c_void = ptr::null_mut();
            if enum_adapters1(factory, idx, &mut adapter) != 0 || adapter.is_null() {
                break;
            }
            idx += 1;

            let adapter_vtbl = *(adapter as *mut *mut usize);
            type GetDesc1Fn =
                unsafe extern "system" fn(*mut std::ffi::c_void, *mut DxgiAdapterDesc1) -> i32;
            type QueryInterfaceFn = unsafe extern "system" fn(
                *mut std::ffi::c_void,
                *const Guid,
                *mut *mut std::ffi::c_void,
            ) -> i32;
            let get_desc1: GetDesc1Fn = std::mem::transmute(*adapter_vtbl.add(10));
            let query_interface: QueryInterfaceFn = std::mem::transmute(*adapter_vtbl.add(0));
            let release_adapter: ReleaseFn = std::mem::transmute(*adapter_vtbl.add(2));

            let mut desc = std::mem::zeroed::<DxgiAdapterDesc1>();
            if get_desc1(adapter, &mut desc) == 0 && desc.flags & DXGI_ADAPTER_FLAG_SOFTWARE == 0 {
                let name_len = desc
                    .description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.description.len());
                let name = String::from_utf16_lossy(&desc.description[..name_len]);
                let luid = ((desc.adapter_luid.high_part as u64) << 32)
                    | (desc.adapter_luid.low_part as u64);

                let mut budget_bytes = None;
                let mut current_usage_bytes = None;

                let mut adapter3: *mut std::ffi::c_void = ptr::null_mut();
                if query_interface(adapter, &IID_IDXGI_ADAPTER3, &mut adapter3) == 0
                    && !adapter3.is_null()
                {
                    let adapter3_vtbl = *(adapter3 as *mut *mut usize);
                    type QueryVideoMemoryInfoFn = unsafe extern "system" fn(
                        *mut std::ffi::c_void,
                        u32,
                        u32,
                        *mut DxgiQueryVideoMemoryInfo,
                    )
                        -> i32;
                    let query_mem: QueryVideoMemoryInfoFn =
                        std::mem::transmute(*adapter3_vtbl.add(14));
                    let release_adapter3: ReleaseFn = std::mem::transmute(*adapter3_vtbl.add(2));

                    let mut mem_info = std::mem::zeroed::<DxgiQueryVideoMemoryInfo>();
                    if query_mem(adapter3, 0, 0, &mut mem_info) == 0 {
                        budget_bytes = Some(mem_info.budget);
                        current_usage_bytes = Some(mem_info.current_usage);
                    }
                    release_adapter3(adapter3);
                }

                results.push(DxgiAdapterInfo {
                    luid,
                    vendor_id: desc.vendor_id,
                    name,
                    dedicated_video_bytes: desc.dedicated_video_memory as u64,
                    _dedicated_system_bytes: desc.dedicated_system_memory as u64,
                    shared_system_bytes: desc.shared_system_memory as u64,
                    budget_bytes,
                    current_usage_bytes,
                });
            }
            release_adapter(adapter);
        }
        release_factory(factory);
        FreeLibrary(dxgi);
    }
    results
}

pub fn has_pci_vendor_gpu(target_vendor_id: u32) -> bool {
    query_dxgi_adapters()
        .iter()
        .any(|adapter| adapter.vendor_id == target_vendor_id)
}

#[derive(Clone, Debug, Default)]
pub struct PhysicalDiskPerf {
    pub instance_name: String,
    pub disk_index: Option<u32>,
    pub drive_letters: Vec<String>,
    pub read_bytes_per_sec: u64,
    pub write_bytes_per_sec: u64,
    pub latency_ms: Option<f32>,
    pub queue_depth: Option<f32>,
}

pub struct WindowsGpu {
    query: usize,
    engine_counter: Option<usize>,
    dedicated_mem_counter: Option<usize>,
    shared_mem_counter: Option<usize>,
    disk_read_counter: Option<usize>,
    disk_write_counter: Option<usize>,
    disk_latency_counter: Option<usize>,
    disk_queue_counter: Option<usize>,
    sample_count: u64,
    last: Option<Instant>,
    cached_stamp_ms: Option<u64>,
    cached_engines: Vec<GpuEngineSample>,
    cached_adapters: Vec<GpuAdapterSample>,
    cached_disk_metrics: HashMap<String, (Option<f32>, Option<f32>)>,
    cached_physical_disks: Vec<PhysicalDiskPerf>,
    cached_total_disk_io: (u64, u64),
    dxgi_adapters: Vec<DxgiAdapterInfo>,
    last_dxgi_refresh: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceIdentity {
    pid: u32,
    luid: u64,
    physical: u32,
    engine: u32,
    engine_type: String,
}

pub fn parse_instance(name: &str) -> Option<InstanceIdentity> {
    let (prefix, raw_engine_type) = name.split_once("_engtype_")?;
    let engine_type = match raw_engine_type.rsplit_once('#') {
        Some((kind, duplicate))
            if !duplicate.is_empty() && duplicate.bytes().all(|b| b.is_ascii_digit()) =>
        {
            kind
        }
        _ => raw_engine_type,
    };
    if engine_type.is_empty() || engine_type.len() > 64 {
        return None;
    }
    let (prefix, engine) = prefix.rsplit_once("_eng_")?;
    let (prefix, physical) = prefix.rsplit_once("_phys_")?;
    let (pid, luid) = prefix.split_once("_luid_")?;
    let pid = pid.strip_prefix("pid_")?.parse().ok()?;
    let (high, low) = luid.split_once('_')?;
    let high_hex = high
        .strip_prefix("0x")
        .or_else(|| high.strip_prefix("0X"))?;
    let low_hex = low.strip_prefix("0x").or_else(|| low.strip_prefix("0X"))?;
    let high = u32::from_str_radix(high_hex, 16).ok()?;
    let low = u32::from_str_radix(low_hex, 16).ok()?;
    Some(InstanceIdentity {
        pid,
        luid: ((high as u64) << 32) | low as u64,
        physical: physical.parse().ok()?,
        engine: engine.parse().ok()?,
        engine_type: engine_type.to_owned(),
    })
}

pub fn parse_adapter_memory_instance(name: &str) -> Option<(u64, u32)> {
    let (prefix, phys) = name.rsplit_once("_phys_")?;
    let luid_str = prefix.strip_prefix("luid_")?;
    let (high, low) = luid_str.split_once('_')?;
    let high_hex = high
        .strip_prefix("0x")
        .or_else(|| high.strip_prefix("0X"))?;
    let low_hex = low.strip_prefix("0x").or_else(|| low.strip_prefix("0X"))?;
    let high = u32::from_str_radix(high_hex, 16).ok()?;
    let low = u32::from_str_radix(low_hex, 16).ok()?;
    let physical: u32 = phys.parse().ok()?;
    Some((((high as u64) << 32) | (low as u64), physical))
}

/// PDH GPU Engine instances describe process contributions. Combine only
/// distinct process instances for the same adapter, physical device and engine.
/// Duplicate PDH instance names are not another contribution.
fn busiest_physical_engine(engines: &[GpuEngineSample], luid: u64) -> Option<Metric<f32>> {
    let mut contributions: HashMap<(u32, u32, String, u32), (f32, u64)> = HashMap::new();
    for sample in engines
        .iter()
        .filter(|sample| sample.adapter_luid == Some(luid))
    {
        let (Some(physical), Some(engine), Some(pid), Some(value)) = (
            sample.physical_adapter,
            sample.engine_id,
            sample.process_pid,
            sample.utilization_percent.value,
        ) else {
            continue;
        };
        let key = (physical, engine, sample.engine_type.clone(), pid);
        let candidate = (value, sample.utilization_percent.sampled_at_ms);
        contributions
            .entry(key)
            .and_modify(|current| {
                if candidate.0 > current.0 {
                    *current = candidate;
                }
            })
            .or_insert(candidate);
    }
    let mut totals: HashMap<(u32, u32, String), (f32, u64)> = HashMap::new();
    for ((physical, engine, kind, _pid), (value, sampled_at)) in contributions {
        let total = totals
            .entry((physical, engine, kind))
            .or_insert((0.0, sampled_at));
        total.0 += value;
        total.1 = total.1.max(sampled_at);
    }
    totals
        .into_values()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(value, sampled_at)| {
            Metric::available(
                value.min(100.0),
                "%",
                "Windows PDH physical GPU engine",
                sampled_at,
            )
        })
}

fn add_counter(query: PDH_HQUERY, path_str: &str) -> Option<PDH_HCOUNTER> {
    let mut counter: PDH_HCOUNTER = ptr::null_mut();
    let path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
    let status = unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
    if status == 0 { Some(counter) } else { None }
}

fn query_counter_items(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    let mut size = 0u32;
    let mut count = 0u32;
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            ptr::null_mut(),
        )
    };
    if status != PDH_MORE_DATA || size == 0 || size > 8 * 1024 * 1024 || count > 4096 {
        return Vec::new();
    }
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    let items = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
            count as usize,
        )
    };
    let mut out = Vec::with_capacity(count as usize);
    for item in items {
        if item.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA
            && item.FmtValue.CStatus != PDH_CSTATUS_NEW_DATA
        {
            continue;
        }
        let val = unsafe { item.FmtValue.Anonymous.doubleValue };
        if !val.is_finite() || item.szName.is_null() {
            continue;
        }
        let mut len = 0usize;
        while len < 1024 && unsafe { *item.szName.add(len) } != 0 {
            len += 1;
        }
        if len == 1024 {
            continue;
        }
        let name =
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(item.szName, len) });
        out.push((name, val));
    }
    out
}

impl WindowsGpu {
    pub fn new() -> Result<Self, String> {
        let mut query: PDH_HQUERY = ptr::null_mut();
        let status = unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) };
        if status != 0 {
            return Err(format!("PdhOpenQueryW: 0x{status:08x}"));
        }

        let engine_counter =
            add_counter(query, "\\GPU Engine(*)\\Utilization Percentage").map(|c| c as usize);
        let dedicated_mem =
            add_counter(query, "\\GPU Adapter Memory(*)\\Dedicated Usage").map(|c| c as usize);
        let shared_mem =
            add_counter(query, "\\GPU Adapter Memory(*)\\Shared Usage").map(|c| c as usize);
        let disk_read =
            add_counter(query, "\\PhysicalDisk(*)\\Disk Read Bytes/sec").map(|c| c as usize);
        let disk_write =
            add_counter(query, "\\PhysicalDisk(*)\\Disk Write Bytes/sec").map(|c| c as usize);
        let disk_latency =
            add_counter(query, "\\PhysicalDisk(*)\\Avg. Disk sec/Transfer").map(|c| c as usize);
        let disk_queue =
            add_counter(query, "\\PhysicalDisk(*)\\Current Disk Queue Length").map(|c| c as usize);

        if engine_counter.is_none()
            && dedicated_mem.is_none()
            && shared_mem.is_none()
            && disk_read.is_none()
            && disk_write.is_none()
            && disk_latency.is_none()
            && disk_queue.is_none()
        {
            unsafe { PdhCloseQuery(query) };
            return Err("No Windows PDH GPU or PhysicalDisk counters available".into());
        }

        let _ = unsafe { PdhCollectQueryData(query) };
        let baseline_instant = Instant::now();

        let dxgi_adapters = query_dxgi_adapters();

        Ok(Self {
            query: query as usize,
            engine_counter,
            dedicated_mem_counter: dedicated_mem,
            shared_mem_counter: shared_mem,
            disk_read_counter: disk_read,
            disk_write_counter: disk_write,
            disk_latency_counter: disk_latency,
            disk_queue_counter: disk_queue,
            sample_count: 1,
            last: Some(baseline_instant),
            cached_stamp_ms: None,
            cached_engines: Vec::new(),
            cached_adapters: Vec::new(),
            cached_disk_metrics: HashMap::new(),
            cached_physical_disks: Vec::new(),
            cached_total_disk_io: (0, 0),
            dxgi_adapters,
            last_dxgi_refresh: baseline_instant,
        })
    }

    pub fn sample(
        &mut self,
        stamp: u64,
    ) -> Result<(Vec<GpuAdapterSample>, Vec<GpuEngineSample>), String> {
        let within_min_interval = self
            .last
            .is_some_and(|last| last.elapsed() < Duration::from_millis(50));
        if within_min_interval
            && (self.sample_count >= 2
                || !self.cached_adapters.is_empty()
                || !self.cached_physical_disks.is_empty())
        {
            return Ok((self.cached_adapters.clone(), self.cached_engines.clone()));
        }

        let is_primed = if within_min_interval {
            false
        } else {
            let status = unsafe { PdhCollectQueryData(self.query as PDH_HQUERY) };
            if status != 0 {
                return Err(format!("PdhCollectQueryData: 0x{status:08x}"));
            }
            self.sample_count += 1;
            self.last = Some(Instant::now());
            self.sample_count >= 2
        };
        self.cached_stamp_ms = Some(stamp);

        // DXGI adapter identities can change after dock/undock, sleep or driver
        // restart. Re-enumeration is intentionally slower than PDH polling.
        if self.last_dxgi_refresh.elapsed() >= Duration::from_secs(30) {
            self.dxgi_adapters = query_dxgi_adapters();
            self.last_dxgi_refresh = Instant::now();
        }

        // Sample GPU engines
        let engine_items = self
            .engine_counter
            .map(|c| query_counter_items(c as PDH_HCOUNTER))
            .unwrap_or_default();
        let mut engines = Vec::with_capacity(engine_items.len());
        for (name, value) in engine_items {
            if !(0.0..=100.5).contains(&value) {
                continue;
            }
            let Some(identity) = parse_instance(&name) else {
                continue;
            };
            let util_metric = if is_primed {
                Metric::available(
                    value.clamp(0.0, 100.0) as f32,
                    planora_core::UNIT_PERCENT,
                    "Windows PDH GPU Engine",
                    stamp,
                )
            } else {
                Metric::unavailable(
                    planora_core::UNIT_PERCENT,
                    "Windows PDH GPU Engine",
                    stamp,
                    AvailabilityReason::WarmingUp,
                )
            };
            engines.push(GpuEngineSample {
                instance: name,
                engine_type: identity.engine_type,
                adapter_luid: Some(identity.luid),
                physical_adapter: Some(identity.physical),
                engine_id: Some(identity.engine),
                process_pid: Some(identity.pid),
                utilization_percent: util_metric,
            });
        }

        // Sample adapter memory counters
        let mut dedicated_map: HashMap<(u64, u32), u64> = HashMap::new();
        if let Some(counter) = self.dedicated_mem_counter {
            for (name, val) in query_counter_items(counter as PDH_HCOUNTER) {
                if let Some(identity) = parse_adapter_memory_instance(&name)
                    && val >= 0.0
                {
                    dedicated_map.insert(identity, val as u64);
                }
            }
        }

        let mut shared_map: HashMap<(u64, u32), u64> = HashMap::new();
        if let Some(counter) = self.shared_mem_counter {
            for (name, val) in query_counter_items(counter as PDH_HCOUNTER) {
                if let Some(identity) = parse_adapter_memory_instance(&name)
                    && val >= 0.0
                {
                    shared_map.insert(identity, val as u64);
                }
            }
        }

        // Build GpuAdapterSample per DXGI adapter
        let mut adapters = Vec::new();
        for dxgi in &self.dxgi_adapters {
            let busiest = busiest_physical_engine(&engines, dxgi.luid).unwrap_or_else(|| {
                if !self.has_gpu_engine_counter() {
                    Metric::unavailable(
                        planora_core::UNIT_PERCENT,
                        "Windows PDH physical GPU engine",
                        stamp,
                        AvailabilityReason::Unsupported(
                            "Windows PDH GPU Engine counter unavailable".into(),
                        ),
                    )
                } else if !is_primed {
                    Metric::unavailable(
                        planora_core::UNIT_PERCENT,
                        "Windows PDH physical GPU engine",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    )
                } else if !engines.is_empty() {
                    Metric::available(
                        0.0,
                        planora_core::UNIT_PERCENT,
                        "Windows PDH physical GPU engine",
                        stamp,
                    )
                } else {
                    Metric::unavailable(
                        planora_core::UNIT_PERCENT,
                        "Windows PDH physical GPU engine",
                        stamp,
                        AvailabilityReason::Unsupported(
                            "no PDH GPU engine instances reported".into(),
                        ),
                    )
                }
            });
            let physical_ids: std::collections::HashSet<u32> = engines
                .iter()
                .filter(|engine| engine.adapter_luid == Some(dxgi.luid))
                .filter_map(|engine| engine.physical_adapter)
                .chain(
                    dedicated_map
                        .keys()
                        .filter(|(luid, _)| *luid == dxgi.luid)
                        .map(|(_, physical)| *physical),
                )
                .chain(
                    shared_map
                        .keys()
                        .filter(|(luid, _)| *luid == dxgi.luid)
                        .map(|(_, physical)| *physical),
                )
                .collect();
            let physical_adapter = (physical_ids.len() == 1)
                .then(|| physical_ids.iter().copied().next())
                .flatten();
            let dedicated_used_bytes = if dxgi.dedicated_video_bytes == 0 {
                Metric::unavailable(
                    planora_core::UNIT_BYTES,
                    "DXGI Adapter Desc",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "Dedicated VRAM not applicable on unified memory architecture".into(),
                    ),
                )
            } else {
                match physical_adapter
                    .and_then(|physical| dedicated_map.get(&(dxgi.luid, physical)).copied())
                    .or(dxgi.current_usage_bytes)
                    .map(|u| u.min(dxgi.dedicated_video_bytes))
                {
                    Some(v) => Metric::available(
                        v,
                        planora_core::UNIT_BYTES,
                        "Windows PDH GPU Adapter Memory",
                        stamp,
                    ),
                    None => Metric::unavailable(
                        planora_core::UNIT_BYTES,
                        "Windows PDH GPU Adapter Memory",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    ),
                }
            };
            let shared_used_bytes = if dxgi.shared_system_bytes == 0 {
                Metric::unavailable(
                    planora_core::UNIT_BYTES,
                    "DXGI Adapter Desc",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "Shared GPU memory not reported by adapter".into(),
                    ),
                )
            } else {
                match physical_adapter
                    .and_then(|physical| shared_map.get(&(dxgi.luid, physical)).copied())
                    .map(|u| u.min(dxgi.shared_system_bytes))
                {
                    Some(v) => Metric::available(
                        v,
                        planora_core::UNIT_BYTES,
                        "Windows PDH GPU Adapter Memory",
                        stamp,
                    ),
                    None => Metric::unavailable(
                        planora_core::UNIT_BYTES,
                        "Windows PDH GPU Adapter Memory",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    ),
                }
            };

            adapters.push(GpuAdapterSample {
                adapter_luid: dxgi.luid,
                physical_adapter,
                name: dxgi.name.clone(),
                busiest_engine_percent: busiest,
                aggregate_busy_percent: None,
                dedicated_used_bytes,
                dedicated_total_bytes: Metric::available(
                    dxgi.dedicated_video_bytes,
                    planora_core::UNIT_BYTES,
                    "DXGI Adapter Desc",
                    stamp,
                ),
                shared_used_bytes,
                shared_total_bytes: Metric::available(
                    dxgi.shared_system_bytes,
                    planora_core::UNIT_BYTES,
                    "DXGI Adapter Desc",
                    stamp,
                ),
                budget_bytes: dxgi.budget_bytes.map(|b| {
                    Metric::available(
                        b,
                        planora_core::UNIT_BYTES,
                        "DXGI QueryVideoMemoryInfo",
                        stamp,
                    )
                }),
            });
        }

        // Sample physical disk performance (throughput, latency, queue depth)
        let read_items = if is_primed {
            self.disk_read_counter
                .map(|c| query_counter_items(c as PDH_HCOUNTER))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let write_items = if is_primed {
            self.disk_write_counter
                .map(|c| query_counter_items(c as PDH_HCOUNTER))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let latency_items = if is_primed {
            self.disk_latency_counter
                .map(|c| query_counter_items(c as PDH_HCOUNTER))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let queue_items = self
            .disk_queue_counter
            .map(|c| query_counter_items(c as PDH_HCOUNTER))
            .unwrap_or_default();

        let mut read_map: HashMap<String, u64> = HashMap::new();
        for (name, val) in read_items {
            read_map.insert(name, val.max(0.0) as u64);
        }
        let mut write_map: HashMap<String, u64> = HashMap::new();
        for (name, val) in write_items {
            write_map.insert(name, val.max(0.0) as u64);
        }

        let total_read = read_map.get("_Total").copied().unwrap_or_else(|| {
            read_map
                .iter()
                .filter(|(k, _)| k.as_str() != "_Total")
                .map(|(_, v)| *v)
                .fold(0u64, u64::saturating_add)
        });
        let total_write = write_map.get("_Total").copied().unwrap_or_else(|| {
            write_map
                .iter()
                .filter(|(k, _)| k.as_str() != "_Total")
                .map(|(_, v)| *v)
                .fold(0u64, u64::saturating_add)
        });
        self.cached_total_disk_io = (total_read, total_write);

        let mut all_instances = std::collections::BTreeSet::new();
        for k in read_map.keys().chain(write_map.keys()) {
            if k != "_Total" {
                all_instances.insert(k.clone());
            }
        }
        for (k, _) in &latency_items {
            if k != "_Total" {
                all_instances.insert(k.clone());
            }
        }
        for (k, _) in &queue_items {
            if k != "_Total" {
                all_instances.insert(k.clone());
            }
        }

        let mut physical_disks = Vec::new();
        let mut disk_metrics = HashMap::new();
        for inst in all_instances {
            let mut parts = inst.split_whitespace();
            let disk_index = parts.next().and_then(|p| p.parse::<u32>().ok());
            let drive_letters: Vec<String> = parts.map(|s| s.to_string()).collect();

            let r_bytes = read_map.get(&inst).copied().unwrap_or(0);
            let w_bytes = write_map.get(&inst).copied().unwrap_or(0);
            let lat = if is_primed {
                latency_items
                    .iter()
                    .find(|(n, _)| n == &inst)
                    .map(|(_, v)| (v * 1000.0) as f32)
                    .filter(|v| v.is_finite() && *v >= 0.0)
            } else {
                None
            };
            let qd = queue_items
                .iter()
                .find(|(n, _)| n == &inst)
                .map(|(_, v)| *v as f32)
                .filter(|v| v.is_finite() && *v >= 0.0);

            disk_metrics.insert(inst.clone(), (lat, qd));

            physical_disks.push(PhysicalDiskPerf {
                instance_name: inst,
                disk_index,
                drive_letters,
                read_bytes_per_sec: r_bytes,
                write_bytes_per_sec: w_bytes,
                latency_ms: lat,
                queue_depth: qd,
            });
        }
        physical_disks.sort_by_key(|d| d.disk_index.unwrap_or(u32::MAX));
        self.cached_physical_disks = physical_disks;
        self.cached_disk_metrics = disk_metrics;

        self.cached_engines = engines.clone();
        self.cached_adapters = adapters.clone();
        Ok((adapters, engines))
    }

    pub fn disk_metrics(&self) -> &HashMap<String, (Option<f32>, Option<f32>)> {
        &self.cached_disk_metrics
    }

    pub fn physical_disks(&self) -> &[PhysicalDiskPerf] {
        &self.cached_physical_disks
    }

    pub fn total_disk_io(&self) -> (u64, u64) {
        self.cached_total_disk_io
    }

    pub fn is_primed(&self) -> bool {
        self.sample_count >= 2
    }

    pub fn has_gpu_engine_counter(&self) -> bool {
        self.engine_counter.is_some()
    }

    pub fn has_disk_latency_counter(&self) -> bool {
        self.disk_latency_counter.is_some()
    }

    pub fn last_stamp_ms(&self) -> Option<u64> {
        self.cached_stamp_ms
    }
}

impl Drop for WindowsGpu {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query as PDH_HQUERY);
        }
    }
}

pub fn counter_available() -> bool {
    WindowsGpu::new().is_ok_and(|g| g.has_gpu_engine_counter())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(
        luid: u64,
        physical: u32,
        engine: u32,
        pid: u32,
        value: f32,
        stamp: u64,
    ) -> GpuEngineSample {
        GpuEngineSample {
            instance: format!("pid_{pid}"),
            engine_type: "3D".into(),
            adapter_luid: Some(luid),
            physical_adapter: Some(physical),
            engine_id: Some(engine),
            process_pid: Some(pid),
            utilization_percent: Metric::available(value, "%", "fixture", stamp),
        }
    }

    #[test]
    fn adapter_summary_combines_only_distinct_contributions_to_same_engine() {
        let samples = vec![
            engine(7, 0, 1, 10, 22.0, 100),
            engine(7, 0, 1, 10, 22.0, 100), // duplicate counter instance
            engine(7, 0, 1, 11, 18.0, 100),
            engine(7, 0, 2, 12, 35.0, 100),
            engine(7, 1, 1, 13, 30.0, 100),
            engine(8, 0, 1, 14, 90.0, 100),
        ];
        let summary = busiest_physical_engine(&samples, 7).unwrap();
        assert_eq!(summary.value, Some(40.0));
        assert_eq!(summary.sampled_at_ms, 100);
        assert!(busiest_physical_engine(&samples, 9).is_none());
    }

    #[test]
    fn parses_all_gpu_engine_identity_fields_without_guessing() {
        let parsed =
            parse_instance("pid_12608_luid_0x00000000_0x0000ddf1_phys_0_eng_2_engtype_3D").unwrap();
        assert_eq!(parsed.pid, 12608);
        assert_eq!(parsed.luid, 0xddf1);
        assert_eq!(parsed.physical, 0);
        assert_eq!(parsed.engine, 2);
        assert_eq!(parsed.engine_type, "3D");
        let duplicate =
            parse_instance("pid_12608_luid_0x00000000_0x0000ddf1_phys_0_eng_2_engtype_3D#1")
                .unwrap();
        assert_eq!(duplicate.engine_type, "3D");
        assert!(parse_instance("pid_1_engtype_3D").is_none());
        assert!(parse_instance("pid_1_luid_bad_0x1_phys_0_eng_0_engtype_3D").is_none());
    }

    #[test]
    fn parses_adapter_memory_instance_fields() {
        let parsed = parse_adapter_memory_instance("luid_0x00000000_0x0000ddf1_phys_0").unwrap();
        assert_eq!(parsed.0, 0xddf1);
        assert_eq!(parsed.1, 0);
        assert!(parse_adapter_memory_instance("bad_instance").is_none());
    }

    #[test]
    fn dxgi_factory_interface_id_is_accepted_by_windows() {
        unsafe {
            let dxgi = windows_sys::Win32::System::LibraryLoader::LoadLibraryExA(
                c"dxgi.dll".as_ptr().cast(),
                ptr::null_mut(),
                windows_sys::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            assert!(!dxgi.is_null());
            let create_factory_ptr = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                dxgi,
                c"CreateDXGIFactory1".as_ptr().cast(),
            );
            let create_factory: unsafe extern "system" fn(
                *const Guid,
                *mut *mut std::ffi::c_void,
            ) -> i32 = std::mem::transmute(create_factory_ptr.expect("CreateDXGIFactory1"));
            let mut factory = ptr::null_mut();
            let status = create_factory(&IID_IDXGI_FACTORY1, &mut factory);
            assert_eq!(status, 0, "CreateDXGIFactory1: 0x{status:08x}");
            assert!(!factory.is_null());
            let vtable = *(factory as *mut *mut usize);
            let release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
                std::mem::transmute(*vtable.add(2));
            release(factory);
            FreeLibrary(dxgi);
        }
    }

    #[test]
    fn dxgi_adapter_enumeration_returns_named_unique_adapters() {
        let adapters = query_dxgi_adapters();
        let mut luids = std::collections::HashSet::new();
        for adapter in adapters {
            assert!(!adapter.name.trim().is_empty());
            assert!(luids.insert(adapter.luid));
            // The primary fallback adapter can have the same name as WARP
            // without DXGI_ADAPTER_FLAG_SOFTWARE. Its name alone is not a
            // reliable software-adapter test.
        }
    }

    #[test]
    fn supported_counter_instances_have_valid_percentages() {
        let Ok(mut provider) = WindowsGpu::new() else {
            return;
        };
        std::thread::sleep(Duration::from_millis(1_100));
        let (adapters, engines) = provider
            .sample(crate::unix_ms())
            .expect("installed GPU Engine counter should be readable");
        assert!(engines.len() <= 4_096);
        for engine in engines {
            assert!(!engine.instance.is_empty());
            assert!(
                engine
                    .utilization_percent
                    .value
                    .is_some_and(|v| (0.0..=100.0).contains(&v))
            );
        }
        for adapter in adapters {
            assert!(!adapter.name.is_empty());
            assert!(
                adapter
                    .busiest_engine_percent
                    .value
                    .is_some_and(|v| (0.0..=100.0).contains(&v))
            );
        }
    }
}
