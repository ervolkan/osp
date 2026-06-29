//! Vision breakdown diagnostic — #7 TypeSurface profile hardening doğrulaması.
//!
//! #1 fix sonrası TypeSurface reject'lerinin ne olduğunu ölçer. Review'in #5
//! "TypeSurface 8 reject hâlâ var, profile rafine edilmeli" şüphesi — bu şüphe
//! #1 bug'ından (tests/types/* → TypeSurface) kaynaklanıyordu. Fix sonrası
//! TypeSurface artık sadece production declaration dosyaları; reject'ler çok
//! azalmış olmalı.
//!
//! Frontend'in updateVisionPreview + evaluateNodeVerdict mantığının backend
//! mirror'ı. Her node için role-aware θ + VisionVerdict hesaplar, sonra:
//!   - verdict dağılımı (pass/warning/advisory/reject/inconclusive)
//!   - role × verdict kırılımı
//!   - axis × reject kırılımı
//!   - TypeSurface reject'lerinin örnek path'leri (eğer varsa)
//!
//! Usage: cargo run --release --example vision_breakdown -- <repo-path>

use std::collections::HashMap;
use std::path::Path;

use osp_analyzer::pipeline::analyze_repo;
use osp_core::space::NodeRole;

/// Backend builtin_role_override mirror (vision_config.rs ile birebir aynı).
fn builtin_role_vision(role: NodeRole) -> Option<(f64, f64, f64)> {
    match role {
        NodeRole::TypeSurface => Some((0.05, 0.80, 0.50)), // type-only ayrımı yapıldı → 0.05 geri
        NodeRole::Core => Some((0.60, 0.75, 0.20)),
        NodeRole::Adapter => Some((0.80, 0.50, 0.80)),
        NodeRole::Utility => Some((0.20, 0.60, 0.50)),
        NodeRole::Runtime => Some((0.40, 0.60, 0.35)),
        NodeRole::Support => None,
    }
}

/// Frontend evaluateNodeVerdict mirror — backend evaluate_node_vision ile aynı.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Verdict {
    Pass,
    Warning,
    Advisory,
    Reject,
    Inconclusive,
}

