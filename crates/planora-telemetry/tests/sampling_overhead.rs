use planora_bench::{ChaseMode, run_cache_probe};
use planora_telemetry::Sampler;
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

#[test]
fn measures_sampling_overhead_and_cache_probe() {
    let topo = planora_topology::discover();
    println!("=== Target Machine Configuration ===");
    println!("Platform: {}", std::env::consts::OS);
    println!("Architecture: {}", std::env::consts::ARCH);
    println!("CPU Vendor: {}", topo.vendor);
    println!("CPU Model: {}", topo.model);
    println!("Physical Cores: {}", topo.cores.len());
    let total_logical: usize = topo.cores.iter().map(|c| c.logical_cpus.len()).sum();
    println!("Logical CPUs: {}", total_logical);
    println!("Caches: {}", topo.caches.len());
    for cache in &topo.caches {
        println!(
            "  L{} - {} KiB, line {} bytes (shared by CPUs {:?}, source: {})",
            cache.level,
            cache.size_bytes / 1024,
            cache.line_bytes,
            cache.shared_by,
            cache.source
        );
    }
    println!("GPUs: {}", topo.gpus.len());
    for gpu in &topo.gpus {
        println!("  GPU #{}: {} ({})", gpu.id, gpu.name, gpu.source);
    }

    println!("\n=== Sampling Overhead Benchmark ===");
    let mut sampler = Sampler::new();
    // Warm-up sample
    let _ = sampler.sample();

    let mut durations = Vec::with_capacity(30);
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        let frame = sampler.sample();
        let elapsed = start.elapsed();
        assert!(frame.collection_duration_ms >= 0.0);
        assert!(!frame.processes.is_empty());
        durations.push(elapsed.as_secs_f64() * 1000.0);
    }

    durations.sort_by(f64::total_cmp);
    let min = durations[0];
    let median = durations[durations.len() / 2];
    let p95 = durations[(durations.len() as f64 * 0.95) as usize];
    let max = *durations.last().unwrap();
    let mean: f64 = durations.iter().sum::<f64>() / durations.len() as f64;

    println!("Samples: {}", durations.len());
    println!(
        "Sampling Duration (ms): min={:.2}ms, median={:.2}ms, mean={:.2}ms, p95={:.2}ms, max={:.2}ms",
        min, median, mean, p95, max
    );

    println!("\n=== Sattolo Cache Probe Run ===");
    let cancel = AtomicBool::new(false);
    let probe_start = Instant::now();
    let probe_res = run_cache_probe(ChaseMode::Line, &cancel);
    let probe_elapsed = probe_start.elapsed();
    assert!(
        probe_res.is_ok(),
        "Cache probe failed: {:?}",
        probe_res.err()
    );
    let result = probe_res.unwrap();
    println!("Probe Total Duration: {:.2}s", probe_elapsed.as_secs_f64());
    println!("Methodology: {}", result.methodology);
    println!("Working Set | Min Latency | Median Latency | P95 Latency | P99 Latency");
    println!("------------------------------------------------------------------------");
    for pt in &result.points {
        let size_kb = pt.working_set_bytes / 1024;
        let size_str = if size_kb >= 1024 {
            format!("{:.1} MiB", size_kb as f64 / 1024.0)
        } else {
            format!("{size_kb} KiB")
        };
        println!(
            "{:<12} | {:>9.2} ns | {:>12.2} ns | {:>9.2} ns | {:>9.2} ns",
            size_str, pt.min_ns, pt.median_ns, pt.p95_ns, pt.p99_ns
        );
    }
}
