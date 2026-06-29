//! OSP Spike — Faz 0/1.10.
//!
//! İki mod:
//! - `analyze` — Faz 0 legacy (graph + witness + metrics, kör-test)
//! - `analyze-v2` — Faz 1.10 osp-core pipeline (tri-state + positions + vision)

use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

mod bridge;
mod graph;
mod metrics;
mod model;
mod report;
mod witness;

use crate::model::RepoAnalysis;

/// CLI ana giriş.
fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }

    match args[1].as_str() {
        "analyze" => run_legacy(&args[2..])?,
        "analyze-v2" => run_v2(&args[2..])?,
        cmd => {
            eprintln!("Bilinmeyen subcommand: {cmd}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}

fn usage() {
    eprintln!("Kullanım:");
    eprintln!("  osp-spike analyze <repo> [--compare <b> <c> ...]       (Faz 0 legacy)");
    eprintln!(
        "  osp-spike analyze-v2 <repo> [--compare <b> <c> ...]   (Faz 1.10 osp-core pipeline)"
    );
}

/// `<repo> [--compare <b> <c> ...]` argümanlarını path listesine çevir.
fn parse_paths(args: &[String]) -> Vec<PathBuf> {
    if args.is_empty() {
        return vec![];
    }
    let mut paths = vec![PathBuf::from(&args[0])];
    if args.len() >= 3 && args[1] == "--compare" {
        for p in &args[2..] {
            paths.push(PathBuf::from(p));
        }
    }
    paths
}

/// Faz 0 legacy — graph + witness + metrics karşılaştırması.
fn run_legacy(args: &[String]) -> Result<()> {
    if args.is_empty() {
        usage();
        std::process::exit(2);
    }
    let paths = parse_paths(args);
    let mut analyses: Vec<RepoAnalysis> = Vec::new();
    for p in &paths {
        analyses.push(analyze_repo(p)?);
    }
    report::print_comparison(&analyses)?;
    Ok(())
}

/// Faz 1.10 — full osp-core pipeline (bridge → space → coordinates → vision → tri-state).
fn run_v2(args: &[String]) -> Result<()> {
    if args.is_empty() {
        usage();
        std::process::exit(2);
    }
    let paths = parse_paths(args);
    let mut reports = Vec::new();
    for p in &paths {
        let analysis = analyze_repo(p)?;
        reports.push(bridge::run_v2_pipeline(&analysis));
    }
    bridge::print_v2_comparison(&reports)?;
    Ok(())
}

/// Tek bir repoyu uçtan uca analiz eder: graf + şahitlik + metrik.
fn analyze_repo(path: &std::path::Path) -> Result<RepoAnalysis> {
    tracing::info!(repo = ?path, "Analiz başlıyor");
    let graph = graph::extract(path)?; // Faz 0.3 — tree-sitter
    let witness = witness::analyze(path)?; // Faz 0.4 — git log
    let metrics = metrics::compute(&graph, &witness); // Faz 0.5
    Ok(RepoAnalysis {
        repo_path: path.to_path_buf(),
        graph,
        witness,
        metrics,
    })
}

/// JSON serileştirme için dışa aktarılan özet (report.rs kullanır).
#[derive(Serialize)]
pub struct SpikeOutput {
    pub analyses: Vec<RepoAnalysis>,
}
