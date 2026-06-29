//! Multi-repo LLM token benchmark (RQ5 / §7.8).
//!
//! Runs the OSP-vs-raw comparison across many repos with REAL per-node
//! metrics (coupling/cohesion/instability from osp-analyzer) so the result
//! distribution is statistically meaningful, not a single 4-node sample.
//!
//! For each repo:
//!   1. analyze_repo() -> per-module metrics (real coupling/cohesion/I).
//!   2. Pick K sample modules (by id) -> OSP coordinate prompt with real coords.
//!   3. Collect K source files (first K sorted) -> raw 2-hop-style dump.
//!   4. complete_raw() both prompts against the model, record real token counts.
//!
//! Output:
//!   - docs/usage-llm-benchmark-multi.json (full per-repo results)
//!   - stdout markdown table (repo / nodes / osp_tok / raw_tok / savings)
//!
//! Usage:
//!     $env:OPENAI_API_KEY = "sk-..."
//!     cargo run -p osp-llm-runtime --example multi_repo_bench -- [repo paths...]
//!
//! With no args, scans P:\Work\repos (the corpus clone dir).

use std::path::{Path, PathBuf};

use osp_analyzer::{analyze_repo, AnalysisResult};
use osp_core::space::EdgeKind;

use osp_llm_runtime::{
    osp_system_prompt, raw_dump_user_prompt, raw_system_prompt, CompletionRequest, Runtime,
    RuntimeConfig,
};

/// Number of modules to include per prompt. Fixed across repos so token
/// comparisons isolate representation quality, not prompt scale.
const K: usize = 8;

/// Max chars per source file in the raw dump (matches the PS baseline cap).
const RAW_FILE_CAP: usize = 2000;

const USAGE: &str = "usage: multi_repo_bench [repo paths...] (env OPENAI_API_KEY required)";

fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let repos: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let repos = if repos.is_empty() {
        discover_repos(Path::new(r"P:\Work\repos"))?
    } else {
        repos
    };
    if repos.is_empty() {
        anyhow::bail!("no repos found. {USAGE}");
    }

    let model = std::env::var("OSP_BENCH_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let cfg = RuntimeConfig {
        model: model.clone(),
        max_tokens: 300,
        ..RuntimeConfig::default()
            .with_env_api_key()
            .map_err(|e| anyhow::anyhow!("{USAGE}: {e}"))?
    };
    let runtime = Runtime::new(cfg)?;

    println!("# Multi-repo token benchmark ({model}, K={K})\n");
    println!("| repo | nodes | osp_chars | raw_chars | osp_tok | raw_tok | ratio | savings |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");

    let mut results: Vec<RepoResult> = Vec::new();
    for repo in &repos {
        let name = repo
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        match run_one(&runtime, repo, &name) {
            Ok(r) => {
                println!(
                    "| {} | {} | {} | {} | {} | {} | {:.1}x | {:.1}% |",
                    r.name,
                    r.node_count,
                    r.osp_input_chars,
                    r.raw_input_chars,
                    r.osp_prompt_tokens,
                    r.raw_prompt_tokens,
                    r.ratio,
                    r.savings_pct,
                );
                results.push(r);
            }
            Err(e) => {
                println!("| {} | — | — | — | — | — | — | ERROR: {e} |", name);
            }
        }
    }

    print_summary(&results);

    let out = "docs/usage-llm-benchmark-multi.json";
    let payload = serde_json::json!({
        "model": model,
        "k": K,
        "timestamp_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "repos": results.iter().map(|r| serde_json::json!({
            "name": r.name,
            "node_count": r.node_count,
            "osp_input_chars": r.osp_input_chars,
            "raw_input_chars": r.raw_input_chars,
            "osp_prompt_tokens": r.osp_prompt_tokens,
            "raw_prompt_tokens": r.raw_prompt_tokens,
            "osp_completion_tokens": r.osp_completion_tokens,
            "raw_completion_tokens": r.raw_completion_tokens,
            "ratio": (r.ratio * 10.0).round() / 10.0,
            "savings_pct": (r.savings_pct * 10.0).round() / 10.0,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(out, serde_json::to_string_pretty(&payload)?)?;
    println!("\nFull results saved to {out}");
    Ok(())
}

#[derive(Debug, Clone)]
struct RepoResult {
    name: String,
    node_count: usize,
    osp_input_chars: usize,
    raw_input_chars: usize,
    osp_prompt_tokens: u64,
    raw_prompt_tokens: u64,
    osp_completion_tokens: u64,
    raw_completion_tokens: u64,
    ratio: f64,
    savings_pct: f64,
}

fn run_one(runtime: &Runtime, repo: &Path, name: &str) -> anyhow::Result<RepoResult> {
    let analysis = analyze_repo(repo)?;
    let (osp_user, raw_user, _source_files) = build_prompts(&analysis, repo)?;

    let osp_req = CompletionRequest {
        system: osp_system_prompt().to_string(),
        user: osp_user,
    };
    let raw_req = CompletionRequest {
        system: raw_system_prompt().to_string(),
        user: raw_user,
    };

    let osp = runtime.complete_raw(&osp_req)?;
    let raw = runtime.complete_raw(&raw_req)?;

    let ratio = raw.usage.prompt_tokens as f64 / osp.usage.prompt_tokens.max(1) as f64;
    let savings = 100.0 * (1.0 - osp.usage.prompt_tokens as f64 / raw.usage.prompt_tokens as f64);

    Ok(RepoResult {
        name: name.to_string(),
        node_count: analysis.space.node_count(),
        osp_input_chars: osp_req.input_chars(),
        raw_input_chars: raw_req.input_chars(),
        osp_prompt_tokens: osp.usage.prompt_tokens,
        raw_prompt_tokens: raw.usage.prompt_tokens,
        osp_completion_tokens: osp.usage.completion_tokens,
        raw_completion_tokens: raw.usage.completion_tokens,
        ratio,
        savings_pct: savings,
    })
}

/// Build the OSP coordinate prompt and raw source dump from a real analysis.
/// Picks K modules by id (stable) and uses their REAL metrics for the OSP
/// prompt. Collects K source files for the raw dump.
fn build_prompts(
    analysis: &AnalysisResult,
    repo: &Path,
) -> anyhow::Result<(String, String, Vec<PathBuf>)> {
    // OSP prompt: K sample modules with real coordinates.
    let mut ids: Vec<_> = analysis.module_metrics.keys().copied().collect();
    ids.sort_unstable();
    let sample_ids: Vec<_> = ids.into_iter().take(K).collect();

    // Resolve node mass (LOC) from space for context.
    let mut osp_nodes_json = Vec::with_capacity(sample_ids.len());
    for (i, id) in sample_ids.iter().enumerate() {
        let m = &analysis.module_metrics[id];
        let node = analysis.space.nodes.get(id);
        let mass = node.map(|n| n.mass).unwrap_or(0.0);
        let (x, y, z) = (m.coupling.value, m.cohesion.value, m.instability.value);
        osp_nodes_json.push(
            serde_json::json!({
                "id": id,
                "kind": "Module",
                "mass": (mass as u64),
                "position": {"x": round3(x), "y": round3(y), "z": round3(z), "w": 0.55, "v": 0.60}
            })
            .to_string(),
        );
        let _ = i;
    }

    // Edges among sample nodes only (compact).
    let sample_set: std::collections::HashSet<_> = sample_ids.iter().copied().collect();
    let mut osp_edges_json = Vec::new();
    for e in &analysis.space.edges {
        if e.kind != EdgeKind::Imports {
            continue;
        }
        if sample_set.contains(&e.from) && sample_set.contains(&e.to) {
            osp_edges_json.push(
                serde_json::json!({"from": e.from, "to": e.to, "kind": "Imports"}).to_string(),
            );
        }
    }

    let osp_user = format!(
        "OspPrompt:\n{{\n  \"space_slice\": {{\n    \"nodes\": [\n      {}\n    ],\n    \"edges\": [\n      {}\n    ]\n  }},\n  \"vision\": {{\"x\": 0.30, \"y\": 0.70, \"z\": 0.50, \"w\": 0.60, \"v\": 0.70}},\n  \"rules\": [\"no_self_import\", \"no_duplicate_node\"],\n  \"intent\": \"Refactor the highest-coupling module to reduce x toward vision\",\n  \"output_contract\": \"Respond with DeltaProposal JSON: new_nodes, new_edges, reasoning\"\n}}\n\nProduce a DeltaProposal for this intent.",
        osp_nodes_json.join(",\n      "),
        osp_edges_json.join(",\n      "),
    );

    // Raw dump: collect K source files from the repo (sorted).
    let mut files = collect_source_files(repo)?;
    files.sort();
    files.truncate(K);
    let snippets: Vec<(&str, String)> = files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_str()?.to_string();
            let body = std::fs::read_to_string(f).ok()?;
            let capped = if body.len() > RAW_FILE_CAP {
                format!("{}…", &body[..RAW_FILE_CAP])
            } else {
                body
            };
            // Leak the name into 'static for the snippet tuple — short-lived CLI.
            Some((Box::leak(name.into_boxed_str()) as &str, capped))
        })
        .collect();
    let snippet_refs: Vec<(&str, &str)> = snippets.iter().map(|(n, b)| (*n, b.as_str())).collect();
    let raw_user = raw_dump_user_prompt(
        &snippet_refs,
        "Refactor the highest-coupling module to reduce its coupling.",
    );

    Ok((osp_user, raw_user, files))
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Recursive source-file collector (mirrors osp-analyzer walk but standalone).
fn collect_source_files(repo: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(repo, &mut out);
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "node_modules"
                        | "target"
                        | "__pycache__"
                        | "venv"
                        | ".venu"
                        | "env"
                        | "build"
                        | "dist"
                        | "site-packages"
                        | "vendor"
                        | ".git"
                )
            {
                continue;
            }
            walk(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "py" | "ts" | "tsx" | "js" | "jsx" | "rs" | "go") {
                out.push(path);
            }
        }
    }
}

fn discover_repos(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(root)?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

fn print_summary(results: &[RepoResult]) {
    if results.is_empty() {
        println!("\n(no successful runs)");
        return;
    }
    let n = results.len() as f64;
    let mean = |f: &dyn Fn(&RepoResult) -> f64| results.iter().map(f).sum::<f64>() / n;
    let mut ratios: Vec<f64> = results.iter().map(|r| r.ratio).collect();
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = ratios[ratios.len() / 2];

    let savings_fn = |r: &RepoResult| r.savings_pct;
    let mut s_all: Vec<f64> = results.iter().map(savings_fn).collect();
    s_all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let s_median = s_all[s_all.len() / 2];

    println!(
        "\n**Summary (n={n}):** ratio mean={:.2}x median={:.2}x | savings mean={:.1}% median={:.1}% | range {:.1}x–{:.1}x",
        mean(&|r| r.ratio),
        median,
        mean(&savings_fn),
        s_median,
        ratios.first().unwrap(),
        ratios.last().unwrap(),
    );
}
