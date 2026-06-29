//! Token Benchmark — Raw File Dump vs OSP Coordinate Prompt.
//!
//! 3 yaklaşımda token tüketimini ölçer:
//!
//! 1. **Full repo baseline:** Tüm kaynak dosyalar (Copilot/Devin'in teorik olarak aldığı)
//! 2. **2-hop context baseline:** Sadece ilgili dosyalar + import'lari (akıllı agent)
//! 3. **OSP prompt:** Koordinat-tabanlı alt-graf dilimi (OSP'nin gönderdiği)
//!
//! Token approximation: chars / 4 (OpenAI tiktoken standard yaklaşımı).
//! Kullanım: cargo run --example token_benchmark -- <repo-path>

use std::collections::HashSet;
use std::path::Path;

use osp_analyzer::contract::AnalysisConfig;
use osp_analyzer::language::AdapterRegistry;
use osp_analyzer::pipeline::analyze_repo_with_config;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let repo_path = args.get(1).expect("usage: token_benchmark <repo-path>");
    let path = Path::new(repo_path);
    let repo_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or("?".into());

    println!("╔══════════════════════════════════════════════════════╗");
    println!(
        "║  OSP Token Benchmark — {:<30} ║",
        &format!("{}/", repo_name)
    );
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    // ── 1. Analyze repo ──
    let config = AnalysisConfig::default();
    let registry = AdapterRegistry::default_all();
    let result = analyze_repo_with_config(path, &registry, &config)?;

    let node_count = result.space.node_count();
    let edge_count = result.space.edge_count();
    println!("Repo: {} nodes, {} edges", node_count, edge_count);
    println!();

    // ── 2. Full repo baseline: all source files concatenated ──
    let source_extensions = [".py", ".ts", ".js", ".rs", ".go"];
    let mut full_text_size = 0usize;
    let mut file_count = 0usize;

    fn walk_dir(dir: &Path, exts: &[&str], total_size: &mut usize, count: &mut usize) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if name.starts_with('.')
                        || name == "node_modules"
                        || name == "target"
                        || name == "__pycache__"
                    {
                        continue;
                    }
                    walk_dir(&path, exts, total_size, count);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let dotted = format!(".{ext}");
                    if exts.contains(&dotted.as_str()) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            *total_size += content.len();
                            *count += 1;
                        }
                    }
                }
            }
        }
    }

    walk_dir(
        path,
        &source_extensions,
        &mut full_text_size,
        &mut file_count,
    );

    let baseline_tokens = full_text_size / 4;
    println!("── 1. Full Repo Baseline (all source files) ──");
    println!("   Files: {}", file_count);
    println!("   Characters: {}", full_text_size);
    println!("   ≈ Tokens: {:>10} (chars/4)", format_num(baseline_tokens));
    println!();

    // ── 3. 2-hop context baseline: relevant files only ──
    // Simulate: agent wants to modify node 0 → needs 2-hop neighbors
    let target_node = result.space.nodes.keys().min().copied().unwrap_or(0);
    let mut relevant_nodes: HashSet<u64> = HashSet::new();
    relevant_nodes.insert(target_node);

    // 1-hop
    for e in &result.space.edges {
        if e.from == target_node {
            relevant_nodes.insert(e.to);
        }
        if e.to == target_node {
            relevant_nodes.insert(e.from);
        }
    }
    // 2-hop
    let one_hop: Vec<u64> = relevant_nodes.iter().copied().collect();
    for e in &result.space.edges {
        if one_hop.contains(&e.from) {
            relevant_nodes.insert(e.to);
        }
        if one_hop.contains(&e.to) {
            relevant_nodes.insert(e.from);
        }
    }

    // Estimate average file size (full repo / file count)
    let avg_file_size = if file_count > 0 {
        full_text_size / file_count
    } else {
        0
    };
    let context_2hop_size = relevant_nodes.len() * avg_file_size;
    let context_2hop_tokens = context_2hop_size / 4;

    println!("── 2. 2-Hop Context Baseline (relevant files only) ──");
    println!(
        "   Target: node {} + {} neighbors (2-hop)",
        target_node,
        relevant_nodes.len() - 1
    );
    println!(
        "   Est. characters: {} ({} files × avg {} chars)",
        context_2hop_size,
        relevant_nodes.len(),
        avg_file_size
    );
    println!("   ≈ Tokens: {:>10}", format_num(context_2hop_tokens));
    println!();

    // ── 4. OSP prompt: coordinate-based subgraph ──
    // Each node: id (8 bytes) + kind (20 bytes) + 5 coordinates (5×8=40 bytes)
    // + mass (8 bytes) + cohesion (8 bytes) ≈ ~100 bytes JSON per node
    // Plus: edges (~30 bytes each), vision (50 bytes), rules (~100 bytes), contract (~200 bytes)

    let osp_node_bytes = relevant_nodes.len() * 120; // ~120 chars per node in JSON
    let relevant_edges: usize = result
        .space
        .edges
        .iter()
        .filter(|e| relevant_nodes.contains(&e.from) && relevant_nodes.contains(&e.to))
        .count();
    let osp_edge_bytes = relevant_edges * 40; // ~40 chars per edge in JSON
    let osp_overhead_bytes = 500; // vision + rules + contract + intent
    let osp_total_bytes = osp_node_bytes + osp_edge_bytes + osp_overhead_bytes;
    let osp_tokens = osp_total_bytes / 4;

    println!("── 3. OSP Coordinate Prompt (typed subgraph) ──");
    println!(
        "   Nodes: {} (coordinates: x,y,z,w,v per node)",
        relevant_nodes.len()
    );
    println!("   Edges: {} (typed)", relevant_edges);
    println!(
        "   JSON est. characters: {} (120/node + 40/edge + 500 overhead)",
        osp_total_bytes
    );
    println!("   ≈ Tokens: {:>10}", format_num(osp_tokens));
    println!();

    // ── 5. Comparison ──
    println!("═══════════════════════════════════════════════════════");
    println!("  COMPRESSION RATIO");
    println!("═══════════════════════════════════════════════════════");

    if osp_tokens > 0 && baseline_tokens > 0 {
        let full_ratio = osp_tokens as f64 / baseline_tokens as f64;
        let full_savings = (1.0 - full_ratio) * 100.0;
        println!();
        println!(
            "  vs Full Repo:      {:>6} → {:>6} = {:.2}% savings (1:{:.0})",
            format_num(baseline_tokens),
            format_num(osp_tokens),
            full_savings,
            baseline_tokens as f64 / osp_tokens as f64
        );
    }

    if osp_tokens > 0 && context_2hop_tokens > 0 {
        let ctx_ratio = osp_tokens as f64 / context_2hop_tokens as f64;
        let ctx_savings = (1.0 - ctx_ratio) * 100.0;
        println!(
            "  vs 2-Hop Context:  {:>6} → {:>6} = {:.2}% savings (1:{:.0})",
            format_num(context_2hop_tokens),
            format_num(osp_tokens),
            ctx_savings,
            context_2hop_tokens as f64 / osp_tokens as f64
        );
    }

    println!();
    println!("  Token approximation: chars / 4 (tiktoken standard)");
    println!("  OSP replaces file CONTENT with coordinate TOPOLOGY");
    println!();

    Ok(())
}

fn format_num(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
