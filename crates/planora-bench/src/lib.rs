//! Bounded, user-triggered hardware probes. Never call these from a render thread.
use planora_core::{BenchmarkPoint, BenchmarkResult, CorePair};
use std::{
    hint::black_box,
    ptr::read_volatile,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub const MAX_WORKING_SET_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
pub const MAX_AUXILIARY_BYTES: usize = 8 * 1024 * 1024; // 8 MiB
pub const MAX_TOTAL_MEMORY_BUDGET: usize = MAX_WORKING_SET_BYTES + MAX_AUXILIARY_BYTES; // 72 MiB
pub const CACHE_PROBE_DEADLINE_SECS: u64 = 12;
pub const CORE_MATRIX_DEADLINE_SECS: u64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChaseMode {
    Line,
    Page,
}

#[repr(C, align(128))]
#[derive(Clone)]
pub struct Node {
    pub next: usize,
    _pad: [u8; 120],
}

impl Default for Node {
    fn default() -> Self {
        Self {
            next: 0,
            _pad: [0; 120],
        }
    }
}

pub struct Cycle {
    pub nodes: Vec<Node>,
    pub start: usize,
    pub active_count: usize,
}

/// Derive system page size dynamically from the OS.
pub fn os_page_size() -> usize {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
        let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut info) };
        if info.dwPageSize > 0 {
            info.dwPageSize as usize
        } else {
            4096
        }
    }
    #[cfg(unix)]
    {
        let sz = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if sz > 0 { sz as usize } else { 4096 }
    }
    #[cfg(not(any(windows, unix)))]
    {
        4096
    }
}

/// Sattolo's strict j < i swap yields one cycle, not several short cycles.
/// Enforces separate auxiliary allocations and total memory budget.
pub fn build_cycle(working_set: usize, mode: ChaseMode) -> Result<Cycle, String> {
    if !(8 * 1024..=MAX_WORKING_SET_BYTES).contains(&working_set) {
        return Err("working set must be between 8 KiB and 64 MiB".into());
    }
    let node_size = std::mem::size_of::<Node>(); // 128 bytes
    let total = working_set / node_size;
    let page_size = os_page_size();
    let step = match mode {
        ChaseMode::Line => 1,
        ChaseMode::Page => (page_size / node_size).max(1),
    };
    let order_len = total.div_ceil(step);
    let aux_memory = order_len * std::mem::size_of::<usize>();
    if aux_memory > MAX_AUXILIARY_BYTES {
        return Err("auxiliary allocation exceeds auxiliary budget".into());
    }
    let total_budget = (total * node_size).saturating_add(aux_memory);
    if total_budget > MAX_TOTAL_MEMORY_BUDGET {
        return Err("allocation exceeds total memory budget".into());
    }
    let mut order: Vec<_> = (0..total).step_by(step).collect();
    if order.len() < 2 {
        return Err("working set too small for cycle".into());
    }
    let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ (working_set as u64);
    for i in (1..order.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed as usize) % i;
        order.swap(i, j);
    }
    let mut nodes = vec![Node::default(); total];
    for pair in order.windows(2) {
        nodes[pair[0]].next = pair[1];
    }
    nodes[*order.last().unwrap()].next = order[0];
    Ok(Cycle {
        nodes,
        start: order[0],
        active_count: order.len(),
    })
}

