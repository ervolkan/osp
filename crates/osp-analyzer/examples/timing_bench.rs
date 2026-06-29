//! Multi-run timing benchmark — 3 repos × 5 runs, median + p50/p90.
//! Usage: cargo run --release --example timing_bench -- <repo> <runs>

use std::path::Path;
use std::time::Instant;

use osp_analyzer::pipeline::analyze_repo;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let repo = args
        .get(1)
        .expect("usage: timing_bench <repo-path> [runs=5]");
    let runs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);

    let path = Path::new(repo);
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".to_string());

    println!("=== Timing: {} ({} runs, release build) ===", name, runs);

    // Warmup: filesystem cache fill
    let _ = analyze_repo(path);

    let mut times: Vec<f64> = Vec::with_capacity(runs);
    for i in 0..runs {
        let start = Instant::now();
        let result = analyze_repo(path)?;
        let elapsed = start.elapsed().as_secs_f64();
        times.push(elapsed);
        eprintln!(
            "  run {}: {:.2}s ({} nodes, {} edges)",
            i + 1,
            elapsed,
            result.space.node_count(),
            result.space.edge_count()
        );
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let min = times.first().copied().unwrap_or(0.0);
    let max = times.last().copied().unwrap_or(0.0);
    let p90 = if times.len() >= 10 {
        times[(times.len() as f64 * 0.9) as usize]
    } else {
        max // too few runs for meaningful p90
    };

    println!();
    println!("--- {} ---", name);
    println!("  median: {:.2}s", median);
    println!("  min:    {:.2}s", min);
    println!("  max:    {:.2}s", max);
    if runs >= 10 {
        println!("  p90:    {:.2}s", p90);
    }
    println!(
        "  runs:   {:?}",
        times
            .iter()
            .map(|t| format!("{:.2}", t))
            .collect::<Vec<_>>()
    );

    Ok(())
}
