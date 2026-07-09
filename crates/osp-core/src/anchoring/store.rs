//! AnchorStore trait + InMemoryAnchorStore + OperatorAcceptance (Faz 1-3, §11, D7).
//!
//! # Faz 3 — AnchorStore abstraction
//! [`AnchorStore`] trait'i `InMemoryAnchorStore` (Faz 0-2) ve `KuzuAnchorStore`
//! (Faz 3, ayrı `osp-kuzu` crate) için ortak abstraction. `osp-core` Kuzu bilmez (D7).
//!
//! INV-C3: `OperatorAcceptance` capability token — `_private: ()` + `pub(crate) issue`.
//! Downstream crate'ler (osp-cli/mcp/osp-kuzu) ve integration test'ler üretemez.
//! Faz 8'de gerçek operator console bu gate'i açer.
//!
//! INV-C5: `apply_plan` her zaman Candidate yazar. Promotion `OperatorReviewSession`
//! ile (INV-C12/C13 denetimli `apply_decision`).

use crate::anchoring::types::{
    AnchorPlan, ConceptEdge, ConceptGraph, ConceptNode, ConceptNodeId, ConceptNodeKind, GraphSeed,
};
use crate::anchoring::{DecisionStatus, PositionFamily};

// ═══════════════════════════════════════════════════════════════════════════════
// OperatorAcceptance — INV-C3 capability token (compile-time gate)
// ═══════════════════════════════════════════════════════════════════════════════

/// Operator kabul yeteneği. INV-C3: Candidate → Accepted geçişi sadece operator.
///
/// # Güvenlik modeli
/// `_private: ()` field'ı private → dış crate'ler construct edemez.
/// `issue_for_tests()` `pub(crate)` → sadece osp-core içi (unit testler).
/// Integration test (`tests/`) ayrı crate derlendiği için erişemez.
/// Downstream (osp-cli/mcp/osp-kuzu) de üretemez. Faz 8 operator API'si bu gate'i gerçek açar.
#[derive(Debug, Clone, Copy)]
pub struct OperatorAcceptance {
    _private: (),
}

impl OperatorAcceptance {
    /// Sadece osp-core içi (unit testler). `pub(crate)` → dış crate erişemez.
    /// Faz 8 operator console gerçek API ile bu gate'i açar.
    #[allow(dead_code)] // osp-core unit test'lerinde kullanılıyor; normal build'de unused
    pub(crate) fn issue_for_tests() -> Self {
        Self { _private: () }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// StoreError — thiserror + serde (Faz 3: Kuzu hataları persist edilebilir)
// ═══════════════════════════════════════════════════════════════════════════════

/// InMemory store hatası.
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum StoreError {
    #[error("node bulunamadı: {0}")]
    NodeNotFound(ConceptNodeId),
    #[error("node zaten Accepted: {0}")]
    AlreadyAccepted(ConceptNodeId),
    #[error("node Candidate değil: {0}")]
    NotCandidate(ConceptNodeId),
    #[error(
        "node {0:?} durumundan promote edilemez (Accepted/Deprecated/SupersededAccepted/Rejected)"
    )]
    NotPromotableFrom(DecisionStatus),
    #[error("basis candidate mismatch: basis={basis}, application={application}")]
    BasisCandidateMismatch {
        basis: ConceptNodeId,
        application: ConceptNodeId,
    },
    #[error("snapshot version uyumsuz: expected={expected}, found={found}")]
    InvalidSnapshotVersion { expected: u32, found: u32 },
    // ─── Faz 8b (PR #49): apply_supersede defense-in-depth ────────────────────
    #[error("supersede basis endpoint mismatch: basis=({basis_superseded}, {basis_successor}), application=({app_superseded}, {app_successor})")]
    SupersedeBasisMismatch {
        basis_superseded: ConceptNodeId,
        basis_successor: ConceptNodeId,
        app_superseded: ConceptNodeId,
        app_successor: ConceptNodeId,
    },
    // u64 digest payload (NodeDigest Serialize-only → Deserialize yok; StoreError Deserialize gerektirir)
    #[error(
        "stale superseded basis: expected_digest={expected_digest}, found_digest={found_digest}"
    )]
    StaleSupersededBasis {
        expected_digest: u64,
        found_digest: u64,
    },
    #[error(
        "stale successor basis: expected_digest={expected_digest}, found_digest={found_digest}"
    )]
    StaleSuccessorBasis {
        expected_digest: u64,
        found_digest: u64,
    },
    #[error("node already superseded (committed incoming Supersedes edge exists): {0}")]
    AlreadySuperseded(ConceptNodeId),
    #[error("node {0:?} supersede edilemez (sadece Accepted)")]
    NotSupersedeableFrom(DecisionStatus),
    #[error("successor node {0:?} Accepted değil")]
    SuccessorNotAccepted(DecisionStatus),
    #[error("self-supersede yasak: {0}")]
    SelfSupersede(ConceptNodeId),
    #[error("incompatible supersede endpoints: superseded=(kind={superseded_kind:?}, family={superseded_family:?}), successor=(kind={successor_kind:?}, family={successor_family:?})")]
    IncompatibleSupersedeEndpoints {
        superseded_kind: ConceptNodeKind,
        successor_kind: ConceptNodeKind,
        superseded_family: PositionFamily,
        successor_family: PositionFamily,
    },
    #[error("supersede cycle: {superseded} →* {successor} yol mevcut")]
    SupersedeCycle {
        superseded: ConceptNodeId,
        successor: ConceptNodeId,
    },
    #[error("audit sequence exhausted (u64 overflow)")]
    AuditSequenceExhausted,
}

// ═══════════════════════════════════════════════════════════════════════════════
// SupersedePreview read-only domain predicates (Rich SupersedePreview query)
//
// Üç canonical read-only accessor + bir typed compatibility read model. `apply_supersede`
// structural validation steps 5/9 (incoming/compatibility) bunlara delegasyon yapar;
// CLI `build_supersede_preview` de aynı kaynakları kullanır → divergence mekanik olarak
// engellenir. Mutation semantiği değişmez (12-step precedence korunur); `apply_supersede`
// cycle step 10 mevcut private `is_reachable_via_committed_supersedes`'i kullanmaya devam eder
// (node existence step 2'de doğrulandı).
//
// Domain policy ayrımı:
//   incoming policy      → committed_supersede_incoming_sources (core accessor)
//   currentness policy   → DecisionStatus::is_current_mainline() (apply step 6-7)
//   compatibility policy → supersede_compatibility_from_parts (core predicate)
//   cycle policy         → would_create_supersede_cycle (core predicate)
//   identity equality    → saf observation (kural yok)
// ═══════════════════════════════════════════════════════════════════════════════

/// Endpoint compatibility read model — coarse structural (same kind AND same family).
/// `apply_supersede` step 9 ile aynı kural (tek source). Semantic replacement judgment
/// operator-reviewed basis'te; lineage/scope key future work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersedeCompatibility {
    pub kind_compatible: bool,
    pub family_compatible: bool,
}

impl SupersedeCompatibility {
    /// Both kind and family compatible (coarse structural eligibility).
    pub fn is_compatible(self) -> bool {
        self.kind_compatible && self.family_compatible
    }
}

/// Canonical compatibility rule — tek source. Preview (`inspect_supersede_compatibility`)
/// ve mutation (`apply_supersede` step 9) aynı hesabı kullanır.
fn supersede_compatibility_from_parts(
    superseded_kind: ConceptNodeKind,
    successor_kind: ConceptNodeKind,
    superseded_family: PositionFamily,
    successor_family: PositionFamily,
) -> SupersedeCompatibility {
    SupersedeCompatibility {
        kind_compatible: superseded_kind == successor_kind,
        family_compatible: superseded_family == successor_family,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorStoreSnapshot + SnapshotError — kalıcı store snapshot (Faz: CLI osp review)
//
// `ConceptGraphSnapshot` (types.rs) yalnız graph'ı taşır; `restore_trusted_snapshot`
// bu yüzden ledger/audit_seq'i DISCARD eder (graph-only bootstrap). Kalıcı operator
// review için tüm state gerekir: graph + decision ledger + supersede ledger + audit_seq.
//
// `restore_snapshot` invariant-validasyon yapar — "persistence epistemic gate'leri
// bypass etmemeli" (Paper 3 §9.3). Bu, paper3'ün "representable-but-not-transitioned
// nodes remain a known gap for a future persisted-graph validator" gap'ini evaluated
// `AnchorStoreSnapshot` path için kapatır.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::anchoring::review::{DecisionRecord, SupersedeRecord};

/// Kalıcı store snapshot — graph + iki ledger + audit_seq. `ConceptGraphSnapshot`'ın
/// (yalnız graph) genişletilmiş hali; `restore_snapshot` ile geri yüklenir.
///
/// **audit_seq semantiği:** last-used (her mutation `checked_add(1)` üretip assign eder).
/// Boş ledger → 0; N kayıt → sequence kümesi tam `{1..N}`, `audit_seq == N`.
///
/// **Serialization:** `export_snapshot` canonical sıralı üretir (nodes→NodeId ascending,
/// edges→(source,kind,target), records→audit_seq) — bit-identik + JSON diff okunabilir.
///
/// **schema_version outer'da değil:** `ConceptGraphSnapshot`'ın inner `schema_version`'ı
/// korunur; store-seviye migration (ileride) CLI envelope'unda (`PersistedStore`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnchorStoreSnapshot {
    pub graph: crate::anchoring::types::ConceptGraphSnapshot,
    /// INV-C13 append-only decision ledger.
    pub decision_records: Vec<DecisionRecord>,
    /// INV-C15 append-only supersede ledger. `decision_records` ile global `audit_seq` paylaşır.
    pub supersede_records: Vec<SupersedeRecord>,
    /// Global audit sequence — iki ledger paylaşımlı (cross-ledger total order).
    pub audit_sequence: u64,
}

/// `restore_snapshot` validasyon hatası. Tüm snapshot-level invariant ihlalleri tek
/// tipte toplanır (inner `ConceptGraphSnapshot.schema_version` mismatch dahil) —
/// CLI eşlemesi temiz kalır (R4 küçük not).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot graph schema version uyumsuz: expected={expected}, found={found}")]
    GraphSchemaMismatch { expected: u32, found: u32 },
    #[error("duplicate node id in snapshot: {0}")]
    DuplicateNodeId(ConceptNodeId),
    #[error("edge endpoint node not found: {0}")]
    EdgeEndpointNotFound(ConceptNodeId),
    #[error("decision record references missing node: {0}")]
    DecisionRecordNodeMissing(ConceptNodeId),
    #[error("supersede record references missing node: {0}")]
    SupersedeRecordNodeMissing(ConceptNodeId),
    /// record varsa target status ile uyumlu olmalı (forward integrity, R3#4).
    /// seed_trusted istisnası: Accepted node record'suz olabilir; ama record varsa
    /// status tutarlı olmalı.
    #[error(
        "decision record {seq} decision={decision:?} inconsistent with node status {status:?}"
    )]
    DecisionStatusInconsistent {
        seq: u64,
        decision: crate::anchoring::review::DecisionKind,
        status: DecisionStatus,
    },
    #[error("supersede record {seq} inconsistent: superseded status {superseded_status:?}, successor status {successor_status:?}")]
    SupersedeStatusInconsistent {
        seq: u64,
        superseded_status: DecisionStatus,
        successor_status: DecisionStatus,
    },
    /// Aynı node için en fazla 1 reviewed accept/reject record (reopen yok — schema v1).
    #[error(
        "duplicate reviewed decision record for node {node}: seq {first_seq} and {second_seq}"
    )]
    DuplicateReviewedRecord {
        node: ConceptNodeId,
        first_seq: u64,
        second_seq: u64,
    },
    /// audit_seq yoğunluk ihlali (R3#8): union'da unique + contiguous + == max.
    #[error("audit sequence not dense: expected {expected}, found {found}")]
    AuditSequenceNotDense { expected: u64, found: u64 },
    #[error("audit sequence has duplicate: {0}")]
    AuditSequenceDuplicate(u64),
    /// INV-C15 üç yönlü triangulation: committed Supersedes edge ↔ SupersedeRecord ↔ status.
    #[error("supersede triangulation mismatch: committed edge pairs != record pairs")]
    SupersedeTriangulationMismatch,
    #[error("superseded node {0} has no committed incoming Supersedes edge")]
    SupersedeMissingIncomingEdge(ConceptNodeId),
    #[error("node {0} has multiple committed incoming Supersedes edges")]
    SupersedeMultipleIncomingEdges(ConceptNodeId),
    #[error("committed Supersedes edge target {0} is not SupersededAccepted")]
    SupersedeEdgeTargetNotSuperseded(ConceptNodeId),
    #[error("supersede cycle detected in committed edges")]
    SupersedeCycle,
    /// Aynı (successor, superseded) pair'ine ait birden fazla committed edge/record (Review P1.3).
    #[error("duplicate supersede pair: successor={successor}, superseded={superseded}, count={count} (expected exactly 1)")]
    SupersedeDuplicatePair {
        successor: String,
        superseded: String,
        count: u32,
    },
    /// DecisionRecord transition kendi içinde çelişkili (Review 3.tur P1.2). Record Deserialize
    /// desteklediği için prior_status/new_status sahte/çelişkili olabilir — "persistence does not
    /// weaken epistemic gates" iddiası bunu reject eder.
    #[error("decision record {seq} transition inconsistent: decision={decision:?}, prior_status={prior_status:?}, new_status={new_status:?}")]
    DecisionRecordTransitionInconsistent {
        seq: u64,
        decision: crate::anchoring::review::DecisionKind,
        prior_status: DecisionStatus,
        new_status: DecisionStatus,
    },
    /// SupersedeRecord transition kendi içinde çelişkili (Review 3.tur P1.2).
    #[error("supersede record {seq} transition inconsistent: prior_status={prior_status:?}, new_status={new_status:?}")]
    SupersedeRecordTransitionInconsistent {
        seq: u64,
        prior_status: DecisionStatus,
        new_status: DecisionStatus,
    },
}

/// `PresentedBasis`'in deterministic fingerprint'i → `[u8; 32]`.
/// `DecisionRecord.basis_fingerprint` için. v1'de FNV-based (harici crate yok);
/// audit kayıt bütünlüğü için yeterli, cryptographic security değil (doc'a not).
/// İleri sürüm: gerçek sha256 crate + serde serialize.
fn basis_fingerprint(basis: &crate::anchoring::review::PresentedBasis) -> [u8; 32] {
    let fnv = |seed: u64, bytes: &[u8]| -> u64 {
        let mut h = seed;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x6c62272e07bb0142;
    h1 = fnv(h1, basis.canonical().as_bytes());
    h1 = fnv(h1, &basis.node_digest().get().to_le_bytes());
    h2 = fnv(h2, basis.candidate_id().0.as_bytes());
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&h1.to_le_bytes());
    out[8..16].copy_from_slice(&h2.to_le_bytes());
    out[16..24].copy_from_slice(&(h1 ^ h2).to_le_bytes());
    out[24..32].copy_from_slice(&(h2.wrapping_add(h1)).to_le_bytes());
    out
}

impl std::fmt::Display for ConceptNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorStore trait — Faz 3 (D7 abstraction)
// ═══════════════════════════════════════════════════════════════════════════════

