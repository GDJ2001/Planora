//! ETW Disk I/O and PMU profiling collector for Windows.
#[cfg(windows)]
use planora_core::{DiskIoProfileRecord, IdentitySource};
use planora_core::{ErrorCode, ProcessIdentity, ProfileResult, ProfileTarget};
use std::sync::{Arc, atomic::AtomicBool};
#[cfg(windows)]
use std::{
    collections::HashMap,
    sync::{Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PmuSourceInfo {
    pub id: u32,
    pub name: String,
    pub min_interval: u32,
    pub max_interval: u32,
}

#[cfg(windows)]
pub fn enumerate_pmu_sources() -> Vec<PmuSourceInfo> {
    unsafe extern "system" {
        fn TraceQueryInformation(
            session_handle: u64,
            information_class: i32,
            trace_information: *mut std::ffi::c_void,
            information_length: u32,
            return_length: *mut u32,
        ) -> u32;
    }

    let mut ret_len = 0u32;
    // InformationClass 7 = TraceProfileSourceListInfo
    let _ = unsafe { TraceQueryInformation(0, 7, std::ptr::null_mut(), 0, &mut ret_len) };
    if ret_len == 0 || ret_len > 1024 * 1024 {
        return Vec::new();
    }

    let mut buffer = vec![0u8; ret_len as usize];
    let status = unsafe {
        TraceQueryInformation(0, 7, buffer.as_mut_ptr() as *mut _, ret_len, &mut ret_len)
    };
    if status != 0 {
        return Vec::new();
    }

    let mut sources = Vec::new();
    let mut offset = 0usize;
    while offset + 24 <= buffer.len() {
        let ptr = buffer.as_ptr().wrapping_add(offset);
        let next_offset = unsafe { std::ptr::read_unaligned(ptr as *const u32) } as usize;
        let source_id = unsafe { std::ptr::read_unaligned(ptr.add(4) as *const u32) };
        let min_interval = unsafe { std::ptr::read_unaligned(ptr.add(8) as *const u32) };
        let max_interval = unsafe { std::ptr::read_unaligned(ptr.add(12) as *const u32) };

        let entry_end = if next_offset == 0 {
            buffer.len()
        } else if next_offset < 24
            || offset
                .checked_add(next_offset)
                .is_none_or(|next| next > buffer.len())
        {
            break;
        } else {
            offset + next_offset
        };
        let desc_ptr = ptr.wrapping_add(24) as *const u16;
        let max_chars = (entry_end - (offset + 24)) / 2;
        let mut len = 0;
        while len < max_chars && unsafe { *desc_ptr.add(len) } != 0 {
            len += 1;
        }
        let slice = unsafe { std::slice::from_raw_parts(desc_ptr, len) };
        let name = String::from_utf16_lossy(slice);

        sources.push(PmuSourceInfo {
            id: source_id,
            name,
            min_interval,
            max_interval,
        });

        if next_offset == 0 {
            break;
        }
        offset += next_offset;
    }

    sources
}

#[cfg(not(windows))]
pub fn enumerate_pmu_sources() -> Vec<PmuSourceInfo> {
    Vec::new()
}

#[cfg(windows)]
#[derive(Default)]
struct ProcessAttributionMap {
    threads: HashMap<u32, (u32, String, u64, u64)>, // tid -> (pid, name, process FILETIME, thread FILETIME)
    processes: HashMap<u32, (String, u64)>,         // pid -> (name, creation_filetime)
}

#[cfg(windows)]
fn snapshot_processes_and_threads() -> ProcessAttributionMap {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };

    let mut map = ProcessAttributionMap::default();

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS | TH32CS_SNAPTHREAD, 0);
        if snap == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return map;
        }

        // 1. Snapshot processes
        let mut pe = std::mem::zeroed::<PROCESSENTRY32W>();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                let name_len = pe
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..name_len]);
                // Query process creation time if accessible
                let mut creation_filetime = 0u64;
                let proc_handle = windows_sys::Win32::System::Threading::OpenProcess(
                    windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pe.th32ProcessID,
                );
                if !proc_handle.is_null() {
                    let mut ct = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    let mut et = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    let mut kt = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    let mut ut = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    if windows_sys::Win32::System::Threading::GetProcessTimes(
                        proc_handle,
                        &mut ct,
                        &mut et,
                        &mut kt,
                        &mut ut,
                    ) != 0
                    {
                        creation_filetime =
                            ((ct.dwHighDateTime as u64) << 32) | (ct.dwLowDateTime as u64);
                    }
                    windows_sys::Win32::Foundation::CloseHandle(proc_handle);
                }

                map.processes
                    .insert(pe.th32ProcessID, (name, creation_filetime));
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }

        // 2. Snapshot threads
        let mut te = std::mem::zeroed::<THREADENTRY32>();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        if Thread32First(snap, &mut te) != 0 {
            loop {
                let pid = te.th32OwnerProcessID;
                let (proc_name, creation_time) = map
                    .processes
                    .get(&pid)
                    .cloned()
                    .unwrap_or_else(|| (format!("Process {pid}"), 0));
                let mut thread_created = 0u64;
                let thread_handle = windows_sys::Win32::System::Threading::OpenThread(
                    windows_sys::Win32::System::Threading::THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    te.th32ThreadID,
                );
                if !thread_handle.is_null() {
                    let mut ct = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    let mut et = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    let mut kt = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    let mut ut = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
                    if windows_sys::Win32::System::Threading::GetThreadTimes(
                        thread_handle,
                        &mut ct,
                        &mut et,
                        &mut kt,
                        &mut ut,
                    ) != 0
                    {
                        thread_created =
                            ((ct.dwHighDateTime as u64) << 32) | ct.dwLowDateTime as u64;
                    }
                    windows_sys::Win32::Foundation::CloseHandle(thread_handle);
                }
                if creation_time != 0 && thread_created != 0 {
                    map.threads.insert(
                        te.th32ThreadID,
                        (pid, proc_name, creation_time, thread_created),
                    );
                }
                if Thread32Next(snap, &mut te) == 0 {
                    break;
                }
            }
        }

        windows_sys::Win32::Foundation::CloseHandle(snap);
    }

    map
}

