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

#[derive(Debug, Clone, Copy)]
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

/// Sattolo's strict j < i swap yields one cycle, not several short cycles.
pub fn build_cycle(working_set: usize, mode: ChaseMode) -> Result<Cycle, String> {
    if !(8 * 1024..=64 * 1024 * 1024).contains(&working_set) {
        return Err("working set must be 8 KiB–64 MiB".into());
    }
    let step = match mode {
        ChaseMode::Line => 1,
        ChaseMode::Page => 4096 / 128,
    };
    let total = working_set / 128;
    let mut order: Vec<_> = (0..total).step_by(step).collect();
    if order.len() < 2 {
        return Err("working set too small".into());
    }
    let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ working_set as u64;
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
    let sizes = [
        8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536,
    ];
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut points = Vec::new();
    for kib in sizes {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("benchmark time limit reached".into());
        }
        let cycle = build_cycle(kib * 1024, mode)?;
        let mut cursor = cycle.start;
        let iterations = cycle.active_count.clamp(4096, 65_536);
        for _ in 0..iterations {
            cursor = unsafe { read_volatile(&cycle.nodes[cursor].next) };
        }
        let mut samples = Vec::with_capacity(100);
        for _ in 0..100 {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let start = Instant::now();
            for _ in 0..iterations {
                cursor = unsafe { read_volatile(&cycle.nodes[cursor].next) };
            }
            black_box(cursor);
            samples.push(start.elapsed().as_nanos() as f64 / iterations as f64);
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
    Ok(BenchmarkResult {
        mode: match mode {
            ChaseMode::Line => "line",
            ChaseMode::Page => "page",
        }
        .into(),
        methodology: "128-byte aligned nodes; Sattolo single cycle (strictly j < i); 100 monotonic-clock batches per working set; reports min, median, p95, p99 nanoseconds per dependent load; memory capped at 64 MiB, total time capped at 12 s".into(),
        comparable_to_x86_cycles: false,
        points,
        core_pairs: Vec::new(),
    })
}

/// CAS ping-pong. Returns a measured transfer estimate, never a guaranteed fabric latency.
pub fn core_pair_latency(a: usize, b: usize, cancel: Arc<AtomicBool>) -> Result<f32, String> {
    if a == b {
        return Err("inter-core latency requires two distinct logical CPUs".into());
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
    let mut rounds = 0u64;
    while rounds < 10_000 && Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        if state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            while state.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
                std::hint::spin_loop();
            }
            if state.load(Ordering::Acquire) == 0 {
                rounds += 1;
            }
        }
    }
    let elapsed = start.elapsed();
    stop.store(true, Ordering::Relaxed);
    responder.join().map_err(|_| "responder panicked")??;
    if rounds < 100 {
        return Err("insufficient core transfers".into());
    }
    Ok(elapsed.as_nanos() as f32 / (rounds as f32 * 2.0))
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
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut pairs = Vec::new();
    for (i, a) in cpus.iter().enumerate() {
        for b in &cpus[i + 1..] {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("matrix time limit reached".into());
            }
            let ns = core_pair_latency(*a as usize, *b as usize, cancel.clone())?;
            pairs.push(CorePair {
                cpu_a: *a,
                cpu_b: *b,
                estimated_one_way_ns: ns as f64,
            });
        }
    }
    Ok(BenchmarkResult{mode:"core matrix".into(),methodology:"Pinned logical-CPU atomic ping-pong; elapsed time divided by two transfers per round; estimate includes synchronization and scheduling; up to 8 CPUs and 20 seconds".into(),comparable_to_x86_cycles:false,points:Vec::new(),core_pairs:pairs})
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
}
