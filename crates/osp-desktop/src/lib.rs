//! OSP Desktop — backend command handlers.
//!
//! Bu fonksiyonlar hem HTTP server (mevcut) hem de Tauri IPC (gelecek)
//! tarafından çağrılır. Logic ayrımı: bu modül sadece iş mantığı içerir,
//! sunum (HTTP/Tauri) `main.rs`'te.

use std::path::{Path, PathBuf};

use osp_analyzer::contract::AnalysisConfig;
use osp_analyzer::language::AdapterRegistry;
use osp_analyzer::pipeline::analyze_repo_with_config;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// JSON types (frontend ile paylaşılır)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
pub struct SpaceJson {
    pub nodes: Vec<NodeJson>,
    pub edges: Vec<EdgeJson>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeJson {
    pub id: u64,
    pub kind: String,
    pub mass: f64,
    pub coupling: Option<f64>,
    pub cohesion: Option<f64>,
    pub instability: Option<f64>,
    /// Node ID'ye karşılık gelen source file relative path (Inspector için).
    pub path: Option<String>,
    /// Dosya-rolü sınıflandırması (test/production/migration/...) — context-aware
    /// vision uyarıları için. Eski snapshot'lar "Unknown" default ile deserialize olur.
    #[serde(default)]
    pub classification: String,
    /// Mimari rol (TypeSurface/Core/Adapter/Utility/Runtime/Support) —
    /// role-aware vision için. Eski snapshot'lar "Runtime" default.
    #[serde(default)]
    pub role: String,
    /// Cohesion değeri nereden geldi: "scip" / "placeholder" / "heuristic".
    /// UI, placeholder cohesion (0.5) gerçek cohesion'dan ayırt etmek için kullanır.
    #[serde(default)]
    pub cohesion_source: Option<String>,
    /// Cohesion confidence ∈ [0,1] (0.95 × coverage × stale_penalty veya 0.0 placeholder).
    #[serde(default)]
    pub cohesion_confidence: Option<f64>,
    /// SCIP semantic: bu dosyada tanımlı class sayısı (0 = SCIP yok/indekslenmemiş).
    #[serde(default)]
    pub scip_class_count: usize,
    /// SCIP semantic: bu dosyadaki tüm class'lardaki toplam method sayısı.
    #[serde(default)]
    pub scip_method_count: usize,
    /// SCIP semantic: bu dosyadaki tüm class'lardaki toplam field sayısı.
    #[serde(default)]
    pub scip_field_count: usize,
    /// SCIP semantic: en yüksek LCOM4 component sayısı (en düşük cohesion'lı class).
    #[serde(default)]
    pub scip_max_lcom4: u32,
    // --- Node-level witness (git history evidence, §3.2) ---
    // Tümü `#[serde(default)]` — git yoksa veya eski snapshot'ta yoksa None/0.
    /// Dosyayı değiştiren distinct commit sayısı (volatility). None = git yok.
    #[serde(default)]
    pub witness_commits: Option<usize>,
    /// Dosyaya dokunan distinct yazar sayısı.
    #[serde(default)]
    pub witness_authors: Option<usize>,
    /// HEAD'e göre son değişiklikten geçen gün (recent_volatility).
    #[serde(default)]
    pub witness_last_modified_days: Option<u32>,
    /// Toplam added+deleted lines (churn).
    #[serde(default)]
    pub witness_churn: Option<u64>,
    /// En aktif yazarın payı ∈ [0,1] (1=solo, düşük=shared).
    #[serde(default)]
    pub witness_ownership: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EdgeJson {
    pub from: u64,
    pub to: u64,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisResultJson {
    pub space: SpaceJson,
    pub repo_metrics: RepoMetricsJson,
    pub semantic_coverage: SemanticCoverageJson,
    pub module_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoMetricsJson {
    pub abstractness: f64,
    /// Abstractness confidence ∈ [0,1] — düşük SCIP coverage'da değer güvenilmez.
    #[serde(default)]
    pub abstractness_confidence: f64,
    pub main_sequence_distance: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SemanticCoverageJson {
    pub files_total: usize,
    pub files_with_scip: usize,
    pub coverage_ratio: f64,
    pub classes_total: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command: analyze_repo
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir repo için mevcut SCIP index'i tespit et (frontend otomatik-detect için).
///
/// Repo root'nda `index.scip` arar. Bulursa absolute path döndürür; bulamazsa
/// `None`. Frontend, repo path girilince bunu çağırır ve SCIP Path kutusunu
/// otomatik doldurur — kullanıcı "neden SCIP inactive?" kafa karışıklığını önler.
pub fn cmd_detect_scip(repo_path: &str) -> Option<String> {
    let candidate = Path::new(repo_path).join("index.scip");
    if candidate.is_file() {
        Some(candidate.to_string_lossy().replace('\\', "/"))
    } else {
        None
    }
}

/// Bir reponun tam analizini yap → JSON (frontend için).
///
/// `scip_path` verilirse gerçek LCOM4 cohesion; yoksa placeholder.
pub fn cmd_analyze_repo(
    repo_path: &str,
    scip_path: Option<&str>,
) -> Result<AnalysisResultJson, String> {
    let config = AnalysisConfig {
        scip_index: scip_path.map(PathBuf::from),
        ..Default::default()
    };
    let registry = AdapterRegistry::default_all();
    let result = analyze_repo_with_config(Path::new(repo_path), &registry, &config)
        .map_err(|e| e.to_string())?;

    // Convert Space → JSON
    let nodes: Vec<NodeJson> = result
        .space
        .nodes
        .values()
        .map(|n| {
            let metrics = result.module_metrics.get(&n.id);
            let sem = result.node_semantics.get(&n.id);
            let wit = result.node_witnesses.get(&n.id);
            // Cohesion: node.cohesion (SCIP direkt) veya module_metrics'ten.
            // Source/confidence module_metrics'ten gelir (node.cohesion sadece değer taşır).
            let (cohesion_val, cohesion_src, cohesion_conf) = if let Some(m) = metrics {
                (
                    Some(m.cohesion.value),
                    Some(format!("{:?}", m.cohesion.source).to_lowercase()),
                    Some(m.cohesion.confidence),
                )
            } else {
                (n.cohesion, None, None)
            };
            NodeJson {
                id: n.id,
                kind: format!("{:?}", n.kind),
                mass: n.mass,
                coupling: metrics.map(|m| m.coupling.value),
                cohesion: cohesion_val,
                cohesion_source: cohesion_src,
                cohesion_confidence: cohesion_conf,
                instability: metrics.map(|m| m.instability.value),
                path: result.node_paths.get(&n.id).cloned(),
                classification: format!("{:?}", n.classification),
                role: format!("{:?}", n.role),
                scip_class_count: sem.map(|s| s.class_count).unwrap_or(0),
                scip_method_count: sem.map(|s| s.method_count).unwrap_or(0),
                scip_field_count: sem.map(|s| s.field_count).unwrap_or(0),
                scip_max_lcom4: sem.map(|s| s.max_lcom4).unwrap_or(0),
                witness_commits: wit.map(|w| w.commits_touching),
                witness_authors: wit.map(|w| w.distinct_authors),
                witness_last_modified_days: wit.map(|w| w.last_modified_days_ago),
                witness_churn: wit.map(|w| w.churn),
                witness_ownership: wit.map(|w| w.ownership_concentration),
            }
        })
        .collect();

    let edges: Vec<EdgeJson> = result
        .space
        .edges
        .iter()
        .map(|e| EdgeJson {
            from: e.from,
            to: e.to,
            kind: format!("{:?}", e.kind),
        })
        .collect();

    let node_count = nodes.len();

    Ok(AnalysisResultJson {
        space: SpaceJson { nodes, edges },
        repo_metrics: RepoMetricsJson {
            abstractness: result.repo_metrics.abstractness.value,
            abstractness_confidence: result.repo_metrics.abstractness.confidence,
            main_sequence_distance: result.repo_metrics.main_sequence_distance.value,
        },
        semantic_coverage: SemanticCoverageJson {
            files_total: result.semantic_coverage.files_total,
            files_with_scip: result.semantic_coverage.files_with_scip,
            coverage_ratio: result.semantic_coverage.coverage_ratio,
            classes_total: result.semantic_coverage.classes_total,
        },
        module_count: node_count,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command: simulate claim (commit pipeline visualizer)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
pub struct GateResultJson {
    pub name: String,
    pub passed: bool,
    pub detail: String,
    pub hallucination: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PipelineResultJson {
    pub gates: Vec<GateResultJson>,
    pub computed_raw: Vec<f64>,
    pub outcome: String, // "Commit" | "Rejected" | "Hold"
}

/// Commit pipeline simülasyonu — senaryo seçerek gate akışını gösterir.
///
/// Senaryolar:
/// - "valid": geçerli node + edge → tüm gate'ler geçer → Commit
/// - "syntax_fail": self-import → Q4 reddeder
/// - "vision_fail": zero-vector position → Q5 redder
/// - "rule_fail": duplicate node → Q6 redder (default rules ile)
pub fn cmd_simulate_claim(repo_path: &str, scenario: &str) -> Result<PipelineResultJson, String> {
    use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
    use osp_core::coords::CoordinateSystem;
    use osp_core::coords::MetricSource;
    use osp_core::engine::{EngineConfig, SpaceEngine};
    use osp_core::space::{Edge, EdgeKind, Node, NodeKind};
    use osp_core::vision::VisionVector;
    use osp_core::witness::{EvidenceEvent, WitnessKind, WitnessSet};

    // Build space from repo analysis
    let config = AnalysisConfig::default();
    let registry = AdapterRegistry::default_all();
    let result = analyze_repo_with_config(Path::new(repo_path), &registry, &config)
        .map_err(|e| e.to_string())?;

    // Build engine with default rules + vision
    // INV-T9 #70: production topology_source = TreeSitter, observed cohesion = Scip.
    let cs = CoordinateSystem::default_raw_five(
        MetricSource::TreeSitter,
        CohesionAxis::try_with_observed_source(MetricSource::Scip).map_err(|e| e.to_string())?,
        EntropyAxis::from_commit_entropy(6.0),
        WitnessDepthAxis::from_witness(0.3, 5),
    )
    .map_err(|e| e.to_string())?;
    let vision = VisionVector::new(osp_core::coords::RawPosition {
        x: 0.4,
        y: 0.6,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    });
    let engine = SpaceEngine::with_default_rules(
        result.space,
        cs,
        vision,
        EngineConfig::default_calibrated(),
    )
    .map_err(|e| format!("rule registration failed: {e}"))?;

    // Scenario → delta
    let next_id = engine.space().nodes.keys().max().copied().unwrap_or(0) + 1;
    let (delta_nodes, delta_edges, computed_raw_override) = match scenario {
        "valid" => (
            vec![Node {
                id: next_id,
                kind: NodeKind::Module,
                mass: 50.0,
                ..Default::default()
            }],
            vec![Edge {
                from: next_id,
                to: 1,
                kind: EdgeKind::Imports,
                ..Default::default()
            }],
            None, // let engine compute
        ),
        "syntax_fail" => (
            vec![Node {
                id: next_id,
                kind: NodeKind::Module,
                mass: 50.0,
                ..Default::default()
            }],
            vec![Edge {
                from: next_id,
                to: next_id,
                kind: EdgeKind::Imports,
                ..Default::default()
            }], // self-import
            None,
        ),
        "vision_fail" => (
            vec![Node {
                id: next_id,
                kind: NodeKind::Module,
                mass: 50.0,
                ..Default::default()
            }],
            vec![],
            Some(osp_core::coords::RawPosition::default()), // zero-vector → max θ
        ),
        "rule_fail" => {
            // Duplicate an existing node ID
            let existing_id = engine.space().nodes.keys().next().copied().unwrap_or(1);
            (
                vec![Node {
                    id: existing_id,
                    kind: NodeKind::Module,
                    mass: 99.0,
                    ..Default::default()
                }],
                vec![],
                None,
            )
        }
        _ => return Err(format!("Unknown scenario: {scenario}")),
    };

    // Compute position (or use override)
    let computed_raw = computed_raw_override
        .unwrap_or_else(|| engine.compute_raw_from_delta(&delta_nodes, &delta_edges));

    // Build claim
    let claim = osp_core::witness::Claim {
        id: 1,
        intent: osp_core::witness::Intent::new(42, osp_core::coords::RawPosition::default()),
        author: 42,
        computed_raw,
        delta_nodes,
        delta_edges,
    };

    // Mock witnesses (2 MergeCommit)
    let omega = WitnessSet::new(vec![
        EvidenceEvent::new(1, "github", WitnessKind::MergeCommit, 200, 1),
        EvidenceEvent::new(2, "github", WitnessKind::MergeCommit, 201, 1),
    ]);

    // Run all gates
    let gate_results = engine.check_all_gates(&claim, &omega);
    let outcome = if gate_results.last().map(|g| g.passed).unwrap_or(false) {
        "Commit".to_string()
    } else if gate_results
        .iter()
        .any(|g| !g.passed && g.name.starts_with("Q1"))
    {
        "Hold".to_string()
    } else {
        "Rejected".to_string()
    };

    Ok(PipelineResultJson {
        gates: gate_results
            .iter()
            .map(|g| GateResultJson {
                name: g.name.to_string(),
                passed: g.passed,
                detail: g.detail.clone(),
                hallucination: g.hallucination.clone(),
            })
            .collect(),
        computed_raw: vec![
            computed_raw.x,
            computed_raw.y,
            computed_raw.z,
            computed_raw.w,
            computed_raw.v,
        ],
        outcome,
    })
}

pub fn cmd_health() -> &'static str {
    "OSP Desktop v0.1 — ready"
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command: graveyard (rejected claims — epistemic negative-space data)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
pub struct GraveyardEntryJson {
    pub scenario: String,
    pub gate_failed: String,
    pub hallucination_type: String,
    pub detail: String,
    pub computed_position: Vec<f64>,
    pub would_be_dangerous: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraveyardJson {
    pub entries: Vec<GraveyardEntryJson>,
    pub total: usize,
}

/// Graveyard — reddedilmiş claim'lerin listesi.
///
/// Mevcut implementasyonda, `cmd_simulate_claim`'in failure senaryolarını
/// kullanarak graveyard oluştururur. Her entry: hangi senaryo, hangi gate,
/// hangi hallucination türü, ve "kabul edilseydi tehlikeli olur muydu?".
///
/// Gerçek kullanımda: engine'in commit history'sinden EngineCommitError'ları toplar.
pub fn cmd_get_graveyard(repo_path: &str) -> Result<GraveyardJson, String> {
    let scenarios = ["syntax_fail", "vision_fail", "rule_fail"];

    let mut entries = vec![];
    for scenario in &scenarios {
        match cmd_simulate_claim(repo_path, scenario) {
            Ok(result) => {
                if result.outcome != "Commit" {
                    let failed = result.gates.iter().find(|g| !g.passed);
                    let failed_gate = failed
                        .map(|g| g.name.clone())
                        .unwrap_or("Unknown".to_string());
                    let hallucination = failed
                        .and_then(|g| g.hallucination.clone())
                        .unwrap_or("Unknown hallucination".to_string());

                    let dangerous = scenario == &"vision_fail";

                    entries.push(GraveyardEntryJson {
                        scenario: scenario.to_string(),
                        gate_failed: failed_gate.clone(),
                        hallucination_type: hallucination,
                        detail: format!("Scenario '{}' rejected at {}", scenario, failed_gate),
                        computed_position: result.computed_raw.clone(),
                        would_be_dangerous: dangerous,
                    });
                }
            }
            Err(_) => continue,
        }
    }

    Ok(GraveyardJson {
        total: entries.len(),
        entries,
    })
}

/// What-if simulation — "bu claim kabul edilseydi ne olurdu?"
///
/// Bir failure senaryosunu hypothetical olarak uygular:
/// 1. Engine space'i klonlar
/// 2. Delta'yı uygular (ne olursa olsun — gates'i atla)
/// 3. Yeni pozisyonları hesaplar
/// 4. Hangi node'ların θ > θ_bound'a kayacağını bulur (impact wave)
#[derive(Debug, Serialize, Deserialize)]
pub struct ImpactNodeJson {
    pub node_id: u64,
    pub old_theta: f64,
    pub new_theta: f64,
    pub delta_theta: f64,
    pub would_enter_negative_space: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WhatIfResultJson {
    pub scenario: String,
    pub accepted_hypothetically: bool,
    pub impacted_nodes: Vec<ImpactNodeJson>,
    pub nodes_entering_negative_space: usize,
    pub message: String,
}

pub fn cmd_compute_whatif(repo_path: &str, scenario: &str) -> Result<WhatIfResultJson, String> {
    use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
    use osp_core::coords::CoordinateSystem;
    use osp_core::coords::MetricSource;
    use osp_core::coords::RawPosition;
    use osp_core::engine::{EngineConfig, SpaceEngine};
    use osp_core::space::{Edge, EdgeKind, Node, NodeKind};
    use osp_core::vision::{CosineDeviation, DeviationMetric, VisionVector};

    // Analyze repo
    let config = AnalysisConfig::default();
    let registry = AdapterRegistry::default_all();
    let result = analyze_repo_with_config(Path::new(repo_path), &registry, &config)
        .map_err(|e| e.to_string())?;

    // Build engine
    // INV-T9 #70: production topology_source = TreeSitter, observed cohesion = Scip.
    let cs = CoordinateSystem::default_raw_five(
        MetricSource::TreeSitter,
        CohesionAxis::try_with_observed_source(MetricSource::Scip).map_err(|e| e.to_string())?,
        EntropyAxis::from_commit_entropy(6.0),
        WitnessDepthAxis::from_witness(0.3, 5),
    )
    .map_err(|e| e.to_string())?;
    let vision = VisionVector::new(RawPosition {
        x: 0.4,
        y: 0.6,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    });
    let engine = SpaceEngine::with_default_rules(
        result.space,
        cs,
        vision,
        EngineConfig::default_calibrated(),
    )
    .map_err(|e| format!("rule registration failed: {e}"))?;

    // Build delta from scenario (same logic as cmd_simulate_claim)
    let next_id = engine.space().nodes.keys().max().copied().unwrap_or(0) + 1;
    let (delta_nodes, delta_edges) = match scenario {
        "syntax_fail" => (
            vec![Node {
                id: next_id,
                kind: NodeKind::Module,
                mass: 50.0,
                ..Default::default()
            }],
            vec![Edge {
                from: next_id,
                to: next_id,
                kind: EdgeKind::Imports,
                ..Default::default()
            }],
        ),
        "vision_fail" => (
            vec![Node {
                id: next_id,
                kind: NodeKind::Module,
                mass: 50.0,
                ..Default::default()
            }],
            vec![],
        ),
        "rule_fail" => {
            let existing = engine.space().nodes.keys().next().copied().unwrap_or(1);
            (
                vec![Node {
                    id: existing,
                    kind: NodeKind::Module,
                    mass: 99.0,
                    ..Default::default()
                }],
                vec![],
            )
        }
        _ => return Err(format!("Unknown scenario: {scenario}")),
    };

    // Compute current θ for all nodes
    let theta_bound = engine.config().theta_bound;
    let current_thetas: Vec<(u64, f64)> = engine
        .space()
        .nodes
        .values()
        .map(|n| {
            let raw = engine.coord_system().raw_position_of(n, engine.space());
            let theta = CosineDeviation.theta(&raw, engine.vision(), engine.space());
            (n.id, theta)
        })
        .collect();

    // Hypothetical: apply delta (ignore gates) and compute new θ
    let mut hypothetical_space = engine.space().clone();
    for n in &delta_nodes {
        hypothetical_space.insert_node(n.clone());
    }
    for e in &delta_edges {
        hypothetical_space.insert_edge(*e);
    }

    let impacted: Vec<ImpactNodeJson> = current_thetas
        .iter()
        .map(|(id, old_theta)| {
            let node = hypothetical_space.nodes.get(id);
            let new_raw = node
                .map(|n| {
                    engine
                        .coord_system()
                        .raw_position_of(n, &hypothetical_space)
                })
                .unwrap_or_default();
            let new_theta = CosineDeviation.theta(&new_raw, engine.vision(), &hypothetical_space);
            ImpactNodeJson {
                node_id: *id,
                old_theta: *old_theta,
                new_theta,
                delta_theta: new_theta - old_theta,
                would_enter_negative_space: new_theta > theta_bound && *old_theta <= theta_bound,
            }
        })
        .filter(|i| i.delta_theta.abs() > 0.001) // only show changed nodes
        .collect();

    let neg_count = impacted
        .iter()
        .filter(|i| i.would_enter_negative_space)
        .count();

    let message = if neg_count > 0 {
        format!("⚠ If this claim had been accepted, {} node(s) would have entered negative space (θ > {:.2}).", neg_count, theta_bound)
    } else if !impacted.is_empty() {
        format!(
            "This claim would have shifted {} node(s) but none into negative space.",
            impacted.len()
        )
    } else {
        "This claim would have minimal impact on the space.".to_string()
    };

    Ok(WhatIfResultJson {
        scenario: scenario.to_string(),
        accepted_hypothetically: true, // we force-accept for what-if
        impacted_nodes: impacted,
        nodes_entering_negative_space: neg_count,
        message,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command: vision config (get/set)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfigJson {
    pub x: f64, // coupling target
    pub y: f64, // cohesion target
    pub z: f64, // instability target
    pub w: f64, // entropy target
    pub v: f64, // witness-depth target
    pub theta_bound: f64,
    pub theta_quorum: f64,
    pub min_approvers: usize,
}

impl Default for VisionConfigJson {
    fn default() -> Self {
        Self {
            x: 0.4,
            y: 0.6,
            z: 0.5,
            w: 0.5,
            v: 0.5,
            theta_bound: 0.30,
            theta_quorum: 1.5,
            min_approvers: 2,
        }
    }
}

pub fn cmd_get_vision_config() -> VisionConfigJson {
    VisionConfigJson::default()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command: repo stats (git history → tri-state witness classification)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoStatsJson {
    pub commit_count: usize,
    pub author_count: usize,
    pub merge_count: usize,
    pub merge_ratio: f64,
    pub witness_status: String,
    pub witness_detail: String,
}

/// Git geçmişini analiz et → tri-state witness classification.
///
/// `git log` subprocess ile çalışır (osp-spike'a bağımlılık yok).
pub fn cmd_get_repo_stats(repo_path: &str) -> Result<RepoStatsJson, String> {
    let path = std::path::Path::new(repo_path);

    // git rev-list --count HEAD
    let commit_count = git_int(path, &["rev-list", "--count", "HEAD"])?;
    // git shortlog -sne HEAD | wc -l
    let authors = git_output(path, &["shortlog", "-sne", "HEAD"])?;
    let author_count = authors.lines().filter(|l| !l.trim().is_empty()).count();
    // git rev-list --count --merges HEAD
    let merge_count = git_int(path, &["rev-list", "--count", "--merges", "HEAD"])?;

    let merge_ratio = if commit_count > 0 {
        merge_count as f64 / commit_count as f64
    } else {
        0.0
    };

    // Tri-state classification (calibration-corpus.md threshold: 10%)
    let (status, detail) = if author_count <= 1 {
        (
            "Unwitnessed".to_string(),
            format!("Solo project ({} author)", author_count),
        )
    } else if merge_ratio >= 0.10 {
        (
            "Witnessed".to_string(),
            format!(
                "{:.1}% merge ratio, {} authors",
                merge_ratio * 100.0,
                author_count
            ),
        )
    } else {
        (
            "Unobservable-locally".to_string(),
            format!(
                "{} authors but {:.1}% merge (squash/rebase hides evidence)",
                author_count,
                merge_ratio * 100.0
            ),
        )
    };

    Ok(RepoStatsJson {
        commit_count,
        author_count,
        merge_count,
        merge_ratio,
        witness_status: status,
        witness_detail: detail,
    })
}

fn git_int(path: &std::path::Path, args: &[&str]) -> Result<usize, String> {
    let output = git_output(path, args)?;
    output
        .trim()
        .parse()
        .map_err(|e| format!("git parse error: {e}"))
}

fn git_output(path: &std::path::Path, args: &[&str]) -> Result<String, String> {
    std::process::Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(args)
        .output()
        .map_err(|e| format!("git failed: {e}"))
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).map_err(|e| format!("utf8: {e}"))
            } else {
                Err(format!("git error: {}", String::from_utf8_lossy(&o.stderr)))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cmd_analyze_repo, NodeJson.path'i AnalysisResult.node_paths'ten doldurur.
    /// Inspector feature'ının backend contract'ı: her node'un source path'i olmalı.
    #[test]
    fn analyze_repo_populates_node_json_path_for_inspector() {
        // Minimal Python fixture: 2 dosya, birbirini import etmesin (edges 0 OK)
        let dir = std::env::temp_dir().join(format!("osp-desktop-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("alpha.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join("beta.py"), "y = 2\n").unwrap();

        let result = cmd_analyze_repo(dir.to_str().unwrap(), None).expect("analyze ok");

        // 2 node olmalı
        assert_eq!(result.space.nodes.len(), 2, "2 .py files → 2 nodes");

        // Her node'un path'i olmalı (Inspector contract)
        for node in &result.space.nodes {
            assert!(
                node.path.is_some(),
                "node {} should have a source path",
                node.id
            );
            let path = node.path.as_ref().unwrap();
            assert!(
                path.ends_with(".py"),
                "node path should end with .py, got: {path}"
            );
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// NodeJson serde round-trip: frontend JSON ↔ Rust struct (snapshot ile uyumlu).
    #[test]
    fn node_json_serializes_with_path_and_classification() {
        let node = NodeJson {
            id: 42,
            kind: "Module".to_string(),
            mass: 100.0,
            coupling: Some(0.3),
            cohesion: Some(0.7),
            cohesion_source: Some("scip".to_string()),
            cohesion_confidence: Some(0.85),
            instability: Some(0.5),
            path: Some("src/main.py".to_string()),
            classification: "Production".to_string(),
            role: "Runtime".to_string(),
            scip_class_count: 3,
            scip_method_count: 12,
            scip_field_count: 8,
            scip_max_lcom4: 2,
            witness_commits: Some(15),
            witness_authors: Some(4),
            witness_last_modified_days: Some(12),
            witness_churn: Some(340),
            witness_ownership: Some(0.4),
        };
        let json = serde_json::to_string(&node).expect("serialize");
        assert!(
            json.contains("\"path\":\"src/main.py\""),
            "path field in JSON"
        );
        assert!(
            json.contains("\"classification\":\"Production\""),
            "classification field"
        );
        assert!(
            json.contains("\"scip_class_count\":3"),
            "scip_class_count field"
        );
        assert!(
            json.contains("\"cohesion_source\":\"scip\""),
            "cohesion_source field"
        );
        assert!(
            json.contains("\"scip_method_count\":12"),
            "scip_method_count field"
        );
        // Round-trip
        let back: NodeJson = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.path, Some("src/main.py".to_string()));
        assert_eq!(back.classification, "Production");
        assert_eq!(back.scip_class_count, 3);
        assert_eq!(back.scip_max_lcom4, 2);
        assert_eq!(back.id, 42);
    }

    /// Eski snapshot (SCIP alanları yok) deserialize → 0 default.
    #[test]
    fn node_json_backward_compat_missing_scip_fields() {
        let old_json = r#"{"id":1,"kind":"Module","mass":10.0,"coupling":null,"cohesion":null,"instability":null,"path":"a.py"}"#;
        let n: NodeJson = serde_json::from_str(old_json).expect("deserialize old node");
        assert_eq!(
            n.scip_class_count, 0,
            "missing scip_class_count defaults to 0"
        );
        assert_eq!(
            n.scip_method_count, 0,
            "missing scip_method_count defaults to 0"
        );
        assert_eq!(
            n.classification, "",
            "missing classification defaults to empty"
        );
    }
}
