//! Operator review session — Faz 8a (INV-C12, INV-C13).
//!
//! # Ana tez
//! *The protocol's missing organ is not a UI; it is a promotion path.* Bugün
//! `OperatorAcceptance` `pub(crate)` — kendi crate'ler bile üretemiyor. Bu modül,
//! token'ı `pub(crate)` tutarak dışarıya açılan tek şeyin **denetimli bir oturum**
//! olmasını sağlar. Operator review, artık seeded simülasyon değil, gerçek bir
//! protokol aktı.
//!
//! # Token dışarı çıkmaz, session çıkar
//! `OperatorReviewSession` public; `OperatorAcceptance` `pub(crate)` kalır. Session
//! her `accept`/`reject`'te içeride bir token üretir, harcar. Dış kod token'ı
//! tutamaz; tutabildiği şey, gerekçeli karar başına bir promotion üreten oturumdur.
//!
//! Bu, `OperatorCapability::issue_for_operator_session` (PR35) ile açılan
//! *trusted-boundary naming hardening* sınıfının ikinci üyesidir. Tam type-level
//! unforgeability sağlamaz (operator olduğunu runtime doğrular); audit invariantları
//! (INV-C12/C13) telafi eder.
//!
//! # İki yeni invariant
//! - **INV-C12 — Informed acceptance:** Karar kaydı karar anındaki temeli taşır;
//!   temel adayın karar anındaki içeriğine karşı tazelik-doğrulamalıdır (`node_digest`).
//!   Bayat temele onay → `StaleBasis` (TOCTOU açığı kapatır).
//! - **INV-C13 — No decision without record:** Accepted/Rejected durum geçişi
//!   karşılık gelen `DecisionRecord` ile atomik olarak paired olmalıdır
//!   (`AnchorStore::apply_decision` içinde).
//!
//! # INV-C11 (deployment disiplini)
//! `OperatorReviewSession::open_for_operator` agent-facing hiçbir yüzeyde var
//! olmamalıdır (osp-mcp tool listesi, agent API). Tip sınırı süreç sınırını
//! aşamadığı için bu bir deployment disiplinidir; araç listesi seviyesinde
//! denetlenir.
//!
//! # SystemTime notu
//! `DecisionRecord` ve `PresentedBasis` `SystemTime::now()` kullanır. Ledger v1'de
//! InMemory + freeze edilmediği için sorun yok. İleride ledger snapshot testlerine
//! girerse, deterministic `Clock` trait (testte deterministic, production'da system)
//! gerekir (Phase 8b+).

use crate::anchoring::store::AnchorStore;
use crate::anchoring::types::{ConceptNode, ConceptNodeId, NonEmptyExplanation};
use crate::anchoring::DecisionStatus;

use std::time::SystemTime;

// ═══════════════════════════════════════════════════════════════════════════════
// Yardımcı tipler — SessionId, OperatorId, DecisionKind
// ═══════════════════════════════════════════════════════════════════════════════

/// Review oturumunun deterministik kimliği. v1'de mütevazı (FNV-1a(operator + opened_at)).
/// Enterprise kimlik sonranın işi; INV-C11 deployment disiplini.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SessionId(String);