pub fn run_cache_probe(mode: ChaseMode, cancel: &AtomicBool) -> Result<BenchmarkResult, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let timer_overhead_ns = (0..256)
        .map(|_| {
            let start = Instant::now();
            start.elapsed().as_nanos()
        })
        .min()
        .unwrap_or(0);
    let sizes = [
        8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536,
    ];
    let deadline = Instant::now() + Duration::from_secs(CACHE_PROBE_DEADLINE_SECS);
    let mut points = Vec::new();
    let mut was_cancelled = false;
    let mut hit_timeout = false;

    for kib in sizes {
        if cancel.load(Ordering::Relaxed) {
            was_cancelled = true;
            break;
        }
        if Instant::now() >= deadline {
            hit_timeout = true;
            break;
        }
        let cycle = build_cycle(kib * 1024, mode)?;
        let mut cursor = cycle.start;
        let iterations = cycle.active_count.clamp(4096, 65_536);
        for _ in 0..iterations {
            if cancel.load(Ordering::Relaxed) {
                was_cancelled = true;
                break;
            }
            cursor = unsafe { read_volatile(&cycle.nodes[cursor].next) };
        }
        if was_cancelled {
            break;
        }
        let mut samples = Vec::with_capacity(100);
        for _ in 0..100 {
            if cancel.load(Ordering::Relaxed) {
                was_cancelled = true;
                break;
            }
            if Instant::now() >= deadline {
                hit_timeout = true;
                break;
            }
            let start = Instant::now();
            for _ in 0..iterations {
                cursor = unsafe { read_volatile(&cycle.nodes[cursor].next) };
            }
            black_box(cursor);
            samples.push(
                start.elapsed().as_nanos().saturating_sub(timer_overhead_ns) as f64
                    / iterations as f64,
            );
        }
        if was_cancelled || hit_timeout {
            break;
        }
        samples.sort_by(f64::total_cmp);
        points.push(BenchmarkPoint {
            working_set_bytes: (kib * 1024) as u64,
            min_ns: samples[0],
            median_ns: samples[50],
            p95_ns: samples[94],
            p99_ns: samples[98],
        });
    }

    if (was_cancelled || hit_timeout) && points.is_empty() {
        return Err(if was_cancelled {
            "cancelled".into()
        } else {
            "benchmark time limit reached".into()
        });
    }

    let is_partial = was_cancelled || hit_timeout;
    let page_size = os_page_size();
    let methodology = format!(
        "128-byte aligned nodes; Sattolo single cycle (strictly j < i); page size {page_size} bytes; 100 monotonic batches per working set; min timer overhead {timer_overhead_ns} ns subtracted; reports min, median, p95, p99 nanoseconds per dependent load; memory capped at 64 MiB, timeout 12 s{status}",
        status = if is_partial { " (PARTIAL RUN)" } else { "" }
    );

    Ok(BenchmarkResult {
        mode: match mode {
            ChaseMode::Line => "line",
            ChaseMode::Page => "page",
        }
        .into(),
        methodology,
        comparable_to_x86_cycles: false,
        is_partial,
        points,
        core_pairs: Vec::new(),
    })
}

/// CAS ping-pong. Returns distribution (median_ns, min_ns, p95_ns) of measured one-way transfers.
pub fn core_pair_latency(
    a: usize,
    b: usize,
    cancel: Arc<AtomicBool>,
) -> Result<(f64, f64, f64), String> {
    if a == b {
        return Err("distinct logical CPUs required".into());
    }
    if !cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
        return Err("exact core pinning unsupported on this architecture".into());
    }
    let ids = core_affinity::get_core_ids().ok_or("CPU affinity unavailable")?;
    let ida = ids
        .iter()
        .find(|id| id.id == a)
        .copied()
        .ok_or("core A unavailable")?;
    let idb = ids
        .iter()
        .find(|id| id.id == b)
        .copied()
        .ok_or("core B unavailable")?;
    let state = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (s, c, stop2) = (state.clone(), cancel.clone(), stop.clone());
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let responder = std::thread::spawn(move || -> Result<(), String> {
        if !core_affinity::set_for_current(idb) {
            let _ = ready_tx.send(false);
            return Err("cannot pin responder".into());
        }
        let _ = ready_tx.send(true);
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if c.load(Ordering::Relaxed) || stop2.load(Ordering::Relaxed) || Instant::now() >= until
            {
                break;
            }
            if s.compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                std::hint::spin_loop();
            }
        }
        Ok(())
    });
    if !core_affinity::set_for_current(ida) {
        stop.store(true, Ordering::Relaxed);
        let _ = responder.join();
        return Err("cannot pin initiator".into());
    }
    if ready_rx.recv_timeout(Duration::from_secs(1)).ok() != Some(true) {
        stop.store(true, Ordering::Relaxed);
        let _ = responder.join();
        return Err("responder pinning failed".into());
    }

    let start = Instant::now();
    let deadline = start + Duration::from_secs(2);
    let mut round_times = Vec::with_capacity(100);
    let mut rounds = 0u64;
    let mut batch_start = Instant::now();
    let mut batch_rounds = 0u64;

    while rounds < 10_000 && Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        if state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            while state.load(Ordering::Acquire) != 0
                && Instant::now() < deadline
                && !cancel.load(Ordering::Relaxed)
            {
                std::hint::spin_loop();
            }
            if state.load(Ordering::Acquire) == 0 {
                rounds += 1;
                batch_rounds += 1;
                if batch_rounds >= 100 {
                    let elapsed = batch_start.elapsed();
                    let one_way = elapsed.as_nanos() as f64 / (batch_rounds as f64 * 2.0);
                    round_times.push(one_way);
                    batch_rounds = 0;
                    batch_start = Instant::now();
                }
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    let _ = responder.join();

    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    if rounds < 100 || round_times.is_empty() {
        return Err("insufficient core transfers".into());
    }

    round_times.sort_by(f64::total_cmp);
    let min = round_times[0];
    let median = round_times[round_times.len() / 2];
    let p95 = round_times[(round_times.len() as f64 * 0.95) as usize];

    Ok((median, min, p95))
}