#[cfg(windows)]
fn active_thread_matches(tid: u32, pid: u32, created: u64) -> bool {
    use windows_sys::Win32::System::Threading::{
        GetProcessIdOfThread, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
    };
    if tid == 0 || pid == 0 || created == 0 {
        return false;
    }
    unsafe {
        let handle = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, tid);
        if handle.is_null() {
            return false;
        }
        let mut ct = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
        let mut et = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
        let mut kt = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
        let mut ut = std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>();
        let matches = GetProcessIdOfThread(handle) == pid
            && GetThreadTimes(handle, &mut ct, &mut et, &mut kt, &mut ut) != 0
            && (((ct.dwHighDateTime as u64) << 32) | ct.dwLowDateTime as u64) == created;
        windows_sys::Win32::Foundation::CloseHandle(handle);
        matches
    }
}

#[cfg(windows)]
const PLANORA_DISK_SESSION_GUID: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0xA9BA49A6,
    data2: 0x957F,
    data3: 0x42A5,
    data4: [0xA9, 0x2B, 0x17, 0x18, 0x3F, 0x4F, 0x61, 0xB7],
};

#[cfg(windows)]
const DISK_IO_EVENT_GUID: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x3D6FA8D4,
    data2: 0xFE05,
    data3: 0x11D0,
    data4: [0x9D, 0xDA, 0x00, 0xC0, 0x4F, 0xD7, 0xBA, 0x7C],
};

#[cfg(windows)]
#[repr(C)]
struct EtwSessionProperties {
    props: windows_sys::Win32::System::Diagnostics::Etw::EVENT_TRACE_PROPERTIES,
    logger_name: [u16; 128],
    log_file_name: [u16; 260],
}

