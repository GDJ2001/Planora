//! OS-reported topology with CPUID enrichment on x86. Unknown relationships stay unknown.
#[cfg(any(target_os = "linux", windows))]
use planora_core::CacheDescriptor;
#[cfg(target_os = "linux")]
use planora_core::GpuDescriptor;
use planora_core::{CoreDescriptor, Topology};
#[cfg(target_os = "linux")]
use std::collections::BTreeMap;

pub fn discover() -> Topology {
    let _count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(u16::MAX as usize);
    let mut t = Topology {
        vendor: String::new(),
        model: String::new(),
        architecture: std::env::consts::ARCH.into(),
        cores: Vec::new(),
        caches: Vec::new(),
        gpus: Vec::new(),
        notes: Vec::new(),
    };
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        let cpuid = raw_cpuid::CpuId::new();
        t.vendor = cpuid
            .get_vendor_info()
            .map(|v| v.as_str().to_owned())
            .unwrap_or_default();
        t.model = cpuid
            .get_processor_brand_string()
            .map(|v| v.as_str().trim().to_owned())
            .unwrap_or_default();
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    {
        t.vendor = "OS-reported".into();
    }
    #[cfg(target_os = "linux")]
    {
        discover_linux(&mut t, _count);
    }
    #[cfg(windows)]
    {
        discover_windows(&mut t);
    }
    #[cfg(target_os = "macos")]
    {
        discover_macos(&mut t);
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        for i in 0.._count {
            t.cores.push(CoreDescriptor {
                id: i as u16,
                package_id: None,
                numa_node: None,
                logical_cpus: vec![i as u16],
                class: None,
            });
        }
        t.notes.push("Logical CPU count is known; package, SMT, NUMA and cache sharing relationships need native OS discovery on this platform".into());
    }
    if t.model.is_empty() {
        t.model = std::env::consts::ARCH.into();
    }
    t
}

#[cfg(target_os = "linux")]
fn discover_linux(t: &mut Topology, count: usize) {
    use std::{fs, path::Path};
    let mut grouped: BTreeMap<(u16, u16), Vec<u16>> = BTreeMap::new();
    let read_num = |cpu: usize, name: &str| -> Option<u16> {
        fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/topology/{name}"))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    for cpu in 0..count {
        let package = read_num(cpu, "physical_package_id");
        let core = read_num(cpu, "core_id");
        match (package, core) {
            (Some(p), Some(c)) => grouped.entry((p, c)).or_default().push(cpu as u16),
            _ => t.cores.push(CoreDescriptor {
                id: cpu as u16,
                package_id: package,
                numa_node: None,
                logical_cpus: vec![cpu as u16],
                class: None,
            }),
        }
    }
    for ((package, core), logical_cpus) in grouped {
        t.cores.push(CoreDescriptor {
            id: core,
            package_id: Some(package),
            numa_node: None,
            logical_cpus,
            class: None,
        });
    }
    let cache_root = Path::new("/sys/devices/system/cpu/cpu0/cache");
    if let Ok(entries) = fs::read_dir(cache_root) {
        for e in entries.flatten() {
            let p = e.path();
            if !e.file_name().to_string_lossy().starts_with("index") {
                continue;
            }
            let read = |key: &str| {
                fs::read_to_string(p.join(key))
                    .ok()
                    .map(|v| v.trim().to_owned())
            };
            let (Some(level), Some(size)) = (
                read("level").and_then(|v| v.parse::<u8>().ok()),
                read("size"),
            ) else {
                continue;
            };
            let size_bytes = parse_size(&size);
            let shared_by = read("shared_cpu_list")
                .map(|v| parse_cpu_list(&v))
                .unwrap_or_default();
            let line_bytes = read("coherency_line_size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            t.caches.push(CacheDescriptor {
                level,
                size_bytes,
                line_bytes,
                shared_by,
                source: "Linux sysfs cpu0 cache index".into(),
            });
        }
    }
    if t.caches.is_empty() {
        t.notes.push("Cache topology unavailable from sysfs".into());
    }
    if t.cores.is_empty() {
        t.notes.push("Core grouping unavailable from sysfs".into());
    }
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || !name[4..].chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if !entry.path().join("device").exists() {
                continue;
            }
            let id = t.gpus.len() as u16;
            let vendor = fs::read_to_string(entry.path().join("device/vendor")).unwrap_or_default();
            let device = fs::read_to_string(entry.path().join("device/device")).unwrap_or_default();
            t.gpus.push(GpuDescriptor {
                id,
                name: format!("{name} PCI {}:{}", vendor.trim(), device.trim()),
                source: "Linux DRM sysfs node".into(),
            });
        }
    }
}

