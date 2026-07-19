//! OSP CLI komut handler'ları. osp-core API'sini çağırır — CLI = truth surface.
//!
//! Pattern: osp-desktop cmd_simulate_claim (lib.rs:257-278) reuse —
//! analyze_repo_with_config → CoordinateSystem::default_raw_five → SpaceEngine.

use std::path::PathBuf;

use clap::Args;
use osp_analyzer::contract::AnalysisConfig;
use osp_analyzer::language::AdapterRegistry;
use osp_analyzer::pipeline::analyze_repo_with_config;

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 — CLI exit-code contract (snapshot testlerle sabitlenir)
// ═══════════════════════════════════════════════════════════════════════════════

/// Navigator trajectory attempt exit codes.
///
/// Bu kodlar sabittir — downstream tooling (CI, scripts) bunlara güvenebilir.
/// Yeni kod eklemek mümkündür ama mevcut kodların anlamı değişmez.
pub mod exit_codes {
    /// Task completed — predicate satisfied, mainline applied.
    pub const COMPLETED: i32 = 0;
    /// INV-T9 — witness authorization bekleme (expected domain outcome, hata DEĞİL).
    pub const AWAITING_WITNESSES: i32 = 10;
    /// Explicit witness rejection — agent must revise proposal.
    pub const REQUIRES_REVISION: i32 = 11;
    /// INV-T7 — maneuver limit aşıldı (agent-correctable retryable failures tükendi).
    pub const EXCEEDED_MANEUVER_LIMIT: i32 = 12;
    /// Critical domain — insan review gerekli.
    pub const REQUIRES_OPERATOR_APPROVAL: i32 = 13;
    /// Invalid witness evidence — operational fault (malformed/author-self/duplicate).
    pub const WITNESS_EVALUATION_ERROR: i32 = 20;
    /// Pending authorization persistence failure — terminal (non-retryable).
    pub const PENDING_AUTHORIZATION_PERSISTENCE_FAILURE: i32 = 40;
    /// System failure — persistence/internal error. Terminal.
    pub const SYSTEM_FAILURE: i32 = 70;
    /// Task resolver'da bulunamadı.
    pub const TASK_NOT_FOUND: i32 = 80;
    /// LLM hatası (NoMoreProposals, parse, network).
    pub const LLM_ERROR: i32 = 90;
}

pub mod graph;
pub(crate) mod resolve_code_entity_preview_render;
pub mod review;
pub(crate) mod supersede_preview_render;

/// Output format (text/json).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => Self::Json,
            _ => Self::Text,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Komut argüman yapıları
// ═══════════════════════════════════════════════════════════════════════════════

