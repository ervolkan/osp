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
    // ─── Resolution-specific (PR E2 — NotFound reuse; tur 1 review karar #4) ────
    /// Candidate Accepted değil (tur 3 P1-3 — `NotPromotableFrom(non-Accepted)` map).
    #[error("candidate is not Accepted: {id} (status: {status})")]
    CandidateNotAccepted { id: String, status: String },
    /// Candidate digest değişti (tur 2 P0-1 — candidate TOCTOU).
    #[error("stale resolution basis: candidate changed after operator reviewed it")]
    StaleResolutionBasis,
    /// Candidate zaten resolve edilmiş (R6 — outgoing ResolvesTo mevcut).
    #[error("candidate already resolved (outgoing ResolvesTo exists): {0}")]
    AlreadyResolved(String),
    /// Operator'ın gördüğü target drift etti (tur 2 P0-2 — Create/Reuse outcome changed).
    #[error("stale resolution target: outcome drifted (re-run resolve-code-entity-preview)")]
    StaleResolutionTarget,
    /// Reuse target inactive (Rejected/Deprecated/SupersededAccepted) — Create'e düşmez.
    #[error("entity not live for resolution: {entity_id} (status: {status})")]
    EntityNotLiveForResolution { entity_id: String, status: String },
    /// Hash collision fail-closed (aynı ID + farklı material/key).
    #[error("entity identity collision: {0} (different material)")]
    EntityIdentityCollision(String),
    /// Aynı identity key için >1 live CodeEntity (R7 violation).
    #[error("duplicate live entity for this identity key (R7 violation)")]
    DuplicateLiveEntity,
    /// Candidate için identity binding yok (binding seeding yapılmamış).
    #[error("missing identity binding for candidate: {0}")]
    MissingIdentityBinding(String),
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

// ═══════════════════════════════════════════════════════════════════════════════
// PR E2 — Resolution types (resolve-code-entity; supersede pattern tek-endpoint)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tur 2 P0-2 — operator-pinned target. Confirmation'da görülen tam target command'e taşınır.
/// Mutation lock altında re-compile edilen target ↔ expected karşılaştırılır (StaleResolutionTarget).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedResolutionTarget {
    Create {
        proposed_entity_id: ConceptNodeId,
    },
    Reuse {
        entity_id: ConceptNodeId,
        entity_digest: NodeDigest,
    },
}

/// `osp review resolve-code-entity <candidate>` tek-endpoint mutation command.
///
/// Tur 2 P0-2: candidate digest + expected target ikili pinning (core `StaleResolutionTarget`
/// garantisi operator presentation sınırına taşınır).
#[derive(Debug, Clone)]
pub struct ResolveCodeEntityCommand {
    pub candidate: ConceptNodeId,
    pub expected_candidate_digest: NodeDigest,
    /// Tur 2 P0-2 — operator-pinned target (Create proposed_entity_id / Reuse entity_id + digest).
    pub expected_target: ExpectedResolutionTarget,
    pub reason: String,
}

/// Tur 2 P2-A — typed outcome (String değil; anti-corruption CLI projection).
/// Tur 3 P2-4 — `as_str()` text output ile JSON terminolojiyi hizalar (Debug değil).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcomeView {
    Created,
    Reused,
}

impl ResolutionOutcomeView {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Reused => "reused",
        }
    }
}

/// Resolution mutation sonucu — tek candidate + resolved entity (supersede iki-endpoint'ten farklı).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolveCodeEntityMutation {
    pub status: String,
    pub candidate_node_id: String,
    pub entity_node_id: String,
    /// Tur 2 P2-A / tur 3 P2-4 — typed enum (String değil).
    pub outcome: ResolutionOutcomeView,
    pub resolution_sequence: u64,
}

/// Persisted resolution sonucu — named output.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PersistedResolveCodeEntityOutput {
    pub mutation: ResolveCodeEntityMutation,
    pub revision: u64,
}