pub fn run_core_matrix(cpus: &[u16], cancel: Arc<AtomicBool>) -> Result<BenchmarkResult, String> {
    if cpus.len() < 2 || cpus.len() > 8 {
        return Err("select 2–8 logical CPUs".into());
    }
    let mut unique = cpus.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != cpus.len() {
        return Err("CPU selection contains duplicates".into());
    }
    let deadline = Instant::now() + Duration::from_secs(CORE_MATRIX_DEADLINE_SECS);
    let mut pairs = Vec::new();
    let mut was_cancelled = false;
    let mut hit_timeout = false;

    for (i, a) in cpus.iter().enumerate() {
        for b in &cpus[i + 1..] {
            if cancel.load(Ordering::Relaxed) {
                was_cancelled = true;
                break;
            }
            if Instant::now() + Duration::from_secs(2) >= deadline {
                hit_timeout = true;
                break;
            }
            let (median, min, p95) =
                match core_pair_latency(*a as usize, *b as usize, cancel.clone()) {
                    Ok(res) => res,
                    Err(e) => {
                        if cancel.load(Ordering::Relaxed) {
                            was_cancelled = true;
                            break;
                        }
                        return Err(e);
                    }
                };
            pairs.push(CorePair {
                cpu_a: *a,
                cpu_b: *b,
                estimated_one_way_ns: median,
                min_ns: min,
                p95_ns: p95,
            });
        }
        if was_cancelled || hit_timeout {
            break;
        }
    }

    if (was_cancelled || hit_timeout) && pairs.is_empty() {
        return Err(if was_cancelled {
            "cancelled".into()
        } else {
            "matrix time limit reached".into()
        });
    }

    let is_partial = was_cancelled || hit_timeout;
    Ok(BenchmarkResult {
        mode: "core matrix".into(),
        methodology: format!(
            "Pinned logical-CPU atomic ping-pong; elapsed time divided by two transfers per round; reports median, min, p95 nanoseconds; up to 8 CPUs and 20 s{status}",
            status = if is_partial { " (PARTIAL RUN)" } else { "" }
        ),
        comparable_to_x86_cycles: false,
        is_partial,
        points: Vec::new(),
        core_pairs: pairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_page_cycles_visit_every_node_once() {
        for mode in [ChaseMode::Line, ChaseMode::Page] {
            for size in [8192, 65536, 1_048_576] {
                let cycle = build_cycle(size, mode).unwrap();
                let mut seen = std::collections::HashSet::new();
                let mut i = cycle.start;
                for _ in 0..cycle.active_count {
                    assert!(seen.insert(i));
                    i = cycle.nodes[i].next;
                }
                assert_eq!(i, cycle.start);
                assert_eq!(seen.len(), cycle.active_count);
            }
        }
    }

    #[test]
    fn rejects_excessive_memory() {
        assert!(build_cycle(128 * 1024 * 1024, ChaseMode::Line).is_err());
        assert!(build_cycle(1024, ChaseMode::Line).is_err());
    }

    #[test]
    fn probe_respects_cancellation() {
        let cancel = AtomicBool::new(true);
        let res = run_cache_probe(ChaseMode::Line, &cancel);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "cancelled");
    }

    #[test]
    fn same_cpu_never_produces_fake_zero_latency() {
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(core_pair_latency(0, 0, cancel).is_err());
    }

    #[test]
    fn matrix_validates_cpu_selection() {
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(run_core_matrix(&[0], cancel.clone()).is_err());
        assert!(run_core_matrix(&[0, 1, 2, 3, 4, 5, 6, 7, 8], cancel.clone()).is_err());
        assert!(run_core_matrix(&[0, 0], cancel).is_err());
    }

    #[test]
    fn os_page_size_is_positive_power_of_two() {
        let sz = os_page_size();
        assert!(sz >= 4096);
        assert!(sz.is_power_of_two());
    }
}
