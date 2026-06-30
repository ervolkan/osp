//! OSP CLI — truth surface (Aşama F1).
//!
//! "CLI = truth surface. UI/MCP/SDK ne yaparsa yapsın, en altta CLI/osp-core aynı sonucu
//! üretmeli." CLI çalıştıran insan = operator (INV-T2). Agent decomposition yapamaz,
//! hedef koordinat göremez (INV-T1).
//!
//! Komutlar: analyze, trajectory (init/attempt), task (view), evidence export.
//! D1'de MockLlmClient; D3'te RuntimeLlmClient (osp-llm-runtime adapter).

use clap::{Parser, Subcommand};

mod commands;
mod mock_llm;

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
    }
}
