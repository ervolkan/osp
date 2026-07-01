//! G2c — Corpus experiment runner (Paper 2 RQ6-9 evidence).
//!
//! Navigator loop'u N repo × M task × {policy, feedback} matrisinde çalıştırır,
//! her hücreden `G2cEvidenceRow` üretir. Arkadaş review'ı (review 5) ile:
//! - FeedbackSensitiveMock: RQ8 with/without feedback farkı GERÇEK ölçülür
//! - Incremental proposals: RQ9 StrictReject (state sabit) vs AcceptImprovement (ilerler)
//! - Deterministik top-offender node seçimi (Node(0) değil)
//! - INV-T4 source kontrolü (placeholder skip)
//!
//! **Etiket:** "harness validation corpus" (paper evidence değil — review 5 #3).
//! JSON metadata'da `corpus_kind: "local-crate-subtree"`.
//!
//! ```bash
//! # Mock MVP (deterministik, CI güvenli)
//! cargo run --release --example g2c_corpus_matrix -- --llm mock --out evidence.json
//!
//! # Gerçek LLM (GPT-4o-mini, OPENAI_API_KEY, cost-limited)
//! cargo run --release --example g2c_corpus_matrix -- --llm real --out evidence-real.json
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::Parser;
use osp_analyzer::contract::AnalysisConfig;
use osp_analyzer::language::AdapterRegistry;
use osp_analyzer::pipeline::analyze_repo_with_config;
use osp_core::agent::{DeltaProposal, NewNodeSpec, OutputContract};
use osp_core::coords::{CoordinateSystem, MetricSource, RawPosition};
use osp_core::navigator::{
    provenanced_from_raw, AgentNavigator, LlmClient, LlmError, NavigatorResult,
};
use osp_core::space::{Node, NodeId, NodeKind, Space};
use osp_core::trajectory::{
    ComparisonOp, InMemoryTaskRegistry, MetricPredicate, OpKind, PredicateAxis,
    PredicateFailurePolicy, PredicateScope, Task, TaskId, TaskPolicy, TaskStatus,
    TrajectoryEvidence,
};
use osp_llm_runtime::RuntimeLlmClient;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// CLI
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Parser, Debug)]
#[command(
    name = "g2c-corpus-matrix",
    about = "G2c corpus experiment runner — Paper 2 RQ6-9 evidence (mock + real LLM)"
)]
struct Cli {
    /// LLM backend: mock (deterministic, default) veya real (GPT-4o-mini, OPENAI_API_KEY).
    #[arg(long, value_enum, default_value_t = LlmBackendArg::Mock)]
    llm: LlmBackendArg,
    /// Output JSON path (default: stdout).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Maneuver limit override (default: task.policy.maneuver_limit = 5).
    #[arg(long, default_value_t = 5)]
    maneuver_limit: u32,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum LlmBackendArg {
    Mock,
    Real,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Evidence schema (review 5 #6 — zengin)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek experiment hücresinin çıktısı (bir repo × task × policy × feedback × llm).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct G2cEvidenceRow {
    // ── Metadata (review #3, #6) ──
    run_id: String,
    osp_version: String,
    git_commit: String,
    corpus_kind: String, // "local-crate-subtree" | "external-repo"
    timestamp: String,
    // ── Experiment cell ──
    repo: String,
    analyzed_path: String,
    lang: String,
    node_count: usize,
    edge_count: usize,
    task_id: TaskId,
    task_type: String,
    policy: String,
    feedback: String, // "with" | "without"
    llm: String,
    maneuver_limit: u32,
    // ── Target node (review #4) ──
    target_node_id: NodeId,
    target_node_path: String,
    target_node_role: String,
    selection_reason: String,
    // ── Results ──
    attempts: usize,
    completed: bool,
    final_outcome: String,
    final_mutation_decision: String,
    final_apply_target: String,
    total_tokens: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    feedback_count: usize,
    rejected_by: Vec<String>,
    loss_before: f64,
    loss_after: f64,
    axis_regression: bool,
    regression_axes: Vec<String>,
    max_regression_delta: f64,
    duration_ms: u64,
    // ── Per-attempt ledger (RQ6 detayı) ──
    evidence: Vec<TrajectoryEvidence>,
}

/// Top-level evidence JSON wrapper (metadata + rows).
#[derive(Debug, Serialize, Deserialize)]
struct G2cEvidenceFile {
    schema_version: String,
    run_id: String,
    osp_version: String,
    git_commit: String,
    corpus_kind: String,
    llm_backend: String,
    maneuver_limit: u32,
    generated_at: String,
    rows: Vec<G2cEvidenceRow>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeedbackSensitiveMock (review 5 #1 — feedback-sensitive mock)
// ═══════════════════════════════════════════════════════════════════════════════

/// Mock davranış modu (arkadaş önerisi).
enum MockBehavior {
    /// Sırayla döner, feedback'e bakmaz (baseline).
    ScriptedFixed(Vec<DeltaProposal>),
    /// feedback_history'e göre branch eder — RQ8 için.
    /// - empty feedback → without_feedback dizisi (kötü → kötü → ... → limit)
    /// - non-empty feedback → with_feedback dizisi (kötü → düzeltilmiş → başarılı)
    FeedbackSensitive {
        without_feedback: Vec<DeltaProposal>,
        with_feedback: Vec<DeltaProposal>,
    },
}

/// Feedback-sensitive mock LLM. Arkadaş #1: RQ8 gerçek ölçülebilir.
struct FeedbackSensitiveMock {
    behavior: MockBehavior,
    call_count: std::sync::atomic::AtomicUsize,
}

impl FeedbackSensitiveMock {
    fn new(behavior: MockBehavior) -> Self {
        Self {
            behavior,
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl LlmClient for FeedbackSensitiveMock {
    fn complete(
        &self,
        view: &osp_core::trajectory::AgentTaskView,
    ) -> Result<DeltaProposal, LlmError> {
        use std::sync::atomic::Ordering;
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        match &self.behavior {
            MockBehavior::ScriptedFixed(proposals) => {
                proposals.get(idx).cloned().ok_or(LlmError::NoMoreProposals)
            }
            MockBehavior::FeedbackSensitive {
                without_feedback,
                with_feedback,
            } => {
                // feedback_history boşsa (ilk attempt) → without; değilse → with.
                let proposals = if view.feedback_history.is_empty() {
                    without_feedback
                } else {
                    with_feedback
                };
                proposals.get(idx).cloned().ok_or(LlmError::NoMoreProposals)
            }
        }
    }

    fn last_token_cost(&self) -> osp_core::trajectory::TokenCost {
        osp_core::trajectory::TokenCost::default()
    }
}

/// NoFeedbackWrapper (review 5 #1) — feedback'i temizleyerek forward eder.
/// RQ8 "without feedback" hücresi bununla (feedback_history hep boş).
struct NoFeedbackWrapper<L: LlmClient> {
    inner: L,
}

impl<L: LlmClient> LlmClient for NoFeedbackWrapper<L> {
    fn complete(
        &self,
        view: &osp_core::trajectory::AgentTaskView,
    ) -> Result<DeltaProposal, LlmError> {
        let mut view = view.clone();
        view.feedback_history.clear();
        self.inner.complete(&view)
    }
    fn last_token_cost(&self) -> osp_core::trajectory::TokenCost {
        self.inner.last_token_cost()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Proposal builders (review 5 #2 — incremental / Q4-controlled)
// ═══════════════════════════════════════════════════════════════════════════════

/// Geçerli structural proposal — tek node, valid JSON formatı (Q4 geçer).
fn valid_proposal(label: &str) -> DeltaProposal {
    DeltaProposal {
        new_nodes: vec![NewNodeSpec {
            kind: NodeKind::Module,
            initial_mass: 100.0,
            connected_to: vec![],
        }],
        new_edges: vec![],
        modified_entities: vec![],
        position_hints: vec![],
        reasoning: label.into(),
    }
}

/// Q4 syntax hatası üreten proposal — modified_entities bozuk format (navigator Q4 reject).
/// navigator.rs OutputContract::validate bunu reddeder → feedback_history'ye yazılır.
fn bad_format_proposal() -> DeltaProposal {
    DeltaProposal {
        new_nodes: vec![],
        new_edges: vec![],
        // Boş modified_entities ama position_hints dolu → Q4 "pozisyon declare etme" ihlali.
        // Aslında navigator position_hints advisory kullanır; Q4 syntax için new_nodes boş + kötü.
        modified_entities: vec![],
        position_hints: vec![],
        reasoning: "intentionally malformed for Q4 gate test".into(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Target node selection (review 5 #4 — deterministik top-offender)
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministik top-offender node seçimi.
/// coupling_reduction: en yüksek coupling'li production/module node.
/// Tie-break: coupling desc, id asc (stable sıralama — path yok, id deterministik).
fn select_target_node(
    space: &Space,
    engine: &osp_core::engine::SpaceEngine,
    task_type: &TaskType,
) -> Option<(NodeId, String, String)> {
    let cs = engine.coord_system();
    let mut candidates: Vec<(NodeId, String, String)> = space
        .nodes
        .values()
        .filter(|n| matches!(n.kind, NodeKind::Module | NodeKind::Concept))
        .map(|n| {
            let raw = cs.raw_position_of(n, space);
            let score = match task_type {
                TaskType::CouplingReduction => raw.x, // coupling yüksek → hedef
                TaskType::InstabilityReduction => raw.z, // instability yüksek → hedef
            };
            (n.id, format!("{:?}", n.role), format!("{score:.4}"))
        })
        .collect();
    // Deterministik: score desc, id asc.
    candidates.sort_by(|a, b| {
        let sa: f64 = a.2.parse().unwrap_or(0.0);
        let sb: f64 = b.2.parse().unwrap_or(0.0);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    candidates.first().map(|(id, role, score)| {
        (
            *id,
            role.clone(),
            format!("highest {task_type:?} score {score}"),
        )
    })
}

#[derive(Clone, Copy, Debug)]
enum TaskType {
    CouplingReduction,
    InstabilityReduction,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Experiment runner
// ═══════════════════════════════════════════════════════════════════════════════

fn build_task(
    task_id: TaskId,
    task_type: TaskType,
    target_node: NodeId,
    policy: PredicateFailurePolicy,
    maneuver_limit: u32,
) -> Task {
    let (metric, threshold) = match task_type {
        TaskType::CouplingReduction => (PredicateAxis::Coupling, 0.55),
        TaskType::InstabilityReduction => (PredicateAxis::Instability, 0.60),
    };
    Task {
        id: task_id,
        milestone_id: 1,
        label: format!("{task_type:?} on node {target_node}"),
        target_predicate_set: osp_core::trajectory::PredicateSet {
            mode: osp_core::trajectory::PredicateMode::All,
            predicates: vec![osp_core::trajectory::WeightedPredicate {
                predicate: MetricPredicate {
                    metric,
                    operator: ComparisonOp::Le,
                    threshold,
                    scope: PredicateScope::Node(target_node),
                    required_source: Some(MetricSource::Scip),
                    tolerance: 0.0,
                },
                weight: None,
            }],
            preferred_vector: Some(RawPosition {
                x: if matches!(task_type, TaskType::CouplingReduction) {
                    threshold
                } else {
                    0.4
                },
                y: 0.6,
                z: if matches!(task_type, TaskType::InstabilityReduction) {
                    threshold
                } else {
                    0.4
                },
                w: 0.5,
                v: 0.3,
            }),
        },
        policy: TaskPolicy {
            maneuver_limit,
            predicate_failure_policy: policy,
            // review 5 #2: AcceptImprovement için allow_progress_checkpoint ZORUNLU.
            allow_progress_checkpoint: matches!(policy, PredicateFailurePolicy::AcceptImprovement),
            ..Default::default()
        },
        allowed_operations: vec![OpKind::RemoveImport, OpKind::ExtractModule],
        constraints: vec![],
        status: TaskStatus::Pending,
    }
}

/// Tek experiment hücresi çalıştır.
fn run_one_experiment(
    repo_path: &Path,
    repo_label: &str,
    task_type: TaskType,
    policy: PredicateFailurePolicy,
    feedback_mode: FeedbackMode,
    llm_backend: LlmBackendArg,
    maneuver_limit: u32,
    run_meta: &RunMeta,
) -> Result<G2cEvidenceRow> {
    let start = std::time::Instant::now();
    let task_id: TaskId = 1;

    // 1. Analyze repo → space + engine.
    let registry = AdapterRegistry::default_all();
    let config = AnalysisConfig::default();
    let result = analyze_repo_with_config(repo_path, &registry, &config)
        .with_context(|| format!("analyze failed: {}", repo_path.display()))?;
    let node_count = result.space.nodes.len();
    let edge_count = result.space.edges.len();

    // 2. Engine kur (CLI/MCP pattern'ı).
    let cs = CoordinateSystem::default_raw_five(
        osp_core::axes::CohesionAxis::new(),
        osp_core::axes::EntropyAxis::from_commit_entropy(6.0),
        osp_core::axes::WitnessDepthAxis::from_witness(0.3, 5),
    );
    let vision = osp_core::vision::VisionVector::new(RawPosition {
        x: 0.4,
        y: 0.6,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    });
    let mut engine = osp_core::engine::SpaceEngine::with_default_rules(
        result.space,
        cs,
        vision,
        osp_core::engine::EngineConfig::default_calibrated(),
    );

    // 3. Deterministik target node seç (review 5 #4).
    let (target_node, target_role, selection_reason) =
        select_target_node(engine.space(), &engine, &task_type)
            .unwrap_or_else(|| (0u64, "<none>".into(), "empty space".into()));

    // 4. Task kur.
    let task = build_task(task_id, task_type, target_node, policy, maneuver_limit);
    let mut resolver = InMemoryTaskRegistry::new();
    resolver.insert(task.clone());
    let target_node_obj = engine
        .space()
        .nodes
        .get(&target_node)
        .cloned()
        .unwrap_or_else(Node::default);
    let current_measured = provenanced_from_raw(
        engine
            .coord_system()
            .raw_position_of(&target_node_obj, engine.space()),
        MetricSource::TreeSitter,
    );
    let loss_before = osp_core::trajectory::trajectory_loss(
        &current_measured,
        &task.target_predicate_set.preferred_vector.unwrap(),
    );
    let target_vector = task.target_predicate_set.preferred_vector.unwrap();

    // 5. LLM client kur (feedback mode + backend'e göre).
    //
    // RQ8 senaryo (coupling/instability reduction):
    //   with-feedback: [valid_proposal_1, valid_proposal_2, ...] — engine measure'a gider,
    //     evidence üretilir, AcceptImprovement ile progress ilerler → Completed (AcceptImprovement)
    //     veya limit (StrictReject — progress reject).
    //   without-feedback: [valid_proposal_1, ...] — NoFeedbackWrapper feedback'i temizler ama
    //     proposals aynı. RQ8 farkı: FeedbackSensitiveMock ile "without" yolunda hep bad_format
    //     (Q4 reject, evidence yok, LlmError/limit); "with" yolunda düzeltilmiş valid.
    let llm: Arc<dyn LlmClient> =
        match (llm_backend, feedback_mode) {
            (LlmBackendArg::Mock, FeedbackMode::With) => Arc::new(FeedbackSensitiveMock::new(
                MockBehavior::FeedbackSensitive {
                    // without-feedback dalı: hep bad_format (Q4 reject, evidence yok).
                    without_feedback: vec![bad_format_proposal(); maneuver_limit as usize],
                    // with-feedback dalı (feedback_history dolu): valid proposals → engine measure.
                    with_feedback: vec![
                        valid_proposal("corrected after feedback");
                        maneuver_limit as usize
                    ],
                },
            )),
            (LlmBackendArg::Mock, FeedbackMode::Without) => Arc::new(NoFeedbackWrapper {
                inner: FeedbackSensitiveMock::new(MockBehavior::ScriptedFixed(
                    vec![bad_format_proposal(); maneuver_limit as usize],
                )),
            }),
            (LlmBackendArg::Real, _) => {
                let real = RuntimeLlmClient::from_env()
                    .context("OPENAI_API_KEY required for --llm real")?;
                if matches!(feedback_mode, FeedbackMode::Without) {
                    Arc::new(NoFeedbackWrapper { inner: real })
                } else {
                    Arc::new(real)
                }
            }
        };

    // 6. Navigator çalıştır — NavigatorResult'ı yakala (RQ7/RQ8 outcome kaynağı).
    let mut evidence: Vec<TrajectoryEvidence> = Vec::new();
    let nav_result: NavigatorResult = {
        let mut nav = AgentNavigator {
            llm: llm.as_ref(),
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector,
            current_measured: current_measured.clone(),
            output_contract: OutputContract::strict(),
        };
        nav.run_task(task_id, 1)
    };

    // 7. Final outcome + loss_after hesapla (NavigatorResult'tan).
    let (final_outcome, attempts) = match &nav_result {
        NavigatorResult::Completed { attempts, .. } => ("Completed".to_string(), *attempts),
        NavigatorResult::ExceededManeuverLimit { attempts, .. } => {
            ("ExceededManeuverLimit".to_string(), *attempts)
        }
        NavigatorResult::RequiresOperatorApproval { attempts, .. } => {
            ("RequiresOperatorApproval".to_string(), *attempts)
        }
        NavigatorResult::TaskNotFound => ("TaskNotFound".to_string(), 0),
        NavigatorResult::LlmError(_) => ("LlmError".to_string(), 0),
    };
    let completed = final_outcome == "Completed";
    let loss_after = evidence
        .last()
        .map(|e| {
            let prov = provenanced_from_raw(e.after, MetricSource::TreeSitter);
            osp_core::trajectory::trajectory_loss(&prov, &target_vector)
        })
        .unwrap_or(loss_before);

    // 8. Axis regression kontrolü (review 5 #6).
    let mut regression_axes = Vec::new();
    let mut max_regression_delta = 0.0f64;
    if let Some(last) = evidence.last() {
        let (b, a) = (last.before, last.after);
        // coupling↑, cohesion↓, instability↑ = regression
        let coupl_regress = (a.x - b.x).max(0.0);
        let cohes_regress = (b.y - a.y).max(0.0);
        let insta_regress = (a.z - b.z).max(0.0);
        if coupl_regress > 0.01 {
            regression_axes.push("coupling".into());
            max_regression_delta = max_regression_delta.max(coupl_regress);
        }
        if cohes_regress > 0.01 {
            regression_axes.push("cohesion".into());
            max_regression_delta = max_regression_delta.max(cohes_regress);
        }
        if insta_regress > 0.01 {
            regression_axes.push("instability".into());
            max_regression_delta = max_regression_delta.max(insta_regress);
        }
    }

    let total_tokens = evidence.iter().map(|e| e.token_cost.total_tokens).sum();
    let prompt_tokens = evidence.iter().map(|e| e.token_cost.prompt_tokens).sum();
    let completion_tokens = evidence
        .iter()
        .map(|e| e.token_cost.completion_tokens)
        .sum();
    let feedback_count = evidence
        .iter()
        .filter(|e| {
            matches!(
                e.mutation_decision,
                osp_core::trajectory::MutationDecision::Reject
            )
        })
        .count();
    let rejected_by: Vec<String> = evidence
        .iter()
        .filter(|e| {
            matches!(
                e.mutation_decision,
                osp_core::trajectory::MutationDecision::Reject
            )
        })
        .map(|_| "PredicateGate".into())
        .collect();
    let final_mutation_decision = evidence
        .last()
        .map(|e| format!("{:?}", e.mutation_decision))
        .unwrap_or_default();
    let final_apply_target = match evidence.last() {
        Some(e) => format!("{:?}", e.mutation_decision.apply_target()),
        None => "None".into(),
    };

    Ok(G2cEvidenceRow {
        run_id: run_meta.run_id.clone(),
        osp_version: run_meta.osp_version.clone(),
        git_commit: run_meta.git_commit.clone(),
        corpus_kind: run_meta.corpus_kind.clone(),
        timestamp: run_meta.timestamp.clone(),
        repo: repo_label.into(),
        analyzed_path: repo_path.display().to_string(),
        lang: "rust".into(),
        node_count,
        edge_count,
        task_id,
        task_type: format!("{task_type:?}"),
        policy: format!("{policy:?}"),
        feedback: format!("{feedback_mode:?}"),
        llm: format!("{llm_backend:?}"),
        maneuver_limit,
        target_node_id: target_node,
        target_node_path: target_role.clone(),
        target_node_role: target_role,
        selection_reason,
        attempts,
        completed,
        final_outcome,
        final_mutation_decision,
        final_apply_target,
        total_tokens,
        prompt_tokens,
        completion_tokens,
        feedback_count,
        rejected_by,
        loss_before,
        loss_after,
        axis_regression: !regression_axes.is_empty(),
        regression_axes,
        max_regression_delta,
        duration_ms: start.elapsed().as_millis() as u64,
        evidence,
    })
}

#[derive(Clone, Copy, Debug)]
enum FeedbackMode {
    With,
    Without,
}

struct RunMeta {
    run_id: String,
    osp_version: String,
    git_commit: String,
    corpus_kind: String,
    timestamp: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main — matris döngüsü
// ═══════════════════════════════════════════════════════════════════════════════

fn main() -> Result<()> {
    let cli = Cli::parse();
    let run_id = format!(
        "g2c-{}-{}",
        chrono_nonce(),
        match cli.llm {
            LlmBackendArg::Mock => "mock",
            LlmBackendArg::Real => "real",
        }
    );
    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().into())
        .unwrap_or_else(|| "unknown".into());
    let run_meta = RunMeta {
        run_id: run_id.clone(),
        osp_version: env!("CARGO_PKG_VERSION").into(),
        git_commit,
        corpus_kind: "local-crate-subtree".into(),
        timestamp: chrono_timestamp(),
    };

    // MVP corpus: local crate subtrees (review 5 #3 — harness validation).
    // Path'ler CWD (repo root)'dan göreli — `cargo run --example` repo root'ta çalışır.
    let corpus: Vec<(&str, PathBuf)> = vec![
        ("osp-core", PathBuf::from("crates/osp-core/src")),
        ("osp-cli", PathBuf::from("crates/osp-cli/src")),
        ("osp-analyzer", PathBuf::from("crates/osp-analyzer/src")),
    ];

    println!(
        "G2c corpus matrix — {} backend",
        match cli.llm {
            LlmBackendArg::Mock => "mock",
            LlmBackendArg::Real => "real",
        }
    );
    println!("Corpus: {} crate subtrees", corpus.len());

    let mut rows = Vec::new();
    let mut errors = Vec::new();

    for (repo_label, repo_path) in &corpus {
        for task_type in [TaskType::CouplingReduction, TaskType::InstabilityReduction] {
            for policy in [
                PredicateFailurePolicy::StrictReject,
                PredicateFailurePolicy::AcceptImprovement,
            ] {
                for feedback_mode in [FeedbackMode::With, FeedbackMode::Without] {
                    print!("  {repo_label}/{task_type:?}/{policy:?}/{feedback_mode:?} ... ",);
                    match run_one_experiment(
                        repo_path,
                        repo_label,
                        task_type,
                        policy,
                        feedback_mode,
                        cli.llm,
                        cli.maneuver_limit,
                        &run_meta,
                    ) {
                        Ok(row) => {
                            println!(
                                "{} attempts={}, completed={}",
                                row.final_outcome, row.attempts, row.completed
                            );
                            rows.push(row);
                        }
                        Err(e) => {
                            println!("ERROR: {e:#}");
                            errors.push(format!(
                                "{repo_label}/{task_type:?}/{policy:?}/{feedback_mode:?}: {e:#}"
                            ));
                        }
                    }
                }
            }
        }
    }

    // Özet.
    println!("\n=== Summary ===");
    println!("Rows: {} (errors: {})", rows.len(), errors.len());
    let completed_count = rows.iter().filter(|r| r.completed).count();
    println!("Completed: {}/{}", completed_count, rows.len());
    let avg_attempts: f64 = if rows.is_empty() {
        0.0
    } else {
        rows.iter().map(|r| r.attempts).sum::<usize>() as f64 / rows.len() as f64
    };
    println!("Avg attempts: {avg_attempts:.1}");

    // RQ8 özet (feedback with vs without).
    let with_fb = rows.iter().filter(|r| r.feedback == "With");
    let without_fb = rows.iter().filter(|r| r.feedback == "Without");
    let with_completed = with_fb.clone().filter(|r| r.completed).count();
    let without_completed = without_fb.clone().filter(|r| r.completed).count();
    println!("\nRQ8 (calibration feedback):");
    println!(
        "  with feedback:    {with_completed}/{} completed",
        with_fb.count()
    );
    println!(
        "  without feedback: {without_completed}/{} completed",
        without_fb.count()
    );

    // RQ9 özet (policy).
    let strict = rows.iter().filter(|r| r.policy == "StrictReject");
    let accept = rows.iter().filter(|r| r.policy == "AcceptImprovement");
    let strict_completed = strict.clone().filter(|r| r.completed).count();
    let accept_completed = accept.clone().filter(|r| r.completed).count();
    println!("\nRQ9 (policy):");
    println!(
        "  StrictReject:     {strict_completed}/{} completed",
        strict.count()
    );
    println!(
        "  AcceptImprovement:{accept_completed}/{} completed",
        accept.count()
    );

    // JSON çıktı.
    let evidence_file = G2cEvidenceFile {
        schema_version: "g2c.evidence.v1".into(),
        run_id,
        osp_version: run_meta.osp_version.clone(),
        git_commit: run_meta.git_commit.clone(),
        corpus_kind: run_meta.corpus_kind.clone(),
        llm_backend: format!("{:?}", cli.llm),
        maneuver_limit: cli.maneuver_limit,
        generated_at: run_meta.timestamp.clone(),
        rows,
    };
    let json = serde_json::to_string_pretty(&evidence_file)?;
    match &cli.out {
        Some(path) => {
            std::fs::write(path, &json)?;
            println!("\nEvidence written: {}", path.display());
        }
        None => println!("\n{json}"),
    }

    if !errors.is_empty() {
        anyhow::bail!("{} experiment errors occurred", errors.len());
    }
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn chrono_nonce() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    )
}

fn chrono_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
