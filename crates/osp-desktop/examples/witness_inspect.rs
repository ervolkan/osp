//! Görsel-doğrulama yardımcı: svelte üzerinde cmd_analyze_repo çağırır,
//! birkaç tanıdık node'un NodeJson witness alanlarını yazdırır (backend seviyesi).
//!
//! cargo run --example witness_inspect --package osp-desktop
//! (görsel UI doğrulaması için http_server_only.rs + Playwright kullanılır)

use osp_desktop::cmd_analyze_repo;

fn main() {
    let repo = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "P:/repos/osp-spike/svelte".to_string());
    println!("Analyzing {} ...", repo);
    let result = cmd_analyze_repo(&repo, None).expect("analyze failed");

    let total = result.space.nodes.len();
    let with_witness = result
        .space
        .nodes
        .iter()
        .filter(|n| n.witness_commits.is_some())
        .count();
    println!(
        "\n=== NodeJson witness coverage: {}/{} nodes have witness data ===",
        with_witness, total
    );

    if with_witness == 0 {
        println!("⚠ No witness data — git not detected or no tracked source files.");
        return;
    }

    // Tanıdık svelte dosyalarını ara
    let targets = ["runtime.js", "CHANGELOG.md", "compiler.js", "index.ts", "package.json"];
    println!("\n--- Known files (witness + risk context) ---");
    for n in &result.space.nodes {
        let Some(path) = &n.path else { continue };
        if !targets.iter().any(|t| path.ends_with(t)) {
            continue;
        }
        let w_commits = n.witness_commits.unwrap_or(0);
        let w_authors = n.witness_authors.unwrap_or(0);
        let w_churn = n.witness_churn.unwrap_or(0);
        let w_own = n.witness_ownership.unwrap_or(0.0);
        let w_days = n.witness_last_modified_days.unwrap_or(0);
        let battle = w_commits >= 20 && w_days >= 90 && w_own < 0.6;
        let spec = w_commits <= 2;
        let label = if battle {
            "battle-tested"
        } else if spec {
            "speculative"
        } else {
            "established"
        };
        println!(
            "  {:>6} commits · {} authors · {:>6} churn · {:.0}% solo · {}d ago · [{}] · {}",
            w_commits, w_authors, w_churn, w_own * 100.0, w_days, label, path
        );
    }

    let mut buckets = [0usize; 5];
    for n in &result.space.nodes {
        let Some(c) = n.witness_commits else {
            continue;
        };
        let i = if c <= 1 {
            0
        } else if c <= 5 {
            1
        } else if c <= 20 {
            2
        } else if c <= 100 {
            3
        } else {
            4
        };
        buckets[i] += 1;
    }
    println!("\n--- Commits distribution (source nodes with witness) ---");
    println!("  1 commit    : {}", buckets[0]);
    println!("  2-5 commits : {}", buckets[1]);
    println!("  6-20        : {}", buckets[2]);
    println!("  21-100      : {}", buckets[3]);
    println!("  100+        : {}", buckets[4]);
}
