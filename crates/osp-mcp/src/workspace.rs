//! Startup workspace (`docs/mcp-design.md` §4 — ilk sürüm).
//!
//! AI agent raw `repo_path` ALAMAZ (path traversal riski). MCP server startup'ta
//! `--workspace <path>` alır, analyze eder, [`Workspace`] state olarak saklar.
//! Tüm tool'lar bu tek workspace üzerinde çalışır — agent path geçemez.
//!
//! **Workspace security:**
//! - Path canonicalize → resolve symlink/`..`.
//! - Mevcut directory exists kontrolü.
//! - `to_str()` ile güvenli display path.
//!
//! **Analyze-once:** Server startup'ta analyze edilir, SpaceEngine in-memory saklanır.
//! Tool'lar her çağrıda re-analyze ETMEZ — performans + determinizm için.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use osp_analyzer::contract::{AnalysisConfig, RepoMetrics, SemanticCoverage};
use osp_analyzer::language::AdapterRegistry;
use osp_analyzer::pipeline::analyze_repo_with_config;
use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
use osp_core::coords::{CoordinateSystem, MetricSource};
use osp_core::engine::{EngineConfig, SpaceEngine};
use osp_core::vision::VisionVector;

/// Workspace hatası — startup veya re-analyze sırasında.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace path does not exist: {0}")]
    PathNotFound(PathBuf),
    #[error("workspace path is not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("analyze failed: {0}")]
    Analyze(String),
    #[error("workspace not analyzed yet — call analyze() first")]
    NotAnalyzed,
}

/// Startup workspace — analyze edilmiş SpaceEngine + analyzer result.
///
/// **Concurrency:** MCP server tek bir `Arc<Mutex<Workspace>>` paylaşır. rmcp handler'lar
/// async (tokio), ama osp-core sync — `std::sync::Mutex` yeterli (konsol kapanmaz).
pub struct Workspace {
    /// Canonical path (symlink/`..` resolve edilmiş).
    pub path: PathBuf,
    /// Analyze edilmiş SpaceEngine (Q4-Q6 commit pipeline hazır).
    engine: SpaceEngine,
    /// Repo-level metrikler (abstractness, main_sequence_distance).
    pub repo_metrics: RepoMetrics,
    /// SCIP coverage (kalite — placeholder metric uyarısı için).
    pub semantic_coverage: SemanticCoverage,
    /// Node sayısı (space snapshot — agent'a "kaç node var" verir).
    pub node_count: usize,
    /// Edge sayısı.
    pub edge_count: usize,
}

impl Workspace {
    /// Yeni workspace kur + analyze et (startup'ta çağrılır).
    ///
    /// **Akış:**
    /// 1. Path canonicalize + exists kontrolü
    /// 2. AdapterRegistry::default_all() — 5 dil
    /// 3. analyze_repo_with_config → space + metrics + coverage
    /// 4. CoordinateSystem + Vision + EngineConfig → SpaceEngine::with_default_rules
    pub fn analyze(path: &Path, scip_index: Option<&Path>) -> Result<Self, WorkspaceError> {
        // 1. Path validation (security).
        let canonical = path
            .canonicalize()
            .map_err(|_| WorkspaceError::PathNotFound(path.to_path_buf()))?;
        if !canonical.is_dir() {
            return Err(WorkspaceError::NotADirectory(canonical));
        }

        // 2. Analyze.
        let registry = AdapterRegistry::default_all();
        let config = AnalysisConfig {
            scip_index: scip_index.map(|p| p.to_path_buf()),
            ..Default::default()
        };
        let result = analyze_repo_with_config(&canonical, &registry, &config)
            .map_err(|e| WorkspaceError::Analyze(e.to_string()))?;
        let node_count = result.space.nodes.len();
        let edge_count = result.space.edges.len();
        let repo_metrics = result.repo_metrics.clone();
        let semantic_coverage = result.semantic_coverage.clone();

        // 3. Engine kur (D2 calibrated — osp-cli ve osp-desktop ile aynı).
        // **INV-T9 Adım 3:** default_raw_five artık validated Result döner.
        // **INV-T9 #70:** production topology_source = TreeSitter, observed cohesion = Scip.
        let cohesion = CohesionAxis::try_with_observed_source(MetricSource::Scip)
            .map_err(|e| WorkspaceError::Analyze(format!("cohesion axis source: {e}")))?;
        let cs = CoordinateSystem::default_raw_five(
            MetricSource::TreeSitter,
            cohesion,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .map_err(|e| WorkspaceError::Analyze(format!("axis registration failed: {e}")))?;
        // Default vision — Aşama C'de operator override edebilir.
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
        .map_err(|e| WorkspaceError::Analyze(format!("engine rule registration failed: {e}")))?;

        Ok(Self {
            path: canonical,
            engine,
            repo_metrics,
            semantic_coverage,
            node_count,
            edge_count,
        })
    }

    /// Engine'e mutable reference al (commit_task_claim için — osp-core sync).
    pub fn engine_mut(&mut self) -> &mut SpaceEngine {
        &mut self.engine
    }

    /// Workspace snapshot — agent'a "ne var" özeti (node/edge count + coverage).
    pub fn snapshot_summary(&self) -> serde_json::Value {
        serde_json::json!({
            "workspace_path": self.path.to_string_lossy(),
            "node_count": self.node_count,
            "edge_count": self.edge_count,
            "repo_metrics": {
                "abstractness": self.repo_metrics.abstractness.value,
                "main_sequence_distance": self.repo_metrics.main_sequence_distance.value,
            },
            "semantic_coverage": {
                "files_total": self.semantic_coverage.files_total,
                "files_with_scip": self.semantic_coverage.files_with_scip,
                "coverage_ratio": self.semantic_coverage.coverage_ratio,
            },
        })
    }
}

/// Shared workspace handle — `Arc<Mutex<Workspace>>`. MCP server handler bunu tutar,
/// her tool call'da lock'lar. rmcp async, osp-core sync → std::sync::Mutex yeterli.
pub type SharedWorkspace = Arc<Mutex<Workspace>>;