/// `osp analyze <repo>` — repo analiz → space snapshot.
#[derive(Args, Debug)]
pub struct AnalyzeArgs {
    /// Analiz edilecek repo path'i.
    pub repo: PathBuf,
    /// SCIP index path'i (opsiyonel — gerçek LCOM4 cohesion için).
    #[arg(long)]
    pub scip: Option<PathBuf>,
    /// Çıktı JSON dosyası (default: stdout).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// `osp trajectory init --repo <repo>` — SpaceEngine + Trajectory kur.
#[derive(Args, Debug)]
pub struct TrajectoryInitArgs {
    #[arg(long)]
    pub repo: PathBuf,
    #[arg(long)]
    pub scip: Option<PathBuf>,
    /// Vision TOML config (opsiyonel — default builtin).
    #[arg(long)]
    pub vision: Option<PathBuf>,
}

/// `osp trajectory attempt <task-id>` — navigator attempt.
#[derive(Args, Debug)]
pub struct TrajectoryAttemptArgs {
    /// Task ID.
    pub task_id: u64,
    #[arg(long)]
    pub repo: PathBuf,
    /// Scripted proposals JSON (MockLlmClient). --llm mock ile.
    #[arg(long)]
    pub proposals: Option<PathBuf>,
    /// LLM mode: mock (FileMockLlm, --proposals) or real (RuntimeLlmClient, GPT-4o-mini).
    #[arg(long, default_value = "mock")]
    pub llm: String,
    /// Maneuver limit (default 5).
    #[arg(long, default_value = "5")]
    pub maneuver_limit: u32,
}

/// `osp task view <task-id>` — AgentTaskView göster.
#[derive(Args, Debug)]
pub struct TaskViewArgs {
    pub task_id: u64,
    #[arg(long)]
    pub repo: PathBuf,
    /// Predicate threshold (örn "coupling <= 0.55").
    #[arg(long)]
    pub predicate: String,
}

/// `osp evidence export` — evidence ledger JSON.
#[derive(Args, Debug)]
pub struct EvidenceArgs {
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Evidence JSON input (trajectory attempt çıktısı).
    #[arg(long)]
    pub input: Option<PathBuf>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Komut handler'ları
// ═══════════════════════════════════════════════════════════════════════════════

/// `osp analyze` — repo analiz → space snapshot JSON.
pub fn run_analyze(args: AnalyzeArgs) -> anyhow::Result<()> {
    let registry = AdapterRegistry::default_all();
    let config = AnalysisConfig {
        scip_index: args.scip.clone(),
        ..Default::default()
    };
    let result = analyze_repo_with_config(&args.repo, &registry, &config)?;
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "node_count": result.space.nodes.len(),
        "edge_count": result.space.edges.len(),
        "module_metrics_count": result.module_metrics.len(),
        "repo_metrics": {
            "abstractness": result.repo_metrics.abstractness.value,
            "main_sequence_distance": result.repo_metrics.main_sequence_distance.value,
        },
        "semantic_coverage": {
            "files_total": result.semantic_coverage.files_total,
            "files_with_scip": result.semantic_coverage.files_with_scip,
            "coverage_ratio": result.semantic_coverage.coverage_ratio,
        },
    }))?;
    match args.out {
        Some(path) => {
            std::fs::write(&path, &json)?;
            println!("✓ Space snapshot written to {}", path.display());
        }
        None => println!("{json}"),
    }
    Ok(())
}

/// `osp trajectory init` — SpaceEngine kur (analyze + coord system + vision).
pub fn run_trajectory_init(args: TrajectoryInitArgs) -> anyhow::Result<()> {
    use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
    use osp_core::coords::CoordinateSystem;
    use osp_core::engine::{EngineConfig, SpaceEngine};
    use osp_core::vision::VisionVector;

    let registry = AdapterRegistry::default_all();
    let config = AnalysisConfig {
        scip_index: args.scip.clone(),
        ..Default::default()
    };
    let result = analyze_repo_with_config(&args.repo, &registry, &config)?;
    let cs = CoordinateSystem::default_raw_five(
        CohesionAxis::new(),
        EntropyAxis::from_commit_entropy(6.0),
        WitnessDepthAxis::from_witness(0.3, 5),
    )?;
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
    )?;
    let _ = engine; // engine kuruldu (space private — count analyze'den biliniyor)
    println!("✓ Trajectory initialized");
    println!("  SpaceEngine ready (analyze + coord system + vision)");
    Ok(())
}

/// `osp trajectory attempt` — D2 navigator + MockLlmClient/RuntimeLlmClient.
pub fn run_trajectory_attempt(args: TrajectoryAttemptArgs) -> anyhow::Result<()> {
    use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
    use osp_core::coords::CoordinateSystem;
    use osp_core::engine::{EngineConfig, SpaceEngine};
    use osp_core::vision::VisionVector;

    // 1. Analyze -> space.
    let registry = AdapterRegistry::default_all();
    let config = AnalysisConfig::default();
    let result = analyze_repo_with_config(&args.repo, &registry, &config)?;
    // 2. Engine (D2 gerçek measure).
    let cs = CoordinateSystem::default_raw_five(
        CohesionAxis::new(),
        EntropyAxis::from_commit_entropy(6.0),
        WitnessDepthAxis::from_witness(0.3, 5),
    )?;
    let vision = VisionVector::new(osp_core::coords::RawPosition {
        x: 0.4,
        y: 0.6,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    });
    let mut engine = SpaceEngine::with_default_rules(
        result.space,
        cs,
        vision,
        EngineConfig::default_calibrated(),
    )?;
    // 3. LLM seçimi: mock (FileMockLlm) veya real (RuntimeLlmClient, GPT-4o-mini).
    match args.llm.as_str() {
        "real" => {
            let llm = osp_llm_runtime::RuntimeLlmClient::from_env()
                .map_err(|e| anyhow::anyhow!("LLM runtime (OPENAI_API_KEY?): {e}"))?;
            run_navigator(&llm, &mut engine, &args)?;
        }
        _ => {
            // mock (default)
            let proposals_path = args
                .proposals
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--proposals required for --llm mock"))?;
            let proposals_json = std::fs::read_to_string(proposals_path)?;
            let proposals: Vec<osp_core::agent::DeltaProposal> =
                serde_json::from_str(&proposals_json)?;
            let llm = crate::mock_llm::FileMockLlm::new(proposals);
            run_navigator(&llm, &mut engine, &args)?;
        }
    }
    Ok(())
}