/// Anchor store abstraction (D7). `InMemoryAnchorStore` + `KuzuAnchorStore` aynı trait.
///
/// # Fallible
/// Backend (Kuzu IO, schema, connection) fail olabilir → tüm metodlar `Result`.
/// Associated `type Error` — InMemory `StoreError`, Kuzu `KuzuStoreError`.
///
/// # Owned returns
/// `Vec<ConceptNode>` (owned) — borrow-tied `impl Iterator` trait method olamaz
/// (GAT/lifetime). Clone maliyeti persistence boundary'de kabul edilebilir.
///
/// # INV-C3/C8 persistence boundary (Faz 3 ana invariant)
/// - `apply_plan`: Candidate-only write (INV-C5).
/// - `apply_decision`: `OperatorReviewSession` ile INV-C12/C13 denetimli promotion
///   (Faz 8a; legacy `promote_to_accepted` Faz 8c'de kaldırıldı).
/// - `seed_trusted`: trusted bootstrap/restore (Accepted node yükleyebilir —
///   trusted boundary, normal mutation değil).
///
/// **"Persistence does not weaken epistemic gates."**
pub trait AnchorStore {
    /// Backend-specific error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Trusted bootstrap/restore. Seed Accepted node yükleyebilir — trusted boundary
    /// (fixture pre-state, snapshot restore). Normal mutation DEĞİL.
    fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), Self::Error>;

    /// AnchorPlan uygula. Tüm yeni node/edge'ler Candidate (INV-C5).
    fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, Self::Error>;

    /// INV-C3: Candidate → Accepted. `OperatorAcceptance` gerekir.
    ///
    /// INV-C12/C13 (Faz 8a): Reviewed promotion/reject + ledger append atomik.
    /// `DecisionApplication` opaque (private ctor, Deserialize YOK) — tek üretici
    /// `OperatorReviewSession`. Store uygulama + `DecisionRecord` üretimi +
    /// append'in tek işlemde yapılmasından sorumludur (`seq`, `prior_status`,
    /// `new_status`, `at` store/apply anına ait).
    ///
    /// INV-C13 kapsamı: bu metod *reviewed operator decision path*'tir.
    /// `seed_trusted` (bootstrap) bu invariantın kapsam dışındadır.
    fn apply_decision(
        &mut self,
        application: crate::anchoring::review::DecisionApplication,
    ) -> Result<crate::anchoring::review::DecisionRecord, Self::Error>;

    /// INV-C13: Append-only decision ledger — sorgulanabilir. v1 InMemory;
    /// graph backend Faz 8b+ transaction garantisi ister.
    fn decision_ledger(&self) -> Vec<crate::anchoring::review::DecisionRecord>;

    /// INV-C15 (Faz 8b): Atomic supersession transition. Status (Accepted→SupersededAccepted)
    /// + successor edge (successor→superseded, `Supersedes`) tek işlemde. `SupersedeApplication`
    /// opaque (private fields, pub(crate) ctor, no Deserialize) — production üretici
    /// `SupersedeSession` (PR #50); test üretici `issue_operator_for_tests`. Store:
    /// seq/prior_status/new_status/edge record üretiminden sorumludur.
    fn apply_supersede(
        &mut self,
        application: crate::anchoring::review::SupersedeApplication,
    ) -> Result<crate::anchoring::review::SupersedeRecord, Self::Error>;

    /// INV-C15: Append-only supersede ledger — sorgulanabilir. `decision_ledger` ile
    /// global `audit_seq` paylaşır (cross-ledger total order).
    fn supersede_ledger(&self) -> Vec<crate::anchoring::review::SupersedeRecord>;

    /// INV-C8: canonical exact match (canon gate için).
    fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, Self::Error>;

    /// INV-C3: mainline knowledge — sadece Accepted (currently effective).
    ///
    /// **Deterministik sunum sırası:** node'lar ascending `ConceptNodeId` sırasında döner.
    /// Bu kabul kronolojisi DEĞİL — presentation order'dır. Tüm `AnchorStore`
    /// implementasyonları bu sıralamayı korumak zorundadır (agent-facing context
    /// tekrarlanabilirliği — aynı graph farklı backend/ekleme sırasında aynı projeksiyon).
    fn mainline_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;

    /// INV-C14 (Faz 8b): Acceptance-provenance projection — kabul provenance'ını
    /// koruyan node'lar (Accepted + SupersededAccepted).
    ///
    /// **Bu chronological replay DEĞİLDİR.** Mevcut snapshot'ta kabul provenance'ını
    /// koruyan node'ları döndürür; "t anında mainline neydi" veya kabul sırasını vermez.
    /// Temporal replay decision/event ledger ister.
    ///
    /// **Deterministik sunum sırası:** `mainline_query` ile aynı — node'lar ascending
    /// `ConceptNodeId` sırasında döner. Tüm `AnchorStore` implementasyonları bu sıralamayı
    /// korumak zorundadır.
    fn mainline_history(&self) -> Result<Vec<ConceptNode>, Self::Error>;

    /// Candidate lane — işlem bekleyen.
    fn candidate_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;

    fn node_count(&self) -> Result<usize, Self::Error>;
    fn edge_count(&self) -> Result<usize, Self::Error>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// InMemoryAnchorStore
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory concept anchoring store. `ConceptGraph` sarmalar.
///
/// # INV-C3 / INV-C5 / INV-C15 disiplini
/// - `apply_plan`: tüm yeni node/edge'ler **Candidate** yazılır (INV-C5) — tek untrusted write path.
/// - `mainline_query`: sadece **Accepted** filtre (INV-C3 — Candidate mainline değil).
/// - `apply_decision`: `OperatorReviewSession` ile INV-C12/C13 denetimli promotion (Faz 8a).
/// - `apply_supersede`: INV-C15 atomik supersession (Faz 8b) — trusted-boundary exception
///   (Accepted edge + SupersededAccepted status yazar; C5 "only untrusted write path" kapsamı dışında).
/// - `restore_trusted_snapshot`: trusted restore (Faz 3, INV-C3 persistence boundary).
pub struct InMemoryAnchorStore {
    graph: ConceptGraph,
    /// INV-C13 (Faz 8a): append-only decision ledger. `apply_decision` atomik olarak
    /// promotion + append yapar; ikisi ayrılamaz. v1 InMemory.
    decision_ledger: Vec<crate::anchoring::review::DecisionRecord>,
    /// INV-C15 (Faz 8b): append-only supersede ledger. `apply_supersede` atomik olarak
    /// status + edge + append yapar. `audit_seq` decision ile paylaşımlı (cross-ledger total order).
    supersede_ledger: Vec<crate::anchoring::review::SupersedeRecord>,
    /// Global audit sequence counter — `decision_ledger` ve `supersede_ledger` paylaşımlı.
    /// Cross-ledger total order (chronological replay için initial snapshot + event stream de gerekir).
    audit_seq: u64,
}

impl std::fmt::Debug for InMemoryAnchorStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryAnchorStore")
            .field("node_count", &self.graph.node_count())
            .field("edge_count", &self.graph.edge_count())
            .field("decision_ledger_len", &self.decision_ledger.len())
            .field("supersede_ledger_len", &self.supersede_ledger.len())
            .field("audit_seq", &self.audit_seq)
            .finish()
    }
}

impl Default for InMemoryAnchorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryAnchorStore {
    pub fn new() -> Self {
        Self {
            graph: ConceptGraph::new(),
            decision_ledger: Vec::new(),
            supersede_ledger: Vec::new(),
            audit_seq: 0,
        }
    }

    /// Graph seed ile başlat (fixture given'dan dönüştürülmüş).
    pub fn with_seed(seed: GraphSeed) -> Self {
        let mut s = Self::new();
        s.seed_trusted(&seed).expect("in-memory seed infallible");
        s
    }