fn evaluate(theta: f64, theta_bound: f64, theta_warn: f64, is_support: bool, inconclusive: bool) -> Verdict {
    if inconclusive {
        return Verdict::Inconclusive;
    }
    if theta <= theta_bound {
        return Verdict::Pass;
    }
    if theta <= theta_warn {
        return Verdict::Warning;
    }
    if is_support {
        Verdict::Advisory
    } else {
        Verdict::Reject
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: vision_breakdown <repo-path> [--bound 0.30]");
        std::process::exit(2);
    }
    let repo = Path::new(&args[1]);
    let theta_bound: f64 = args
        .iter()
        .position(|a| a == "--bound")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.30);
    let theta_warn = theta_bound + 0.10;

    let name = repo
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".to_string());

    let result = analyze_repo(repo)?;
    let total = result.space.nodes.len();
    println!("=== Vision breakdown: {} ({} nodes, θ_bound={}, θ_warn={}) ===\n", name, total, theta_bound, theta_warn);

    // Global vision (frontend slider default ≈ nötr 0.5; builtin override kazanır)
    let gx = 0.5_f64;
    let gy = 0.5;
    let gz = 0.5;

    let mut verdict_counts: HashMap<Verdict, usize> = HashMap::new();
    let mut role_verdict: HashMap<(NodeRole, Verdict), Vec<u64>> = HashMap::new();
    let mut axis_reject: HashMap<&str, Vec<u64>> = HashMap::from([
        ("coupling", vec![]),
        ("cohesion", vec![]),
        ("instability", vec![]),
    ]);
    let mut typesurface_rejects: Vec<(u64, String, f64, f64, f64, f64)> = vec![];
    // Calibration teşhisi: Adapter reject örnekleri + ilk 5 node'un RAW değerleri
    let mut adapter_rejects: Vec<(u64, String, f64, f64, f64, f64)> = vec![];
    let mut raw_samples: Vec<(u64, String, f64, Option<f64>, f64)> = vec![];
    // Calibration teşhisi (#1): per-role gerçek eksen dağılımı (mean) + θ distribution.
    // "0 pass" kök nedenini ayırmak için: vision override'lar mı yanlış, yoksa θ_bound mu
    // çok sert? Per-role mean ile builtin override karşılaştır → gap göster.
    let mut role_axis_values: HashMap<NodeRole, Vec<(f64, f64, f64)>> = HashMap::new();
    // θ distribution — percentil/treshold'a kaç node düşüyor?
    let mut all_thetas: Vec<f64> = vec![];

    for (id, node) in &result.space.nodes {
        let role = node.role;
        let (rvx, rvy, rvz) = builtin_role_vision(role).unwrap_or((gx, gy, gz));
        // DÜZELTME: coupling/instability node.position.raw'da DEĞİL, module_metrics'te.
        // Backend (desktop lib.rs) ve frontend doğru yerden okuyor; bu example önce
        // yanlış yerden (position.raw.x, hep 0.00) okuyordu → sahte "0 pass" bulgusu.
        let metrics = result.module_metrics.get(id);
        let coupling = metrics.map(|m| m.coupling.value).unwrap_or(0.0);
        let instability = metrics.map(|m| m.instability.value).unwrap_or(0.0);
        let cohesion = node.cohesion.unwrap_or(0.5);
        let dx = coupling - rvx;
        let dy = cohesion - rvy;
        let dz = instability - rvz;
        let theta = (dx * dx + dy * dy + dz * dz).sqrt() / 3.0_f64.sqrt();

        // Per-role gerçek değerleri topla (calibration için)
        role_axis_values.entry(role).or_default().push((coupling, cohesion, instability));
        all_thetas.push(theta);

        // cohesion placeholder → inconclusive (frontend ile aynı)
        let inconclusive = node.cohesion.is_none();
        let is_support = role == NodeRole::Support;
        let v = evaluate(theta, theta_bound, theta_warn, is_support, inconclusive);

        *verdict_counts.entry(v).or_insert(0) += 1;
        role_verdict.entry((role, v)).or_default().push(*id);

        if v == Verdict::Reject {
            let abs_dx = dx.abs();
            let abs_dy = dy.abs();
            let abs_dz = dz.abs();
            let worst = if abs_dx >= abs_dy && abs_dx >= abs_dz {
                "coupling"
            } else if abs_dy >= abs_dz {
                "cohesion"
            } else {
                "instability"
            };
            axis_reject.entry(worst).or_default().push(*id);
            if role == NodeRole::TypeSurface {
                let path = result.node_paths.get(id).cloned().unwrap_or_default();
                typesurface_rejects.push((*id, path, theta, dx, dy, dz));
            }
            if role == NodeRole::Adapter {
                let path = result.node_paths.get(id).cloned().unwrap_or_default();
                adapter_rejects.push((*id, path, theta, dx, dy, dz));
            }
        }
        // İlk 5 node'un RAW değerleri — metrikler gerçekten üretiliyor mu?
        if raw_samples.len() < 5 {
            let path = result.node_paths.get(id).cloned().unwrap_or_default();
            raw_samples.push((*id, path, coupling, node.cohesion, instability));
        }
    }

    println!("── Verdict distribution ──");
    for v in [Verdict::Pass, Verdict::Warning, Verdict::Advisory, Verdict::Reject, Verdict::Inconclusive] {
        let c = *verdict_counts.get(&v).or(Some(&0)).unwrap();
        let pct = pct(c, total);
        let label = match v {
            Verdict::Pass => "pass",
            Verdict::Warning => "warning",
            Verdict::Advisory => "advisory",
            Verdict::Reject => "reject",
            Verdict::Inconclusive => "inconclusive",
        };
        println!("  {:<14} {:>5}  ({:>5}%)", label, c, pct);
    }

    println!("\n── Role × verdict breakdown ──");
    let roles = [
        NodeRole::TypeSurface,
        NodeRole::Core,
        NodeRole::Adapter,
        NodeRole::Utility,
        NodeRole::Runtime,
        NodeRole::Support,
    ];
    let verdicts = [Verdict::Pass, Verdict::Warning, Verdict::Advisory, Verdict::Reject, Verdict::Inconclusive];
    print!("  {:<12}", "");
    for v in verdicts {
        let s = match v {
            Verdict::Pass => "pass",
            Verdict::Warning => "warn",
            Verdict::Advisory => "adv",
            Verdict::Reject => "rej",
            Verdict::Inconclusive => "inc",
        };
        print!(" {:>6}", s);
    }
    println!();
    for role in roles {
        print!("  {:<12}", format!("{:?}", role));
        for v in verdicts {
            let c = role_verdict.get(&(role, v)).map(|x| x.len()).unwrap_or(0);
            print!(" {:>6}", c);
        }
        println!();
    }

    println!("\n── Reject by worst axis ──");
    for axis in ["coupling", "cohesion", "instability"] {
        let c = axis_reject.get(axis).map(|x| x.len()).unwrap_or(0);
        println!("  {:<12} {}", axis, c);
    }

    // #7 ana çıktı: TypeSurface reject'leri var mı?
    println!("\n── #7 TypeSurface reject diagnosis ──");
    if typesurface_rejects.is_empty() {
        println!("  ✓ TypeSurface reject YOK — #1 fix (tests/types/* → Support) işe yaradı.");
        println!("    Review'in 'TypeSurface 8 reject' şüphesi çürütüldü: o 8 node tests/types/*");
        println!("    dosyalarıydı, artık Support (advisory) olarak değerlendiriliyor.");
    } else {
        println!("  ⚠ {} TypeSurface reject var — production declaration dosyaları threshold'a takılıyor.", typesurface_rejects.len());
        println!("    Bu gerçek reject'ler (test değil). Profile rafine etmek için detaylar:");
        for (id, path, theta, dx, dy, dz) in typesurface_rejects.iter().take(15) {
            println!("    Node {}: θ={:.3} Δx={:+.2} Δy={:+.2} Δz={:+.2} {}", id, theta, dx, dy, dz, path);
        }
        if typesurface_rejects.len() > 15 {
            println!("    ... ({} more)", typesurface_rejects.len() - 15);
        }
    }

    // ── RAW metric dump: metrikler gerçekten üretiliyor mu? ────────────────
    // Calibration'dan ÖNCE bu kontrol şart — eğer coupling/instability hep 0.00 ise
    // vision calibration değil, analyzer pipeline hatası söz konusu.
    println!("\n── RAW metric samples (first 5 nodes) ──");
    println!("  (eğer coupling/instability hep 0.00 ise analyzer metrik üretmiyor demektir)");
    for (id, path, coupling, cohesion, instability) in &raw_samples {
        let coh_str = match cohesion {
            Some(c) => format!("{:.3}", c),
            None => "None (placeholder→0.5)".to_string(),
        };
        println!("  Node {:<4} coupling={:.2} cohesion={} instability={:.2}  {}", id, coupling, coh_str, instability, path);
    }

    // ── Calibration teşhisi (#1): "0 pass" kök nedeni ──────────────────────
    // Per-role GERÇEK eksen dağılımı (mean) vs builtin override target.
    // Gap gösterir: override ile gerçek değer ne kadar uyuşuyor?
    println!("\n── #1 Calibration: per-role real distribution vs builtin target ──");
    println!("  (gap = real mean − builtin target; |gap| > 0.15 → override yanlış kalibre)");
    for role in roles {
        let values = match role_axis_values.get(&role) {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let n = values.len();
        let mean_x: f64 = values.iter().map(|(x, _, _)| *x).sum::<f64>() / n as f64;
        let mean_y: f64 = values.iter().map(|(_, y, _)| *y).sum::<f64>() / n as f64;
        let mean_z: f64 = values.iter().map(|(_, _, z)| *z).sum::<f64>() / n as f64;
        let (tx, ty, tz) = builtin_role_vision(role).unwrap_or((gx, gy, gz));
        let gap_x = mean_x - tx;
        let gap_y = mean_y - ty;
        let gap_z = mean_z - tz;
        let mark = |g: f64| if g.abs() > 0.15 { "⚠" } else { " " };
        println!(
            "  {:<12} n={:>4}  real x={:.2} y={:.2} z={:.2}  target x={:.2} y={:.2} z={:.2}  gap {m1}Δx={:+.2} {m2}Δy={:+.2} {m3}Δz={:+.2}",
            format!("{:?}", role), n, mean_x, mean_y, mean_z, tx, ty, tz,
            gap_x, gap_y, gap_z,
            m1 = mark(gap_x), m2 = mark(gap_y), m3 = mark(gap_z),
        );
    }

    // θ distribution — kaç node hangi eşiğe düşer?
    println!("\n── #1 Calibration: θ distribution (how many nodes pass at each bound) ──");
    let mut sorted = all_thetas.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    for bound in [0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.50, 0.60] {
        let pass_count = sorted.iter().filter(|&&t| t <= bound).count();
        let pct = pct(pass_count, total);
        let bar = "█".repeat((pct / 2.0) as usize);
        let marker = if (bound - theta_bound).abs() < 1e-9 { "  ← current θ_bound" } else { "" };
        println!("  θ ≤ {:.2}: {:>5} / {} ({:>5}%) {}{}", bound, pass_count, total, pct, bar, marker);
    }
    // Percentil tabanlı öneri: eğer θ_bound=0.30 → %0 pass ise, %50'yi yakalayan
    // θ yaklaşık nedir? (median θ). Bu, kalibre edilmiş eşiğin referansı.
    if !sorted.is_empty() {
        let median = sorted[sorted.len() / 2];
        let p25 = sorted[sorted.len() / 4];
        let p75 = sorted[3 * sorted.len() / 4];
        println!("\n  θ percentiles: p25={:.3}  median(p50)={:.3}  p75={:.3}", p25, median, p75);
        println!("  → mevcut θ_bound=0.30 iken {} pass.", sorted.iter().filter(|&&t| t <= 0.30).count());
        println!("    median θ={:.3}, yani node'ların yarısı bu değerden düşük. Bu skala referans.", median);
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
