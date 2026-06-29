//! osp-spike ↔ osp-core köprüsü (Faz 1.6).
//!
//! Faz 0 aggregate verisini osp-core tiplerine çevirir. İki ana fonksiyon:
//! - [`spike_graph_to_space`]: `DepGraph` → osp-core `Space` (generic graf)
//! - [`spike_witness_classify`]: `WitnessProfile` → tri-state `WitnessStatus`
//!
//! **Önemli:** Faz 0 verisi **aggregate**'tir (count'lar), actor-bazlı evidence DEĞİL.
//! Bu yüzden gerçek per-actor `WitnessSet` üretmek yerine direkt tri-state sınıflarız.
//! Gerçek per-actor `EvidenceEvent`'ler Faz 1.10'da `osp_core::witness` (git2-rs trailer
//! parsing) ile gelir — bu köprü yalnızca Faz 0 aggregate → osp-core tri-state çevirir.
//!
//! **Squash/rebase kör-noktası çözümü:** `spike_witness_classify` üç senaryoyu ayırt eder:
//! - `Witnessed`: yeterli merge-commit sinyali (merge_ratio ≥ %10)
//! - `Unwitnessed`: solo/foam (≤1 author + düşük merge) — gerçek şahitsizlik
//! - `UnobservableLocally`: çok-author + düşük merge → squash/rebase saklıyor

use crate::model::{DepGraph, EdgeKind, NodeKind, RepoAnalysis, WitnessProfile};
use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
use osp_core::coords::{CoordinateSystem, RawPosition};
use osp_core::space::{Edge, EdgeKind as CoreEdgeKind, Node, NodeId, Space};
use osp_core::vision::{compute_derived, CosineDeviation, VisionVector};
use osp_core::witness::{classify_status, WitnessStatus};
use serde::Serialize;

/// osp-spike `DepGraph` → osp-core `Space`.
///
/// Faz 0 dosya-bazlı (`Module`) granularity. osp-spike `NodeKind::{Class, Function}`
/// osp-core `Module`'a maplenir (Faz 0 seviyesi). Faz 3'te (SCIP) Class/Function
/// ayrımı gelir. `mass` (LOC) korunur.
pub fn spike_graph_to_space(g: &DepGraph) -> Space {
    let mut space = Space::new();
    for n in &g.nodes {
        space.insert_node(Node {
            id: n.id as NodeId,
            kind: spike_node_kind_to_core(n.kind),
            mass: n.mass as f64,
            ..Default::default()
        });
    }
    for e in &g.edges {
        space.insert_edge(Edge {
            from: e.from as NodeId,
            to: e.to as NodeId,
            kind: spike_edge_kind_to_core(e.kind),
            ..Default::default() // is_type_only: false — spike katmanı type-only bilmez
        });
    }
    space
}

fn spike_node_kind_to_core(_k: NodeKind) -> osp_core::space::NodeKind {
    // Faz 0: tüm düğümler dosya-bazlı Module. Class/Function ayrımı Faz 3'te (SCIP).
    osp_core::space::NodeKind::Module
}

fn spike_edge_kind_to_core(k: EdgeKind) -> CoreEdgeKind {
    match k {
        EdgeKind::Imports => CoreEdgeKind::Imports,
        EdgeKind::Calls => CoreEdgeKind::Calls,
    }
}

