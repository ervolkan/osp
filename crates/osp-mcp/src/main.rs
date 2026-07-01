//! OSP MCP Server — binary entrypoint (`docs/mcp-design.md` §3).
//!
//! stdio transport, tokio async runtime. MCP client (Claude/Cursor) JSON-RPC over
//! stdio ile tool çağırır, server `osp-core` API'sine delegate eder.
//!
//! ```bash
//! # Agent mode + mock LLM (default — offline/CI güvenli, OPENAI_API_KEY yok)
//! osp-mcp --workspace P:/repos/osp-spike/svelte
//!
//! # Operator mode + gerçek LLM (GPT-4o-mini, OPENAI_API_KEY gerekir)
//! osp-mcp --mode operator --llm real --workspace P:/repos/x
//!
//! # SCIP index ile (gerçek LCOM4 cohesion)
//! osp-mcp --workspace P:/repos/x --scip P:/repos/x/index.scip
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use osp_core::navigator::LlmClient;
use osp_mcp::mode::ServerMode;
use osp_mcp::workspace::{Workspace, WorkspaceError};
use osp_mcp::OspMcpServer;
use rmcp::transport::stdio;
use rmcp::ServiceExt;

/// OSP MCP Server — agent access surface (INV-T1..T8 enforced).
#[derive(Parser, Debug)]
#[command(
    name = "osp-mcp",
    version,
    about = "Ontological Space Protocol — MCP server (agent access surface)"
)]
struct Cli {
    /// Server mode: agent (default) veya operator.
    #[arg(long, value_enum, default_value_t = ServerModeArg::Agent)]
    mode: ServerModeArg,
    /// LLM backend: mock (default, offline) veya real (GPT-4o-mini, OPENAI_API_KEY gerekir).
    #[arg(long, value_enum, default_value_t = LlmBackendArg::Mock)]
    llm: LlmBackendArg,
    /// Workspace path (repo root). Agent raw path VEREMEZ — startup'ta alınır.
    #[arg(long)]
    workspace: PathBuf,
    /// SCIP index path (opsiyonel — gerçek LCOM4 cohesion için).
    #[arg(long)]
    scip: Option<PathBuf>,
}

/// clap value enum (ServerMode'u CLI'da kullanılabilir yap).
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ServerModeArg {
    Agent,
    Operator,
}

impl From<ServerModeArg> for ServerMode {
    fn from(arg: ServerModeArg) -> Self {
        match arg {
            ServerModeArg::Agent => ServerMode::Agent,
            ServerModeArg::Operator => ServerMode::Operator,
        }
    }
}

/// clap value enum — LLM backend seçimi (CLI pattern'ı ile aynı).
#[derive(Debug, Clone, Copy, ValueEnum)]
enum LlmBackendArg {
    /// Scripted/static mock — offline, CI güvenli, OPENAI_API_KEY gerekmez.
    Mock,
    /// Gerçek OpenAI-compatible LLM (GPT-4o-mini). OPENAI_API_KEY gerekir.
    Real,
}

/// Startup'ta LLM client kur (`--llm mock|real`). Mock offline güvenli, real API key ister.
fn build_llm_client(backend: LlmBackendArg) -> anyhow::Result<Arc<dyn LlmClient>> {
    match backend {
        LlmBackendArg::Mock => {
            // Mock: scripted proposals ile navigator loop test edilebilir.
            // Boş proposal listesi → navigator ilk complete()'te NoMoreProposals döner.
            // Operator task_add ile gerçek task + mock proposals JSON yüklenebilir (G2c).
            tracing::info!("LLM backend: mock (offline, scripted)");
            Ok(Arc::new(
                osp_core::navigator::MockLlmClient::new(Vec::new()),
            ))
        }
        LlmBackendArg::Real => {
            tracing::info!("LLM backend: real (GPT-4o-mini via OPENAI_API_KEY)");
            let client = osp_llm_runtime::RuntimeLlmClient::from_env()
                .map_err(|e| anyhow::anyhow!("LLM runtime init failed (OPENAI_API_KEY?): {e}"))?;
            Ok(Arc::new(client))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging — tracing. MCP stdio için stderr'e log (stdout JSON-RPC için ayrılmış).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let cli = Cli::parse();
    let mode: ServerMode = cli.mode.into();

    tracing::info!(
        workspace = ?cli.workspace,
        mode = mode.as_str(),
        llm = ?cli.llm,
        "osp-mcp starting"
    );

    // 1. Workspace analyze (startup — analyze-once).
    let workspace = match Workspace::analyze(&cli.workspace, cli.scip.as_deref()) {
        Ok(ws) => ws,
        Err(WorkspaceError::PathNotFound(p)) => {
            anyhow::bail!("workspace path does not exist: {}", p.display());
        }
        Err(WorkspaceError::NotADirectory(p)) => {
            anyhow::bail!("workspace path is not a directory: {}", p.display());
        }
        Err(WorkspaceError::Analyze(e)) => {
            anyhow::bail!("workspace analyze failed: {e}");
        }
        Err(e) => anyhow::bail!("workspace error: {e}"),
    };
    tracing::info!(
        nodes = workspace.node_count,
        edges = workspace.edge_count,
        coverage = workspace.semantic_coverage.coverage_ratio,
        "workspace analyzed"
    );

    // 2. LLM client kur (startup inject — INV-T2 pattern: trusted bootstrap).
    let llm = build_llm_client(cli.llm)?;

    // 3. Server kur.
    let server = OspMcpServer::new(workspace, mode, llm);
    tracing::info!(mode = mode.as_str(), "osp-mcp server ready (stdio)");

    // 4. stdio transport ile serve (rmcp ServiceExt).
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