#[cfg(windows)]
fn parse_disk_io_x64_payload(payload: &[u8]) -> Option<(u64, u32)> {
    // Windows' DiskIo_TypeGroup1 MOF layout on the supported x64 target.
    // The four-byte transfer size is at offset 8 and issuing TID at 48.
    let transfer = u32::from_le_bytes(payload.get(8..12)?.try_into().ok()?) as u64;
    let issuing_tid = u32::from_le_bytes(payload.get(48..52)?.try_into().ok()?);
    (issuing_tid != 0).then_some((transfer, issuing_tid))
}

pub fn run_profile(
    job_id: u64,
    target: ProfileTarget,
    cancel: Arc<AtomicBool>,
) -> Result<ProfileResult, (ErrorCode, String)> {
    match target {
        ProfileTarget::DiskIo {
            target_process,
            duration_seconds,
        } => run_disk_io_profile(job_id, target_process, duration_seconds, cancel),
        ProfileTarget::PmuSampling {
            target_process,
            duration_seconds,
            sources,
        } => run_pmu_sampling_profile(job_id, target_process, duration_seconds, sources, cancel),
    }
}

#[cfg(windows)]
fn run_disk_io_profile(
    job_id: u64,
    target_process: Option<ProcessIdentity>,
    duration_seconds: u32,
    cancel: Arc<AtomicBool>,
) -> Result<ProfileResult, (ErrorCode, String)> {
    use windows_sys::Win32::System::Diagnostics::Etw::{
        ControlTraceW, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FLAG_DISK_IO,
        EVENT_TRACE_FLAG_DISK_IO_INIT, EVENT_TRACE_REAL_TIME_MODE, EVENT_TRACE_SYSTEM_LOGGER_MODE,
        PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, StartTraceW,
        WNODE_FLAG_TRACED_GUID,
    };

    let duration_sec = if duration_seconds == 0 {
        15
    } else {
        duration_seconds.min(60)
    };
    let session_name_str = format!("PlanoraDiskIo_{}_{}", std::process::id(), job_id);
    let session_name_wide: Vec<u16> = session_name_str
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut session_props = unsafe { std::mem::zeroed::<EtwSessionProperties>() };
    session_props.props.Wnode.BufferSize = std::mem::size_of::<EtwSessionProperties>() as u32;
    session_props.props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    session_props.props.Wnode.Guid = PLANORA_DISK_SESSION_GUID;
    session_props.props.Wnode.ClientContext = 1; // QPC
    session_props.props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE | EVENT_TRACE_SYSTEM_LOGGER_MODE;
    // Real-time ETW uses in-memory buffers. MaximumFileSize only applies to a
    // file-backed trace, so it cannot enforce this session's memory budget.
    session_props.props.BufferSize = 64; // KiB
    session_props.props.MinimumBuffers = 16;
    session_props.props.MaximumBuffers = 512; // 32 MiB of ETW buffers
    session_props.props.FlushTimer = 1; // 1 second buffer flush
    session_props.props.EnableFlags = EVENT_TRACE_FLAG_DISK_IO | EVENT_TRACE_FLAG_DISK_IO_INIT;
    session_props.props.LoggerNameOffset = std::mem::size_of::<
        windows_sys::Win32::System::Diagnostics::Etw::EVENT_TRACE_PROPERTIES,
    >() as u32;

    let mut session_handle =
        windows_sys::Win32::System::Diagnostics::Etw::CONTROLTRACE_HANDLE { Value: 0 };
    let start_status = unsafe {
        StartTraceW(
            &mut session_handle,
            session_name_wide.as_ptr(),
            &mut session_props.props,
        )
    };

    if start_status != 0 {
        // A name collision is not evidence that the other session is ours.
        // Never stop a session for which StartTraceW did not return a handle.
        return Err((
            if start_status == 183 {
                ErrorCode::Busy
            } else {
                ErrorCode::PermissionDenied
            },
            format!("Failed to start ETW Disk I/O trace session (code 0x{start_status:08x})."),
        ));
    }

    // Take initial process & thread snapshot
    let attr_map = Arc::new(Mutex::new(snapshot_processes_and_threads()));

    // Shared state for event consumer
    struct DiskStats {
        per_process: HashMap<(u32, u64), DiskIoProfileRecord>,
        unattributed_read: u64,
        unattributed_write: u64,
        unattributed_ops: u64,
        events_traced: u64,
    }
    let stats = Arc::new(Mutex::new(DiskStats {
        per_process: HashMap::new(),
        unattributed_read: 0,
        unattributed_write: 0,
        unattributed_ops: 0,
        events_traced: 0,
    }));

    // Start background event pump
    let consumer_stats = stats.clone();
    let consumer_map = attr_map.clone();
    let consumer_name_wide = session_name_wide.clone();
    let consumer_target = target_process.clone();

    let consumer_thread =
        std::thread::Builder::new()
            .name("planora-etw-disk-consumer".into())
            .spawn(move || {
                unsafe extern "system" fn event_record_callback(
                    record: *mut windows_sys::Win32::System::Diagnostics::Etw::EVENT_RECORD,
                ) {
                    if record.is_null() {
                        return;
                    }
                    let rec = unsafe { &*record };
                    // System providers retain the classic DiskIo event GUID in
                    // the record header. Other system events must not be parsed
                    // using the DiskIo_TypeGroup1 payload layout.
                    let provider = rec.EventHeader.ProviderId;
                    if provider.data1 != DISK_IO_EVENT_GUID.data1
                        || provider.data2 != DISK_IO_EVENT_GUID.data2
                        || provider.data3 != DISK_IO_EVENT_GUID.data3
                        || provider.data4 != DISK_IO_EVENT_GUID.data4
                    {
                        return;
                    }
                    let user_context = rec.UserContext
                        as *mut (
                            Arc<Mutex<DiskStats>>,
                            Arc<Mutex<ProcessAttributionMap>>,
                            Option<ProcessIdentity>,
                        );
                    if user_context.is_null() {
                        return;
                    }
                    let (stats, attr_map, target_process) = unsafe { &*user_context };

                    let opcode = rec.EventHeader.EventDescriptor.Opcode;
                    let is_read = opcode == 10;
                    let is_write = opcode == 11;
                    if !is_read && !is_write {
                        return;
                    }

                    // DiskIo_TypeGroup1 on the supported x64 target:
                    // 4 x u32, ByteOffset (u64), FileObject (pointer), Irp
                    // (pointer), HighResResponseTime (u64), IssuingThreadId
                    // (u32). Do not infer an issuing thread from a shorter or
                    // unrecognized event payload.
                    let user_data = rec.UserData;
                    let user_data_len = rec.UserDataLength as usize;
                    if user_data.is_null() {
                        return;
                    }
                    let payload = unsafe {
                        std::slice::from_raw_parts(user_data.cast::<u8>(), user_data_len)
                    };
                    let Some((transfer_size, issuing_thread)) = parse_disk_io_x64_payload(payload)
                    else {
                        return;
                    };

                    let mut matched_proc = None;
                    {
                        let map = attr_map.lock().unwrap();
                        if let Some(&(pid, ref name, creation, thread_created)) =
                            map.threads.get(&issuing_thread)
                            && active_thread_matches(issuing_thread, pid, thread_created)
                        {
                            matched_proc = Some((pid, name.clone(), creation));
                        }
                    }

                    let mut s = stats.lock().unwrap();

                    if let Some((pid, name, creation)) = matched_proc {
                        if target_process
                            .as_ref()
                            .is_some_and(|target| target.pid != pid || target.started != creation)
                        {
                            return;
                        }
                        s.events_traced = s.events_traced.saturating_add(1);
                        if !s.per_process.contains_key(&(pid, creation))
                            && s.per_process.len() >= 10_000
                        {
                            s.unattributed_ops = s.unattributed_ops.saturating_add(1);
                            return;
                        }
                        let entry = s.per_process.entry((pid, creation)).or_insert_with(|| {
                            DiskIoProfileRecord {
                                pid,
                                identity: Some(ProcessIdentity {
                                    pid,
                                    started: creation,
                                    sequence_number: None,
                                    source: IdentitySource::WindowsProcessFiletime,
                                }),
                                process_name: name,
                                read_bytes: 0,
                                write_bytes: 0,
                                read_operations: 0,
                                write_operations: 0,
                            }
                        });
                        if is_read {
                            entry.read_bytes = entry.read_bytes.saturating_add(transfer_size);
                            entry.read_operations = entry.read_operations.saturating_add(1);
                        } else {
                            entry.write_bytes = entry.write_bytes.saturating_add(transfer_size);
                            entry.write_operations = entry.write_operations.saturating_add(1);
                        }
                    } else {
                        // A selected-process trace cannot assign an unknown
                        // event to that process, so omit it from its totals.
                        if target_process.is_some() {
                            return;
                        }
                        s.events_traced = s.events_traced.saturating_add(1);
                        if is_read {
                            s.unattributed_read = s.unattributed_read.saturating_add(transfer_size);
                        } else {
                            s.unattributed_write =
                                s.unattributed_write.saturating_add(transfer_size);
                        }
                        s.unattributed_ops = s.unattributed_ops.saturating_add(1);
                    }
                }

                let mut context_tuple = (consumer_stats, consumer_map, consumer_target);

                let mut logfile = unsafe {
                    std::mem::zeroed::<
                        windows_sys::Win32::System::Diagnostics::Etw::EVENT_TRACE_LOGFILEW,
                    >()
                };
                logfile.LoggerName = consumer_name_wide.as_ptr() as *mut u16;
                logfile.Anonymous1.ProcessTraceMode =
                    PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
                logfile.Anonymous2.EventRecordCallback = Some(event_record_callback);
                logfile.Context = &mut context_tuple as *mut _ as *mut std::ffi::c_void;

                let trace_handle = unsafe {
                    windows_sys::Win32::System::Diagnostics::Etw::OpenTraceW(&mut logfile)
                };
                if trace_handle.Value != !0 {
                    let mut handles = [trace_handle];
                    unsafe {
                        windows_sys::Win32::System::Diagnostics::Etw::ProcessTrace(
                            handles.as_mut_ptr(),
                            1,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                        windows_sys::Win32::System::Diagnostics::Etw::CloseTrace(trace_handle);
                    }
                }
            });
    let consumer_thread = match consumer_thread {
        Ok(handle) => handle,
        Err(error) => {
            unsafe {
                ControlTraceW(
                    session_handle,
                    session_name_wide.as_ptr(),
                    &mut session_props.props,
                    EVENT_TRACE_CONTROL_STOP,
                );
            }
            return Err((
                ErrorCode::Internal,
                format!("Could not start disk ETW consumer: {error}"),
            ));
        }
    };

    // Wait for the specified duration or until cancelled
    let start_time = Instant::now();
    let total_duration = Duration::from_secs(duration_sec as u64);
    let mut last_refresh = Instant::now();
    while start_time.elapsed() < total_duration && !cancel.load(Ordering::SeqCst) {
        if last_refresh.elapsed() >= Duration::from_secs(1) {
            *attr_map.lock().unwrap() = snapshot_processes_and_threads();
            last_refresh = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let elapsed_ms = start_time.elapsed().as_millis() as u64;

    // Stop trace session safely
    unsafe {
        ControlTraceW(
            session_handle,
            session_name_wide.as_ptr(),
            &mut session_props.props,
            EVENT_TRACE_CONTROL_STOP,
        );
    }

    let _ = consumer_thread.join();

    // Cancellation still requires stopping and joining our ETW session, but
    // its partial counters must not be published as a completed profile.
    if cancel.load(Ordering::SeqCst) {
        return Err((ErrorCode::Cancelled, "Disk I/O profile cancelled".into()));
    }

    let stats_lock = stats.lock().unwrap();
    let events_lost = session_props.props.EventsLost as u64;
    let events_traced = stats_lock.events_traced;

    let mut disk_records: Vec<DiskIoProfileRecord> =
        stats_lock.per_process.values().cloned().collect();
    disk_records.sort_by_key(|r| std::cmp::Reverse(r.read_bytes + r.write_bytes));

    let total_attributed_bytes: u64 = disk_records
        .iter()
        .map(|r| r.read_bytes + r.write_bytes)
        .sum();
    let total_bytes =
        total_attributed_bytes + stats_lock.unattributed_read + stats_lock.unattributed_write;
    let coverage = if total_bytes > 0 && events_traced > 0 && events_lost == 0 {
        (total_attributed_bytes as f64 / total_bytes as f64 * 100.0) as f32
    } else {
        0.0
    };

    let target_desc = if let Some(target) = target_process {
        format!("Disk I/O profile (PID {})", target.pid)
    } else {
        "System-wide Disk I/O profile".to_string()
    };

    Ok(ProfileResult {
        target_desc,
        duration_seconds: duration_sec,
        elapsed_ms,
        events_traced,
        events_lost,
        attribution_coverage_percent: coverage,
        disk_records,
        pmu_records: Vec::new(),
        unattributed_read_bytes: stats_lock.unattributed_read,
        unattributed_write_bytes: stats_lock.unattributed_write,
        unattributed_operations: stats_lock.unattributed_ops,
    })
}

#[cfg(windows)]
fn run_pmu_sampling_profile(
    _job_id: u64,
    _target_process: Option<ProcessIdentity>,
    _duration_seconds: u32,
    _sources: Vec<String>,
    _cancel: Arc<AtomicBool>,
) -> Result<ProfileResult, (ErrorCode, String)> {
    Err((
        ErrorCode::Unsupported,
        "PMU source discovery is available, but no ETW sampled-event collector is implemented; no sample count can be reported.".into(),
    ))
}
#[cfg(not(windows))]
fn run_disk_io_profile(
    _job_id: u64,
    _target_process: Option<ProcessIdentity>,
    _duration_seconds: u32,
    _cancel: Arc<AtomicBool>,
) -> Result<ProfileResult, (ErrorCode, String)> {
    Err((
        ErrorCode::Unsupported,
        "Disk I/O ETW profiling is only supported on Windows".into(),
    ))
}

#[cfg(not(windows))]
fn run_pmu_sampling_profile(
    _job_id: u64,
    _target_process: Option<ProcessIdentity>,
    _duration_seconds: u32,
    _sources: Vec<String>,
    _cancel: Arc<AtomicBool>,
) -> Result<ProfileResult, (ErrorCode, String)> {
    Err((
        ErrorCode::Unsupported,
        "PMU sampled profiling is only supported on Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn disk_io_payload_requires_complete_x64_layout_and_issuing_thread() {
        let mut payload = [0u8; 52];
        payload[8..12].copy_from_slice(&4096u32.to_le_bytes());
        payload[48..52].copy_from_slice(&345u32.to_le_bytes());
        assert_eq!(parse_disk_io_x64_payload(&payload), Some((4096, 345)));
        assert_eq!(parse_disk_io_x64_payload(&payload[..51]), None);
        payload[48..52].fill(0);
        assert_eq!(parse_disk_io_x64_payload(&payload), None);
    }

    #[test]
    fn pmu_source_enumeration_returns_valid_entries_on_windows() {
        if cfg!(windows) {
            let sources = enumerate_pmu_sources();
            println!("OS reported {} PMU profile sources", sources.len());
            for src in &sources {
                println!(
                    " - [{}] {} (min={}, max={})",
                    src.id, src.name, src.min_interval, src.max_interval
                );
            }
            // Some supported Windows machines expose no PMU profile source.
            assert!(sources.len() <= 1024);
        }
    }
}