#[cfg(windows)]
fn discover_windows(t: &mut Topology) {
    use windows_sys::Win32::System::SystemInformation::*;
    use windows_sys::Win32::System::SystemInformation::{
        RelationCache as RELATION_CACHE, RelationNumaNode as RELATION_NUMA_NODE,
        RelationNumaNodeEx as RELATION_NUMA_NODE_EX,
        RelationProcessorCore as RELATION_PROCESSOR_CORE,
        RelationProcessorPackage as RELATION_PROCESSOR_PACKAGE,
    };
    let mut bytes = 0u32;
    unsafe {
        GetLogicalProcessorInformationEx(RelationAll, std::ptr::null_mut(), &mut bytes);
    }
    if bytes == 0 {
        t.notes
            .push("Windows logical processor API unavailable".into());
        return;
    }
    let mut buffer = vec![0u8; bytes as usize];
    if unsafe {
        GetLogicalProcessorInformationEx(RelationAll, buffer.as_mut_ptr().cast(), &mut bytes)
    } == 0
    {
        t.notes
            .push("Windows logical processor query failed".into());
        return;
    }
    fn cpus(masks: *const GROUP_AFFINITY, count: usize) -> Vec<u16> {
        let mut out = Vec::new();
        for i in 0..count.min(64) {
            let mask = unsafe { *masks.add(i) };
            for bit in 0..usize::BITS {
                if mask.Mask & (1usize << bit) != 0
                    && let Ok(id) =
                        u16::try_from(mask.Group as usize * usize::BITS as usize + bit as usize)
                {
                    out.push(id);
                }
            }
        }
        out
    }
    let mut packages: Vec<Vec<u16>> = Vec::new();
    let mut numas: Vec<(u16, Vec<u16>)> = Vec::new();
    let mut offset = 0usize;
    while offset + 8 <= bytes as usize {
        let size = unsafe { *(buffer.as_ptr().add(offset + 4) as *const u32) } as usize;
        if size < 8 || offset + size > bytes as usize {
            break;
        }
        let record = unsafe {
            &*(buffer.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX)
        };
        unsafe {
            match record.Relationship {
                RELATION_PROCESSOR_CORE | RELATION_PROCESSOR_PACKAGE => {
                    let item = record.Anonymous.Processor;
                    let logical = cpus(
                        std::ptr::addr_of!(record.Anonymous.Processor.GroupMask).cast(),
                        item.GroupCount as usize,
                    );
                    if record.Relationship == RELATION_PROCESSOR_CORE {
                        t.cores.push(CoreDescriptor {
                            id: t.cores.len() as u16,
                            package_id: None,
                            numa_node: None,
                            logical_cpus: logical,
                            class: Some(format!("efficiency class {}", item.EfficiencyClass)),
                        });
                    } else {
                        packages.push(logical);
                    }
                }
                RELATION_NUMA_NODE | RELATION_NUMA_NODE_EX => {
                    let item = record.Anonymous.NumaNode;
                    let logical = cpus(
                        std::ptr::addr_of!(record.Anonymous.NumaNode.Anonymous.GroupMask).cast(),
                        item.GroupCount.max(1) as usize,
                    );
                    numas.push((item.NodeNumber as u16, logical));
                }
                RELATION_CACHE => {
                    let item = record.Anonymous.Cache;
                    let logical = cpus(
                        std::ptr::addr_of!(record.Anonymous.Cache.Anonymous.GroupMask).cast(),
                        item.GroupCount.max(1) as usize,
                    );
                    t.caches.push(CacheDescriptor {
                        level: item.Level,
                        size_bytes: item.CacheSize as u64,
                        line_bytes: item.LineSize,
                        shared_by: logical,
                        source: "Windows GetLogicalProcessorInformationEx".into(),
                    });
                }
                _ => {}
            }
        }
        offset += size;
    }
    for core in &mut t.cores {
        core.package_id = packages
            .iter()
            .position(|members| core.logical_cpus.iter().all(|cpu| members.contains(cpu)))
            .map(|i| i as u16);
        core.numa_node = numas
            .iter()
            .find(|(_, members)| core.logical_cpus.iter().all(|cpu| members.contains(cpu)))
            .map(|(id, _)| *id);
    }
    if t.cores.is_empty() {
        t.notes
            .push("No physical core relations returned by Windows".into());
    }
    discover_windows_gpus(t);
    if t.gpus.is_empty() {
        t.notes
            .push("No display adapter returned by Windows display device enumeration".into());
    }
}

