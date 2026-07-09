//! CLI hata tipleri — store I/O + review application.

use std::path::PathBuf;

/// Store I/O hatası — persistence envelope (lock, atomic replace, serde, schema).
#[derive(Debug, thiserror::Error)]
pub enum StoreIoError {
    #[error("invalid store path (no parent/filename): {0}")]
    InvalidStorePath(PathBuf),
    #[error("cannot acquire store lock at {path}: {source}")]
    LockAcquire {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot read store at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot deserialize store at {path}: {source}")]
    Deserialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("cannot serialize store: {source}")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("cannot write tmp file at {path}: {source}")]
    WriteTmp {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot atomically replace {from} → {to}: {source}")]
    AtomicReplace {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Envelope store_schema_version uyumsuz (osp-core `SnapshotError` ayrı — graph-seviye).
    #[error("unsupported store schema version: expected={expected}, found={found}")]
    UnsupportedStoreSchema { expected: u32, found: u32 },
}

/// Review application hatası — domain transition (basis freshness, promotability, store).
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("node not found: {0}")]
    NotFound(String),
    #[error("stale basis: node changed after operator reviewed it (TOCTOU)")]
    StaleBasis,
    #[error("not promotable: {0}")]
    NotPromotable(String),
    /// Store-level hata (osp-core `StoreError` veya `SnapshotError`) sarmalanmış.
    #[error("store error: {0}")]
    Store(String),
    /// Persistence katmanı hatası (lock/atomic replace/serde).
    #[error("persistence error: {0}")]
    Persistence(#[from] StoreIoError),
    // ─── Supersession-specific (Review 2.tur R1#2 + #4) ─────────────────────────
    /// Endpoint var ama Accepted/current mainline değil (R1#2 — NotFound'dan ayrı).
    /// `status` application precheck'ten Some(gerçek status); core fallback'ten None
    /// (lock altında tautological — CLI precheck önce). R3#5: None durumunda parantez yok.
    #[error("{endpoint} endpoint is not current Accepted: {id}{formatted_status}")]
    EndpointNotCurrent {
        endpoint: SupersedeEndpoint,
        id: String,
        /// " (status: Rejected)" formatında (parantez dahil) veya "" — R3#5.
        formatted_status: String,
    },
    /// Superseded endpoint değişti (R1#4 — endpoint-specific stale).
    #[error("stale superseded basis: superseded node changed after operator reviewed it")]
    StaleSupersededBasis,
    /// Successor endpoint değişti (R1#4).
    #[error("stale successor basis: successor node changed after operator reviewed it")]
    StaleSuccessorBasis,
    /// old == new (self-supersede).
    #[error("self-supersede forbidden: {0}")]
    SelfSupersede(String),
    /// Endpoint'in zaten committed incoming Supersedes edge'i var (INV-C15 cardinality).
    #[error("node already superseded (committed incoming edge exists): {0}")]
    AlreadySuperseded(String),
    /// Endpoint kind/family uyumsuz (G1 — 4 alan: kind×2 + family×2; family-kaynaklı yakalanır).
    #[error(
        "incompatible supersede endpoints: superseded=(kind={superseded_kind}, family={superseded_family}), successor=(kind={successor_kind}, family={successor_family})"
    )]
    IncompatibleSupersedeEndpoints {
        superseded_kind: String,
        successor_kind: String,
        superseded_family: String,
        successor_family: String,
    },
    /// Committed supersede zincirinde cycle (INV-C15 cycle absence).
    #[error("supersede cycle: {superseded} →* {successor} path exists")]
    SupersedeCycle {
        superseded: String,
        successor: String,
    },
}

/// Supersede endpoint rolü — NotFound vs EndpointNotCurrent ayrımı + endpoint-specific stale (R1#2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupersedeEndpoint {
    Superseded,
    Successor,
}

impl std::fmt::Display for SupersedeEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Superseded => write!(f, "superseded"),
            Self::Successor => write!(f, "successor"),
        }
    }
}

/// `EndpointNotCurrent` için status format helper — R3#5 (None durumunda parantez yok).
pub fn format_endpoint_status(status: Option<&str>) -> String {
    match status {
        Some(s) => format!(" (status: {s})"),
        None => String::new(),
    }
}

/// Review işleminin sonucu (mutation — revision bilmez; revision envelope seviyesinde).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewMutation {
    pub status: String,
    pub node_id: String,
    pub decision_sequence: u64,
}

/// Persisted review sonucu — domain mutation + persistence revision.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PersistedReviewOutput {
    pub mutation: ReviewMutation,
    pub revision: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Supersession types — ayrı command/output (accept/reject ReviewMutationCommand'ını
// ve output kontratını kirletmez; Review 2.tur R1#1 + R3).
// ═══════════════════════════════════════════════════════════════════════════════

use osp_core::anchoring::review::NodeDigest;
use osp_core::anchoring::types::ConceptNodeId;

/// Named digest pair (R1#3/R2-R2 — tuple swap bug yok; sıra açık).
#[derive(Debug, Clone)]
pub struct SupersedeDigests {
    pub superseded: NodeDigest,
    pub successor: NodeDigest,
}

/// Supersede komutu — ayrı tip (`ReviewMutationCommand` accept/reject'te kalır).
#[derive(Debug, Clone)]
pub struct SupersedeCommand {
    pub superseded: ConceptNodeId,
    pub successor: ConceptNodeId,
    pub expected: SupersedeDigests,
    pub reason: String,
}

/// Supersede mutation sonucu — iki endpoint (accept/reject şemasını kirletmez).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewSupersedeMutation {
    pub status: String,
    pub superseded_node_id: String,
    pub successor_node_id: String,
    pub decision_sequence: u64,
}

/// Persisted supersede sonucu — named output (raw tuple değil; R1#1).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PersistedSupersedeOutput {
    pub mutation: ReviewSupersedeMutation,
    pub revision: u64,
}