/// osp-spike `WitnessProfile` → tri-state `WitnessStatus` (inv #3).
///
/// **Faz 0 squash/rebase kör-noktasını çözen ana fonksiyon** (`spike-results.md §2`).
///
/// # Senaryolar
/// - **`Witnessed`**: `merge_ratio ≥ MERGE_RATIO_OBSERVABLE` (%10) ve support ≥ quorum.
///   Yeterli merge-commit sinyali var → yerel olarak gözlemlenebilir.
/// - **`Unwitnessed`**: `distinct_authors ≤ 1` VE düşük merge. Solo/foam — gerçek
///   şahitsizlik (ör. worms-supabase).
/// - **`UnobservableLocally`**: `distinct_authors > 1` AMA düşük merge. Çok-author
///   collaboration var AMA squash/rebase review metadata'yı saklıyor (ör. fastapi).
///
/// # Kalibrasyon
/// `MERGE_RATIO_OBSERVABLE` (%10) ve author eşiği Faz 1.11 kalibrasyon korpusunda
/// (15-20 repo) tune edilir.
pub fn spike_witness_classify(w: &WitnessProfile) -> WitnessStatus {
    const MERGE_RATIO_OBSERVABLE: f64 = 0.10; // %10 merge-commit → gözlemlenebilir

    let merge_ratio = if w.total_commits > 0 {
        w.merge_commits as f64 / w.total_commits as f64
    } else {
        0.0
    };

    if merge_ratio >= MERGE_RATIO_OBSERVABLE {
        // Yeterli merge-commit sinyali → lokalde gözlemlenebilir.
        // Her merge-commit 1.0 ağırlık (MergeCommit, §4.1).
        let support = w.merge_commits as f64;
        classify_status(support, 1.5, /* observable = */ true)
    } else if w.distinct_authors <= 1 {
        // Solo + düşük merge → gerçekten şahitsiz (foam).
        WitnessStatus::Unwitnessed
    } else {
        // Çok-author + düşük merge → squash/rebase review'yi saklıyor.
        WitnessStatus::UnobservableLocally
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 1.10 — Full osp-core pipeline (bridge + coordinates + vision + tri-state)
// ═══════════════════════════════════════════════════════════════════════════════

/// Faz 1.10 pipeline çıktısı — 5 gerçek repo'da osp-core entegrasyon doğrulaması.
///
/// `V2Report` plain f64/String alanlar kullanır (osp-core tiplerini serde'siz serileştirilebilir
/// tutmak için). Bridge, osp-core tiplerini bu alanlara dönüştürür.
#[derive(Debug, Clone, Serialize)]
pub struct V2Report {
    pub repo: String,
    // Faz 0 raw
    pub total_commits: usize,
    pub merge_commits: usize,
    pub merge_ratio: f64,
    pub distinct_authors: usize,
    // Tri-state (Faz 1.6 bridge)
    pub witness_status: String,
    // Graph bridged (Faz 1.6)
    pub nodes: usize,
    pub edges: usize,
    pub coupling_density: f64,
    // Sample node (highest-mass) positions — Faz 1.9/1.7
    pub sample_node_id: u64,
    pub sample_mass: f64,
    pub raw_x: f64,
    pub raw_y: f64,
    pub raw_z: f64,
    pub raw_w: f64,
    pub raw_v: f64,
    pub derived_u: f64,
    pub derived_theta: f64,
    pub derived_d: f64,
    // Declared vision (sample)
    pub vision_x: f64,
    pub vision_y: f64,
    pub vision_z: f64,
    pub vision_w: f64,
    pub vision_v: f64,
}

/// Örnek vizyon — "balanced ideal" (Faz 2 Space Engine elle-deklare eder).
const SAMPLE_VISION: RawPosition = RawPosition {
    x: 0.4, // moderate coupling
    y: 0.7, // cohesive
    z: 0.5, // balanced instability
    w: 0.5, // moderate entropy
    v: 0.5, // moderate witness-depth
};

/// Faz 0 `RepoAnalysis` → Faz 1.10 `V2Report`.
///
/// Full pipeline: `spike_graph_to_space` + `spike_witness_classify` (tri-state) +
/// `CoordinateSystem::default_raw_five` + `raw_position_of` + `compute_derived`.
///
/// **CohesionAxis (y)** placeholder `0.5` (LCOM4 için tree-sitter class/field analizi
/// gerekir — Faz 3 SCIP). Diğer 4 eksen gerçek veriden.
/// **Abstractness (A)** `0.5` placeholder (sınıf sayımı Faz 3). `D = |A + I − 1|` buna göre.
pub fn run_v2_pipeline(analysis: &RepoAnalysis) -> V2Report {
    let repo_name = analysis
        .repo_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Bridge: graph → space
    let space = spike_graph_to_space(&analysis.graph);

    // Bridge: witness → tri-state
    let status = spike_witness_classify(&analysis.witness);
    let status_str = match status {
        WitnessStatus::Witnessed => "Witnessed",
        WitnessStatus::Unwitnessed => "Unwitnessed",
        WitnessStatus::UnobservableLocally => "UnobservableLocally",
    };

    let merge_ratio = if analysis.witness.total_commits > 0 {
        analysis.witness.merge_commits as f64 / analysis.witness.total_commits as f64
    } else {
        0.0
    };

    // Coordinate system — 5 raw eksen
    let cs = CoordinateSystem::default_raw_five(
        CohesionAxis::from_normalized(0.5), // placeholder (Faz 3 SCIP)
        EntropyAxis::from_commit_entropy(analysis.witness.commit_entropy),
        WitnessDepthAxis::from_witness(
            analysis.witness.witnessed_ratio,
            analysis.witness.distinct_authors,
        ),
    );

    // Sample node: highest-mass (LOC) — representatif "biggest module"
    let (sample_id, sample_mass, sample_raw) = space
        .nodes
        .iter()
        .max_by(|(_, a), (_, b)| {
            a.mass
                .partial_cmp(&b.mass)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, node)| {
            let raw = cs.raw_position_of(node, &space);
            (*id, node.mass, raw)
        })
        .unwrap_or((0, 0.0, RawPosition::default()));

    // Derived: raw + vision → DerivedPosition (u, θ, D)
    let vision = VisionVector::new(SAMPLE_VISION);
    let derived = compute_derived(
        &sample_raw,
        &vision,
        &space,
        &CosineDeviation,
        sample_raw.z, // instability I (from InstabilityAxis)
        0.5,          // abstractness A placeholder (Faz 3 SCIP)
    );

    V2Report {
        repo: repo_name,
        total_commits: analysis.witness.total_commits,
        merge_commits: analysis.witness.merge_commits,
        merge_ratio,
        distinct_authors: analysis.witness.distinct_authors,
        witness_status: status_str.into(),
        nodes: space.node_count(),
        edges: space.edge_count(),
        coupling_density: if space.node_count() > 0 {
            space.edge_count() as f64 / space.node_count() as f64
        } else {
            0.0
        },
        sample_node_id: sample_id,
        sample_mass,
        raw_x: sample_raw.x,
        raw_y: sample_raw.y,
        raw_z: sample_raw.z,
        raw_w: sample_raw.w,
        raw_v: sample_raw.v,
        derived_u: derived.u,
        derived_theta: derived.theta,
        derived_d: derived.main_sequence_distance,
        vision_x: SAMPLE_VISION.x,
        vision_y: SAMPLE_VISION.y,
        vision_z: SAMPLE_VISION.z,
        vision_w: SAMPLE_VISION.w,
        vision_v: SAMPLE_VISION.v,
    }
}

/// Faz 1.10 v2 karşılaştırma raporu (JSON + tablo).
pub fn print_v2_comparison(reports: &[V2Report]) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(reports)?;
    std::fs::write("spike-output-v2.json", &json)?;
    tracing::info!(path = "spike-output-v2.json", "v2 JSON rapor yazıldı");

    println!("\n=== OSP Spike v2 — osp-core pipeline + tri-state (Faz 1.10) ===\n");
    println!(
        "{:<16} {:<22} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
        "repo", "witness_status", "nodes", "edges", "κ", "x", "z", "w", "v", "θ", "D"
    );
    println!("{}", "-".repeat(102));
    for r in reports {
        println!(
            "{:<16} {:<22} {:>6} {:>5} {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>5.2}",
            r.repo,
            r.witness_status,
            r.nodes,
            r.edges,
            r.coupling_density,
            r.raw_x,
            r.raw_z,
            r.raw_w,
            r.raw_v,
            r.derived_theta,
            r.derived_d,
        );
    }
    println!();
    println!(
        "Sample node = highest-mass (LOC). Vision = (0.4, 0.7, 0.5, 0.5, 0.5) declared ideal."
    );
    println!(
        "y (cohesion) = 0.5 placeholder (LCOM4 → Faz 3 SCIP). A (abstractness) = 0.5 placeholder."
    );
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DepGraph, Edge, Node, WitnessProfile};

    fn profile(total: usize, merges: usize, authors: usize) -> WitnessProfile {
        WitnessProfile {
            total_commits: total,
            merge_commits: merges,
            distinct_authors: authors,
            ..Default::default()
        }
    }

    fn mod_node(id: u32, mass: usize) -> Node {
        Node {
            id,
            kind: NodeKind::Module,
            path: format!("file_{id}"),
            mass,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // spike_graph_to_space
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn graph_to_space_preserves_node_and_edge_counts() {
        let g = DepGraph {
            nodes: vec![mod_node(1, 10), mod_node(2, 20)],
            edges: vec![Edge {
                from: 1,
                to: 2,
                kind: EdgeKind::Imports,
            }],
        };
        let s = spike_graph_to_space(&g);
        assert_eq!(s.node_count(), 2);
        assert_eq!(s.edge_count(), 1);
    }

    #[test]
    fn graph_to_space_maps_imports_and_calls_edges() {
        let g = DepGraph {
            nodes: vec![mod_node(1, 1), mod_node(2, 1)],
            edges: vec![
                Edge {
                    from: 1,
                    to: 2,
                    kind: EdgeKind::Imports,
                },
                Edge {
                    from: 2,
                    to: 1,
                    kind: EdgeKind::Calls,
                },
            ],
        };
        let s = spike_graph_to_space(&g);
        assert_eq!(s.edge_count_of(CoreEdgeKind::Imports), 1);
        assert_eq!(s.edge_count_of(CoreEdgeKind::Calls), 1);
    }

    #[test]
    fn graph_to_space_preserves_mass() {
        let g = DepGraph {
            nodes: vec![mod_node(1, 42)],
            edges: vec![],
        };
        let s = spike_graph_to_space(&g);
        let n = s.nodes.get(&1).expect("id=1 mevcut");
        assert!((n.mass - 42.0).abs() < 1e-9);
    }

    #[test]
    fn graph_to_space_all_nodes_become_module() {
        // Faz 0: Class/Function → Module (dosya-bazlı granularity)
        let g = DepGraph {
            nodes: vec![Node {
                id: 1,
                kind: NodeKind::Class, // spike tarafında Class
                path: "x".into(),
                mass: 5,
            }],
            edges: vec![],
        };
        let s = spike_graph_to_space(&g);
        let n = s.nodes.get(&1).unwrap();
        assert_eq!(n.kind, osp_core::space::NodeKind::Module);
    }

    #[test]
    fn graph_to_space_empty_depgraph() {
        let g = DepGraph::default();
        let s = spike_graph_to_space(&g);
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.edge_count(), 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // spike_witness_classify (tri-state) — Faz 0 squash kör-noktası çözümü
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn classify_high_merge_ratio_witnessed() {
        // click-benzeri: %35 merge → Witnessed
        let w = profile(3242, 1141, 30);
        assert_eq!(spike_witness_classify(&w), WitnessStatus::Witnessed);
    }

    #[test]
    fn classify_solo_low_merge_unwitnessed() {
        // worms-benzeri: 0 merge, 1 author → Unwitnessed (foam, genuinely)
        let w = profile(50, 0, 1);
        assert_eq!(spike_witness_classify(&w), WitnessStatus::Unwitnessed);
    }

    #[test]
    fn classify_multi_author_low_merge_unobservable_locally() {
        // fastapi-benzeri: %0.18 merge, çok author → squash saklıyor
        let w = profile(7336, 13, 100);
        assert_eq!(
            spike_witness_classify(&w),
            WitnessStatus::UnobservableLocally
        );
    }

    #[test]
    fn classify_5_repos_matches_faz0_reality() {
        // spike-results.md §1 Faz 0 verisiyle ARI tri-state doğrulaması.
        // Bu, squash kör-noktasının çözüldüğünün kanıtıdır.
        let cases = &[
            // (name, total_commits, merge_commits, distinct_authors, expected)
            (
                "worms-supabase",
                50_usize,
                0_usize,
                1_usize,
                WitnessStatus::Unwitnessed,
            ),
            ("click", 3242, 1141, 30, WitnessStatus::Witnessed),
            ("fastapi", 7336, 13, 100, WitnessStatus::UnobservableLocally),
            (
                "django",
                34704,
                591,
                100,
                WitnessStatus::UnobservableLocally,
            ),
            (
                "date-fns",
                2588,
                152,
                50,
                WitnessStatus::UnobservableLocally,
            ),
        ];
        for (name, total, merges, authors, expected) in cases {
            let w = profile(*total, *merges, *authors);
            assert_eq!(
                spike_witness_classify(&w),
                *expected,
                "{name} için tri-state uyumsuz — Faz 0 squash kör-noktası çözülmemiş olabilir"
            );
        }
    }

    #[test]
    fn classify_empty_repo_unwitnessed() {
        let w = profile(0, 0, 0);
        // total=0 → merge_ratio=0; authors=0 ≤ 1 → Unwitnessed
        assert_eq!(spike_witness_classify(&w), WitnessStatus::Unwitnessed);
    }

    #[test]
    fn classify_boundary_merge_ratio() {
        // %10 tam sınırda → observable (>= threshold)
        let w = profile(100, 10, 5); // 10/100 = %10
        assert_eq!(spike_witness_classify(&w), WitnessStatus::Witnessed);
        // %9 → squash şüphesi (multi-author)
        let w2 = profile(100, 9, 5); // 9/100 = %9
        assert_eq!(
            spike_witness_classify(&w2),
            WitnessStatus::UnobservableLocally
        );
    }

    #[test]
    fn classify_two_authors_low_merge_still_unobservable() {
        // 2 author + düşük merge → hala UnobservableLocally (≤1 değil)
        let w = profile(1000, 5, 2);
        assert_eq!(
            spike_witness_classify(&w),
            WitnessStatus::UnobservableLocally
        );
    }

    // --- Faz 1.10: run_v2_pipeline (full integration) ---

    #[test]
    fn run_v2_pipeline_produces_complete_report() {
        let analysis = RepoAnalysis {
            repo_path: "test/myrepo".into(),
            graph: DepGraph {
                nodes: vec![
                    Node {
                        id: 1,
                        kind: NodeKind::Module,
                        path: "big".into(),
                        mass: 100,
                    },
                    Node {
                        id: 2,
                        kind: NodeKind::Module,
                        path: "small".into(),
                        mass: 50,
                    },
                ],
                edges: vec![Edge {
                    from: 1,
                    to: 2,
                    kind: EdgeKind::Imports,
                }],
            },
            witness: WitnessProfile {
                total_commits: 100,
                merge_commits: 50, // %50 merge → Witnessed
                distinct_authors: 5,
                commit_entropy: 6.0,
                witnessed_ratio: 0.5,
                ..Default::default()
            },
            metrics: Default::default(),
        };
        let r = run_v2_pipeline(&analysis);

        assert_eq!(r.repo, "myrepo");
        assert_eq!(r.witness_status, "Witnessed"); // %50 merge, multi-author
        assert_eq!(r.nodes, 2);
        assert_eq!(r.edges, 1);
        assert!((r.merge_ratio - 0.5).abs() < 1e-9);
        // sample = highest-mass = node 1 (mass 100)
        assert_eq!(r.sample_node_id, 1);
        assert!((r.sample_mass - 100.0).abs() < 1e-9);
        // raw_x: node 1 has 1 import → 1/(1+1) = 0.5
        assert!((r.raw_x - 0.5).abs() < 1e-9, "raw_x = {}", r.raw_x);
        // raw_z: node 1 Ce=1, Ca=0 → I = 1.0 (pure unstable leaf)
        assert!(
            (r.raw_z - 1.0).abs() < 1e-9,
            "raw_z (instability) = {}",
            r.raw_z
        );
        // derived θ ve D finite
        assert!(r.derived_theta.is_finite());
        assert!(r.derived_d.is_finite());
        // vision recorded
        assert!((r.vision_x - 0.4).abs() < 1e-9);
    }

    #[test]
    fn run_v2_pipeline_solo_foam_unwitnessed() {
        let analysis = RepoAnalysis {
            repo_path: "test/foam".into(),
            graph: DepGraph {
                nodes: vec![Node {
                    id: 1,
                    kind: NodeKind::Module,
                    path: "x".into(),
                    mass: 10,
                }],
                edges: vec![],
            },
            witness: WitnessProfile {
                total_commits: 50,
                merge_commits: 0,
                distinct_authors: 1,
                ..Default::default()
            },
            metrics: Default::default(),
        };
        let r = run_v2_pipeline(&analysis);
        assert_eq!(r.witness_status, "Unwitnessed"); // solo + 0 merge
        assert_eq!(r.nodes, 1);
    }

    #[test]
    fn run_v2_pipeline_empty_graph_safe() {
        let analysis = RepoAnalysis {
            repo_path: "test/empty".into(),
            graph: DepGraph::default(),
            witness: WitnessProfile::default(),
            metrics: Default::default(),
        };
        let r = run_v2_pipeline(&analysis);
        assert_eq!(r.nodes, 0);
        assert_eq!(r.witness_status, "Unwitnessed"); // empty → solo convention
        assert_eq!(r.sample_node_id, 0); // no nodes
    }
}