impl SessionId {
    fn derive(operator: &OperatorId, opened_at: SystemTime) -> Self {
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in operator.0.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        if let Ok(dur) = opened_at.duration_since(std::time::UNIX_EPOCH) {
            for b in dur.as_nanos().to_le_bytes() {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        Self(format!("session:{hash:016x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Operator kimliği. v1'de mütevazı — OS user + timestamp kadar. Tip sistemi
/// çağıranın insan olduğunu doğrulayamaz; sadece *şu operator* diyebilir.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct OperatorId(pub String);

impl OperatorId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Karar türü — Accept veya Reject. Reject de evidence'tır (Paper 2 ilkesinin
/// genesis karşılığı): reddedilen candidate, gerekçesiyle negatif epistemik veri.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DecisionKind {
    Accept,
    Reject,
}

// ═══════════════════════════════════════════════════════════════════════════════
// NodeDigest — INV-C12 tazelik fingerprint'i
// ═══════════════════════════════════════════════════════════════════════════════

/// `ConceptNode` içeriğinin **deterministik freshness fingerprint**'i.
///
/// **NOT:** Bu bir audit-security hash DEĞİLDİR — tazelik kontrolü için, çarpışma
/// direnci ikincil. Audit-security için `basis_fingerprint` zaten `DecisionRecord`'da var.
///
/// `decision_status` HARİÇ tutulur (promotion sonrası her zaman stale görünürdü).
/// Hesaplama: FNV-1a(canonical + sorted(aliases) + node_kind + position_family).
/// `aliases` sıralanır — girilen sıra deterministic olmayabilir.
pub fn node_digest(node: &ConceptNode) -> NodeDigest {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    feed(node.canonical.as_bytes());
    let mut aliases = node.aliases.clone();
    aliases.sort();
    for a in &aliases {
        feed(a.as_bytes());
        feed(&[0]); // ayraç
    }
    feed(format!("{:?}", node.node_kind).as_bytes());
    feed(format!("{:?}", node.position_family).as_bytes());
    if hash == 0 {
        hash = 1;
    }
    NodeDigest(hash)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub struct NodeDigest(u64);

impl NodeDigest {
    pub fn get(self) -> u64 {
        self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PresentedBasis — yalnız store'dan derlenebilir (INV-C12)
// ═══════════════════════════════════════════════════════════════════════════════

/// Karar anında operator'e sunulan temel. **Yalnız store'dan derlenebilir** —
/// çağıran uyduramaz. INV-C12: karar kaydı bunu taşır.
///
/// Serialize-only (audit); Deserialize YOK — yeniden apply edilememeli
/// (PR30/Faz4 serde boundary pattern).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PresentedBasis {
    candidate_id: ConceptNodeId,
    node_digest: NodeDigest,
    canonical: String,
    explanation: Option<String>,
    evidence_summary: EvidenceSummary,
    high_stake_flags: Vec<String>,
    compiled_at: SystemTime,
}

/// Evidence özeti — v1'de minimal placeholder (Faz 4 evidence tam entegre когда).
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvidenceSummary {
    pub has_evidence: bool,
    pub note: String,
}

impl PresentedBasis {
    /// Tek üretim yolu — store'daki gerçek durumdan derlenir. Generic `<S: AnchorStore>`
    /// (associated type sorunu yok — trait object kullanılmaz).
    pub fn compile<S: AnchorStore + ?Sized>(
        store: &S,
        id: &ConceptNodeId,
    ) -> Result<Self, ReviewError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        // Store'dan adayın node'unu bul (candidate_query içinde ara).
        let candidates = store
            .candidate_query()
            .map_err(|e| ReviewError::Store(Box::new(e)))?;
        let node = candidates
            .iter()
            .find(|n| &n.id == id)
            .ok_or_else(|| ReviewError::NotFound(id.clone()))?;
        Ok(Self {
            candidate_id: node.id.clone(),
            node_digest: node_digest(node),
            canonical: node.canonical.clone(),
            explanation: None, // v1: high-stake edge explanation hook (ileri sürüm)
            evidence_summary: EvidenceSummary {
                has_evidence: false,
                note: "v1: evidence hook placeholder (Faz 4 tam entegrasyon ileri sürüm)".into(),
            },
            high_stake_flags: Vec::new(), // v1: hook
            compiled_at: SystemTime::now(),
        })
    }

    pub fn candidate_id(&self) -> &ConceptNodeId {
        &self.candidate_id
    }
    pub fn node_digest(&self) -> NodeDigest {
        self.node_digest
    }
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DecisionApplication — opaque trait parametresi (AcceptedTaskCandidateRef deseni)
// ═══════════════════════════════════════════════════════════════════════════════

/// Public tip, **private fields + constructor YOK + Deserialize YOK**.
/// Tek üretici: `OperatorReviewSession` (in-crate). Store implementer'lar
/// read-only accessor'lar ile okur, uygulayabilir, **üretemez**.
///
/// Token (`OperatorAcceptance`) içeride kalır; trait dışarıda kalır;
/// kapı yalnız session'dan geçer.
#[derive(Debug, Clone)]
pub struct DecisionApplication {
    candidate_id: ConceptNodeId,
    decision: DecisionKind,
    basis: PresentedBasis,
    reason: NonEmptyExplanation,
    session_id: SessionId,
    operator: OperatorId,
    decided_at: SystemTime,
}

impl DecisionApplication {
    /// In-crate constructor — sadece `OperatorReviewSession` çağırır.
    pub(crate) fn new(
        candidate_id: ConceptNodeId,
        decision: DecisionKind,
        basis: PresentedBasis,
        reason: NonEmptyExplanation,
        session_id: SessionId,
        operator: OperatorId,
        decided_at: SystemTime,
    ) -> Self {
        Self {
            candidate_id,
            decision,
            basis,
            reason,
            session_id,
            operator,
            decided_at,
        }
    }

    // Read-only accessors — store implementer'lar için.
    pub fn candidate_id(&self) -> &ConceptNodeId {
        &self.candidate_id
    }
    pub fn decision(&self) -> DecisionKind {
        self.decision
    }
    pub fn basis(&self) -> &PresentedBasis {
        &self.basis
    }
    pub fn reason(&self) -> &NonEmptyExplanation {
        &self.reason
    }
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
    pub fn operator(&self) -> &OperatorId {
        &self.operator
    }
    pub fn decided_at(&self) -> SystemTime {
        self.decided_at
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DecisionRecord — ledger kaydı (Deserialize serbest — tarih okur, yetki vermez)
// ═══════════════════════════════════════════════════════════════════════════════

/// Append-only ledger kaydı. **Deserialize serbest** — diskten okumak yetki vermez,
/// tarih okur. `PresentedBasis` Serialize-only kalırken bunun serbest olması
/// kasıtlı: yasağın ilkesi "yeniden inşa = yetki sahteciliği"; bir karar kaydını
/// diskten okumak yetki vermez.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    pub seq: u64,
    pub session_id: SessionId,
    pub operator: OperatorId,
    pub candidate_id: ConceptNodeId,
    pub node_digest_serde: u64, // NodeDigest private field ama Serialize; serde için get()
    pub decision: DecisionKind,
    pub reason: NonEmptyExplanation,
    pub basis_fingerprint: [u8; 32],
    pub prior_status: DecisionStatus,
    pub new_status: DecisionStatus,
    pub at: SystemTime,
}

impl DecisionRecord {
    pub fn node_digest(&self) -> NodeDigest {
        NodeDigest(self.node_digest_serde)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ReviewError
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("candidate not found: {0}")]
    NotFound(ConceptNodeId),
    #[error("node not promotable from {current:?}")]
    NotPromotable { current: DecisionStatus },
    #[error("stale basis: expected {expected:?}, found {found:?} (TOCTOU — node changed after basis compile)")]
    StaleBasis {
        expected: NodeDigest,
        found: NodeDigest,
    },
    #[error("basis candidate mismatch: basis={basis}, id={id}")]
    BasisCandidateMismatch {
        basis: ConceptNodeId,
        id: ConceptNodeId,
    },
    #[error("ledger append failed")]
    RecordAppendFailed,
    #[error("store error")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

// ═══════════════════════════════════════════════════════════════════════════════
// SessionSummary — close() dönüşü (v1: ledger'a yazmaz)
// ═══════════════════════════════════════════════════════════════════════════════

/// Oturum özeti. `close(self)` consume eder; v1'de ledger'a close-event yazmaz
/// (audit'in asıl gücü `DecisionRecord` ledger; close-event opsiyonel future).
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub operator: OperatorId,
    pub decisions: u32,
    pub accepts: u32,
    pub rejects: u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
// OperatorReviewSession — public, içeride token harcar
// ═══════════════════════════════════════════════════════════════════════════════

/// Operator review oturumu. `OperatorAcceptance`'ı içeride harcar; dışarı çıkmaz.
///
/// **Yapısal garanti:** Clone YOK, Serialize/Deserialize YOK, literal construct
/// engelli (private fields + smart constructor).
///
/// # Trusted-boundary constructor
/// `open_for_operator` `OperatorCapability::issue_for_operator_session` (PR35) ile
/// **aynı güven sınıfı** — trusted-boundary naming hardening. Tam type-level
/// unforgeability sağlamaz; operator olduğunu runtime doğrular. Audit invariantları
/// (INV-C12/C13) telafi eder.
///
/// # INV-C11 (deployment disiplini)
/// Bu tipin agent-facing hiçbir yüzeyde var olmaması gerekir. Tip sınırı süreç
/// sınırını aşamadığı için deployment disiplini; araç listesi seviyesinde denetlenir.
pub struct OperatorReviewSession {
    session_id: SessionId,
    operator: OperatorId,
    opened_at: SystemTime,
    decisions: u32,
    accepts: u32,
    rejects: u32,
}

impl OperatorReviewSession {
    /// Trusted-boundary constructor. Çağıran kod operator authority boundary'sidir
    /// (CLI operator mode, operator console) ve runtime'da operator olduğunu
    /// doğrulamış olmalı. **INV-C11:** agent-facing yüzeylerde çağrılmamalı.
    pub fn open_for_operator(operator: OperatorId) -> Self {
        let opened_at = SystemTime::now();
        Self {
            session_id: SessionId::derive(&operator, opened_at),
            operator,
            opened_at,
            decisions: 0,
            accepts: 0,
            rejects: 0,
        }
    }

    /// Candidate → Accepted. INV-C12: basis karar anındaki içeriğe karşı
    /// tazelik-doğrulamalı. INV-C13: promotion + ledger append atomik
    /// (`store.apply_decision` içinde).
    ///
    /// **v1 scope:** Yalnız `DecisionStatus::Candidate` node'lar review edilebilir
    /// (`PresentedBasis::compile` `candidate_query` üzerinden bulur; o da sadece
    /// Candidate döndürür).
    pub fn accept<S: AnchorStore + ?Sized>(
        &mut self,
        store: &mut S,
        id: &ConceptNodeId,
        basis: PresentedBasis,
        reason: NonEmptyExplanation,
    ) -> Result<DecisionRecord, ReviewError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let record = self.decide(store, id, basis, reason, DecisionKind::Accept)?;
        self.accepts += 1;
        Ok(record)
    }

    /// Candidate → Rejected. Reject de reason + basis ister —
    /// reddedilen candidate, gerekçesiyle negatif epistemik veri olur.
    /// (Accept ile aynı Candidate-only v1 scope.)
    pub fn reject<S: AnchorStore + ?Sized>(
        &mut self,
        store: &mut S,
        id: &ConceptNodeId,
        basis: PresentedBasis,
        reason: NonEmptyExplanation,
    ) -> Result<DecisionRecord, ReviewError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let record = self.decide(store, id, basis, reason, DecisionKind::Reject)?;
        self.rejects += 1;
        Ok(record)
    }

    /// Ortak karar akışı — INV-C12 tazelik kontrolü + DecisionApplication üretimi
    /// + store'a delege (INV-C13 atomiklik).
    fn decide<S: AnchorStore + ?Sized>(
        &mut self,
        store: &mut S,
        id: &ConceptNodeId,
        basis: PresentedBasis,
        reason: NonEmptyExplanation,
        decision: DecisionKind,
    ) -> Result<DecisionRecord, ReviewError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        // (1) basis candidate mismatch kontrolü
        if basis.candidate_id() != id {
            return Err(ReviewError::BasisCandidateMismatch {
                basis: basis.candidate_id().clone(),
                id: id.clone(),
            });
        }

        // (2) INV-C12 — tazelik: basis'teki digest ile karar anındaki digest eşleşmeli
        let current_digest = {
            let candidates = store
                .candidate_query()
                .map_err(|e| ReviewError::Store(Box::new(e)))?;
            let node = candidates
                .iter()
                .find(|n| &n.id == id)
                .ok_or_else(|| ReviewError::NotFound(id.clone()))?;
            node_digest(node)
        };
        if basis.node_digest() != current_digest {
            return Err(ReviewError::StaleBasis {
                expected: basis.node_digest(),
                found: current_digest,
            });
        }

        // (3) DecisionApplication üret (in-crate ctor) — token içeride.
        let application = DecisionApplication::new(
            id.clone(),
            decision,
            basis,
            reason,
            self.session_id.clone(),
            self.operator.clone(),
            SystemTime::now(),
        );

        // (4) store.apply_decision — INV-C13 atomik (promotion + ledger append).
        let record = store
            .apply_decision(application)
            .map_err(|e| ReviewError::Store(Box::new(e)))?;

        self.decisions += 1;
        Ok(record)
    }

    /// Session'ı consume eder. v1'de sadece in-memory `SessionSummary` döner;
    /// ledger'a close-event yazmaz (opsiyonel future).
    pub fn close(self) -> SessionSummary {
        SessionSummary {
            session_id: self.session_id,
            operator: self.operator,
            decisions: self.decisions,
            accepts: self.accepts,
            rejects: self.rejects,
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
    pub fn operator(&self) -> &OperatorId {
        &self.operator
    }
    pub fn opened_at(&self) -> SystemTime {
        self.opened_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::store::{AnchorStore, InMemoryAnchorStore};
    use crate::anchoring::types::{ConceptNodeKind, GraphSeed};
    use crate::anchoring::{DecisionStatus, PositionFamily};

    /// Test yardımcı: Candidate bir RuleCandidate node'u seed'le.
    fn store_with_candidate(id: &str) -> InMemoryAnchorStore {
        let node = ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(node);
        InMemoryAnchorStore::with_seed(seed)
    }

    #[test]
    fn review_session_accept_promotes_candidate_to_accepted() {
        // Faz 8a mutlu yol: open → compile basis → accept → Accepted + ledger record.
        let mut store = store_with_candidate("RuleCandidate:CouplingMustNot");
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let id = ConceptNodeId("RuleCandidate:CouplingMustNot".into());

        let basis = PresentedBasis::compile(&store, &id).expect("basis compile");
        let reason = NonEmptyExplanation::new("rule accepted by operator").unwrap();
        let record = session.accept(&mut store, &id, basis, reason).expect("accept");

        assert_eq!(record.decision, DecisionKind::Accept);
        assert_eq!(record.prior_status, DecisionStatus::Candidate);
        assert_eq!(record.new_status, DecisionStatus::Accepted);
        // INV-C13: ledger'a atomik append edildi.
        let ledger = store.decision_ledger();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].seq, 1);
        // mainline_query artık Accepted node'u görmeli.
        let mainline = store.mainline_query().unwrap();
        assert_eq!(mainline.len(), 1);
    }

    #[test]
    fn review_session_reject_marks_candidate_rejected() {
        // Reject de reason + basis ister — reddedilen candidate negatif epistemik veri.
        let mut store = store_with_candidate("RuleCandidate:Bad");
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let id = ConceptNodeId("RuleCandidate:Bad".into());

        let basis = PresentedBasis::compile(&store, &id).expect("basis");
        let reason = NonEmptyExplanation::new("rule rejected — out of scope").unwrap();
        let record = session.reject(&mut store, &id, basis, reason).expect("reject");

        assert_eq!(record.decision, DecisionKind::Reject);
        assert_eq!(record.new_status, DecisionStatus::Rejected);
        // Rejected node mainline'da DEĞİL (INV-C3).
        assert_eq!(store.mainline_query().unwrap().len(), 0);
        // Ama ledger'da var (reject'ler evidence'tır).
        assert_eq!(store.decision_ledger().len(), 1);
    }

    #[test]
    fn review_session_stale_basis_rejects_touctou() {
        // INV-C12: operator basis derledi, araya node değişti, accept → StaleBasis.
        let mut store = store_with_candidate("RuleCandidate:Changing");
        let id = ConceptNodeId("RuleCandidate:Changing".into());

        // Basis'i derle (şu anki durumdan).
        let basis = PresentedBasis::compile(&store, &id).expect("basis");

        // Araya girip node'un canonical'ını değiştir (TOCTOU).
        {
            let node = store.graph_mut().node_mut(&id).expect("node");
            node.canonical = "ChangedAfterBasis".into();
        }

        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("op"));
        let reason = NonEmptyExplanation::new("reason").unwrap();
        let err = session
            .accept(&mut store, &id, basis, reason)
            .expect_err("stale basis should reject");
        assert!(
            matches!(err, ReviewError::StaleBasis { .. }),
            "StaleBasis bekleniyordu, got {err:?}"
        );
    }

    #[test]
    fn review_session_not_promotable_rejects_accepted_node() {
        // Accepted node'u tekrar accept → NotPromotable (diriltme ayrı mekanizma).
        let mut store = store_with_candidate("RuleCandidate:Done");
        let id = ConceptNodeId("RuleCandidate:Done".into());
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("op"));

        // İlk accept.
        let basis1 = PresentedBasis::compile(&store, &id).unwrap();
        let reason1 = NonEmptyExplanation::new("first accept").unwrap();
        session.accept(&mut store, &id, basis1, reason1).unwrap();

        // İkinci accept → NotPromotable (store tarafında StoreError'a map'lenir).
        let basis2 = PresentedBasis::compile(&store, &id); // artık candidate_query'de yok
        assert!(basis2.is_err(), "Accepted node candidate_query'de değil");
    }

    #[test]
    fn review_session_basis_candidate_mismatch_rejects() {
        // basis candidate_id ≠ accept id → BasisCandidateMismatch.
        let mut store = store_with_candidate("RuleCandidate:A");
        let id_a = ConceptNodeId("RuleCandidate:A".into());
        let id_b = ConceptNodeId("RuleCandidate:B".into());

        let basis = PresentedBasis::compile(&store, &id_a).unwrap();
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("op"));
        let reason = NonEmptyExplanation::new("reason").unwrap();
        let err = session
            .accept(&mut store, &id_b, basis, reason)
            .expect_err("mismatch should reject");
        assert!(
            matches!(err, ReviewError::BasisCandidateMismatch { .. }),
            "BasisCandidateMismatch bekleniyordu"
        );
    }

    #[test]
    fn review_session_close_returns_summary() {
        // close(self) consume eder, summary döner. v1'de ledger'a yazmaz.
        let mut store = store_with_candidate("RuleCandidate:X");
        let id = ConceptNodeId("RuleCandidate:X".into());
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("op"));

        let basis = PresentedBasis::compile(&store, &id).unwrap();
        let reason = NonEmptyExplanation::new("ok").unwrap();
        session.accept(&mut store, &id, basis, reason).unwrap();

        let summary = session.close();
        assert_eq!(summary.decisions, 1);
        assert_eq!(summary.accepts, 1);
        assert_eq!(summary.rejects, 0);
    }

    #[test]
    fn review_session_not_found_rejects_unknown_candidate() {
        let store = store_with_candidate("RuleCandidate:A");
        let unknown = ConceptNodeId("RuleCandidate:Yok".into());
        let err = PresentedBasis::compile(&store, &unknown).expect_err("not found");
        assert!(matches!(err, ReviewError::NotFound(_)));
    }

    #[test]
    fn node_digest_excludes_decision_status() {
        // decision_status değişse bile digest aynı kalmalı (promotion sonrası stale değil).
        let mut node = ConceptNode {
            id: ConceptNodeId("RuleCandidate:X".into()),
            canonical: "X".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let d1 = node_digest(&node);
        node.decision_status = DecisionStatus::Accepted;
        let d2 = node_digest(&node);
        assert_eq!(d1, d2, "decision_status digest'e dahil DEĞİL");
    }

    #[test]
    fn node_digest_sorted_aliases_deterministic() {
        // aliases sırası farklı olsa bile digest aynı.
        let mk = |aliases: Vec<&str>| ConceptNode {
            id: ConceptNodeId("RuleCandidate:X".into()),
            canonical: "X".into(),
            aliases: aliases.into_iter().map(String::from).collect(),
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let d1 = node_digest(&mk(vec!["a", "b", "c"]));
        let d2 = node_digest(&mk(vec!["c", "a", "b"]));
        assert_eq!(d1, d2, "aliases sıralanır — deterministic");
    }

    #[test]
    fn ledger_is_append_only_seq_monotonic() {
        // INV-C13: seq monotonik artar, ledger append-only.
        let mk_node = |id: &str| ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(mk_node("RuleCandidate:A"));
        seed.rule_candidates.push(mk_node("RuleCandidate:B"));
        let mut store = InMemoryAnchorStore::with_seed(seed);

        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("op"));
        let id_a = ConceptNodeId("RuleCandidate:A".into());
        let id_b = ConceptNodeId("RuleCandidate:B".into());

        let ba = PresentedBasis::compile(&store, &id_a).unwrap();
        session
            .accept(&mut store, &id_a, ba, NonEmptyExplanation::new("a").unwrap())
            .unwrap();
        let bb = PresentedBasis::compile(&store, &id_b).unwrap();
        session
            .reject(&mut store, &id_b, bb, NonEmptyExplanation::new("b").unwrap())
            .unwrap();

        let ledger = store.decision_ledger();
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger[0].seq, 1);
        assert_eq!(ledger[1].seq, 2);
        assert!(ledger[1].seq > ledger[0].seq);
    }

    /// Path 6 gerçek kanıtı: apply_decision doğrudan çağrılıp Accepted node'a
    /// uygulandığında StoreError::NotPromotableFrom döner. Bu, session yolundan
    /// (NotFound öncesi) erişilemeyen dalın doğrudan egzersizidir — defense-in-depth.
    /// Aynı zamanda "basis digest status'u dışlar → kabul sonrası da taze" kanıtı.
    #[test]
    fn apply_decision_rejects_accepted_node_not_promotable_from() {
        use crate::anchoring::store::{AnchorStore, StoreError};
        // Candidate node seed'le, basis derive et, sonra session ile promote et.
        let node = ConceptNode {
            id: ConceptNodeId("RuleCandidate:WillBeAccepted".into()),
            canonical: "WillBeAccepted".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(node);
        let mut store = InMemoryAnchorStore::with_seed(seed);
        let id = ConceptNodeId("RuleCandidate:WillBeAccepted".into());

        // Basis derive et (Candidate durumdan).
        let basis = PresentedBasis::compile(&store, &id).unwrap();

        // Session ile gerçek promotion → Accepted.
        let mut session =
            OperatorReviewSession::open_for_operator(OperatorId::new("test"));
        session
            .accept(&mut store, &id, basis.clone(), NonEmptyExplanation::new("first").unwrap())
            .unwrap();

        // Artık node Accepted. apply_decision doğrudan çağrılırsa (in-crate ctor)
        // basis.candidate_id == application.candidate_id (eşleşir — defense-in-depth id check),
        // sonra NotPromotableFrom(Accepted). basis digest status dışladığı için hala taze.
        let app = DecisionApplication::new(
            id.clone(),
            DecisionKind::Accept,
            basis,
            NonEmptyExplanation::new("should reject — already accepted").unwrap(),
            SessionId("session:test".into()),
            OperatorId::new("test"),
            SystemTime::now(),
        );
        let err = AnchorStore::apply_decision(&mut store, app).unwrap_err();
        assert!(
            matches!(err, StoreError::NotPromotableFrom(DecisionStatus::Accepted)),
            "NotPromotableFrom(Accepted) bekleniyordu, got {err:?}"
        );
    }

    /// Defense-in-depth id-mismatch: apply_decision basis.candidate_id ≠ application.candidate_id
    /// reddeder. Session bu kontrolü yapar ama apply_decision da yapar (Review 1 gözlemi).
    /// Kontrol sırası: id-mismatch ÖNCE, NotPromotableFrom sonra.
    #[test]
    fn apply_decision_rejects_basis_application_id_mismatch() {
        use crate::anchoring::store::{AnchorStore, StoreError};
        // İki Candidate node — basis birinden, application diğerinden.
        let mk = |id: &str| ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(mk("RuleCandidate:A"));
        seed.rule_candidates.push(mk("RuleCandidate:B"));
        let store = InMemoryAnchorStore::with_seed(seed);

        // A için basis derive et.
        let basis =
            PresentedBasis::compile(&store, &ConceptNodeId("RuleCandidate:A".into())).unwrap();

        // Application'ı B id'siyle kur (in-crate ctor) — mismatch.
        let mut store = store;
        let app = DecisionApplication::new(
            ConceptNodeId("RuleCandidate:B".into()), // application id ≠ basis id (A)
            DecisionKind::Accept,
            basis,
            NonEmptyExplanation::new("mismatch").unwrap(),
            SessionId("session:test".into()),
            OperatorId::new("test"),
            SystemTime::now(),
        );
        let err = AnchorStore::apply_decision(&mut store, app).unwrap_err();
        assert!(
            matches!(err, StoreError::BasisCandidateMismatch { .. }),
            "BasisCandidateMismatch bekleniyordu (id-mismatch önce), got {err:?}"
        );
    }
}
