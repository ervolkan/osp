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
    /// opaque (private fields, pub(crate) ctor, no Deserialize) — tek üretici PR #50
    /// `SupersedeSession`. Store: seq/prior_status/new_status/edge record üretiminden sorumludur.
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
    fn mainline_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;

    /// INV-C14 (Faz 8b): Acceptance-provenance projection — kabul provenance'ını
    /// koruyan node'lar (Accepted + SupersededAccepted).
    ///
    /// **Bu chronological replay DEĞİLDİR.** Mevcut snapshot'ta kabul provenance'ını
    /// koruyan node'ları döndürür; "t anında mainline neydi" veya kabul sırasını vermez.
    /// Temporal replay decision/event ledger ister.
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
    pub fn restore_trusted_snapshot(
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
                    && &e.from == &current
                {
                    stack.push(e.to.clone());
                }
            }
        }
        false
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

        // (5) INV-C15 committed incoming edge — Accepted only (Candidate proposal'lar sayılmaz).
        let already_superseded = self.graph.edges().any(|e| {
            e.kind == ConceptEdgeKind::Supersedes
                && e.decision_status == DecisionStatus::Accepted
                && &e.to == &superseded_id
        });
        if already_superseded {
            return Err(StoreError::AlreadySuperseded(superseded_id.clone()));
        }

        // (6) Superseded status — Accepted olmalı.
        if sup_status != DecisionStatus::Accepted {
            return Err(StoreError::NotSupersedeableFrom(sup_status));
        }
        // (7) Successor status — Accepted olmalı (creation-time).
        if suc_status != DecisionStatus::Accepted {
            return Err(StoreError::SuccessorNotAccepted(suc_status));
        }
        // (8) Self-supersede.
        if superseded_id == successor_id {
            return Err(StoreError::SelfSupersede(superseded_id.clone()));
        }
        // (9) Endpoint compatibility — coarse structural (same kind + family).
        // Semantic replacement judgment operator-reviewed basis'te; lineage/scope key future work.
        if sup_kind != suc_kind || sup_family != suc_family {
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
        // Candidate proposal edge silinmez — historical proposal provenance olarak kalır (PR #50).
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
        Ok(self
            .graph
            .nodes_iter()
            .filter(|n| n.decision_status.is_current_mainline())
            .cloned()
            .collect())
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
        Ok(self
            .graph
            .nodes_iter()
            .filter(|n| matches!(n.decision_status, DecisionStatus::Candidate))
            .cloned()
            .collect())
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
    fn restore_trusted_snapshot_roundtrip() {
        // Faz 3: ConceptGraphSnapshot restore (INV-C3 trusted boundary)
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
        let store = InMemoryAnchorStore::restore_trusted_snapshot(snapshot).unwrap();
        assert_eq!(store.node_count().unwrap(), 1);
        assert_eq!(
            store.mainline_query().unwrap().len(),
            1,
            "restored Accepted"
        );
    }

    #[test]
    fn restore_trusted_snapshot_rejects_version_mismatch() {
        // Faz 3 #2: schema_version mismatch → InvalidSnapshotVersion
        use crate::anchoring::types::ConceptGraphSnapshot;
        let snapshot = ConceptGraphSnapshot {
            nodes: vec![],
            edges: vec![],
            schema_version: 999, // mismatch
        };
        let err = InMemoryAnchorStore::restore_trusted_snapshot(snapshot).unwrap_err();
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

    /// `mainline_history` deterministik ID sıralaması — ters insert'ten bağımsız.
    /// NOT: bu sunum sırasıdır, kabul kronolojisi DEĞİL.
    #[test]
    fn mainline_history_is_deterministically_ordered() {
        let mk = node_with_status;
        // Ters sırayla insert (Z, A) — çıktı sıralı olmalı (A, Z).
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

    // Not: apply_decision'ın SupersededAccepted terminal-statü defense-in-depth testi
    // review.rs test bloğunda — orada SessionId constructor erişilebilir (aynı modül).
    // (apply_decision_rejects_superseded_accepted_not_promotable)
}
