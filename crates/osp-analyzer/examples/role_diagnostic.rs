//! Classifier diagnostic — role + classification dağılımını döker.
//!
//! #1 classifier diagnostic görevi için: "Support=2829 gerçek mi yoksa classifier
//! mı geniş davranıyor?" sorusunu cevaplar. Production pipeline'ını çağırır
//! (classify_path + infer_role), sonra:
//!   - NodeRole dağılımı (TypeSurface/Core/Adapter/Utility/Runtime/Support)
//!   - NodeClassification dağılımı (Production/Test/Fixture/.../Support inherit kaynakları)
//!   - Support'e düşen dosyaların classification kırılımı + örnek path'ler
//!
//! Usage: cargo run --release --example role_diagnostic -- <repo-path> [--sample N]

use std::collections::HashMap;
use std::path::Path;

use osp_analyzer::pipeline::analyze_repo;
use osp_core::space::{NodeClassification, NodeRole};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: role_diagnostic <repo-path> [--sample N]");
        std::process::exit(2);
    }
    let repo = Path::new(&args[1]);
    let sample_n: usize = args
        .iter()
        .position(|a| a == "--sample")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);

    let name = repo
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".to_string());

    let result = analyze_repo(repo)?;
    let total = result.space.nodes.len();
    println!(
        "=== Role/Classification diagnostic: {} ({} nodes) ===\n",
        name, total
    );

    // Role dağılımı
    let mut role_counts: HashMap<NodeRole, usize> = HashMap::new();
    // Classification dağılımı
    let mut cls_counts: HashMap<NodeClassification, usize> = HashMap::new();
    // Support'e düşenlerin classification kırılımı
    let mut support_by_cls: HashMap<NodeClassification, usize> = HashMap::new();
    // Her (role, classification) için örnek path'ler
    let mut examples: HashMap<(NodeRole, NodeClassification), Vec<String>> = HashMap::new();

    for (id, node) in &result.space.nodes {
        *role_counts.entry(node.role).or_insert(0) += 1;
        *cls_counts.entry(node.classification).or_insert(0) += 1;
        if node.role == NodeRole::Support {
            *support_by_cls.entry(node.classification).or_insert(0) += 1;
        }
        let path = result.node_paths.get(id).cloned().unwrap_or_default();
        examples
            .entry((node.role, node.classification))
            .or_default()
            .push(path);
    }

    println!("── NodeRole distribution ──");
    for role in [
        NodeRole::TypeSurface,
        NodeRole::Core,
        NodeRole::Adapter,
        NodeRole::Utility,
        NodeRole::Runtime,
        NodeRole::Support,
    ] {
        let c = *role_counts.get(&role).unwrap_or(&0);
        let pct = pct(c, total);
        println!("  {:<12} {:>5}  ({:>5}%)", format!("{:?}", role), c, pct);
    }

    println!("\n── NodeClassification distribution ──");
    for cls in [
        NodeClassification::Production,
        NodeClassification::Test,
        NodeClassification::Fixture,
        NodeClassification::Migration,
        NodeClassification::Config,
        NodeClassification::Script,
        NodeClassification::Generated,
        NodeClassification::Documentation,
        NodeClassification::Unknown,
    ] {
        let c = *cls_counts.get(&cls).unwrap_or(&0);
        let pct = pct(c, total);
        println!("  {:<14} {:>5}  ({:>5}%)", format!("{:?}", cls), c, pct);
    }

    let support_total = *role_counts.get(&NodeRole::Support).unwrap_or(&0);
    println!("\n── Support role breakdown (which classifications feed it) ──");
    println!(
        "  Support total: {} ({}%)",
        support_total,
        pct(support_total, total)
    );
    if support_total > 0 {
        for (cls, c) in support_by_cls.iter() {
            println!(
                "    via {:<14} {:>5}  ({}% of Support, {}% of repo)",
                format!("{:?}", cls),
                c,
                pct(*c, support_total),
                pct(*c, total),
            );
        }
    }

    println!(
        "\n── Example paths per (role, classification) ──  (up to {} each)",
        sample_n
    );
    for ((role, cls), mut paths) in examples {
        // path'e göre sırala — grupları görmek için
        paths.sort();
        println!("\n  [{:?} / {:?}] ({} paths)", role, cls, paths.len());
        for p in paths.iter().take(sample_n) {
            println!("    {}", p);
        }
        if paths.len() > sample_n {
            println!("    ... ({} more)", paths.len() - sample_n);
        }
    }

    // Diagnostik özet: Support oranı > %50 ise uyarı
    let support_pct = pct(support_total, total);
    println!("\n── Diagnostic verdict ──");
    // Önemli: "Support yüksek" tek başına classifier hatası DEĞİLdir. Support oranı
    // çoğunlukla Test dosyalarından gelir ve bu repo'nun gerçek test/production oranını
    // dürüstçe yansıtır (date-fns %2, svelte %87 — her ikisi de geçerli repo karakteri).
    // Classifier kalitesini ölçen asıl metrik: Support'un ne kadarı gerçek Test'ten geliyor?
    let support_via_test = *support_by_cls.get(&NodeClassification::Test).unwrap_or(&0);
    let support_via_test_of_repo = pct(support_via_test, total);
    let support_test_ratio = support_via_test as f64 / support_total.max(1) as f64;

    println!("\n── Diagnostic verdict ──");
    println!(
        "  Support = {:.1}% ({} / {})",
        support_pct, support_total, total
    );
    println!(
        "    of which Test-origin: {:.1}% of repo ({} files, {:.0}% of Support)",
        support_via_test_of_repo,
        support_via_test,
        support_test_ratio * 100.0
    );
    if support_test_ratio > 0.9 {
        println!("  ✓ Support çoğunlukla gerçek Test dosyaları — classifier geniş değil,");
        println!(
            "    repo test-heavy. advisory/vision-degrade mantığı burada asıl değerini verir."
        );
    } else {
        println!("  ◐ Support'a Test dışı (fixture/migration/script/config) karışımı yüksek —");
        println!("    path-pattern'leri bu repo için gözden geçir.");
    }

    Ok(())
}

fn pct(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}
