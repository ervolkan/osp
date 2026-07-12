//! OSP CLI — truth surface (Aşama F1).
//!
//! "CLI = truth surface. UI/MCP/SDK ne yaparsa yapsın, en altta CLI/osp-core aynı sonucu
//! üretmeli." CLI operator-facing yüzeydir (INV-T2 attribution); çağıranın gerçekten
//! operator olduğunun doğrulanması deployment/authentication sorumluluğudur (INV-C11).
//! Agent decomposition yapamaz, hedef koordinat göremez (INV-T1).
//!
//! Komutlar: analyze, trajectory (init/attempt), task (view), evidence export,
//! graph (init/status/validate), review (list/show/accept/reject/session).
//! D1'de MockLlmClient; D3'te RuntimeLlmClient (osp-llm-runtime adapter).

use clap::{Parser, Subcommand};

mod analysis_bridge;
mod application;
mod canonical_identity;
mod commands;
mod errors;
mod evidence_projection;
mod graph_seed_builder;
mod identity_bridge;
mod metric_projection;
mod mock_llm;
mod review_session;
mod seed_file;
mod store_io;

/// OSP — Ontological Space Protocol CLI (truth surface).
#[derive(Parser, Debug)]
#[command(
    name = "osp",
    version,
    about = "Ontological Space Protocol — architecture trajectory CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Repo'yu analiz et → space snapshot.
    Analyze(commands::AnalyzeArgs),
    /// Trajectory işlemleri (init, attempt).
    Trajectory {
        #[command(subcommand)]
        action: TrajectoryAction,
    },
    /// Task işlemleri (view).
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Evidence ledger export.
    Evidence(commands::EvidenceArgs),
    /// Concept graph işlemleri (init, status, validate) — Candidate-only bootstrap.
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },
    /// Operator review (list, show, accept, reject) — Candidate → Accepted/Rejected.
    /// Argümansız `osp review` interactive wizard açar (default store + operator prompt).
    /// Subcommand'lar kendi --store/--operator flag'lerini taşır; root flag yoktur
    /// (sessiz yok sayılma riski — Review 2.tur P1.1).
    Review {
        #[command(subcommand)]
        action: Option<ReviewAction>,
    },
}

#[derive(Subcommand, Debug)]
enum TrajectoryAction {
    /// Trajectory + SpaceEngine kur (analyze + coord system + vision).
    Init(commands::TrajectoryInitArgs),
    /// Bir task için navigator attempt (D2 navigator, MockLlmClient).
    Attempt(commands::TrajectoryAttemptArgs),
}

#[derive(Subcommand, Debug)]
enum TaskAction {
    /// Task'ın AgentTaskView'ını göster (INV-T1 — preferred_vector ASLA).
    View(commands::TaskViewArgs),
}

#[derive(Subcommand, Debug)]
enum GraphAction {
    /// Candidate seed JSON → trusted store (nodes-only bootstrap).
    Init(commands::graph::GraphInitArgs),
    /// Store durumu (node/edge/ledger counts, audit_seq).
    Status(commands::graph::GraphStatusArgs),
    /// Restore + invariant-validasyon (read-only).
    Validate(commands::graph::GraphValidateArgs),
}

#[derive(Subcommand, Debug)]
enum ReviewAction {
    /// Candidate lane'i listele.
    List(commands::review::ReviewListArgs),
    /// Node detayı + basis digest (Candidate için).
    Show(commands::review::ReviewShowArgs),
    /// Candidate → Accepted (OperatorReviewSession, informed basis).
    Accept(commands::review::ReviewAcceptArgs),
    /// Candidate → Rejected.
    Reject(commands::review::ReviewRejectArgs),
    /// Accepted → SupersededAccepted (iki-endpoint supersession).
    Supersede(commands::review::ReviewSupersedeArgs),
    /// Rich supersede preview — read-only lineage DAG + compatibility + eligibility.
    SupersedePreview(commands::review::ReviewSupersedePreviewArgs),
    /// PR E2 — Accepted CodeEntityCandidate → CodeEntity identity resolution (tek-endpoint).
    ResolveCodeEntity(commands::review::ReviewResolveCodeEntityArgs),
    /// PR E2 — minimal canonical resolution preview (target reveal: Create/Reuse).
    ResolveCodeEntityPreview(commands::review::ReviewResolveCodeEntityPreviewArgs),
    /// Interactive wizard — custom store/operator ile (argümansız `osp review` default kullanır).
    Session(commands::review::ReviewSessionArgs),
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze(args) => commands::run_analyze(args),
        Commands::Trajectory { action } => match action {
            TrajectoryAction::Init(args) => commands::run_trajectory_init(args),
            TrajectoryAction::Attempt(args) => commands::run_trajectory_attempt(args),
        },
        Commands::Task { action } => match action {
            TaskAction::View(args) => commands::run_task_view(args),
        },
        Commands::Evidence(args) => commands::run_evidence_export(args),
        Commands::Graph { action } => match action {
            GraphAction::Init(args) => commands::graph::run_graph_init(args),
            GraphAction::Status(args) => commands::graph::run_graph_status(args),
            GraphAction::Validate(args) => commands::graph::run_graph_validate(args),
        },
        Commands::Review { action } => match action {
            None => commands::review::run_review_session_default(),
            Some(ReviewAction::List(args)) => commands::review::run_review_list(args),
            Some(ReviewAction::Show(args)) => commands::review::run_review_show(args),
            Some(ReviewAction::Accept(args)) => commands::review::run_review_accept(args),
            Some(ReviewAction::Reject(args)) => commands::review::run_review_reject(args),
            Some(ReviewAction::Supersede(args)) => commands::review::run_review_supersede(args),
            Some(ReviewAction::SupersedePreview(args)) => {
                commands::review::run_review_supersede_preview(args)
            }
            Some(ReviewAction::ResolveCodeEntity(args)) => {
                commands::review::run_review_resolve_code_entity(args)
            }
            Some(ReviewAction::ResolveCodeEntityPreview(args)) => {
                commands::review::run_review_resolve_code_entity_preview(args)
            }
            Some(ReviewAction::Session(args)) => commands::review::run_review_session(args),
        },
    }
}