#[cfg(windows)]
fn discover_windows_gpus(t: &mut Topology) {
    use windows_sys::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW};
    let mut index = 0u32;
    let mut seen = std::collections::HashSet::new();
    loop {
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), index, &mut dd, 0) } == 0 {
            break;
        }
        index += 1;
        // Skip mirroring drivers (DISPLAY_DEVICE_MIRRORING_DRIVER = 0x00000008)
        if dd.StateFlags & 0x00000008 != 0 {
            continue;
        }
        let len = dd
            .DeviceString
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(dd.DeviceString.len());
        let name = String::from_utf16_lossy(&dd.DeviceString[..len])
            .trim()
            .to_string();
        if !name.is_empty() && seen.insert(name.clone()) {
            let id = t.gpus.len() as u16;
            t.gpus.push(planora_core::GpuDescriptor {
                id,
                name,
                source: "Windows display device enumeration".into(),
            });
        }
    }
}

#[cfg(target_os = "macos")]
fn discover_macos(t: &mut Topology) {
    use std::ffi::CString;
    fn number(name: &str) -> Option<u64> {
        let name = CString::new(name).ok()?;
        let mut value = 0u64;
        let mut len = std::mem::size_of::<u64>();
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut value as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 { Some(value) } else { None }
    }
    fn string(name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        let mut len = 0usize;
        if unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        let mut buffer = vec![0u8; len];
        if unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        Some(
            String::from_utf8_lossy(&buffer)
                .trim_end_matches('\0')
                .into(),
        )
    }
    t.vendor = "Apple".into();
    t.model = string("hw.model").unwrap_or_else(|| "Apple Silicon".into());
    let logical = number("hw.logicalcpu_max")
        .unwrap_or(1)
        .min(u16::MAX as u64) as u16;
    let packages = number("hw.packages");
    for id in 0..logical {
        t.cores.push(CoreDescriptor {
            id,
            package_id: if packages == Some(1) { Some(0) } else { None },
            numa_node: None,
            logical_cpus: vec![id],
            class: None,
        });
    }
    if let Some(physical) = number("hw.physicalcpu_max") {
        t.notes.push(format!("macOS reports {physical} physical cores; logical-to-core mapping is unavailable through this query"));
    }
    if let Some(l1d) = number("hw.l1dcachesize") {
        t.caches.push(planora_core::CacheDescriptor {
            level: 1,
            size_bytes: l1d,
            line_bytes: 64,
            shared_by: Vec::new(),
            source: "macOS sysctl hw.l1dcachesize".into(),
        });
    }
    if let Some(l2) = number("hw.l2cachesize") {
        t.caches.push(planora_core::CacheDescriptor {
            level: 2,
            size_bytes: l2,
            line_bytes: 128,
            shared_by: Vec::new(),
            source: "macOS sysctl hw.l2cachesize".into(),
        });
    }
    t.gpus.push(planora_core::GpuDescriptor {
        id: 0,
        name: "Apple Silicon Integrated GPU".into(),
        source: "macOS hardware platform".into(),
    });
    t.notes.push("macOS sysctl supplies counts, not an exact SMT/NUMA/cache-sharing map; each canvas node represents a logical CPU".into());
}

pub fn parse_size(s: &str) -> u64 {
    let unit = s.chars().last().unwrap_or('B');
    let number = s
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .parse::<u64>()
        .unwrap_or(0);
    number.saturating_mul(match unit {
        'K' => 1024,
        'M' => 1024 * 1024,
        'G' => 1024 * 1024 * 1024,
        _ => 1,
    })
}

pub fn parse_cpu_list(s: &str) -> Vec<u16> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let mut items = part.split('-');
        let Some(a) = items.next().and_then(|x| x.parse::<u16>().ok()) else {
            continue;
        };
        let b = items
            .next()
            .and_then(|x| x.parse::<u16>().ok())
            .unwrap_or(a);
        if b >= a && (b - a) < 4096 {
            out.extend(a..=b)
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_range() {
        assert_eq!(parse_cpu_list("0-2,4,8-9"), vec![0, 1, 2, 4, 8, 9]);
        assert_eq!(parse_cpu_list("3"), vec![3]);
        assert_eq!(parse_cpu_list(""), Vec::<u16>::new());
    }

    #[test]
    fn parses_size_units() {
        assert_eq!(parse_size("32K"), 32 * 1024);
        assert_eq!(parse_size("512M"), 512 * 1024 * 1024);
        assert_eq!(parse_size("2G"), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("128B"), 128);
    }

    #[test]
    fn discover_returns_valid_topology() {
        let topo = discover();
        assert!(!topo.architecture.is_empty());
        assert!(!topo.cores.is_empty());
    }
}
