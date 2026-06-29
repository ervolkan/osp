//! Faz 0.6 — Karşılaştırma raporu üreteci (JSON + insan-okur tablo).

use anyhow::Result;

use crate::model::RepoAnalysis;

/// Tüm analizleri hem JSON'a hem de okunaklı tabloya basar.
pub fn print_comparison(analyses: &[RepoAnalysis]) -> Result<()> {
    // JSON özeti — makale için makine-okur kanıt.
    let json = serde_json::to_string_pretty(analyses)?;
    std::fs::write("spike-output.json", &json)?;
    tracing::info!(path = "spike-output.json", "JSON rapor yazıldı");

    // İnsan-okur tablo.
    // - `ff?` sütunu: * = FF/squash/rebase workflow kokusu (model.rs uyarısına bakın).
    // - `κ` = coupling density (edges/nodes). `H` = commit→dosya Shannon entropisi.
    println!("\n=== OSP Spike — Karşılaştırma ===\n");
    println!(
        "{:<15} {:>7} {:>6} {:>7} {:>4} {:>7} {:>7} {:>7} {:>7}",
        "repo", "commits", "merges", "w_ratio", "ff?", "w_depth", "θ_proxy", "κ(e/n)", "H"
    );
    println!("{}", "-".repeat(75));
    for a in analyses {
        let name = a
            .repo_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| a.repo_path.to_string_lossy().into_owned());
        let ff = if a.witness.likely_ff_workflow {
            "*"
        } else {
            ""
        };
        let kappa = if a.graph.nodes.is_empty() {
            0.0
        } else {
            a.graph.edges.len() as f64 / a.graph.nodes.len() as f64
        };
        println!(
            "{:<15} {:>7} {:>6} {:>7.2} {:>4} {:>7.2} {:>7.2} {:>7.2} {:>7.2}",
            name,
            a.witness.total_commits,
            a.witness.merge_commits,
            a.witness.witnessed_ratio,
            ff,
            a.metrics.witness_depth,
            a.metrics.deviation_proxy,
            kappa,
            a.witness.commit_entropy,
        );
    }
    println!();
    Ok(())
}