    /// Trusted restore — `ConceptGraphSnapshot`'tan graph'ı geri yükle (Faz 3).
    /// INV-C3 persistence boundary: bu trusted restore path (operator-belirlenmiş
    /// Accepted node'lar dahil). Normal mutation DEĞİL — snapshot deserialize.
    ///
    /// # schema_version kontrolü
    /// Snapshot `schema_version` mevcut `SCHEMA_VERSION` ile eşleşmeli; değilse
    /// `StoreError::InvalidSnapshotVersion`. Trusted restore boundary'nin en hassas
    /// kapısı — Accepted node içerebilir, o yüzden version mismatch reject.
    /// **Deprecated** — graph-only trusted bootstrap. **Decision ve supersession
    /// provenance'ı (ledger'lar) ve audit sequence'i discard eder.** Kalıcı operator
    /// review restoration için [`restore_snapshot`](Self::restore_snapshot) kullanın;
    /// bu method yalnızca graph-only bootstrap/test içindir.
    ///
    /// Açık ad: [`restore_graph_only_for_trusted_bootstrap`](Self::restore_graph_only_for_trusted_bootstrap).
    #[deprecated(
        since = "0.1.0",
        note = "Graph-only trusted bootstrap. Do not use for persistence restoration; \
                decision and supersession provenance and audit sequence are discarded. \
                Use restore_snapshot for full-state restore."
    )]
    pub fn restore_trusted_snapshot(
        snapshot: crate::anchoring::types::ConceptGraphSnapshot,
    ) -> Result<Self, StoreError> {
        Self::restore_graph_only_for_trusted_bootstrap(snapshot)
    }

    /// Graph-only trusted bootstrap — `ConceptGraphSnapshot`'tan graph'ı geri yükler
    /// (Faz 3, INV-C3 persistence boundary). `restore_trusted_snapshot`'ın açık-ad
    /// versiyonu. **Ledger/audit_seq yüklenmez** (sıfırlanır); kalıcı restore için
    /// [`restore_snapshot`](Self::restore_snapshot).
    ///
    /// INV-C3 persistence boundary: bu trusted restore path (operator-belirlenmiş
    /// Accepted node'lar dahil). Normal mutation DEĞİL — snapshot deserialize.
    ///
    /// # schema_version kontrolü
    /// Snapshot `schema_version` mevcut `SCHEMA_VERSION` ile eşleşmeli; değilse
    /// `StoreError::InvalidSnapshotVersion`. Trusted restore boundary'nin en hassas
    /// kapısı — Accepted node içerebilir, o yüzden version mismatch reject.
    pub fn restore_graph_only_for_trusted_bootstrap(
        snapshot: crate::anchoring::types::ConceptGraphSnapshot,
    ) -> Result<Self, StoreError> {
        use crate::anchoring::types::ConceptGraphSnapshot;
        if snapshot.schema_version != ConceptGraphSnapshot::SCHEMA_VERSION {
            return Err(StoreError::InvalidSnapshotVersion {
                expected: ConceptGraphSnapshot::SCHEMA_VERSION,
                found: snapshot.schema_version,
            });
        }
        let mut s = Self::new();
        for node in snapshot.nodes {
            s.graph.insert_node(node);
        }
        for edge in snapshot.edges {
            s.graph.insert_edge(edge);
        }
        Ok(s)
    }

    /// Store state'inin kalıcı snapshot'ını üretir (graph + iki ledger + audit_seq).
    ///
    /// **Canonical serialization:** nodes NodeId ascending, edges (source,kind,target),
    /// records audit_seq sıralı — `ConceptGraph.nodes` HashMap nondeterministic sıra
    /// verir; export deterministic olmalı (bit-identik round-trip + JSON diff okunabilirliği).
    pub fn export_snapshot(&self) -> AnchorStoreSnapshot {
        use crate::anchoring::types::ConceptGraphSnapshot;
        // Canonical: nodes sorted by id.
        let mut nodes: Vec<ConceptNode> = self.graph.nodes_iter().cloned().collect();
        nodes.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        // Canonical: edges sorted by (from, kind, to).
        let mut edges: Vec<ConceptEdge> = self.graph.edges().cloned().collect();
        edges.sort_by(|a, b| {
            a.from
                .0
                .cmp(&b.from.0)
                .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
                .then_with(|| a.to.0.cmp(&b.to.0))
        });
        // Canonical: records sorted by audit seq.
        let mut decision_records = self.decision_ledger.clone();
        decision_records.sort_by_key(|r| r.seq);
        let mut supersede_records = self.supersede_ledger.clone();
        supersede_records.sort_by_key(|r| r.seq);
        AnchorStoreSnapshot {
            graph: ConceptGraphSnapshot {
                nodes,
                edges,
                schema_version: ConceptGraphSnapshot::SCHEMA_VERSION,
            },
            decision_records,
            supersede_records,
            audit_sequence: self.audit_seq,
        }
    }

    /// Kalıcı snapshot'tan store'u geri yükler + invariant-validasyon yapar.
    ///
    /// **"Persistence does not weaken epistemic gates"** (Paper 3 §9.3): restore,
    /// deserialize ile epistemic gate'leri bypass etmemeli. Bu method, paper3'ün
    /// "representable-but-not-transitioned nodes remain a known gap for a future
    /// persisted-graph validator" gap'ini evaluated `AnchorStoreSnapshot` path için
    /// kapatır (alternate backends equivalent validation gerekir).
    ///
    /// Validasyon (restore sırasında):
    /// - graph schema_version uyumu
    /// - node ID uniqueness; edge endpoint existence
    /// - record → node existence (tek yönlü — seed_trusted Accepted node'lar ledger'sız)
    /// - record → status forward integrity (Accept→Accepted|SupersededAccepted;
    ///   Reject→Rejected; Supersede→SupersededAccepted/Accepted)
    /// - aynı node en fazla 1 reviewed accept/reject record (reopen yok — schema v1)
    /// - audit_seq yoğunluk: union'da unique + `{1..N}` + `audit_seq == N`
    /// - INV-C15 üç yönlü triangulation: committed Supersedes edge ↔ SupersedeRecord ↔ status
    pub fn restore_snapshot(snapshot: AnchorStoreSnapshot) -> Result<Self, SnapshotError> {
        validate_snapshot(&snapshot)?;
        let graph = snapshot.graph;
        let mut s = Self::new();
        for node in graph.nodes {
            s.graph.insert_node(node);
        }
        for edge in graph.edges {
            s.graph.insert_edge(edge);
        }
        s.decision_ledger = snapshot.decision_records;
        s.supersede_ledger = snapshot.supersede_records;
        s.audit_seq = snapshot.audit_sequence;
        Ok(s)
    }

    /// Graph referansı (read-only — gate/extractor için). Trait dışı özel accessor.
    pub fn graph(&self) -> &ConceptGraph {
        &self.graph
    }

    /// Test-only graph mut accessor (TOCTOU testleri için — node canonical değiştirme).
    #[cfg(test)]
    pub(crate) fn graph_mut(&mut self) -> &mut ConceptGraph {
        &mut self.graph
    }

    // Faz 3 backward-compat inherent wrapper'lar — downstream crate'ler `use AnchorStore`
    // import etmeden eski API'yi kullanabilsin diye. Trait abstraction korunur,
    // kaynak uyumluluğu kırılmaz.
    pub fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, StoreError> {
        <Self as AnchorStore>::apply_plan(self, plan)
    }
    pub fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), StoreError> {
        <Self as AnchorStore>::seed_trusted(self, seed)
    }
    pub fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::find_concepts_by_canonical(self, name)
    }
    pub fn mainline_query(&self) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::mainline_query(self)
    }
    pub fn mainline_history(&self) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::mainline_history(self)
    }
    pub fn candidate_query(&self) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::candidate_query(self)
    }
    pub fn apply_decision(
        &mut self,
        application: crate::anchoring::review::DecisionApplication,
    ) -> Result<crate::anchoring::review::DecisionRecord, StoreError> {
        <Self as AnchorStore>::apply_decision(self, application)
    }
    pub fn apply_supersede(
        &mut self,
        application: crate::anchoring::review::SupersedeApplication,
    ) -> Result<crate::anchoring::review::SupersedeRecord, StoreError> {
        <Self as AnchorStore>::apply_supersede(self, application)
    }
    pub fn supersede_ledger(&self) -> Vec<crate::anchoring::review::SupersedeRecord> {
        <Self as AnchorStore>::supersede_ledger(self)
    }
    pub fn decision_ledger(&self) -> Vec<crate::anchoring::review::DecisionRecord> {
        <Self as AnchorStore>::decision_ledger(self)
    }
    pub fn node_count(&self) -> Result<usize, StoreError> {
        <Self as AnchorStore>::node_count(self)
    }
    pub fn edge_count(&self) -> Result<usize, StoreError> {
        <Self as AnchorStore>::edge_count(self)
    }

    /// `from`'dan outgoing *committed* (Accepted) Supersedes edge'lerini DFS ile takip
    /// edip `to`'ya ulaşılırsa true. Cycle check: apply_supersede'de `from=superseded,
    /// to=successor` — yani superseded'ın lineage'inde successor zaten var mı?
    /// Production API dizisiyle unreachable (her Supersedes hedefi atomik SupersededAccepted
    /// olur); seeded/deserialized adversarial graph savunması.
    fn is_reachable_via_committed_supersedes(
        &self,
        from: &ConceptNodeId,
        to: &ConceptNodeId,
    ) -> bool {
        use crate::anchoring::ConceptEdgeKind;
        use std::collections::HashSet;
        let mut visited: HashSet<ConceptNodeId> = HashSet::new();
        let mut stack = vec![from.clone()];
        while let Some(current) = stack.pop() {
            if &current == to {
                return true;
            }
            if !visited.insert(current.clone()) {
                continue;
            }
            for e in self.graph.edges() {
                if e.kind == ConceptEdgeKind::Supersedes
                    && e.decision_status == DecisionStatus::Accepted
                    && e.from == current
                {
                    stack.push(e.to.clone());
                }
            }
        }
        false
    }

    /// INV-C15 incoming committed Supersedes source'ları — `apply_supersede` step 5 filtresiyle
    /// birebir aynı: `Supersedes && decision_status==Accepted && to==id` (Candidate proposal
    /// edge'leri sayılmaz). Tek source — preview (`superseded_incoming` presentation +
    /// `AlreadySuperseded` blocker) ve mutation (`apply_supersede` step 5) buradan beslenir.
    ///
    /// Validated snapshot'ta INV-C15 ≤1 (restore validator incoming cardinality'yi doğrular);
    /// invalid/direct-store'da Vec dürüst davranır. Deterministik (source ID ascending).
    pub fn committed_supersede_incoming_sources(
        &self,
        id: &ConceptNodeId,
    ) -> Result<Vec<ConceptNodeId>, StoreError> {
        use crate::anchoring::ConceptEdgeKind;
        self.graph
            .node(id)
            .ok_or_else(|| StoreError::NodeNotFound(id.clone()))?;
        let mut sources: Vec<ConceptNodeId> = self
            .graph
            .edges()
            .filter(|e| {
                e.kind == ConceptEdgeKind::Supersedes
                    && e.decision_status == DecisionStatus::Accepted
                    && &e.to == id
            })
            .map(|e| e.from.clone())
            .collect();
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(sources)
    }

    /// Endpoint compatibility — coarse structural (same kind AND same family). `apply_supersede`
    /// step 9 ile aynı kural (tek canonical helper). Node existence doğrulanır (public API
    /// kendi sözleşmesini korur). Otomasyon/preview divergence'ı engellenir: compatibility
    /// bir presentation detayı değil domain policy — core'da kalır.
    pub fn inspect_supersede_compatibility(
        &self,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
    ) -> Result<SupersedeCompatibility, StoreError> {
        let sup = self
            .graph
            .node(superseded)
            .ok_or_else(|| StoreError::NodeNotFound(superseded.clone()))?;
        let suc = self
            .graph
            .node(successor)
            .ok_or_else(|| StoreError::NodeNotFound(successor.clone()))?;
        Ok(supersede_compatibility_from_parts(
            sup.node_kind,
            suc.node_kind,
            sup.position_family,
            suc.position_family,
        ))
    }

    /// Proposed `successor --Supersedes--> superseded` edge'i committed supersede graph'ında
    /// cycle yaratır mı? `apply_supersede` step 10 ile aynı hesap: `superseded →* successor`
    /// committed Supersedes yolu var mı (tek source of truth).
    ///
    /// Node existence doğrulanır (public API sözleşmesi). `apply_supersede` bu wrapper'ı
    /// çağırmak zorunda değil — node existence step 2'de doğrulandı, private predicate'i
    /// kullanmaya devam eder (mutation yolu değişmez).
    ///
    /// **Self-supersede notu:** `superseded == successor` için trivially `Ok(true)` döner
    /// (ilk stack elemanı hedefe eşit). Caller önce self-supersede kontrol etmeli; preview
    /// builder self blocker'ı (step 8) cycle'dan (step 10) önce üretir ve cycle'ı bastırır.
    pub fn would_create_supersede_cycle(
        &self,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
    ) -> Result<bool, StoreError> {
        self.graph
            .node(superseded)
            .ok_or_else(|| StoreError::NodeNotFound(superseded.clone()))?;
        self.graph
            .node(successor)
            .ok_or_else(|| StoreError::NodeNotFound(successor.clone()))?;
        Ok(self.is_reachable_via_committed_supersedes(superseded, successor))
    }

    /// Test-only: audit_seq'i zorla set et (overflow exhaustion test için).
    #[cfg(test)]
    pub(crate) fn set_audit_seq_for_tests(&mut self, seq: u64) {
        self.audit_seq = seq;
    }
    /// Test-only: audit_seq oku.
    #[cfg(test)]
    pub(crate) fn audit_seq_for_tests(&self) -> u64 {
        self.audit_seq
    }

    // Inherent apply_plan (trait'in arkasında kullanılan gerçek implementasyon)
    fn apply_plan_inner(&mut self, plan: &AnchorPlan) -> ApplyResult {
        let mut new_nodes = 0u32;
        let mut new_edges = 0u32;

        for c in &plan.candidates {
            // Target node yoksa ghost node oluştur (Candidate, INV-C5)
            if self.graph.node(&c.target_node_id).is_none() {
                let (kind, canonical) = parse_target(&c.target_node_id.0);
                let node = ConceptNode {
                    id: c.target_node_id.clone(),
                    canonical,
                    aliases: Vec::new(),
                    node_kind: kind,
                    decision_status: DecisionStatus::Candidate,
                    position_family: crate::anchoring::PositionFamily::ConceptualIntent,
                };
                self.graph.insert_node(node);
                new_nodes += 1;
            }

            // Edge yaz (Candidate status, INV-C5)
            let edge = ConceptEdge {
                from: plan.packet_id.clone().into_node_id(),
                to: c.target_node_id.clone(),
                kind: c.edge_kind,
                decision_status: DecisionStatus::Candidate,
                explanation: c.explanation.clone(),
            };
            self.graph.insert_edge(edge);
            new_edges += 1;
        }

        ApplyResult {
            new_nodes,
            new_edges,
        }
    }
}

impl AnchorStore for InMemoryAnchorStore {
    type Error = StoreError;

    fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), Self::Error> {
        // Faz 5a: 3 yeni candidate bucket (rule/task/risk candidates) — Patch 6.
        // Deterministik sıra all_nodes() ile uyumlu.
        for node in seed
            .concepts
            .iter()
            .chain(&seed.decisions)
            .chain(&seed.code_entities)
            .chain(&seed.rule_candidates)
            .chain(&seed.task_candidates)
            .chain(&seed.risk_candidates)
        {
            self.graph.insert_node(node.clone());
        }
        Ok(())
    }

    fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, Self::Error> {
        Ok(self.apply_plan_inner(plan))
    }

    /// INV-C12/C13 (Faz 8a): reviewed promotion/reject + ledger append atomik.
    /// `seq`, `prior_status`, `new_status`, `at` burada üretilir — session değil.
    fn apply_decision(
        &mut self,
        application: crate::anchoring::review::DecisionApplication,
    ) -> Result<crate::anchoring::review::DecisionRecord, Self::Error> {
        use crate::anchoring::review::{DecisionKind, DecisionRecord};

        let id = application.candidate_id();
        let decision = application.decision();

        // Defense-in-depth (Review 1 tasarım gözlemi): basis.candidate_id ile
        // application.candidate_id eşleşmeli. Session bu kontrolü yapar ama apply_decision
        // da yapmalı — NotPromotableFrom için aynı defense-in-depth argümanı.
        // **Kontrol sırası:** id-mismatch ÖNCE, sonra NotPromotableFrom.
        let basis = application.basis();
        if basis.candidate_id() != id {
            return Err(StoreError::BasisCandidateMismatch {
                basis: basis.candidate_id().clone(),
                application: id.clone(),
            });
        }

        // Node'u bul + prior_status + NotPromotable kontrolü.
        let node = self
            .graph
            .node_mut(id)
            .ok_or_else(|| StoreError::NodeNotFound(id.clone()))?;
        let prior_status = node.decision_status;

        // NotPromotable: Accepted/Deprecated/SupersededAccepted/Rejected'dan
        // accept/reject geçersiz. (Diriltme ayrı mekanizma — v1 dışı.)
        match (prior_status, decision) {
            (DecisionStatus::Accepted, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            (DecisionStatus::Deprecated, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            (DecisionStatus::SupersededAccepted, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            (DecisionStatus::Rejected, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            _ => {}
        }

        // Status geçişini uygula.
        let new_status = match decision {
            DecisionKind::Accept => DecisionStatus::Accepted,
            DecisionKind::Reject => DecisionStatus::Rejected,
        };

        // INV-C13/INV-C15 atomiklik: audit_seq overflow'ı mutation ÖNCESİ checked_add ile
        // doğrula (Review PR #49 tur 3). u64 += 1 debug/test build'inde overflow panic
        // ederse status mutation sonrası panic = atomiklik kırılırdı.
        let next_seq = self
            .audit_seq
            .checked_add(1)
            .ok_or(StoreError::AuditSequenceExhausted)?;

        // All fallible domain validation is complete. The following mutations contain
        // no expected Result-returning failure path. (Rust panic/OOM transaction rollback
        // is out of scope — INV-C13/C15 guarantee graph/ledger/seq unchanged on returned Err.)
        node.decision_status = new_status;
        self.audit_seq = next_seq;
        let seq = next_seq;

        // INV-C13: DecisionRecord üret + ledger'a atomik append.
        let basis = application.basis();
        // basis_fingerprint: PresentedBasis seçili alanlarının (canonical + node_digest +
        // candidate_id) FNV-based deterministic fingerprint'i. Audit-security hash DEĞİL
        // (v1'de harici crate yok); cryptographic için ileri sürüm sha2 crate.
        let basis_fp = basis_fingerprint(basis);
        let record = DecisionRecord {
            seq,
            session_id: application.session_id().clone(),
            operator: application.operator().clone(),
            candidate_id: id.clone(),
            node_digest_serde: basis.node_digest().get(),
            decision,
            reason: application.reason().clone(),
            basis_fingerprint: basis_fp,
            prior_status,
            new_status,
            at: application.decided_at(),
        };
        self.decision_ledger.push(record.clone());
        Ok(record)
    }

    fn decision_ledger(&self) -> Vec<crate::anchoring::review::DecisionRecord> {
        self.decision_ledger.clone()
    }

    fn supersede_ledger(&self) -> Vec<crate::anchoring::review::SupersedeRecord> {
        self.supersede_ledger.clone()
    }

    /// INV-C15 (Faz 8b): Atomic supersession transition.
    ///
    /// Edge yönü (tasarım doc §8.3): `successor --Supersedes--> superseded`.
    /// Cardinalite: incoming — `superseded`'e gelen *committed* (Accepted) Supersedes
    /// edge sayısı == 1. Candidate proposal edge'ler (apply_plan yazar) sayılmaz;
    /// consolidation (C→A ve C→B) serbest (outgoing sınırı yok).
    ///
    /// Deterministic error precedence (tests may rely on it):
    ///   1. basis endpoint mismatch (SupersedeBasisMismatch)
    ///   2. node existence (NodeNotFound)
    ///   3. superseded digest freshness (StaleSupersededBasis)
    ///   4. successor digest freshness (StaleSuccessorBasis)
    ///   5. existing committed incoming edge (AlreadySuperseded)
    ///   6. superseded status (NotSupersedeableFrom)
    ///   7. successor status (SuccessorNotAccepted)
    ///   8. self-supersede (SelfSupersede)
    ///   9. endpoint compatibility (IncompatibleSupersedeEndpoints) — coarse structural
    ///  10. cycle (SupersedeCycle)
    ///  11. audit_seq availability (AuditSequenceExhausted)
    ///  12. mutation (status + edge + ledger)
    fn apply_supersede(
        &mut self,
        application: crate::anchoring::review::SupersedeApplication,
    ) -> Result<crate::anchoring::review::SupersedeRecord, Self::Error> {
        use crate::anchoring::review::{supersede_basis_fingerprint, SupersedeRecord};
        use crate::anchoring::ConceptEdgeKind;

        let superseded_id = application.superseded().clone();
        let successor_id = application.successor().clone();
        let basis = application.basis();

        // (1) Basis endpoint mismatch — defense-in-depth (session da kontrol eder).
        if basis.superseded_id() != &superseded_id || basis.successor_id() != &successor_id {
            return Err(StoreError::SupersedeBasisMismatch {
                basis_superseded: basis.superseded_id().clone(),
                basis_successor: basis.successor_id().clone(),
                app_superseded: superseded_id.clone(),
                app_successor: successor_id.clone(),
            });
        }

        // (2) Node existence + (3)(4) digest freshness + validation verilerini kopyala
        // (borrow-safe: immutable borrow kapat, node_mut için) — Review PR #49 tur 2.
        let (
            sup_status,
            suc_status,
            sup_kind,
            suc_kind,
            sup_family,
            suc_family,
            cur_sup_digest,
            cur_suc_digest,
        ) = {
            let sup_node = self
                .graph
                .node(&superseded_id)
                .ok_or_else(|| StoreError::NodeNotFound(superseded_id.clone()))?;
            let suc_node = self
                .graph
                .node(&successor_id)
                .ok_or_else(|| StoreError::NodeNotFound(successor_id.clone()))?;
            (
                sup_node.decision_status,
                suc_node.decision_status,
                sup_node.node_kind,
                suc_node.node_kind,
                sup_node.position_family,
                suc_node.position_family,
                crate::anchoring::review::node_digest(sup_node),
                crate::anchoring::review::node_digest(suc_node),
            )
        };

        // (3) Superseded digest freshness — u64 payload (NodeDigest Serialize-only).
        if basis.superseded_digest() != cur_sup_digest {
            return Err(StoreError::StaleSupersededBasis {
                expected_digest: basis.superseded_digest().get(),
                found_digest: cur_sup_digest.get(),
            });
        }
        // (4) Successor digest freshness — successor da karar anında taze olmalı.
        if basis.successor_digest() != cur_suc_digest {
            return Err(StoreError::StaleSuccessorBasis {
                expected_digest: basis.successor_digest().get(),
                found_digest: cur_suc_digest.get(),
            });
        }

        // (5) INV-C15 committed incoming edge — canonical accessor (preview ile aynı source).
        // Accepted only (Candidate proposal'lar sayılmaz).
        let incoming = self.committed_supersede_incoming_sources(&superseded_id)?;
        if !incoming.is_empty() {
            return Err(StoreError::AlreadySuperseded(superseded_id.clone()));
        }

        // (6) Superseded currentness — canonical predicate (mainline_query ile aynı).
        if !sup_status.is_current_mainline() {
            return Err(StoreError::NotSupersedeableFrom(sup_status));
        }
        // (7) Successor currentness — canonical predicate (creation-time Accepted).
        if !suc_status.is_current_mainline() {
            return Err(StoreError::SuccessorNotAccepted(suc_status));
        }
        // (8) Self-supersede.
        if superseded_id == successor_id {
            return Err(StoreError::SelfSupersede(superseded_id.clone()));
        }
        // (9) Endpoint compatibility — canonical predicate (preview ile aynı source).
        // Coarse structural (same kind + family). Semantic replacement judgment operator-reviewed
        // basis'te; lineage/scope key future work.
        let compatibility = supersede_compatibility_from_parts(
            sup_kind, suc_kind, sup_family, suc_family,
        );
        if !compatibility.is_compatible() {
            return Err(StoreError::IncompatibleSupersedeEndpoints {
                superseded_kind: sup_kind,
                successor_kind: suc_kind,
                superseded_family: sup_family,
                successor_family: suc_family,
            });
        }
        // (10) Cycle: superseded →* successor committed Supersedes yolu var mı?
        // Production API dizisiyle unreachable (her Supersedes hedefi atomik SupersededAccepted
        // olur); seeded/deserialized adversarial graph savunması.
        if self.is_reachable_via_committed_supersedes(&superseded_id, &successor_id) {
            return Err(StoreError::SupersedeCycle {
                superseded: superseded_id.clone(),
                successor: successor_id.clone(),
            });
        }

        // (11) audit_seq overflow — mutation öncesi checked_add (Review PR #49 tur 3).
        let next_seq = self
            .audit_seq
            .checked_add(1)
            .ok_or(StoreError::AuditSequenceExhausted)?;

        // --- All fallible domain validation complete. No expected Result-returning
        // failure path below. (Rust panic/OOM transaction rollback out of scope;
        // INV-C15 guarantees graph/ledger/seq unchanged on returned Err.) ---
        let prior_status = DecisionStatus::Accepted;
        let new_status = DecisionStatus::SupersededAccepted;

        // (12a) Status transition.
        self.graph
            .node_mut(&superseded_id)
            .expect("validated node exists")
            .decision_status = new_status;

        // (12b) Successor edge: successor → superseded (Accepted/committed, high-stake INV-C7).
        // Candidate Supersedes proposals are preserved as historical proposal provenance.
        // A successful session appends a distinct Accepted Supersedes lineage edge; it does
        // not promote or delete the proposal edge (lane-sensitive separation). Kalıcı
        // sözleşme (PR #50, 4-tur review mutabık).
        let edge = ConceptEdge {
            from: successor_id.clone(),
            to: superseded_id.clone(),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Accepted,
            explanation: Some(application.reason().clone()),
        };
        self.graph.insert_edge(edge);

        // (12c) SupersedeRecord + ledger append (atomic — global audit_seq).
        self.audit_seq = next_seq;
        let basis_fp = supersede_basis_fingerprint(basis);
        let record = SupersedeRecord {
            seq: next_seq,
            session_id: application.session_id().clone(),
            operator: application.operator().clone(),
            superseded: superseded_id.clone(),
            successor: successor_id.clone(),
            authority_level: application.authority_level(),
            reason: application.reason().clone(),
            superseded_digest_serde: basis.superseded_digest().get(),
            successor_digest_serde: basis.successor_digest().get(),
            basis_fingerprint: basis_fp,
            prior_status,
            new_status,
            at: application.decided_at(),
        };
        self.supersede_ledger.push(record.clone());
        Ok(record)
    }

    fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, Self::Error> {
        Ok(self
            .graph
            .find_concept_by_canonical(name)
            .into_iter()
            .cloned()
            .collect())
    }

    fn mainline_query(&self) -> Result<Vec<ConceptNode>, Self::Error> {
        let mut nodes: Vec<ConceptNode> = self
            .graph
            .nodes_iter()
            .filter(|n| n.decision_status.is_current_mainline())
            .cloned()
            .collect();
        // Deterministic presentation order — `mainline_history` ile aynı desen.
        // Agent-facing context tekrarlanabilirliği: graph ekleme sırasına değil ID'ye bağlı.
        nodes.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(nodes)
    }

    fn mainline_history(&self) -> Result<Vec<ConceptNode>, Self::Error> {
        let mut nodes: Vec<ConceptNode> = self
            .graph
            .nodes_iter()
            .filter(|n| n.decision_status.preserves_accepted_provenance())
            .cloned()
            .collect();
        // Deterministic presentation order; NOT acceptance chronology.
        nodes.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(nodes)
    }

    fn candidate_query(&self) -> Result<Vec<ConceptNode>, Self::Error> {
        // Deterministic: HashMap iteration sırası process'ler arasında değişir.
        // mainline_query/mainline_history ile aynı — ID ascending sort (Review 2.tur P2.1).
        let mut nodes: Vec<ConceptNode> = self
            .graph
            .nodes_iter()
            .filter(|n| matches!(n.decision_status, DecisionStatus::Candidate))
            .cloned()
            .collect();
        nodes.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(nodes)
    }

    fn node_count(&self) -> Result<usize, Self::Error> {
        Ok(self.graph.node_count())
    }

    fn edge_count(&self) -> Result<usize, Self::Error> {
        Ok(self.graph.edge_count())
    }
}

/// Apply plan sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyResult {
    pub new_nodes: u32,
    pub new_edges: u32,
}

/// "Concept:Payment" → (ConceptNodeKind, canonical).
fn parse_target(id: &str) -> (crate::anchoring::types::ConceptNodeKind, String) {
    use crate::anchoring::types::ConceptNodeKind;
    if let Some((prefix, name)) = id.split_once(':') {
        let kind = ConceptNodeKind::from_prefix(prefix).unwrap_or(ConceptNodeKind::Concept);
        (kind, name.to_string())
    } else {
        (ConceptNodeKind::Concept, id.to_string())
    }
}

/// ConceptPacketId'den ConceptNodeId'ye (ConceptPacket node'u graph'ta).
impl crate::anchoring::types::ConceptPacketId {
    pub fn into_node_id(&self) -> ConceptNodeId {
        ConceptNodeId(format!("ConceptPacket:{}", self.0))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// validate_snapshot — restore_snapshot'ın invariant-validasyonu
//
// "Persistence does not weaken epistemic gates" (Paper 3 §9.3). Bu fonksiyon,
// paper3'ün "representable-but-not-transitioned nodes remain a known gap for a
// future persisted-graph validator" gap'ini evaluated `AnchorStoreSnapshot` path
// için kapatır. Alternate backends equivalent validation gerekir.
// ═══════════════════════════════════════════════════════════════════════════════

/// `restore_snapshot` öncesi snapshot invariant-validasyonu. Tüm ihlaller
/// `SnapshotError` olarak döner; mutation yapılmadan (pure validation).
fn validate_snapshot(snapshot: &AnchorStoreSnapshot) -> Result<(), SnapshotError> {
    use crate::anchoring::review::DecisionKind;
    use crate::anchoring::types::ConceptGraphSnapshot;
    use crate::anchoring::ConceptEdgeKind;
    use std::collections::{BTreeMap, BTreeSet};

    // (1) Graph schema version.
    if snapshot.graph.schema_version != ConceptGraphSnapshot::SCHEMA_VERSION {
        return Err(SnapshotError::GraphSchemaMismatch {
            expected: ConceptGraphSnapshot::SCHEMA_VERSION,
            found: snapshot.graph.schema_version,
        });
    }

    // Node id set + uniqueness.
    let mut node_ids: BTreeSet<String> = BTreeSet::new();
    for n in &snapshot.graph.nodes {
        if !node_ids.insert(n.id.0.clone()) {
            return Err(SnapshotError::DuplicateNodeId(n.id.clone()));
        }
    }
    let node_exists = |id: &ConceptNodeId| node_ids.contains(&id.0);

    // (2) Edge endpoint existence.
    for e in &snapshot.graph.edges {
        if !node_exists(&e.from) {
            return Err(SnapshotError::EdgeEndpointNotFound(e.from.clone()));
        }
        if !node_exists(&e.to) {
            return Err(SnapshotError::EdgeEndpointNotFound(e.to.clone()));
        }
    }

    // Status lookup (id → status) — record→status forward integrity için.
    let status_of: BTreeMap<String, DecisionStatus> = snapshot
        .graph
        .nodes
        .iter()
        .map(|n| (n.id.0.clone(), n.decision_status))
        .collect();

    // (3)(4) Decision record → node existence + status forward integrity.
    let mut reviewed_nodes: BTreeMap<String, u64> = BTreeMap::new(); // node → first seq
    for r in &snapshot.decision_records {
        if !node_exists(&r.candidate_id) {
            return Err(SnapshotError::DecisionRecordNodeMissing(
                r.candidate_id.clone(),
            ));
        }
        let status = status_of
            .get(&r.candidate_id.0)
            .copied()
            .unwrap_or(DecisionStatus::Candidate);
        let consistent = match r.decision {
            DecisionKind::Accept => {
                status == DecisionStatus::Accepted || status == DecisionStatus::SupersededAccepted
            }
            DecisionKind::Reject => status == DecisionStatus::Rejected,
        };
        if !consistent {
            return Err(SnapshotError::DecisionStatusInconsistent {
                seq: r.seq,
                decision: r.decision,
                status,
            });
        }
        // (4b) Record transition kendi içinde tutarlı mı (Review 3.tur P1.2)? Record Deserialize
        // desteklediği için prior_status/new_status sahte olabilir. Schema v1 sabit transitionlar:
        //   Accept: Candidate → Accepted
        //   Reject: Candidate → Rejected
        let transition_ok = match r.decision {
            DecisionKind::Accept => {
                r.prior_status == DecisionStatus::Candidate
                    && r.new_status == DecisionStatus::Accepted
            }
            DecisionKind::Reject => {
                r.prior_status == DecisionStatus::Candidate
                    && r.new_status == DecisionStatus::Rejected
            }
        };
        if !transition_ok {
            return Err(SnapshotError::DecisionRecordTransitionInconsistent {
                seq: r.seq,
                decision: r.decision,
                prior_status: r.prior_status,
                new_status: r.new_status,
            });
        }
        // (5) Aynı node en fazla 1 reviewed accept/reject record (reopen yok — schema v1).
        if let Some(&prev) = reviewed_nodes.get(&r.candidate_id.0) {
            return Err(SnapshotError::DuplicateReviewedRecord {
                node: r.candidate_id.clone(),
                first_seq: prev,
                second_seq: r.seq,
            });
        }
        reviewed_nodes.insert(r.candidate_id.0.clone(), r.seq);
    }

    // (3) Supersede record → node existence (her iki endpoint) + status forward integrity.
    for r in &snapshot.supersede_records {
        if !node_exists(&r.superseded) {
            return Err(SnapshotError::SupersedeRecordNodeMissing(
                r.superseded.clone(),
            ));
        }
        if !node_exists(&r.successor) {
            return Err(SnapshotError::SupersedeRecordNodeMissing(
                r.successor.clone(),
            ));
        }
        let sup_status = status_of
            .get(&r.superseded.0)
            .copied()
            .unwrap_or(DecisionStatus::Candidate);
        let suc_status = status_of
            .get(&r.successor.0)
            .copied()
            .unwrap_or(DecisionStatus::Candidate);
        // superseded SupersededAccepted; successor Accepted veya (chain'de) SupersededAccepted.
        if sup_status != DecisionStatus::SupersededAccepted
            || (suc_status != DecisionStatus::Accepted
                && suc_status != DecisionStatus::SupersededAccepted)
        {
            return Err(SnapshotError::SupersedeStatusInconsistent {
                seq: r.seq,
                superseded_status: sup_status,
                successor_status: suc_status,
            });
        }
        // (3b) Record transition kendi içinde tutarlı mı (Review 3.tur P1.2)? Schema v1:
        //   supersede: Accepted → SupersededAccepted
        if r.prior_status != DecisionStatus::Accepted
            || r.new_status != DecisionStatus::SupersededAccepted
        {
            return Err(SnapshotError::SupersedeRecordTransitionInconsistent {
                seq: r.seq,
                prior_status: r.prior_status,
                new_status: r.new_status,
            });
        }
    }

    // (6) audit_seq yoğunluk: union'da unique + {1..N} + audit_seq == N.
    let mut all_seqs: Vec<u64> = snapshot
        .decision_records
        .iter()
        .map(|r| r.seq)
        .chain(snapshot.supersede_records.iter().map(|r| r.seq))
        .collect();
    all_seqs.sort_unstable();
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    for &s in &all_seqs {
        if !seen.insert(s) {
            return Err(SnapshotError::AuditSequenceDuplicate(s));
        }
    }
    let n = all_seqs.len() as u64;
    for (idx, &s) in all_seqs.iter().enumerate() {
        let expected = (idx as u64) + 1;
        if s != expected {
            return Err(SnapshotError::AuditSequenceNotDense { expected, found: s });
        }
    }
    let expected_audit = n;
    if snapshot.audit_sequence != expected_audit {
        return Err(SnapshotError::AuditSequenceNotDense {
            expected: expected_audit,
            found: snapshot.audit_sequence,
        });
    }

    // (7) INV-C15 üç yönlü triangulation: committed Supersedes edge ↔ SupersedeRecord ↔ status.
    //     Lane-sensitive: yalnız committed (Accepted) Supersedes edge'leri sayılır.
    //     Edge yönü: successor --Supersedes--> superseded (from=successor, to=superseded).
    //
    //     Duplicate detection (Review P1.3): BTreeSet yerine BTreeMap<Pair, usize> —
    //     aynı (successor, superseded) çiftine sahip iki committed edge set içinde tek
    //     elemana çökerdi. Occurrence count ile her pair tam 1 kez görünmeli.
    let mut committed_pairs: BTreeMap<(String, String), u32> = BTreeMap::new();
    for e in snapshot.graph.edges.iter().filter(|e| {
        e.kind == ConceptEdgeKind::Supersedes && e.decision_status == DecisionStatus::Accepted
    }) {
        *committed_pairs
            .entry((e.from.0.clone(), e.to.0.clone()))
            .or_default() += 1;
    }
    let mut recorded_pairs: BTreeMap<(String, String), u32> = BTreeMap::new();
    for r in &snapshot.supersede_records {
        *recorded_pairs
            .entry((r.successor.0.clone(), r.superseded.0.clone()))
            .or_default() += 1;
    }
    if committed_pairs != recorded_pairs {
        return Err(SnapshotError::SupersedeTriangulationMismatch);
    }
    // Her pair occurrence count tam 1 olmalı (duplicate edge/record reject).
    for (pair, count) in &committed_pairs {
        if *count != 1 {
            return Err(SnapshotError::SupersedeDuplicatePair {
                successor: pair.0.clone(),
                superseded: pair.1.clone(),
                count: *count,
            });
        }
    }

    // (7a) Her SupersededAccepted node'un tam 1 committed incoming edge'i (cardinality).
    let mut incoming: BTreeMap<String, u32> = BTreeMap::new();
    for pair in committed_pairs.keys() {
        *incoming.entry(pair.1.clone()).or_default() += 1;
    }
    for n in &snapshot.graph.nodes {
        if n.decision_status == DecisionStatus::SupersededAccepted {
            let count = incoming.get(&n.id.0).copied().unwrap_or(0);
            if count == 0 {
                return Err(SnapshotError::SupersedeMissingIncomingEdge(n.id.clone()));
            }
            if count > 1 {
                return Err(SnapshotError::SupersedeMultipleIncomingEdges(n.id.clone()));
            }
        }
    }
    // (7b) Her committed edge'in target'ı SupersededAccepted.
    for pair in committed_pairs.keys() {
        let target_status = status_of
            .get(&pair.1)
            .copied()
            .unwrap_or(DecisionStatus::Candidate);
        if target_status != DecisionStatus::SupersededAccepted {
            return Err(SnapshotError::SupersedeEdgeTargetNotSuperseded(
                ConceptNodeId(pair.1.clone()),
            ));
        }
    }

    // (7c) Committed-only cycle absence: successor --Supersedes--> superseded graph'ında cycle yok.
    //      DFS over committed edges only (lane-sensitive).
    if has_committed_supersedes_cycle(&snapshot.graph.edges) {
        return Err(SnapshotError::SupersedeCycle);
    }

    Ok(())
}

/// Committed (Accepted) Supersedes edge'lerinden oluşan graph'ta cycle var mı?
/// Lane-sensitive: Candidate proposal edge'leri dahil DEĞİL. Edge yönü:
/// successor --Supersedes--> superseded (from=successor, to=superseded).
fn has_committed_supersedes_cycle(edges: &[crate::anchoring::types::ConceptEdge]) -> bool {
    use crate::anchoring::ConceptEdgeKind;
    use std::collections::{BTreeMap, BTreeSet};

    // Committed adjacency: from → [to, ...].
    let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in edges {
        if e.kind == ConceptEdgeKind::Supersedes && e.decision_status == DecisionStatus::Accepted {
            adj.entry(e.from.0.clone())
                .or_default()
                .push(e.to.0.clone());
        }
    }

    // İteratif DFS (renk-bazlı) — adversarial derin zincir stack overflow savunması
    // (Review 2.tur F3). `restore_snapshot` güvenilir olmayan input'tan çalışır;
    // recursion derinliği zincir uzunluğu kadardır → explicit stack ile sınırsız.
    //
    // Renkler: WHITE (ziyaret edilmedi), GRAY (path üzerinde — back-edge = cycle),
    // BLACK (tamamen işlendi).
    let mut gray: BTreeSet<String> = BTreeSet::new();
    let mut black: BTreeSet<String> = BTreeSet::new();
    for start in adj.keys() {
        if black.contains(start) {
            continue;
        }
        // (node, neighbor_index) stack — her frame bir node'un neighbor'larını tüketir.
        let mut work: Vec<(String, usize)> = vec![(start.clone(), 0)];
        gray.insert(start.clone());
        while let Some((node, idx)) = work.last().cloned() {
            let neighbors = adj.get(&node).cloned().unwrap_or_default();
            if idx >= neighbors.len() {
                // Tüm neighbor'lar işlendi — node BLACK, stack'ten çıkar.
                work.pop();
                gray.remove(&node);
                black.insert(node);
                continue;
            }
            // index'i ilerlet.
            work.last_mut().unwrap().1 = idx + 1;
            let next = neighbors[idx].clone();
            if gray.contains(&next) {
                return true; // back-edge → cycle
            }
            if !black.contains(&next) {
                gray.insert(next.clone());
                work.push((next, 0));
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::types::{
        AnchorCandidate, AnchorScoreBreakdown, ConceptNodeKind, GraphSeed,
    };
    use crate::anchoring::{AnchorDecisionKind, ConceptEdgeKind, PositionFamily, ThresholdBand};
    use std::collections::BTreeSet;

    fn make_plan(candidates: Vec<AnchorCandidate>) -> AnchorPlan {
        AnchorPlan {
            packet_id: crate::anchoring::types::ConceptPacketId("pkt:1".into()),
            candidates,
            decision: AnchorDecisionKind::TentativeLink,
            threshold_band: ThresholdBand::Tentative,
            requires_operator_review: false,
            negative_assertions: vec![],
            redirects: vec![],
        }
    }

    fn candidate(target: &str, kind: ConceptEdgeKind) -> AnchorCandidate {
        AnchorCandidate {
            packet_id: crate::anchoring::types::ConceptPacketId("pkt:1".into()),
            target_node_id: ConceptNodeId(target.into()),
            edge_kind: kind,
            score: AnchorScoreBreakdown::zeroed(),
            explanation: Some(
                crate::anchoring::types::NonEmptyExplanation::from_validated("test".into()),
            ),
        }
    }

    #[test]
    fn apply_plan_creates_ghost_nodes_as_candidate() {
        let mut store = InMemoryAnchorStore::new();
        let plan = make_plan(vec![
            candidate("Concept:Payment", ConceptEdgeKind::Mentions),
            candidate("RiskCandidate:X", ConceptEdgeKind::DerivesRisk),
        ]);
        let res = store.apply_plan(&plan).expect("apply");
        assert_eq!(res.new_nodes, 2);
        assert_eq!(res.new_edges, 2);
        // INV-C5: tüm yeni node'lar Candidate
        for n in store.graph().nodes_iter() {
            assert_eq!(n.decision_status, DecisionStatus::Candidate);
        }
    }

    #[test]
    fn mainline_query_empty_when_all_candidate() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:X",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();
        assert_eq!(
            store.mainline_query().unwrap().len(),
            0,
            "INV-C3: Candidate mainline değil"
        );
    }

    #[test]
    fn candidate_query_returns_pending() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:X",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();
        assert_eq!(store.candidate_query().unwrap().len(), 1);
    }

    /// candidate_query deterministic — ID ascending sort (HashMap iteration random;
    /// mainline_query ile aynı disiplin — Review 2.tur P2.1).
    #[test]
    fn candidate_query_is_deterministically_ordered() {
        let mk = |id: &str| ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        // Farklı ekleme sırasıyla.
        seed.rule_candidates.push(mk("RuleCandidate:Zeta"));
        seed.rule_candidates.push(mk("RuleCandidate:Alpha"));
        seed.rule_candidates.push(mk("RuleCandidate:Mid"));
        let store = InMemoryAnchorStore::with_seed(seed);
        let ids: Vec<String> = store
            .candidate_query()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        assert_eq!(
            ids,
            vec![
                "RuleCandidate:Alpha".to_string(),
                "RuleCandidate:Mid".to_string(),
                "RuleCandidate:Zeta".to_string(),
            ],
            "candidate_query ID-ascending deterministic regardless of insertion order"
        );
    }

    #[test]
    fn store_promotion_via_review_session_moves_to_mainline() {
        // Faz 8c: promote_to_accepted legacy path kaldırıldı.
        // Promotion artık OperatorReviewSession + apply_decision ile (INV-C12/C13).
        use crate::anchoring::review::{OperatorId, OperatorReviewSession, PresentedBasis};
        use crate::anchoring::types::NonEmptyExplanation;

        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:Payment",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();

        let node_id = ConceptNodeId("Concept:Payment".into());
        let mut session =
            OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let basis = PresentedBasis::compile(&store, &node_id).expect("basis compile");
        let reason = NonEmptyExplanation::from_validated("test promotion".into());
        session
            .accept(&mut store, &node_id, basis, reason)
            .expect("accept");

        // Artık mainline'da (INV-C3 — Candidate→Accepted promotion)
        assert_eq!(store.mainline_query().unwrap().len(), 1);
        assert_eq!(store.candidate_query().unwrap().len(), 0);
        // INV-C13: ledger'a kayıt düştü
        assert_eq!(store.decision_ledger().len(), 1);
    }

    // Faz 8c: 'apply_decision_rejects_unknown_node' testi kaldırıldı.
    // Bilinmeyen node ve NotPromotableFrom yolları review.rs'te zaten thorough test ediliyor:
    //   - review_session_not_found_rejects_unknown_candidate
    //   - review_session_not_promotable_rejects_accepted_node
    //   - apply_decision_rejects_accepted_node_not_promotable_from

    #[test]
    fn seed_loads_graph_state() {
        let mut seed = GraphSeed::default();
        seed.concepts.push(ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        });
        let store = InMemoryAnchorStore::with_seed(seed);
        assert_eq!(store.node_count().unwrap(), 1);
        assert_eq!(
            store.mainline_query().unwrap().len(),
            1,
            "seed Accepted mainline"
        );
    }

    #[test]
    fn find_concepts_by_canonical() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:Payment",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();
        let found = store.find_concepts_by_canonical("Payment").unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn seed_trusted_via_trait() {
        // Faz 3: seed_trusted trait metodu
        let mut store = InMemoryAnchorStore::new();
        let mut seed = GraphSeed::default();
        seed.concepts.push(ConceptNode {
            id: ConceptNodeId("Concept:Auth".into()),
            canonical: "Auth".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        });
        AnchorStore::seed_trusted(&mut store, &seed).unwrap();
        assert_eq!(store.mainline_query().unwrap().len(), 1);
    }

    #[test]
    fn restore_graph_only_for_trusted_bootstrap_roundtrip() {
        // Faz 3: ConceptGraphSnapshot restore (INV-C3 trusted boundary — graph-only).
        use crate::anchoring::types::ConceptGraphSnapshot;
        let node = ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec!["ödeme".into()],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        };
        let snapshot = ConceptGraphSnapshot {
            nodes: vec![node],
            edges: vec![],
            schema_version: 1,
        };
        let store =
            InMemoryAnchorStore::restore_graph_only_for_trusted_bootstrap(snapshot).unwrap();
        assert_eq!(store.node_count().unwrap(), 1);
        assert_eq!(
            store.mainline_query().unwrap().len(),
            1,
            "restored Accepted"
        );
    }

    #[test]
    fn restore_graph_only_for_trusted_bootstrap_rejects_version_mismatch() {
        // Faz 3 #2: schema_version mismatch → InvalidSnapshotVersion (graph-only).
        use crate::anchoring::types::ConceptGraphSnapshot;
        let snapshot = ConceptGraphSnapshot {
            nodes: vec![],
            edges: vec![],
            schema_version: 999, // mismatch
        };
        let err =
            InMemoryAnchorStore::restore_graph_only_for_trusted_bootstrap(snapshot).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::InvalidSnapshotVersion {
                    expected: 1,
                    found: 999
                }
            ),
            "version mismatch reject"
        );
    }

    #[test]
    fn store_error_serde_roundtrip() {
        // Faz 3 #3: StoreError thiserror+serde — #[serde(tag="kind")] + newtype kombinasyonu
        let err = StoreError::NodeNotFound(ConceptNodeId("Concept:X".into()));
        let json = serde_json::to_string(&err).unwrap();
        let back: StoreError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);

        let err2 = StoreError::InvalidSnapshotVersion {
            expected: 1,
            found: 2,
        };
        let json2 = serde_json::to_string(&err2).unwrap();
        let back2: StoreError = serde_json::from_str(&json2).unwrap();
        assert_eq!(err2, back2);
    }

    // ── Faz 5a (T6, Patch 4/5): TaskCandidate promote — INV-T2 boundary ───────

    #[test]
    fn promote_task_candidate_does_not_create_trajectory_task() {
        // Faz 8c: promote_to_accepted kaldırıldı; OperatorReviewSession kullanılır.
        // Patch 4/5: Accepted TaskCandidate ≠ trajectory::Task.
        // PR33a anchoring içinde kalır — trajectory genesis'e (OperatorCapability,
        // INV-T2) dokunmaz. Bu test promote sonrası node'un hala TaskCandidate
        // olduğunu + status Accepted olduğunu doğrular. trajectory::Task yaratımı
        // PR33b'ye (operator console / bridge).
        use crate::anchoring::review::{OperatorId, OperatorReviewSession, PresentedBasis};
        use crate::anchoring::types::NonEmptyExplanation;

        let task_node = ConceptNode {
            id: ConceptNodeId("TaskCandidate:AuthServiceRefactor".into()),
            canonical: "AuthServiceRefactor".into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::TaskCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        };
        let mut store = InMemoryAnchorStore::with_seed(GraphSeed {
            task_candidates: vec![task_node],
            ..Default::default()
        });

        // Faz 8c: OperatorReviewSession ile promote (INV-C12/C13).
        let node_id = ConceptNodeId("TaskCandidate:AuthServiceRefactor".into());
        let mut session =
            OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let basis = PresentedBasis::compile(&store, &node_id).expect("basis compile");
        let reason = NonEmptyExplanation::from_validated("task candidate promote".into());
        session
            .accept(&mut store, &node_id, basis, reason)
            .expect("TaskCandidate promote");

        // Node hala TaskCandidate (kind değişmez), status Accepted.
        let node = store
            .graph()
            .node(&ConceptNodeId("TaskCandidate:AuthServiceRefactor".into()))
            .expect("node mevcut");
        assert_eq!(
            node.node_kind,
            ConceptNodeKind::TaskCandidate,
            "kind değişmez"
        );
        assert_eq!(node.decision_status, DecisionStatus::Accepted);

        // PR33a: trajectory::Task yaratılmaz (compile-level — bu test `trajectory`
        // modülüne reference içermiyor; navigator'a bağlanma PR33b).
        // INV-T2 ihlal yok: Task genesis OperatorCapability gerektirir, PR33a'da yok.
    }

    #[test]
    fn graph_seed_candidate_buckets_backward_compatible() {
        // Patch 6: GraphSeed yeni bucket'lar Default ile backward-compat.
        // Eski yapı (3 bucket) hala çalışır; yeni bucket'lar boş başlar.
        let seed = GraphSeed {
            concepts: vec![ConceptNode {
                id: ConceptNodeId("Concept:Payment".into()),
                canonical: "Payment".into(),
                aliases: Vec::new(),
                node_kind: ConceptNodeKind::Concept,
                decision_status: DecisionStatus::Accepted,
                position_family: crate::anchoring::PositionFamily::ConceptualIntent,
            }],
            ..Default::default()
        };
        let store = InMemoryAnchorStore::with_seed(seed);
        assert_eq!(store.node_count().unwrap(), 1, "concepts seed'lendi");

        // Yeni bucket'lar boş → all_nodes yine 1 node.
        let seed2 = GraphSeed {
            rule_candidates: vec![ConceptNode {
                id: ConceptNodeId("RuleCandidate:NoCoupling".into()),
                canonical: "NoCoupling".into(),
                aliases: Vec::new(),
                node_kind: ConceptNodeKind::RuleCandidate,
                decision_status: DecisionStatus::Candidate,
                position_family: crate::anchoring::PositionFamily::ConceptualIntent,
            }],
            task_candidates: vec![ConceptNode {
                id: ConceptNodeId("TaskCandidate:Refactor".into()),
                canonical: "Refactor".into(),
                aliases: Vec::new(),
                node_kind: ConceptNodeKind::TaskCandidate,
                decision_status: DecisionStatus::Candidate,
                position_family: crate::anchoring::PositionFamily::ConceptualIntent,
            }],
            ..Default::default()
        };
        let store2 = InMemoryAnchorStore::with_seed(seed2);
        assert_eq!(
            store2.node_count().unwrap(),
            2,
            "candidate bucket'lar seed'lendi"
        );
    }

    /// Re-proposal after rejection — characterization (Paper 3 Faz 8a+).
    ///
    /// Bu test normative DEĞİL — mevcut apply_plan davranışını karakterize eder.
    /// "Observed behavior ≠ intended reversal protocol."
    ///
    /// Senaryo: RuleCandidate:X reject edilir. Aynı canonical'a ikinci DerivesRule
    /// candidate içeren plan apply_plan'a verilirse ne olur?
    ///
    /// Gözlenen: apply_plan_inner "node yoksa ghost oluştur" mantığıyla çalışır —
    /// reddedilmiş node zaten var, new_nodes=0, sadece edge eklenir. Status DEĞİŞMEZ.
    /// Phase 8b ReopenSession normative reversal semantics tanımlayacak.
    #[test]
    fn re_proposal_after_rejection_current_semantics_is_characterized() {
        use crate::anchoring::types::{
            AnchorCandidate, AnchorPlan, AnchorScoreBreakdown, ConceptPacketId,
        };
        use crate::anchoring::{AnchorDecisionKind, ConceptEdgeKind, ThresholdBand};

        // Reddedilmiş RuleCandidate:X seed'le.
        let rejected_node = ConceptNode {
            id: ConceptNodeId("RuleCandidate:X".into()),
            canonical: "X".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Rejected,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(rejected_node);
        let mut store = InMemoryAnchorStore::with_seed(seed);

        // Aynı RuleCandidate:X target'ına ikinci DerivesRule edge'li plan.
        let plan = AnchorPlan {
            packet_id: ConceptPacketId("pkt:reproposal".into()),
            candidates: vec![AnchorCandidate {
                packet_id: ConceptPacketId("pkt:reproposal".into()),
                target_node_id: ConceptNodeId("RuleCandidate:X".into()),
                edge_kind: ConceptEdgeKind::DerivesRule,
                score: AnchorScoreBreakdown::zeroed(),
                explanation: Some(
                    crate::anchoring::types::NonEmptyExplanation::new(
                        "re-proposal after rejection",
                    )
                    .unwrap(),
                ),
            }],
            decision: AnchorDecisionKind::TentativeLink,
            threshold_band: ThresholdBand::Tentative,
            requires_operator_review: true,
            negative_assertions: vec![],
            redirects: vec![],
        };

        let apply_result = store.apply_plan(&plan).expect("apply_plan");

        // KARAKTERİZASYON: reddedilmiş node zaten var → new_nodes = 0.
        assert_eq!(
            apply_result.new_nodes, 0,
            "reddedilmiş node zaten var → yeni node doğmuyor (duplicate önleniyor)"
        );
        assert_eq!(
            apply_result.new_edges, 1,
            "ikinci DerivesRule edge ekleniyor"
        );

        // Status DEĞİŞMEDİ — hala Rejected.
        let node_after = store
            .graph()
            .node(&ConceptNodeId("RuleCandidate:X".into()))
            .expect("node mevcut");
        assert_eq!(
            node_after.decision_status,
            DecisionStatus::Rejected,
            "re-proposal status'ü DEĞİŞTİRMİYOR — hala Rejected"
        );

        // SONUÇ (Paper 3 §10/§11'e yansır): "The current canon gate preserves canonical
        // identity even across rejected nodes, but this is not yet a reversal protocol;
        // it makes re-proposal visible as a collision with prior rejection (new edge to
        // rejected node, no status change)." Phase 8b ReopenSession normative reversal.
    }

    // ─── Faz 8b (PR #48): mainline_history + INV-C14 tests ──────────────────────

    /// Test yardımcı: belirli bir statüde bir node seed'le.
    fn node_with_status(id: &str, status: DecisionStatus) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: status,
            position_family: PositionFamily::ConceptualIntent,
        }
    }

    /// INV-C14 exact-set: `mainline_history` tam olarak {Accepted, SupersededAccepted}
    /// döndürür. Candidate/Deprecated/Rejected hariç. BTreeSet ile ID karşılaştırması
    /// — yanlışlıkla üçüncü statünün eklenmesi veya yanlış node gelmesi kaçmaz.
    #[test]
    fn mainline_history_contains_exactly_accepted_provenance_statuses() {
        let mk = node_with_status;
        let mut seed = GraphSeed::default();
        seed.rule_candidates
            .push(mk("RuleCandidate:Cand", DecisionStatus::Candidate));
        seed.rule_candidates
            .push(mk("RuleCandidate:Acc", DecisionStatus::Accepted));
        seed.rule_candidates
            .push(mk("RuleCandidate:Dep", DecisionStatus::Deprecated));
        seed.rule_candidates
            .push(mk("RuleCandidate:Rej", DecisionStatus::Rejected));
        seed.rule_candidates
            .push(mk("RuleCandidate:Sup", DecisionStatus::SupersededAccepted));
        let store = InMemoryAnchorStore::with_seed(seed);

        let history_ids: BTreeSet<String> = store
            .mainline_history()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        let expected: BTreeSet<String> = [
            "RuleCandidate:Acc".to_string(),
            "RuleCandidate:Sup".to_string(),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            history_ids, expected,
            "history = exactly Accepted + SupersededAccepted"
        );
    }

    /// INV-C14 subset yarısı: `mainline_query ⊆ mainline_history`.
    /// ID setleri üzerinden — Vec sırasından bağımsız.
    #[test]
    fn mainline_query_is_subset_of_mainline_history() {
        let mk = node_with_status;
        let mut seed = GraphSeed::default();
        seed.rule_candidates
            .push(mk("RuleCandidate:Acc", DecisionStatus::Accepted));
        seed.rule_candidates
            .push(mk("RuleCandidate:Sup", DecisionStatus::SupersededAccepted));
        let store = InMemoryAnchorStore::with_seed(seed);

        let current_ids: BTreeSet<String> = store
            .mainline_query()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        let history_ids: BTreeSet<String> = store
            .mainline_history()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        assert!(
            current_ids.is_subset(&history_ids),
            "INV-C14: mainline_query must be a subset of mainline_history"
        );
        // SupersededAccepted current'da DEĞİL (intersection boş).
        assert!(
            !current_ids.contains("RuleCandidate:Sup"),
            "INV-C14: SupersededAccepted excluded from current mainline"
        );
    }

    /// `mainline_history` deterministik ID sıralaması — seed sırasından bağımsız.
    /// NOT: bu sunum sırasıdır, kabul kronolojisi DEĞİL.
    #[test]
    fn mainline_history_is_deterministically_ordered() {
        let mk = node_with_status;
        // Node'ları ascending-olmayan sırada seed et (Z, A). ConceptGraph HashMap
        // kullandığı için iteration sırası insertion'u takip etmez; query sonucu her
        // durumda ID-ascending olmalı (trait sözleşmesi).
        let mut seed = GraphSeed::default();
        seed.rule_candidates
            .push(mk("RuleCandidate:Zeta", DecisionStatus::Accepted));
        seed.rule_candidates.push(mk(
            "RuleCandidate:Alpha",
            DecisionStatus::SupersededAccepted,
        ));
        let store = InMemoryAnchorStore::with_seed(seed);

        let history: Vec<String> = store
            .mainline_history()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        assert_eq!(
            history,
            vec![
                "RuleCandidate:Alpha".to_string(),
                "RuleCandidate:Zeta".to_string()
            ],
            "deterministic ID-ascending order regardless of insertion order"
        );
    }

    /// `mainline_query` deterministik ID sıralaması — seed sırasından bağımsız.
    /// `mainline_history` ile aynı sunum sırası (agent-facing context tekrarlanabilirliği).
    /// Sadece Accepted node'lar (INV-C3 current mainline); SupersededAccepted hariç.
    #[test]
    fn mainline_query_is_deterministically_ordered() {
        let mk = node_with_status;
        // Node'ları ascending-olmayan sırada seed et (Z, A). ConceptGraph HashMap
        // kullandığı için iteration sırası insertion'u takip etmez; query sonucu her
        // durumda ID-ascending olmalı (trait sözleşmesi).
        let mut seed = GraphSeed::default();
        seed.rule_candidates
            .push(mk("RuleCandidate:Zeta", DecisionStatus::Accepted));
        seed.rule_candidates
            .push(mk("RuleCandidate:Alpha", DecisionStatus::Accepted));
        let store = InMemoryAnchorStore::with_seed(seed);

        let current: Vec<String> = store
            .mainline_query()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        assert_eq!(
            current,
            vec![
                "RuleCandidate:Alpha".to_string(),
                "RuleCandidate:Zeta".to_string()
            ],
            "deterministic ID-ascending order regardless of insertion order"
        );
    }

    // Not: apply_decision'ın SupersededAccepted terminal-statü defense-in-depth testi
    // review.rs test bloğunda — orada SessionId constructor erişilebilir (aynı modül).
    // (apply_decision_rejects_superseded_accepted_not_promotable)

    // ═══════════════════════════════════════════════════════════════════════════════
    // AnchorStoreSnapshot — export/restore round-trip + invariant-validasyon testleri.
    //
    // "Persistence does not weaken epistemic gates" — restore_snapshot, deserialize
    // ile epistemic gate'leri bypass etmemeli. paper3'ün "known gap" cümlesini
    // (representable-but-not-transitioned nodes) evaluated AnchorStoreSnapshot path
    // için kapatır.
    // ═══════════════════════════════════════════════════════════════════════════════

    use crate::anchoring::review::{
        OperatorId, OperatorReviewSession, PresentedBasis, SupersedeSession,
    };
    use crate::anchoring::NonEmptyExplanation;

    /// Test yardımcı: belirli statüde node seed'le.
    fn snap_node(id: &str, status: DecisionStatus) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: status,
            position_family: PositionFamily::ConceptualIntent,
        }
    }

    /// Test yardımcı: Candidate node'lu store (review için).
    fn snap_store_with_candidates(ids: &[&str]) -> InMemoryAnchorStore {
        let mut seed = GraphSeed::default();
        for id in ids {
            seed.rule_candidates
                .push(snap_node(id, DecisionStatus::Candidate));
        }
        InMemoryAnchorStore::with_seed(seed)
    }

    /// Test yardımcı: Candidate'ı Accepted'e promote et (production path — OperatorReviewSession).
    fn snap_accept(
        store: &mut InMemoryAnchorStore,
        id: &str,
    ) -> crate::anchoring::review::DecisionRecord {
        let nid = ConceptNodeId(id.into());
        let basis = PresentedBasis::compile(store, &nid).expect("basis");
        let reason = NonEmptyExplanation::new("test accept").unwrap();
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test"));
        session.accept(store, &nid, basis, reason).expect("accept")
    }

    /// Test yardımcı: Candidate'ı Rejected'a çevir (production path).
    fn snap_reject(
        store: &mut InMemoryAnchorStore,
        id: &str,
    ) -> crate::anchoring::review::DecisionRecord {
        let nid = ConceptNodeId(id.into());
        let basis = PresentedBasis::compile(store, &nid).expect("basis");
        let reason = NonEmptyExplanation::new("test reject").unwrap();
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test"));
        session.reject(store, &nid, basis, reason).expect("reject")
    }

    /// Test yardımcı: supersede (production path — SupersedeSession).
    fn snap_supersede(
        store: &mut InMemoryAnchorStore,
        superseded: &str,
        successor: &str,
    ) -> crate::anchoring::review::SupersedeRecord {
        use crate::anchoring::review::PresentedSupersedeBasis;
        let sup = ConceptNodeId(superseded.into());
        let suc = ConceptNodeId(successor.into());
        let basis = PresentedSupersedeBasis::compile(store, &sup, &suc).expect("basis");
        let reason = NonEmptyExplanation::new("test supersede").unwrap();
        let mut session = SupersedeSession::open_for_operator(OperatorId::new("test"));
        session
            .supersede(store, &sup, &suc, basis, reason)
            .expect("supersede")
    }

    /// Mutlu yol round-trip: seed → accept/reject → supersede → export → restore → aynı state.
    #[test]
    fn snapshot_roundtrip_preserves_full_state() {
        let mut store =
            snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"]);
        snap_accept(&mut store, "RuleCandidate:A"); // seq 1
        snap_reject(&mut store, "RuleCandidate:B"); // seq 2
        snap_accept(&mut store, "RuleCandidate:C"); // seq 3
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:C"); // seq 4

        let before = snapshot_store(&store);
        let exported = store.export_snapshot();
        let restored = InMemoryAnchorStore::restore_snapshot(exported).expect("restore");
        let after = snapshot_store(&restored);
        assert_eq!(after.graph, before.graph, "graph identical");
        assert_eq!(
            after.decision_records, before.decision_records,
            "decision ledger identical"
        );
        assert_eq!(
            after.supersede_records, before.supersede_records,
            "supersede ledger identical"
        );
        assert_eq!(
            after.audit_sequence, before.audit_sequence,
            "audit_seq preserved"
        );
    }

    /// Snapshot yardımcı: full store-state karşılaştırma için.
    fn snapshot_store(store: &InMemoryAnchorStore) -> AnchorStoreSnapshot {
        store.export_snapshot()
    }

    /// audit_seq yoğunluk: boş ledger → audit_seq == 0.
    #[test]
    fn snapshot_empty_store_has_audit_seq_zero() {
        let store = InMemoryAnchorStore::new();
        let exported = store.export_snapshot();
        assert_eq!(exported.audit_sequence, 0);
        assert!(exported.decision_records.is_empty());
        assert!(exported.supersede_records.is_empty());
        InMemoryAnchorStore::restore_snapshot(exported).expect("empty restore");
    }

    /// audit_seq yoğunluk: union unique + {1..N} + == N. (production yollar dense üretir)
    #[test]
    fn snapshot_dense_audit_sequence_validates() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B"]);
        snap_accept(&mut store, "RuleCandidate:A"); // seq 1
        snap_reject(&mut store, "RuleCandidate:B"); // seq 2
        let exported = store.export_snapshot();
        assert_eq!(exported.audit_sequence, 2);
        let seqs: Vec<u64> = exported.decision_records.iter().map(|r| r.seq).collect();
        assert_eq!(seqs, vec![1, 2]);
        InMemoryAnchorStore::restore_snapshot(exported).expect("dense validates");
    }

    /// audit_seq yoğunluk ihlali: audit_seq != max(seq) → reject.
    #[test]
    fn snapshot_audit_seq_mismatch_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A"]);
        snap_accept(&mut store, "RuleCandidate:A"); // seq 1, audit_seq 1
        let mut exported = store.export_snapshot();
        exported.audit_sequence = 99; // yanlış
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(
            err,
            SnapshotError::AuditSequenceNotDense {
                expected: 1,
                found: 99
            }
        ));
    }

    /// audit_seq boşluk (gap) → reject.
    #[test]
    fn snapshot_audit_seq_gap_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B"]);
        snap_accept(&mut store, "RuleCandidate:A"); // seq 1
        snap_accept(&mut store, "RuleCandidate:B"); // seq 2
        let mut exported = store.export_snapshot();
        exported.decision_records[1].seq = 5; // boşluk
        exported.audit_sequence = 5;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(err, SnapshotError::AuditSequenceNotDense { .. }),
            "gap should reject, got {err:?}"
        );
    }

    /// audit_seq duplicate → reject.
    #[test]
    fn snapshot_audit_seq_duplicate_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B"]);
        snap_accept(&mut store, "RuleCandidate:A"); // seq 1
        snap_accept(&mut store, "RuleCandidate:B"); // seq 2
        let mut exported = store.export_snapshot();
        exported.decision_records[1].seq = 1; // duplicate seq 1
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(err, SnapshotError::AuditSequenceDuplicate(1)));
    }

    /// record → node existence: kayıt olmayan node'a referans → reject.
    #[test]
    fn snapshot_decision_record_missing_node_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A"]);
        snap_accept(&mut store, "RuleCandidate:A");
        let mut exported = store.export_snapshot();
        exported.decision_records[0].candidate_id = ConceptNodeId("RuleCandidate:Ghost".into());
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(err, SnapshotError::DecisionRecordNodeMissing(_)));
    }

    /// record → status forward integrity: Accept record ama node Candidate → reject.
    #[test]
    fn snapshot_decision_status_inconsistent_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A"]);
        snap_accept(&mut store, "RuleCandidate:A"); // node Accepted, record Accept
        let mut exported = store.export_snapshot();
        exported.graph.nodes[0].decision_status = DecisionStatus::Candidate; // node'u geri al
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(err, SnapshotError::DecisionStatusInconsistent { .. }),
            "got {err:?}"
        );
    }

    /// DecisionRecord prior_status/new_status transition kendi içinde çelişkili → reject
    /// (Review 3.tur P1.2). Record Deserialize destekli; sahte transition "persistence does
    /// not weaken epistemic gates" iddiasını deler.
    #[test]
    fn snapshot_decision_record_inconsistent_transition_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:A"]);
        snap_accept(&mut store, "RuleCandidate:A"); // record: Accept, Candidate→Accepted
        let mut exported = store.export_snapshot();
        // Record'un prior/new status'unu boz: decision=Accept ama prior=Rejected, new=Candidate.
        exported.decision_records[0].prior_status = DecisionStatus::Rejected;
        exported.decision_records[0].new_status = DecisionStatus::Candidate;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(
                err,
                SnapshotError::DecisionRecordTransitionInconsistent { seq: 1, .. }
            ),
            "got {err:?}"
        );
    }

    /// SupersedeRecord transition çelişkili → reject (Review 3.tur P1.2).
    #[test]
    fn snapshot_supersede_record_inconsistent_transition_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:Old", "RuleCandidate:New"]);
        snap_accept(&mut store, "RuleCandidate:Old");
        snap_accept(&mut store, "RuleCandidate:New");
        snap_supersede(&mut store, "RuleCandidate:Old", "RuleCandidate:New");
        let mut exported = store.export_snapshot();
        // Record: prior=Accepted, new=SupersededAccepted → boz: prior=Candidate, new=Rejected.
        exported.supersede_records[0].prior_status = DecisionStatus::Candidate;
        exported.supersede_records[0].new_status = DecisionStatus::Rejected;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(
                err,
                SnapshotError::SupersedeRecordTransitionInconsistent { .. }
            ),
            "got {err:?}"
        );
    }

    /// C15 triangulation: SupersedeRecord var ama committed edge yok → reject.
    #[test]
    fn snapshot_supersede_missing_edge_rejected() {
        let mut store =
            snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"]);
        snap_accept(&mut store, "RuleCandidate:A");
        snap_accept(&mut store, "RuleCandidate:C");
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:C"); // C→A edge + record
        let mut exported = store.export_snapshot();
        exported.graph.edges.retain(|e| {
            !(e.kind == ConceptEdgeKind::Supersedes
                && e.decision_status == DecisionStatus::Accepted)
        });
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(
                err,
                SnapshotError::SupersedeTriangulationMismatch
                    | SnapshotError::SupersedeMissingIncomingEdge(_)
            ),
            "got {err:?}"
        );
    }

    /// C15 triangulation: committed edge var ama record yok → reject.
    #[test]
    fn snapshot_supersede_missing_record_rejected() {
        let mut store =
            snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"]);
        snap_accept(&mut store, "RuleCandidate:A"); // seq 1
        snap_accept(&mut store, "RuleCandidate:C"); // seq 2
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:C"); // seq 3
        let mut exported = store.export_snapshot();
        exported.supersede_records.clear();
        // audit_seq düzelt: supersede record (seq 3) kalktı → audit_seq 2 olmalı,
        // böylece yoğunluk patlamadan triangulation mismatch yakalanır.
        exported.audit_sequence = 2;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(err, SnapshotError::SupersedeTriangulationMismatch));
    }

    /// C15: SupersededAccepted node ama incoming committed edge yok → reject.
    #[test]
    fn snapshot_superseded_without_incoming_edge_rejected() {
        let mut store =
            snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"]);
        snap_accept(&mut store, "RuleCandidate:A");
        snap_accept(&mut store, "RuleCandidate:C");
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:C"); // seq 3 (accept, accept, supersede)
        let mut exported = store.export_snapshot();
        // Hem edge hem record kaldır, node SupersededAccepted kalsın.
        exported.graph.edges.retain(|e| {
            !(e.kind == ConceptEdgeKind::Supersedes
                && e.decision_status == DecisionStatus::Accepted)
        });
        exported.supersede_records.clear();
        // audit_seq düzelt: artık sadece 2 decision record (A accept seq 1, C accept seq 2),
        // supersede seq 3 kalktı → audit_seq 2 olmalı.
        exported.audit_sequence = 2;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(err, SnapshotError::SupersedeMissingIncomingEdge(_)),
            "got {err:?}"
        );
    }

    /// C15: lane-sensitivity — Candidate Supersedes edge cardinality/cycle'i etkilemiyor.
    #[test]
    fn snapshot_candidate_supersede_edge_does_not_affect_validation() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:Old", "RuleCandidate:New"]);
        snap_accept(&mut store, "RuleCandidate:Old");
        snap_accept(&mut store, "RuleCandidate:New");
        // Candidate Supersedes edge ekle (apply_plan proposal simülasyonu).
        store.graph_mut().insert_edge(ConceptEdge {
            from: ConceptNodeId("RuleCandidate:New".into()),
            to: ConceptNodeId("RuleCandidate:Old".into()),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Candidate,
            explanation: Some(NonEmptyExplanation::new("candidate proposal").unwrap()),
        });
        let exported = store.export_snapshot();
        InMemoryAnchorStore::restore_snapshot(exported).expect("candidate edge engellemedi");
    }

    /// Successor chain (C→B→A) geçerli — successor sonradan supersede edilmiş olabilir.
    #[test]
    fn snapshot_successor_chain_valid() {
        let mut store =
            snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"]);
        snap_accept(&mut store, "RuleCandidate:A");
        snap_accept(&mut store, "RuleCandidate:B");
        snap_accept(&mut store, "RuleCandidate:C");
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:B"); // B→A
        snap_supersede(&mut store, "RuleCandidate:B", "RuleCandidate:C"); // C→B
        let exported = store.export_snapshot();
        let restored = InMemoryAnchorStore::restore_snapshot(exported).expect("chain valid");
        assert_eq!(
            restored
                .graph()
                .node(&ConceptNodeId("RuleCandidate:A".into()))
                .unwrap()
                .decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            restored
                .graph()
                .node(&ConceptNodeId("RuleCandidate:B".into()))
                .unwrap()
                .decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            restored
                .graph()
                .node(&ConceptNodeId("RuleCandidate:C".into()))
                .unwrap()
                .decision_status,
            DecisionStatus::Accepted
        );
    }

    /// Canonical serialization bit-identik: aynı store → aynı JSON bytes.
    #[test]
    fn snapshot_export_is_canonical_bit_identical() {
        let mk = |order: &str| -> InMemoryAnchorStore {
            let mut seed = GraphSeed::default();
            let ids: &[&str] = if order == "forward" {
                &[
                    "RuleCandidate:Zeta",
                    "RuleCandidate:Alpha",
                    "RuleCandidate:Mid",
                ]
            } else {
                &[
                    "RuleCandidate:Mid",
                    "RuleCandidate:Alpha",
                    "RuleCandidate:Zeta",
                ]
            };
            for id in ids {
                seed.rule_candidates
                    .push(snap_node(id, DecisionStatus::Candidate));
            }
            InMemoryAnchorStore::with_seed(seed)
        };
        let e1 = mk("forward").export_snapshot();
        let e2 = mk("reverse").export_snapshot();
        let j1 = serde_json::to_string(&e1).unwrap();
        let j2 = serde_json::to_string(&e2).unwrap();
        assert_eq!(
            j1, j2,
            "canonical serialization bit-identical regardless of insertion order"
        );
        let ids: Vec<&str> = e1.graph.nodes.iter().map(|n| n.id.0.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "RuleCandidate:Alpha",
                "RuleCandidate:Mid",
                "RuleCandidate:Zeta"
            ]
        );
    }

    /// Graph schema mismatch → SnapshotError.
    #[test]
    fn snapshot_graph_schema_mismatch_rejected() {
        let store = InMemoryAnchorStore::new();
        let mut exported = store.export_snapshot();
        exported.graph.schema_version = 999;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(
            err,
            SnapshotError::GraphSchemaMismatch {
                expected: 1,
                found: 999
            }
        ));
    }

    /// Duplicate node id → reject.
    #[test]
    fn snapshot_duplicate_node_id_rejected() {
        let mut exported = InMemoryAnchorStore::new().export_snapshot();
        let n = snap_node("RuleCandidate:Dup", DecisionStatus::Candidate);
        exported.graph.nodes.push(n.clone());
        exported.graph.nodes.push(n);
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(err, SnapshotError::DuplicateNodeId(_)));
    }

    /// Edge endpoint missing → reject.
    #[test]
    fn snapshot_edge_endpoint_missing_rejected() {
        let mut exported = InMemoryAnchorStore::new().export_snapshot();
        exported.graph.edges.push(ConceptEdge {
            from: ConceptNodeId("RuleCandidate:Ghost".into()),
            to: ConceptNodeId("RuleCandidate:Also".into()),
            kind: ConceptEdgeKind::RelatedTo,
            decision_status: DecisionStatus::Candidate,
            explanation: None,
        });
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(matches!(err, SnapshotError::EdgeEndpointNotFound(_)));
    }

    /// Supersede read-only korunma: round-trip sonrası supersede ledger + status korunuyor.
    #[test]
    fn snapshot_preserves_supersede_readonly() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:Old", "RuleCandidate:New"]);
        snap_accept(&mut store, "RuleCandidate:Old");
        snap_accept(&mut store, "RuleCandidate:New");
        snap_supersede(&mut store, "RuleCandidate:Old", "RuleCandidate:New");
        let exported = store.export_snapshot();
        assert_eq!(exported.supersede_records.len(), 1);
        let restored = InMemoryAnchorStore::restore_snapshot(exported).expect("restore");
        assert_eq!(restored.supersede_ledger().len(), 1);
        assert_eq!(
            restored
                .graph()
                .node(&ConceptNodeId("RuleCandidate:Old".into()))
                .unwrap()
                .decision_status,
            DecisionStatus::SupersededAccepted
        );
    }

    /// C15 duplicate pair detection (Review P1.3): aynı (successor, superseded) çiftine
    /// sahip iki committed edge + iki record dense audit_seq ile — BTreeSet çökerdi,
    /// BTreeMap occurrence count ile her pair tam 1 kez görünmeli.
    #[test]
    fn snapshot_duplicate_committed_supersede_pair_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:Old", "RuleCandidate:New"]);
        snap_accept(&mut store, "RuleCandidate:Old"); // seq 1
        snap_accept(&mut store, "RuleCandidate:New"); // seq 2
        snap_supersede(&mut store, "RuleCandidate:Old", "RuleCandidate:New"); // seq 3
        let mut exported = store.export_snapshot();
        // Duplicate committed edge ekle (aynı pair New→Old).
        exported.graph.edges.push(ConceptEdge {
            from: ConceptNodeId("RuleCandidate:New".into()),
            to: ConceptNodeId("RuleCandidate:Old".into()),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Accepted,
            explanation: Some(NonEmptyExplanation::new("duplicate edge").unwrap()),
        });
        // Duplicate record ekle (seq 4, audit_seq 4 — dense korunur).
        let mut dup_record = exported.supersede_records[0].clone();
        dup_record.seq = 4;
        exported.supersede_records.push(dup_record);
        exported.audit_sequence = 4;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        assert!(
            matches!(err, SnapshotError::SupersedeDuplicatePair { count: 2, .. }),
            "duplicate pair should reject, got {err:?}"
        );
    }

    /// C15 duplicate SupersedeRecord (aynı pair, edge tek) → pair mismatch değil,
    /// duplicate olarak yakalanmalı (BTreeMap count farkı → mismatch önce dönebilir;
    /// bu test edge count 1, record count 2 → committed_pairs != recorded_pairs).
    #[test]
    fn snapshot_duplicate_record_with_single_edge_rejected() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:Old", "RuleCandidate:New"]);
        snap_accept(&mut store, "RuleCandidate:Old");
        snap_accept(&mut store, "RuleCandidate:New");
        snap_supersede(&mut store, "RuleCandidate:Old", "RuleCandidate:New");
        let mut exported = store.export_snapshot();
        // Sadece record duplicate (edge tek) → recorded count 2, committed count 1.
        let mut dup_record = exported.supersede_records[0].clone();
        dup_record.seq = 4;
        exported.supersede_records.push(dup_record);
        exported.audit_sequence = 4;
        let err = InMemoryAnchorStore::restore_snapshot(exported).unwrap_err();
        // Pair map'ler farklı (count 1 vs 2) → TriangulationMismatch önce döner.
        assert!(
            matches!(
                err,
                SnapshotError::SupersedeTriangulationMismatch
                    | SnapshotError::SupersedeDuplicatePair { .. }
            ),
            "duplicate record should reject, got {err:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Rich SupersedePreview read-only predicates (preview ↔ mutation divergence guard)
    // ═══════════════════════════════════════════════════════════════════════════

    /// Test yardımcı: belirli kind/family ile node (compatibility matrisi için).
    fn preview_node(
        id: &str,
        status: DecisionStatus,
        kind: ConceptNodeKind,
        family: PositionFamily,
    ) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: kind,
            decision_status: status,
            position_family: family,
        }
    }

    /// Test yardımcı: iki Accepted endpoint'li store (farklı kind/family override ile).
    fn preview_store_with_nodes(nodes: Vec<ConceptNode>) -> InMemoryAnchorStore {
        let mut seed = GraphSeed::default();
        for n in nodes {
            match n.node_kind {
                ConceptNodeKind::Concept => seed.concepts.push(n),
                ConceptNodeKind::Decision => seed.decisions.push(n),
                ConceptNodeKind::CodeEntity => seed.code_entities.push(n),
                ConceptNodeKind::RuleCandidate => seed.rule_candidates.push(n),
                ConceptNodeKind::TaskCandidate => seed.task_candidates.push(n),
                ConceptNodeKind::RiskCandidate => seed.risk_candidates.push(n),
                ConceptNodeKind::Risk => seed.risk_candidates.push(n),
                ConceptNodeKind::CodeEntityCandidate => seed.code_entities.push(n),
            }
        }
        InMemoryAnchorStore::with_seed(seed)
    }

    /// Compatibility matrix: same/diff kind × same/diff family → 4 vaka.
    #[test]
    fn supersede_compatibility_matrix() {
        // supersede_compatibility_from_parts store.rs'in kendi private fn'i — super::* ile erişilir.
        // (sup_kind, suc_kind, sup_family, suc_family) → (kind_ok, family_ok)
        let mk = |k1, k2, f1, f2| supersede_compatibility_from_parts(k1, k2, f1, f2);
        // same kind, same family
        let c = mk(
            ConceptNodeKind::RuleCandidate,
            ConceptNodeKind::RuleCandidate,
            PositionFamily::ConceptualIntent,
            PositionFamily::ConceptualIntent,
        );
        assert!(c.is_compatible() && c.kind_compatible && c.family_compatible);
        // diff kind, same family
        let c = mk(
            ConceptNodeKind::RuleCandidate,
            ConceptNodeKind::Concept,
            PositionFamily::ConceptualIntent,
            PositionFamily::ConceptualIntent,
        );
        assert!(!c.is_compatible() && !c.kind_compatible && c.family_compatible);
        // same kind, diff family
        let c = mk(
            ConceptNodeKind::RuleCandidate,
            ConceptNodeKind::RuleCandidate,
            PositionFamily::ConceptualIntent,
            PositionFamily::PhysicalCode,
        );
        assert!(!c.is_compatible() && c.kind_compatible && !c.family_compatible);
        // both diff
        let c = mk(
            ConceptNodeKind::RuleCandidate,
            ConceptNodeKind::Concept,
            PositionFamily::ConceptualIntent,
            PositionFamily::PhysicalCode,
        );
        assert!(!c.is_compatible() && !c.kind_compatible && !c.family_compatible);
    }

    /// inspect_supersede_compatibility: missing endpoint → NodeNotFound.
    #[test]
    fn inspect_supersede_compatibility_node_not_found() {
        let store = preview_store_with_nodes(vec![preview_node(
            "RuleCandidate:A",
            DecisionStatus::Accepted,
            ConceptNodeKind::RuleCandidate,
            PositionFamily::ConceptualIntent,
        )]);
        let err = store
            .inspect_supersede_compatibility(
                &ConceptNodeId("RuleCandidate:A".into()),
                &ConceptNodeId("RuleCandidate:MISSING".into()),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::NodeNotFound(_)));
    }

    /// inspect_supersede_compatibility ↔ apply_supersede step 9 characterization:
    /// predicate false ise mutation IncompatibleSupersedeEndpoints döner (tek source).
    /// Production path (SupersedeSession → apply_supersede) ile.
    #[test]
    fn inspect_supersede_compatibility_matches_apply_supersede_step9() {
        use crate::anchoring::review::{PresentedSupersedeBasis, SupersedeSession};
        let mut store = preview_store_with_nodes(vec![
            preview_node(
                "RuleCandidate:Old",
                DecisionStatus::Accepted,
                ConceptNodeKind::RuleCandidate,
                PositionFamily::ConceptualIntent,
            ),
            preview_node(
                "Concept:New",
                DecisionStatus::Accepted,
                ConceptNodeKind::Concept, // different kind → incompatible
                PositionFamily::ConceptualIntent,
            ),
        ]);
        let sup = ConceptNodeId("RuleCandidate:Old".into());
        let suc = ConceptNodeId("Concept:New".into());
        // predicate: incompatible (diff kind)
        let compat = store.inspect_supersede_compatibility(&sup, &suc).unwrap();
        assert!(!compat.is_compatible());
        // mutation (production path): same verdict (tek source). SupersedeSession → apply_supersede.
        let basis = PresentedSupersedeBasis::compile(&store, &sup, &suc).expect("basis");
        let reason = NonEmptyExplanation::new("test").unwrap();
        let mut session = SupersedeSession::open_for_operator(OperatorId::new("test"));
        let err = session
            .supersede(&mut store, &sup, &suc, basis, reason)
            .unwrap_err();
        // SupersedeError::Store(Box<IncompatibleSupersedeEndpoints>) — downcast ile doğrula.
        use crate::anchoring::review::SupersedeError;
        match err {
            SupersedeError::Store(source) => {
                assert!(
                    source
                        .downcast_ref::<StoreError>()
                        .map_or(false, |e| matches!(
                            e,
                            StoreError::IncompatibleSupersedeEndpoints { .. }
                        )),
                    "mutation must match predicate (tek source), got source: {source}"
                );
            }
            other => panic!("expected Store(IncompatibleSupersedeEndpoints), got {other:?}"),
        }
    }

    /// committed_supersede_incoming_sources: committed incoming → [successor].
    #[test]
    fn committed_supersede_incoming_sources_returns_successor() {
        let mut store = snap_store_with_candidates(&["RuleCandidate:Old", "RuleCandidate:New"]);
        snap_accept(&mut store, "RuleCandidate:Old");
        snap_accept(&mut store, "RuleCandidate:New");
        snap_supersede(&mut store, "RuleCandidate:Old", "RuleCandidate:New"); // New→Old committed
        let sources = store
            .committed_supersede_incoming_sources(&ConceptNodeId("RuleCandidate:Old".into()))
            .unwrap();
        assert_eq!(sources, vec![ConceptNodeId("RuleCandidate:New".into())]);
    }

    /// committed_supersede_incoming_sources: Candidate proposal edge sayılmaz.
    #[test]
    fn committed_supersede_incoming_sources_ignores_candidate_proposal() {
        let mut store = preview_store_with_nodes(vec![
            preview_node(
                "RuleCandidate:Old",
                DecisionStatus::Accepted,
                ConceptNodeKind::RuleCandidate,
                PositionFamily::ConceptualIntent,
            ),
            preview_node(
                "RuleCandidate:New",
                DecisionStatus::Accepted,
                ConceptNodeKind::RuleCandidate,
                PositionFamily::ConceptualIntent,
            ),
        ]);
        // Candidate proposal edge (decision_status Candidate) — sayılmamalı.
        store.graph_mut().insert_edge(ConceptEdge {
            from: ConceptNodeId("RuleCandidate:New".into()),
            to: ConceptNodeId("RuleCandidate:Old".into()),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Candidate,
            explanation: Some(
                crate::anchoring::types::NonEmptyExplanation::from_validated("proposal".into()),
            ),
        });
        let sources = store
            .committed_supersede_incoming_sources(&ConceptNodeId("RuleCandidate:Old".into()))
            .unwrap();
        assert!(sources.is_empty(), "Candidate proposal must not count");
    }

    /// committed_supersede_incoming_sources: missing node → NodeNotFound.
    #[test]
    fn committed_supersede_incoming_sources_missing_node() {
        let store = InMemoryAnchorStore::new();
        let err = store
            .committed_supersede_incoming_sources(&ConceptNodeId("RuleCandidate:MISSING".into()))
            .unwrap_err();
        assert!(matches!(err, StoreError::NodeNotFound(_)));
    }

    /// committed_supersede_incoming_sources: invalid store, çoklu incoming → deterministik sorted.
    /// (Validated snapshot INV-C15 ≤1; bu invalid direct-store davranışını sabitler.)
    #[test]
    fn committed_supersede_incoming_sources_multiple_deterministic() {
        let mut store = preview_store_with_nodes(vec![
            preview_node(
                "RuleCandidate:Old",
                DecisionStatus::SupersededAccepted,
                ConceptNodeKind::RuleCandidate,
                PositionFamily::ConceptualIntent,
            ),
            preview_node(
                "RuleCandidate:New1",
                DecisionStatus::Accepted,
                ConceptNodeKind::RuleCandidate,
                PositionFamily::ConceptualIntent,
            ),
            preview_node(
                "RuleCandidate:New2",
                DecisionStatus::Accepted,
                ConceptNodeKind::RuleCandidate,
                PositionFamily::ConceptualIntent,
            ),
        ]);
        // İki committed incoming edge (invalid — INV-C15 ihlali, ama accessor dürüst davranır).
        for succ in ["RuleCandidate:New2", "RuleCandidate:New1"] {
            store.graph_mut().insert_edge(ConceptEdge {
                from: ConceptNodeId(succ.into()),
                to: ConceptNodeId("RuleCandidate:Old".into()),
                kind: ConceptEdgeKind::Supersedes,
                decision_status: DecisionStatus::Accepted,
                explanation: Some(
                    crate::anchoring::types::NonEmptyExplanation::from_validated("t".into()),
                ),
            });
        }
        let sources = store
            .committed_supersede_incoming_sources(&ConceptNodeId("RuleCandidate:Old".into()))
            .unwrap();
        // Deterministik: ID ascending (New1 < New2).
        assert_eq!(
            sources,
            vec![
                ConceptNodeId("RuleCandidate:New1".into()),
                ConceptNodeId("RuleCandidate:New2".into())
            ]
        );
    }

    /// would_create_supersede_cycle: existing B→A, proposed A→B → true.
    #[test]
    fn would_create_supersede_cycle_true() {
        let mut store = snap_store_with_candidates(&[
            "RuleCandidate:A",
            "RuleCandidate:B",
            "RuleCandidate:C",
        ]);
        snap_accept(&mut store, "RuleCandidate:A");
        snap_accept(&mut store, "RuleCandidate:B");
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:B"); // committed B→A
        // proposed: A→B (superseded=B, successor=A) → B→A mevcut → cycle
        assert!(store
            .would_create_supersede_cycle(
                &ConceptNodeId("RuleCandidate:B".into()), // superseded
                &ConceptNodeId("RuleCandidate:A".into()), // successor
            )
            .unwrap());
    }

    /// would_create_supersede_cycle: unrelated endpoint → false.
    #[test]
    fn would_create_supersede_cycle_false() {
        let mut store =
            snap_store_with_candidates(&["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"]);
        snap_accept(&mut store, "RuleCandidate:A");
        snap_accept(&mut store, "RuleCandidate:B");
        snap_accept(&mut store, "RuleCandidate:C");
        snap_supersede(&mut store, "RuleCandidate:A", "RuleCandidate:B"); // committed B→A
        // proposed: A→C (C unrelated) → no cycle
        assert!(!store
            .would_create_supersede_cycle(
                &ConceptNodeId("RuleCandidate:C".into()), // superseded
                &ConceptNodeId("RuleCandidate:A".into()), // successor
            )
            .unwrap());
    }

    /// would_create_supersede_cycle: missing node → NodeNotFound.
    #[test]
    fn would_create_supersede_cycle_missing_node() {
        let store = InMemoryAnchorStore::new();
        let err = store
            .would_create_supersede_cycle(
                &ConceptNodeId("RuleCandidate:A".into()),
                &ConceptNodeId("RuleCandidate:B".into()),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::NodeNotFound(_)));
    }

    /// Accessor doğruluğu — already-superseded multi-blocker vaka.
    ///
    /// Bu vaka preview'ın AlreadySuperseded + SupersededNotCurrent multi-blocker'ını doğrular
    /// (accessor'lar doğru değer döner). **Önemli nüans (Review 2):** `apply_supersede`'in
    /// inline step 5<6 precedence'ı production path'te already-superseded için erişilemez —
    /// `PresentedSupersedeBasis::compile` currentness'ı step 5'ten önce kontrol eder ve
    /// `SupersededNotCurrent` döner. Yani store'un "step 5 < step 6" inline precedence'ı bu
    /// vakada ölü; preview `AlreadySuperseded`'i primary seçer (apply inline sırasına göre)
    /// ama production mutation `SupersededNotCurrent`/compile hatası döner. Bu, structurally_eligible=false
    /// her iki sırada da doğru olduğu için fonksiyonel olarak zararsız; sadece *primary blocker*
    /// etiketi farklılaşabilir. Bu test inline precedence'i kilitlemez — accessor doğruluğunu
    /// sabitler. Preview↔production primary-sebep hizalaması future work (production-path
    /// reddetme sırasına karşı characterization).
    #[test]
    fn accessor_facts_observe_already_superseded_and_non_current() {
        let mut store = snap_store_with_candidates(&[
            "RuleCandidate:Old",
            "RuleCandidate:New",
            "RuleCandidate:Newer",
        ]);
        snap_accept(&mut store, "RuleCandidate:Old");
        snap_accept(&mut store, "RuleCandidate:New");
        snap_accept(&mut store, "RuleCandidate:Newer");
        snap_supersede(&mut store, "RuleCandidate:Old", "RuleCandidate:New"); // committed New→Old
        let sup = ConceptNodeId("RuleCandidate:Old".into());
        let suc = ConceptNodeId("RuleCandidate:Newer".into());
        // Production path: compile currentness precheck (SupersededNotCurrent) — step 5'e ulaşmaz.
        use crate::anchoring::review::PresentedSupersedeBasis;
        assert!(
            PresentedSupersedeBasis::compile(&store, &sup, &suc).is_err(),
            "Old non-current → basis compile rejects (currentness precheck, not step 5)"
        );
        // Accessor doğruluğu (preview multi-blocker kaynağı):
        let incoming = store.committed_supersede_incoming_sources(&sup).unwrap();
        assert_eq!(incoming, vec![ConceptNodeId("RuleCandidate:New".into())]);
        let old_node = store.graph().node(&sup).unwrap();
        assert!(!old_node.decision_status.is_current_mainline());
        let compat = store.inspect_supersede_compatibility(&sup, &suc).unwrap();
        assert!(compat.is_compatible(), "same kind+family → compatible");
    }

    /// apply_supersede currentness accessor characterization: is_current_mainline() predicate
    /// apply_supersede step 6-7 ile aynı source. Accepted → true; SupersededAccepted → false.
    #[test]
    fn is_current_mainline_matches_apply_supersede_currentness_gate() {
        // Accepted → current mainline (supersedeable).
        let accepted = preview_node(
            "RuleCandidate:Ok",
            DecisionStatus::Accepted,
            ConceptNodeKind::RuleCandidate,
            PositionFamily::ConceptualIntent,
        );
        assert!(accepted.decision_status.is_current_mainline());
        // SupersededAccepted → not current (apply step 6 NotSupersedeableFrom döner).
        let superseded = preview_node(
            "RuleCandidate:Old",
            DecisionStatus::SupersededAccepted,
            ConceptNodeKind::RuleCandidate,
            PositionFamily::ConceptualIntent,
        );
        assert!(!superseded.decision_status.is_current_mainline());
        // Rejected / Candidate → not current.
        assert!(!DecisionStatus::Rejected.is_current_mainline());
        assert!(!DecisionStatus::Candidate.is_current_mainline());
    }
}