/// Navigator çalıştır (generic LlmClient — mock veya real).
fn run_navigator<L: osp_core::navigator::LlmClient>(
    llm: &L,
    engine: &mut osp_core::engine::SpaceEngine,
    args: &TrajectoryAttemptArgs,
) -> anyhow::Result<()> {
    use osp_core::navigator::{AgentNavigator, NavigatorResult};
    use osp_core::trajectory::{
        InMemoryTaskRegistry, MilestoneId, OperatorCapability, PredicateFailurePolicy, Task,
        TaskPolicy, TaskStatus, TrajectoryId,
    };
    // 4. Task registry (basit — coupling <= 0.55 predicate).
    // CLI = operator mode (INV-T2) — trusted-boundary API (PR35 hardening).
    let _cap = OperatorCapability::issue_for_operator_session();
    let mut task_registry = InMemoryTaskRegistry::new();
    let mut policy = TaskPolicy::default();
    policy.maneuver_limit = args.maneuver_limit;
    policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
    let task = Task {
        id: args.task_id,
        milestone_id: 1,
        label: "CLI trajectory attempt".into(),
        target_predicate_set: osp_core::trajectory::PredicateSet {
            mode: osp_core::trajectory::PredicateMode::All,
            predicates: vec![osp_core::trajectory::WeightedPredicate {
                predicate: osp_core::trajectory::MetricPredicate {
                    metric: osp_core::trajectory::PredicateAxis::Coupling,
                    operator: osp_core::trajectory::ComparisonOp::Le,
                    threshold: 0.55,
                    scope: osp_core::trajectory::PredicateScope::Node(0),
                    required_source: Some(osp_core::coords::MetricSource::Scip),
                    tolerance: 0.0,
                },
                weight: None,
            }],
            preferred_vector: Some(osp_core::coords::RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            }),
        },
        policy,
        allowed_operations: vec![osp_core::trajectory::OpKind::RemoveImport],
        constraints: vec![],
        status: TaskStatus::Pending,
    };
    task_registry.insert(task);
    // 5. Navigator.
    let current_measured = osp_core::navigator::provenanced_from_raw(
        osp_core::coords::RawPosition {
            x: 0.7,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.3,
        },
        osp_core::coords::MetricSource::Scip,
    );
    let mut evidence = vec![];
    let mut nav = AgentNavigator {
        llm,
        resolver: &task_registry,
        engine,
        evidence: &mut evidence,
        trajectory_id: 1 as TrajectoryId,
        milestone_id: 1 as MilestoneId,
        target_vector: osp_core::coords::RawPosition {
            x: 0.55,
            y: 0.6,
            z: 0.4,
            w: 0.5,
            v: 0.3,
        },
        current_measured,
        output_contract: osp_core::agent::OutputContract::strict(),
        // CLI = production → Production witness (min_approvers=2, Paper 1 güven modeli).
        witness_policy: osp_core::navigator::NavigatorWitnessPolicy::Production,
        // INV-T9: production filesystem store — cwd altında .osp/pending-authorizations/.
        pending_authorization_store: Box::new(
            osp_core::authorization::FilesystemPendingAuthorizationStore::new("."),
        ),
        clock: Box::new(osp_core::authorization::SystemClock),
    };
    let result = nav.run_task(args.task_id, 1);
    // 6. Sonuç yazdır + exit code.
    let exit_code = match result {
        NavigatorResult::Completed {
            attempts,
            total_tokens,
        } => {
            println!("✓ Task completed in {attempts} attempts");
            println!("  Total tokens: {}", total_tokens.total_tokens);
            exit_codes::COMPLETED
        }
        NavigatorResult::ExceededManeuverLimit { attempts, .. } => {
            println!("✗ Maneuver limit exceeded after {attempts} attempts");
            exit_codes::EXCEEDED_MANEUVER_LIMIT
        }
        NavigatorResult::AwaitingWitnesses {
            pending,
            persistence,
        } => {
            // **INV-T9** — expected authorization bekleme. Domain outcome, hata DEĞİL.
            println!(
                "⏸ Awaiting witnesses (INV-T9) — task {}, claim {}",
                pending.task_id, pending.claim_id
            );
            println!(
                "  Witness hold reason: {}",
                pending.witness_hold_reason.as_reason_str()
            );
            println!("  Commit state: awaiting_witnesses");
            println!("  Mainline mutation: not_applied");
            println!("  Next action: await external evidence");
            println!(
                "  Pending artifact: {}",
                persistence.artifact_path.display()
            );
            exit_codes::AWAITING_WITNESSES
        }
        NavigatorResult::RequiresRevision(rev) => {
            println!(
                "↻ Requires revision (explicit witness rejection) — task {}, claim {}",
                rev.task_id(),
                rev.claim_id()
            );
            exit_codes::REQUIRES_REVISION
        }
        NavigatorResult::PendingAuthorizationPersistenceFailure { pending, error } => {
            println!(
                "✗ Pending authorization persistence failed — task {}, claim {}: {error}",
                pending.task_id, pending.claim_id
            );
            exit_codes::PENDING_AUTHORIZATION_PERSISTENCE_FAILURE
        }
        NavigatorResult::WitnessEvaluationError(msg) => {
            println!("✗ Witness evaluation error: {msg}");
            exit_codes::WITNESS_EVALUATION_ERROR
        }
        NavigatorResult::SystemFailure(msg) => {
            println!("✗ System failure: {msg}");
            exit_codes::SYSTEM_FAILURE
        }
        NavigatorResult::TaskNotFound => {
            println!("✗ Task {} not found", args.task_id);
            exit_codes::TASK_NOT_FOUND
        }
        NavigatorResult::RequiresOperatorApproval { attempts, .. } => {
            println!("⚠ Operator approval required after {attempts} attempts");
            exit_codes::REQUIRES_OPERATOR_APPROVAL
        }
        NavigatorResult::LlmError(e) => {
            println!("✗ LLM error: {e}");
            exit_codes::LLM_ERROR
        }
    };
    println!("  Evidence entries: {}", evidence.len());
    if !evidence.is_empty() {
        let json = serde_json::to_string_pretty(&evidence)?;
        println!("{json}");
    }
    if exit_code != exit_codes::COMPLETED {
        std::process::exit(exit_code);
    }
    Ok(())
}

/// `osp task view` — AgentTaskView göster (INV-T1 — preferred_vector ASLA).
pub fn run_task_view(args: TaskViewArgs) -> anyhow::Result<()> {
    // D1: basit — task view predicate string parse + AgentTaskView üret.
    // Tam implementasyon D2 sonrası (task registry persistence).
    println!("Task {} view (INV-T1 — no preferred_vector):", args.task_id);
    println!("  Predicate: {}", args.predicate);
    println!("  Repo: {}", args.repo.display());
    println!("  (Full AgentTaskView serialization — D2 navigator integration)");
    Ok(())
}

/// `osp evidence export` — evidence ledger JSON.
pub fn run_evidence_export(args: EvidenceArgs) -> anyhow::Result<()> {
    if let Some(input) = args.input {
        let data = std::fs::read_to_string(input)?;
        let json =
            serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(&data)?)?;
        match args.out {
            Some(path) => {
                std::fs::write(&path, &json)?;
                println!("✓ Evidence exported to {}", path.display());
            }
            None => println!("{json}"),
        }
    } else {
        println!("No evidence input provided. Run `osp trajectory attempt` first.");
    }
    Ok(())
}
