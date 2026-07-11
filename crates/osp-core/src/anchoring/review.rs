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
//! - **INV-C12 — Fresh basis for reviewed transitions:** Karar kaydı karar anındaki
//!   temeli taşır; temel adayın karar anındaki içeriğine karşı tazelik-doğrulamalıdır
//!   (`node_digest`). Bayat temele onay → `StaleBasis` (TOCTOU açığı kapatır).
//!   `PresentedSupersedeBasis` supersession için iki endpoint'in digest'ini taşır.
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
///
/// # Freshness scope — ileriye dönük risk (Review 2.tur F1)
/// v1'de bu digest, `PresentedBasis`'in anlamlı içeriğinin tamamını kapsar (çünkü
/// explanation/evidence_summary/high_stake_flags placeholder). Ama §7.6/§11'de bu
/// alanlar komşu graph'tan/evidence'tan doldurulduğunda, digest yalnızca node'un kendi
/// alanlarını kapsadığı için basis'ten dar hale gelir → komşu değişince StaleBasis
/// tetiklenmez. O an geldiğinde: ya digest'i derlenmiş `PresentedBasis` üzerinden
/// hesapla (`basis_digest` ayrımı), ya da basis'e yeni alan eklenirse fail eden guard koy.
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

    /// Raw u64'ten digest kur — operator yüzeyinde (CLI `--basis-digest`) "gördüğünü
    /// onaylama" tazelik karşılaştırması için. FNV tabanlı non-cryptographic bir
    /// **karşılaştırma değeri**dir — authority/capability token DEĞİL, güvenlik önlemi
    /// de değildir; amaç, operator'ın gördüğü basis ile karar anındaki basis'in aynı
    /// olduğunu doğrulamak (INV-C12 informed-acceptance precondition).
    ///
    /// `pub(crate)` değil çünkü CLI operator yüzeyi (`osp review`) osp-core dışında;
    /// `PresentedBasis::compile` hala tek **üretim** yolu (bu yalnızca re-construction
    /// için karşılaştırma değeri üretir).
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 8b (PR #49): Supersede — PresentedSupersedeBasis, SupersedeApplication,
//                   SupersedeRecord, SupersedeError, supersede_basis_fingerprint
//
// `OperatorReviewSession`/`DecisionApplication`/`apply_decision` desenine paralel.
// Üretici yol: apply_supersede (store.rs). Authority issuance PR #50 SupersedeSession.
//
// Edge yönü (tasarım doc §8.3): successor --Supersedes--> superseded
//   (A supersedes B = A, B'nin yerine geçer; lineage C→B→A doğal yürüyüş).
//   Inverse human reading: superseded --SupersededBy--> successor.
// Cardinalite (INV-C15): incoming — every SupersededAccepted node has exactly one
//   *committed* (decision_status==Accepted) incoming Supersedes edge. Candidate
//   proposal edges (apply_plan yazar) cardinality/cycle'ye KATILMAZ. Consolidation
//   (C→A ve C→B) serbest — outgoing sınırı yok.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::anchoring::gate::{SupersedeAuthority, SupersedeAuthorityLevel};

/// Supersede karar basis'i — iki Accepted endpoint. `PresentedBasis`'ten farklı:
/// bu, Candidate-only `candidate_query`'den değil, current mainline (`mainline_query`)
/// Accepted node'larından derlenir. Her iki node'un `NodeDigest`'ini taşır (TOCTOU:
/// successor'un içeriği de karar anında taze olmalı — status kontrolü bunu yakalamaz).
///
/// Serialize-only (Deserialize YOK — `PresentedBasis` serde boundary deseni).
/// Yeniden apply edilememeli; tek *production* üretim yolu `compile`. In-module cfg(test)
/// literal (`app_with_basis_for_tests`) sahte basis senaryoları için kullanılır (NodeNotFound /
/// SelfSupersede gibi defense-in-depth dallarını exercise etmek).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PresentedSupersedeBasis {
    superseded_id: ConceptNodeId,
    successor_id: ConceptNodeId,
    superseded_digest: NodeDigest,
    successor_digest: NodeDigest,
    compiled_at: SystemTime,
}

impl PresentedSupersedeBasis {
    /// İki Accepted node'u current mainline'dan derler. Self-supersede ve non-current
    /// node'lar compile aşamasında reddedilir (gereksiz application üretimini engeller).
    pub fn compile<S: AnchorStore + ?Sized>(
        store: &S,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
    ) -> Result<Self, SupersedeError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        if superseded == successor {
            return Err(SupersedeError::SelfSupersede(superseded.clone()));
        }
        let current = store
            .mainline_query()
            .map_err(|e| SupersedeError::Store(Box::new(e)))?;
        let old = current
            .iter()
            .find(|n| &n.id == superseded)
            .ok_or_else(|| SupersedeError::SupersededNotCurrent(superseded.clone()))?;
        let new = current
            .iter()
            .find(|n| &n.id == successor)
            .ok_or_else(|| SupersedeError::SuccessorNotCurrent(successor.clone()))?;
        Ok(Self {
            superseded_id: old.id.clone(),
            successor_id: new.id.clone(),
            superseded_digest: node_digest(old),
            successor_digest: node_digest(new),
            compiled_at: SystemTime::now(),
        })
    }

    pub fn superseded_id(&self) -> &ConceptNodeId {
        &self.superseded_id
    }
    pub fn successor_id(&self) -> &ConceptNodeId {
        &self.successor_id
    }
    pub fn superseded_digest(&self) -> NodeDigest {
        self.superseded_digest
    }
    pub fn successor_digest(&self) -> NodeDigest {
        self.successor_digest
    }
}

/// Supersede basis compile + session hataları (`ReviewError` desenine paralel — tek error
/// evreni compile/session için; store hatları `StoreError`'da, session `Store(_)` ile sarmalar).
///
/// **Tazelik çift-katman (Tur 2 §3):** `StaleSupersededBasis`/`StaleSuccessorBasis` hem
/// session'da (typed-error ergonomisi — erken) hem de store'da (`StoreError::Stale*`,
/// defense-in-depth) yaşar. Generic `S::Error` üzerinden store'un Stale hatası
/// pattern-match edilemediği için session-seviyesi erken kontrol gerekir. Store aynı kontrolü
/// defense-in-depth olarak tekrarlar; production yolunda session'ınki önce ateşlenir.
/// *Digest semantiği değişirse iki yer değişmeli (constraint-propagation hata sınıfı).*
#[derive(Debug, thiserror::Error)]
pub enum SupersedeError {
    #[error("superseded node not currently accepted (not in mainline): {0}")]
    SupersededNotCurrent(ConceptNodeId),
    #[error("successor node not currently accepted (not in mainline): {0}")]
    SuccessorNotCurrent(ConceptNodeId),
    #[error("superseded and successor must differ: {0}")]
    SelfSupersede(ConceptNodeId),
    #[error("stale superseded basis: expected {expected:?}, found {found:?} (TOCTOU)")]
    StaleSupersededBasis {
        expected: NodeDigest,
        found: NodeDigest,
    },
    #[error("stale successor basis: expected {expected:?}, found {found:?} (TOCTOU)")]
    StaleSuccessorBasis {
        expected: NodeDigest,
        found: NodeDigest,
    },
    #[error("supersede basis mismatch: basis superseded={basis_superseded}, successor={basis_successor}; requested superseded={req_superseded}, successor={req_successor}")]
    SupersedeBasisMismatch {
        basis_superseded: ConceptNodeId,
        basis_successor: ConceptNodeId,
        req_superseded: ConceptNodeId,
        req_successor: ConceptNodeId,
    },
    #[error("supersede session counter exhausted")]
    SessionCounterExhausted,
    #[error("store error")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Opaque supersede application. `DecisionApplication` deseni: private fields +
/// `pub(crate)` ctor + no `Deserialize`. Tek production üretici: `SupersedeSession` (PR #50);
/// test üretici: `issue_operator_for_tests`.
///
/// **Authority semantics:** `SupersedeSession` authority'yi içeride mint eder
/// (`SupersedeAuthority::issue_for_supersede_session`, crate-private) ve application'a gömer;
/// token dışarı çıkmaz. PR #49 `SupersedeAuthority` `Copy` olduğu için by-value geçiş
/// capability'yi tüketmiyordu ("issuance-gated, not linearly consumed"); PR #50 ile
/// capability structurally confined hale gelir — `SupersedeSession` public entrypoint'tir.
#[derive(Debug, Clone)]
pub struct SupersedeApplication {
    superseded: ConceptNodeId,
    successor: ConceptNodeId,
    authority_level: SupersedeAuthorityLevel, // audit (level); capability tüketilmez
    basis: PresentedSupersedeBasis,
    reason: NonEmptyExplanation,
    session_id: SessionId,
    operator: OperatorId,
    decided_at: SystemTime,
}

impl SupersedeApplication {
    /// In-crate constructor. Authority'yi by-value alır (Copy → tüketilmez, level çıkarılır).
    /// Tek production caller: `SupersedeSession` (PR #50); token içeride mint edilir.
    pub(crate) fn new(
        superseded: ConceptNodeId,
        successor: ConceptNodeId,
        authority: SupersedeAuthority,
        basis: PresentedSupersedeBasis,
        reason: NonEmptyExplanation,
        session_id: SessionId,
        operator: OperatorId,
        decided_at: SystemTime,
    ) -> Self {
        Self {
            superseded,
            successor,
            authority_level: authority.level(),
            basis,
            reason,
            session_id,
            operator,
            decided_at,
        }
    }

    pub fn superseded(&self) -> &ConceptNodeId {
        &self.superseded
    }
    pub fn successor(&self) -> &ConceptNodeId {
        &self.successor
    }
    pub fn authority_level(&self) -> SupersedeAuthorityLevel {
        self.authority_level
    }
    pub fn basis(&self) -> &PresentedSupersedeBasis {
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

/// Supersede ledger kaydı. `DecisionRecord` deseni: Deserialize serbest
/// (diskten okumak yetki vermez, tarih okur). Global `audit_seq` (decision ile paylaşımlı)
/// → cross-ledger total order. Full graph replay ayrıca initial snapshot + event stream ister.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SupersedeRecord {
    pub seq: u64,
    pub session_id: SessionId,
    pub operator: OperatorId,
    pub superseded: ConceptNodeId,
    pub successor: ConceptNodeId,
    pub authority_level: SupersedeAuthorityLevel,
    pub reason: NonEmptyExplanation,
    /// u64 (NodeDigest Serialize-only → Deserialize yok; record için raw u64).
    pub superseded_digest_serde: u64,
    pub successor_digest_serde: u64,
    pub basis_fingerprint: [u8; 32],
    pub prior_status: DecisionStatus, // Accepted
    pub new_status: DecisionStatus,   // SupersededAccepted
    pub at: SystemTime,
}

/// Four independently-seeded 64-bit FNV-1a lanes → `[u8; 32]` (true 256-bit representation).
///
/// **Non-cryptographic** audit/freshness fingerprint. Length-prefixed framing prevents
/// field-boundary ambiguity (e.g. `"ab"+"c"` vs `"a"+"bc"`) — NOT hash collision prevention.
/// Domain tag (`osp:supersede-basis:v1`) `DecisionRecord`'un `basis_fingerprint`'inden ayırır.
/// `compiled_at` hariç: aynı epistemik basis farklı derleme zamanlarında aynı fingerprint.
pub(crate) fn supersede_basis_fingerprint(basis: &PresentedSupersedeBasis) -> [u8; 32] {
    // FNV-1a offset basis'leri — dört bağımsız tohum (farklı asal/irrational kaynaklar).
    const FNV_OFFSET_1: u64 = 0xcbf29ce484222325; // FNV canonical
    const FNV_OFFSET_2: u64 = 0x84222325cbf29ce4; // byte-swap
    const FNV_OFFSET_3: u64 = 0x100000001b3a1b3a; // prime rotate
    const FNV_OFFSET_4: u64 = 0x254a1b3a0d1e0853; // xorshift derive
    const FNV_PRIME: u64 = 0x100000001b3;

    fn feed(h: &mut u64, bytes: &[u8]) {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(FNV_PRIME);
        }
    }
    fn feed_field(h: &mut u64, bytes: &[u8]) {
        feed(h, &(bytes.len() as u64).to_le_bytes()); // length-prefix
        feed(h, bytes);
    }

    let (mut h1, mut h2, mut h3, mut h4) = (FNV_OFFSET_1, FNV_OFFSET_2, FNV_OFFSET_3, FNV_OFFSET_4);
    for h in [&mut h1, &mut h2, &mut h3, &mut h4] {
        feed_field(h, b"osp:supersede-basis:v1"); // domain tag
        feed_field(h, basis.superseded_id.0.as_bytes());
        feed_field(h, basis.successor_id.0.as_bytes());
        feed_field(h, &basis.superseded_digest.get().to_le_bytes());
        feed_field(h, &basis.successor_digest.get().to_le_bytes());
    }
    let mut out = [0u8; 32];
    out[0..8].copy_from_slice(&h1.to_le_bytes());
    out[8..16].copy_from_slice(&h2.to_le_bytes());
    out[16..24].copy_from_slice(&h3.to_le_bytes());
    out[24..32].copy_from_slice(&h4.to_le_bytes());
    out
}

/// PR E — Resolution basis fingerprint (SupersedeRecord mirror; domain-tag `osp:resolution-basis:v1`).
pub(crate) fn resolution_basis_fingerprint(basis: &PresentedResolutionBasis) -> [u8; 32] {
    const FNV_OFFSET_1: u64 = 0xcbf29ce484222325;
    const FNV_OFFSET_2: u64 = 0x84222325cbf29ce4;
    const FNV_OFFSET_3: u64 = 0x100000001b3a1b3a;
    const FNV_OFFSET_4: u64 = 0x254a1b3a0d1e0853;
    const FNV_PRIME: u64 = 0x100000001b3;

    fn feed(h: &mut u64, bytes: &[u8]) {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(FNV_PRIME);
        }
    }
    fn feed_field(h: &mut u64, bytes: &[u8]) {
        feed(h, &(bytes.len() as u64).to_le_bytes());
        feed(h, bytes);
    }

    let (mut h1, mut h2, mut h3, mut h4) = (FNV_OFFSET_1, FNV_OFFSET_2, FNV_OFFSET_3, FNV_OFFSET_4);
    for h in [&mut h1, &mut h2, &mut h3, &mut h4] {
        feed_field(h, b"osp:resolution-basis:v1"); // domain tag
        feed_field(h, basis.candidate_id().0.as_bytes());
        feed_field(h, &basis.candidate_digest().get().to_le_bytes());
        feed_field(h, basis.identity_key().canonical_key().as_bytes());
        match basis.target() {
            PresentedResolutionTarget::Create { proposed_entity_id } => {
                feed_field(h, b"Create");
                feed_field(h, proposed_entity_id.0.as_bytes());
            }
            PresentedResolutionTarget::Reuse {
                entity_id,
                entity_digest,
            } => {
                feed_field(h, b"Reuse");
                feed_field(h, entity_id.0.as_bytes());
                feed_field(h, &entity_digest.get().to_le_bytes());
            }
        }
    }
    let mut out = [0u8; 32];
    out[0..8].copy_from_slice(&h1.to_le_bytes());
    out[8..16].copy_from_slice(&h2.to_le_bytes());
    out[16..24].copy_from_slice(&h3.to_le_bytes());
    out[24..32].copy_from_slice(&h4.to_le_bytes());
    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 8b (PR #50): SupersedeSession — production authority boundary (INV-C15 production path)
//
// `OperatorReviewSession` (Faz 8a) deseninin supersede aynası. Token (`SupersedeAuthority`)
// içeride mint edilir, `SupersedeApplication` içeride üretilir, dışarı çıkmaz. Public
// entrypoint budur; authority capability'si crate-private kalır (gate.rs
// `issue_for_supersede_session`). Sözleşme: *"token dışarı çıkmaz, application dışarıda
// üretemez, session public entrypoint'tir, store atomik geçişi korur."*
//
// **Capability confinement (structural) vs operator authorization (INV-C11, deployment):**
// External caller `SupersedeAuthority` mint edemez (crate-private ctor) ve
// `SupersedeApplication` construct edemez (pub(crate) ctor). Ancak `open_for_operator`'ı
// çağıranın gerçekten authorized operator olup olmadığı tip sistemiyle doğrulanamaz —
// bu INV-C11 deployment/runtime boundary'sinde kalır. *"Preserving `SupersedeSession` as
// the sole in-crate production caller is a TCB discipline."*
//
// **Candidate proposal provenance (INV-C15, kalıcı sözleşme):** Candidate `Supersedes`
// edge'leri (apply_plan proposal) historical proposal provenance olarak korunur; başarılı
// session ayrı bir Accepted lineage edge ekler, proposal edge'i promote/delamine ETMEZ
// (lane-sensitive separation).
// ═══════════════════════════════════════════════════════════════════════════════

/// Supersede session özeti. `SessionSummary` (Faz 8a) deseninin supersede aynası.
/// `close(self)` consume eder; v1'de ledger'a close-event yazmaz (audit'in asıl gücü
/// `SupersedeRecord` ledger; close-event opsiyonel future).
#[derive(Debug, Clone)]
pub struct SupersedeSessionSummary {
    pub session_id: SessionId,
    pub operator: OperatorId,
    pub supersedes: u64,
}

/// Operator supersede oturumu — INV-C15 production invocation boundary'si.
///
/// `SupersedeAuthority`'yi içeride mint eder (`SupersedeAuthority::issue_for_supersede_session`,
/// crate-private); dışarı çıkmaz. `OperatorReviewSession` (Faz 8a) ile aynı yapısal garanti:
/// Clone YOK, Serialize/Deserialize YOK, literal construct engelli (private fields +
/// smart constructor).
///
/// # Sözleşme (PR #50 — 4 tur review)
/// **Structural (type-level):** external caller authority mint edemez, application construct
/// edemez; capability session'dan dışarı çıkmaz. INV-C4 artık C3'ün `OperatorReviewSession`
/// ile ulaştığı olgunluğa paralel — structurally confined capability path.
///
/// **Operator authorization (INV-C11, type-level DEĞİL):** `open_for_operator` çağıranın
/// gerçekten operator olup olmadığını tip sistemi doğrulayamaz; deployment/runtime boundary.
/// *"Capability confinement is structural; operator authorization is an INV-C11
/// deployment/runtime responsibility."*
///
/// # Candidate proposal provenance (INV-C15, kalıcı)
/// Candidate `Supersedes` proposal edge'leri preserved (historical proposal provenance);
/// başarılı session ayrı Accepted lineage edge ekler (lane-sensitive separation, not replacement).
pub struct SupersedeSession {
    session_id: SessionId,
    operator: OperatorId,
    opened_at: SystemTime,
    supersedes: u64,
}

impl SupersedeSession {
    /// Trusted-boundary constructor. Çağıran kod operator authority boundary'sidir
    /// (CLI operator mode, operator console) ve runtime'da operator olduğunu doğrulamış
    /// olmalı. **INV-C11:** agent-facing yüzeylerde çağrılmamalı (deployment disiplini;
    /// PR #51 agent-surface negatif testi kabul kriteri).
    ///
    /// Tip sistemi çağıranın insan olduğunu doğrulayamaz; sadece *şu operator* diyebilir.
    /// INV-C11 deployment boundary'sinde denetlenir (operator console = tek çağıran).
    pub fn open_for_operator(operator: OperatorId) -> Self {
        let opened_at = SystemTime::now();
        Self {
            session_id: SessionId::derive(&operator, opened_at),
            operator,
            opened_at,
            supersedes: 0,
        }
    }

    /// INV-C15 production invocation. Accepted → SupersededAccepted atomik transition +
    /// successor edge (store.rs `apply_supersede`). Authority session içeride mint edilir;
    /// çağıran parametre olarak veremez (token dışarı çıkmaz).
    ///
    /// **Deterministic precedence (1-11):** mutation öncesi tüm fallible validation,
    /// counter `checked_add` store op'undan ÖNCE, counter assign yalnız başarılı store op
    /// sonrası. Store aynı kontrolleri defense-in-depth olarak tekrar eder (kaldırılmaz).
    ///
    /// # Tazelik çift-katman (Tur 2 §3)
    /// `StaleSupersededBasis`/`StaleSuccessorBasis` hem session'da (typed-error ergonomisi —
    /// erken) hem store'da (`StoreError::Stale*`, defense-in-depth) yaşar. Generic `S::Error`
    /// üzerinden store hatası pattern-match edilemediği için session erken kontrol eder.
    /// *Digest semantiği değişirse iki yer değişmeli.*
    ///
    /// # Counter exhaustion precedence (Tur 4 §3)
    /// `SessionCounterExhausted`, tüm session-level basis/currentness/freshness kontrollerinden
    /// SONRA ama authority issuance + store-level defense-in-depth validation'dan ÖNCE değerlendirilir.
    /// Dolayısıyla u64::MAX'te yalnız store katmanında reddedilecek isteklerde öncelik alır;
    /// session-level geçersiz isteklerde ilgili session hatası önce döner.
    pub fn supersede<S: AnchorStore + ?Sized>(
        &mut self,
        store: &mut S,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
        basis: PresentedSupersedeBasis,
        reason: NonEmptyExplanation,
    ) -> Result<SupersedeRecord, SupersedeError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        // (1) Basis endpoint mismatch — defense-in-depth (store da kontrol eder, StoreError::SupersedeBasisMismatch).
        if basis.superseded_id() != superseded || basis.successor_id() != successor {
            return Err(SupersedeError::SupersedeBasisMismatch {
                basis_superseded: basis.superseded_id().clone(),
                basis_successor: basis.successor_id().clone(),
                req_superseded: superseded.clone(),
                req_successor: successor.clone(),
            });
        }

        // (2) mainline_query() TEK çağrı — iki node aynı snapshot'tan (TOCTOU: karar anında taze).
        let current = store
            .mainline_query()
            .map_err(|e| SupersedeError::Store(Box::new(e)))?;

        // (3)(4) Currentness — her iki node Accepted olmalı (mainline'da).
        let cur_sup = current
            .iter()
            .find(|n| &n.id == superseded)
            .ok_or_else(|| SupersedeError::SupersededNotCurrent(superseded.clone()))?;
        let cur_suc = current
            .iter()
            .find(|n| &n.id == successor)
            .ok_or_else(|| SupersedeError::SuccessorNotCurrent(successor.clone()))?;

        // (5) Superseded digest freshness (session katmanı — store da StoreError::StaleSupersededBasis ile tekrar eder).
        let cur_sup_digest = node_digest(cur_sup);
        if basis.superseded_digest() != cur_sup_digest {
            return Err(SupersedeError::StaleSupersededBasis {
                expected: basis.superseded_digest(),
                found: cur_sup_digest,
            });
        }
        // (6) Successor digest freshness (session katmanı — store da StoreError::StaleSuccessorBasis ile tekrar eder).
        let cur_suc_digest = node_digest(cur_suc);
        if basis.successor_digest() != cur_suc_digest {
            return Err(SupersedeError::StaleSuccessorBasis {
                expected: basis.successor_digest(),
                found: cur_suc_digest,
            });
        }

        // (7) Session counter — mutation ÖNCESİ checked_add (PR #49 audit_seq standardı).
        let next_supersedes = self
            .supersedes
            .checked_add(1)
            .ok_or(SupersedeError::SessionCounterExhausted)?;

        // (8) Internal authority issuance (crate-private — capability dışarı çıkmaz).
        let authority = SupersedeAuthority::issue_for_supersede_session();

        // (9) SupersedeApplication construction (in-crate ctor — token içeride gömülü).
        let application = SupersedeApplication::new(
            superseded.clone(),
            successor.clone(),
            authority,
            basis,
            reason,
            self.session_id.clone(),
            self.operator.clone(),
            SystemTime::now(),
        );

        // (10) store.apply_supersede — INV-C15 atomic transition. Store defense-in-depth
        // tekrar kontrol eder (basis mismatch, digests, status, self, compat, cycle, audit_seq).
        let record = store
            .apply_supersede(application)
            .map_err(|e| SupersedeError::Store(Box::new(e)))?;

        // (11) Counter assign — yalnız başarılı store op sonrası (atomiklik).
        self.supersedes = next_supersedes;
        Ok(record)
    }

    /// Session'ı consume eder. v1'de sadece in-memory `SupersedeSessionSummary` döner;
    /// ledger'a close-event yazmaz (opsiyonel future).
    pub fn close(self) -> SupersedeSessionSummary {
        SupersedeSessionSummary {
            session_id: self.session_id,
            operator: self.operator,
            supersedes: self.supersedes,
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

// ═══════════════════════════════════════════════════════════════════════════════
// PR E — CodeEntityResolutionSession (identity resolution; SupersedeSession mirror)
// ═══════════════════════════════════════════════════════════════════════════════
//
// CodeEntityCandidate ─ResolvesTo→ CodeEntity identity resolution. SupersedeSession pattern'ini
// mirror eder: PresentedResolutionBasis (compile) + ResolutionApplication (opaque, session üretir)
// + ResolutionRecord (audit) + CodeEntityResolutionSession (atomik resolve).
//
// # Ontolojik sözleşme (tur 1+2+3)
// node identity ≠ physical code identity. Resolution acceptance DEĞİL; geçici/hipotetik ontolojik
// temsilin kanonik fiziksel varlığa bağlanması. Source Accepted kalır; target Created=Candidate
// (otomatik mainline'a alınmaz), Reused=existing live entity. Edge Accepted + explanation (INV-C7).

use crate::anchoring::identity::CodeIdentityKey;

/// Resolution outcome — Created (yeni entity) veya Reused (mevcut live entity).
///
/// tur 3 P1-D: `PresentedResolutionBasis` target'ı pinler; create→reuse sessiz dönüşümü YOK
/// (`StaleResolutionTarget`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", content = "payload")]
pub enum ResolutionOutcome {
    /// Yeni CodeEntity node oluşturuldu (deterministic material from identity key).
    Created { entity_id: ConceptNodeId },
    /// Mevcut live CodeEntity yeniden kullanıldı (N:1 cardinality, R7).
    Reused { entity_id: ConceptNodeId },
}

/// Basis target — operator'ın gördüğü outcome pinlenir (tur 3 P1-D).
///
/// Create: `proposed_entity_id` deterministic derivation'dan.
/// Reuse: mevcut live entity ID + digest (freshness için).
#[derive(Debug, Clone, PartialEq)]
pub enum PresentedResolutionTarget {
    Create {
        proposed_entity_id: ConceptNodeId,
    },
    Reuse {
        entity_id: ConceptNodeId,
        entity_digest: NodeDigest,
    },
}

/// Resolution basis — store'dan derlenir (INV-C12 freshness). Target outcome pinlenir.
///
/// Mevcut `PresentedBasis::compile` Candidate-only (candidate_query); Accepted source için
/// ayrı compile yolu (tur 2 nokta 3 + tur 3 P2-F). `PresentedResolutionBasis::compile`
/// `resolution_basis_view` üzerinden view alır → digest/fingerprint üretir.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentedResolutionBasis {
    candidate_id: ConceptNodeId,
    candidate_digest: NodeDigest,
    identity_key: CodeIdentityKey,
    target: PresentedResolutionTarget, // create vs reuse pin
    compiled_at: SystemTime,
}

impl PresentedResolutionBasis {
    /// Canonical pre-state compiler (tur 2 nokta 3 + tur 3 P2-F). Accepted candidate için ayrı yol.
    pub fn compile<S: crate::anchoring::store::AnchorStore + ?Sized>(
        store: &S,
        candidate_id: &ConceptNodeId,
    ) -> Result<Self, ResolutionError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let view = store
            .resolution_basis_view(candidate_id)
            .map_err(|e| ResolutionError::Store(Box::new(e)))?;
        let candidate_digest =
            crate::anchoring::review::node_digest(&view.candidate);
        let target = match view.target {
            crate::anchoring::store::ResolutionTargetView::Create { proposed_entity_id } => {
                PresentedResolutionTarget::Create { proposed_entity_id }
            }
            crate::anchoring::store::ResolutionTargetView::Reuse { entity } => {
                PresentedResolutionTarget::Reuse {
                    entity_id: entity.id.clone(),
                    entity_digest: crate::anchoring::review::node_digest(&entity),
                }
            }
        };
        Ok(Self {
            candidate_id: candidate_id.clone(),
            candidate_digest,
            identity_key: view.identity_key,
            target,
            compiled_at: SystemTime::now(),
        })
    }

    pub fn candidate_id(&self) -> &ConceptNodeId {
        &self.candidate_id
    }
    pub fn candidate_digest(&self) -> NodeDigest {
        self.candidate_digest
    }
    pub fn identity_key(&self) -> &CodeIdentityKey {
        &self.identity_key
    }
    pub fn target(&self) -> &PresentedResolutionTarget {
        &self.target
    }
    pub fn compiled_at(&self) -> SystemTime {
        self.compiled_at
    }
}

/// Opaque application — yalnız Session üretir (private fields + pub(crate) new + no Deserialize).
///
/// tur 3 P2-E sadeleşme: redundant `entity_id`/`identity_key`/`outcome` çıkarıldı; basis
/// authoritative (tek representation — store defense-in-depth basis'ten okur).
#[derive(Debug, Clone)]
pub struct ResolutionApplication {
    candidate_id: ConceptNodeId,
    basis: PresentedResolutionBasis,
    reason: NonEmptyExplanation,
    session_id: SessionId,
    operator: OperatorId,
    resolved_at: SystemTime,
}

impl ResolutionApplication {
    /// Session (TCB içi) constructor.
    pub(crate) fn new(
        candidate_id: ConceptNodeId,
        basis: PresentedResolutionBasis,
        reason: NonEmptyExplanation,
        session_id: SessionId,
        operator: OperatorId,
        resolved_at: SystemTime,
    ) -> Self {
        Self {
            candidate_id,
            basis,
            reason,
            session_id,
            operator,
            resolved_at,
        }
    }

    pub fn candidate_id(&self) -> &ConceptNodeId {
        &self.candidate_id
    }
    pub fn basis(&self) -> &PresentedResolutionBasis {
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
    pub fn resolved_at(&self) -> SystemTime {
        self.resolved_at
    }
}

/// Resolution hatası (tur 3 P2-D `EntityNotLiveForResolution` + `EntityIdentityCollision`
/// + `StaleResolutionTarget` dahil).
#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    #[error("resolution basis mismatch: basis={basis_candidate}, request={request_candidate}")]
    BasisMismatch {
        basis_candidate: ConceptNodeId,
        request_candidate: ConceptNodeId,
    },
    #[error("candidate not found: {0}")]
    CandidateNotFound(ConceptNodeId),
    #[error("candidate kind değil CodeEntityCandidate")]
    WrongCandidateKind,
    #[error("candidate family değil PhysicalCode")]
    WrongFamily,
    #[error("candidate status değil Accepted (current: {current:?})")]
    CandidateNotAccepted { current: DecisionStatus },
    #[error("stale resolution basis — candidate digest değişmiş")]
    StaleResolutionBasis,
    #[error("candidate identity binding bulunamadı")]
    MissingIdentityBinding,
    #[error("candidate already resolved (R6 — outgoing ResolvesTo mevcut)")]
    AlreadyResolved,
    #[error("stale resolution target — basis create/reuse outcome artık geçerli değil")]
    StaleResolutionTarget,
    #[error("reuse target kind/family/status/key uyumsuz")]
    ReuseTargetIncompatible,
    /// tur 3 P2-D: aynı key + inactive entity (Rejected/Deprecated/SupersededAccepted).
    #[error("entity not live for resolution: {entity_id} status={status:?}")]
    EntityNotLiveForResolution {
        entity_id: ConceptNodeId,
        status: DecisionStatus,
    },
    /// tur 3 P2-B: aynı ID + farklı material/key (hash collision fail-closed).
    #[error("entity identity collision: {entity_id} farklı material")]
    EntityIdentityCollision { entity_id: ConceptNodeId },
    #[error("duplicate live entity for same key (R7 violation)")]
    DuplicateLiveEntity,
    #[error("audit sequence exhausted")]
    AuditSequenceExhausted,
    #[error("session counter exhausted")]
    SessionCounterExhausted,
    #[error("store error: {0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

/// Resolution audit record — SupersedeRecord mirror.
///
/// `candidate_digest`/`entity_digest` raw u64 (NodeDigest Serialize-only; domain alanı —
/// serde detail değil). `basis_fingerprint` domain-tag `osp:resolution-basis:v1`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolutionRecord {
    pub seq: u64, // global audit_seq (3 ledger union)
    pub session_id: SessionId,
    pub operator: OperatorId,
    pub candidate_id: ConceptNodeId,
    pub entity_id: ConceptNodeId,
    pub identity_key: CodeIdentityKey,
    pub outcome: ResolutionOutcome,
    pub reason: NonEmptyExplanation,
    pub candidate_digest: u64,
    pub entity_digest: u64,
    pub basis_fingerprint: [u8; 32],
    pub at: SystemTime,
}

/// Session close summary (tur 3 P1-3 — SupersedeSession pattern).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionSessionSummary {
    pub session_id: SessionId,
    pub operator: OperatorId,
    pub resolutions: u64,
}

/// CodeEntityResolutionSession — identity resolution (SupersedeSession mirror).
///
/// # Counter exhaustion precedence (SupersedeSession pattern)
/// `SessionCounterExhausted`, session-level basis validation'dan SONRA ama store-level
/// defense-in-depth'tan ÖNCE değerlendirilir.
pub struct CodeEntityResolutionSession {
    session_id: SessionId,
    operator: OperatorId,
    opened_at: SystemTime,
    resolutions: u64,
}

impl CodeEntityResolutionSession {
    /// Trusted-boundary constructor. INV-C11: operator doğrulaması deployment sorumluluğu.
    pub fn open_for_operator(operator: OperatorId) -> Self {
        let opened_at = SystemTime::now();
        Self {
            session_id: SessionId::derive(&operator, opened_at),
            operator,
            opened_at,
            resolutions: 0,
        }
    }

    /// Atomik resolution (tur 3 P1-3 — `&mut self` counter semantics).
    ///
    /// SupersedeSession pattern: (1) basis validation, (2) checked_add counter exhaustion,
    /// (3) opaque application üretimi, (4) store mutation, (5) yalnız başarıdan sonra counter assignment.
    pub fn resolve<S: crate::anchoring::store::AnchorStore + ?Sized>(
        &mut self,
        store: &mut S,
        candidate_id: &ConceptNodeId,
        basis: PresentedResolutionBasis,
        reason: NonEmptyExplanation,
    ) -> Result<ResolutionRecord, ResolutionError>
    where
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        // (1) Basis endpoint match — defense-in-depth (store da kontrol eder).
        if basis.candidate_id() != candidate_id {
            return Err(ResolutionError::BasisMismatch {
                basis_candidate: basis.candidate_id().clone(),
                request_candidate: candidate_id.clone(),
            });
        }

        // (2) Counter exhaustion precedence (Tur 4 §3 — session-level geçerlilik sonrası).
        let next_resolutions = self.resolutions.checked_add(1).ok_or(ResolutionError::SessionCounterExhausted)?;

        // (3) Opaque application üretimi.
        let application = ResolutionApplication::new(
            candidate_id.clone(),
            basis,
            reason,
            self.session_id.clone(),
            self.operator.clone(),
            SystemTime::now(),
        );

        // (4) Store mutation (store-level defense-in-depth tüm 14 step doğrular).
        let record = store
            .apply_resolution(application)
            .map_err(|e| ResolutionError::Store(Box::new(e)))?;

        // (5) Counter assignment — yalnız başarıdan sonra (atomicity guarantee).
        self.resolutions = next_resolutions;
        Ok(record)
    }

    pub fn close(self) -> ResolutionSessionSummary {
        ResolutionSessionSummary {
            session_id: self.session_id,
            operator: self.operator,
            resolutions: self.resolutions,
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
    use crate::anchoring::store::InMemoryAnchorStore;
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
        let mut session =
            OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let id = ConceptNodeId("RuleCandidate:CouplingMustNot".into());

        let basis = PresentedBasis::compile(&store, &id).expect("basis compile");
        let reason = NonEmptyExplanation::new("rule accepted by operator").unwrap();
        let record = session
            .accept(&mut store, &id, basis, reason)
            .expect("accept");

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
        let mut session =
            OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let id = ConceptNodeId("RuleCandidate:Bad".into());

        let basis = PresentedBasis::compile(&store, &id).expect("basis");
        let reason = NonEmptyExplanation::new("rule rejected — out of scope").unwrap();
        let record = session
            .reject(&mut store, &id, basis, reason)
            .expect("reject");

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
            .accept(
                &mut store,
                &id_a,
                ba,
                NonEmptyExplanation::new("a").unwrap(),
            )
            .unwrap();
        let bb = PresentedBasis::compile(&store, &id_b).unwrap();
        session
            .reject(
                &mut store,
                &id_b,
                bb,
                NonEmptyExplanation::new("b").unwrap(),
            )
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
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test"));
        session
            .accept(
                &mut store,
                &id,
                basis.clone(),
                NonEmptyExplanation::new("first").unwrap(),
            )
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

    /// Faz 8b (PR #48): SupersededAccepted node terminal statüdür — apply_decision
    /// (in-crate ctor) ile accept/reject reddedilir: `NotPromotableFrom(SupersededAccepted)`.
    /// `apply_decision_rejects_accepted_node_not_promotable` testine paralel — her terminal
    /// statü için defense-in-depth. Diriltme/reinstate ayrı mekanizma (v1 dışı).
    #[test]
    fn apply_decision_rejects_superseded_accepted_not_promotable() {
        use crate::anchoring::store::{AnchorStore, InMemoryAnchorStore, StoreError};
        // SupersededAccepted node seed'le (PR #48'de henüz üretici yok — doğrudan statü set).
        //
        // NOTE (review PR #48): bu testin geçerliliği `node_digest`'in `decision_status`'u
        // DIŞLAMASINA dayanır — bkz. `node_digest_excludes_decision_status` (yukarıda). Basis,
        // candidate-seed'li ikinci store'dan derlenir (candidate digest == superseded digest,
        // çünkü digest canonical/aliases/kind/family'den gelir, status'tan değil). Bu yüzden
        // StaleBasis tetiklenmeden NotPromotableFrom dalına ulaşılır. İleride biri digest'e
        // status eklerse bu test kırılır — o test neden kırıldığını buradan okusun.
        let node = ConceptNode {
            id: ConceptNodeId("RuleCandidate:Superseded".into()),
            canonical: "Superseded".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::SupersededAccepted,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(node);
        let mut store = InMemoryAnchorStore::with_seed(seed);
        let id = ConceptNodeId("RuleCandidate:Superseded".into());

        // Basis derive et — SupersededAccepted candidate_query'de olmadığı için compile
        // NotFound verir. Bu yüzden basis'i candidate-seed'li ikinci store'dan derive ederiz:
        // basis candidate_id == application candidate_id eşleşmesi yeterli (defense-in-depth
        // id check), ardından apply_decision prior_status kontrolüne ulaşır.
        let cand = ConceptNode {
            id: id.clone(),
            canonical: "Superseded".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed2 = GraphSeed::default();
        seed2.rule_candidates.push(cand);
        let basis_store = InMemoryAnchorStore::with_seed(seed2);
        let basis = PresentedBasis::compile(&basis_store, &id).expect("basis from candidate seed");

        let app = DecisionApplication::new(
            id.clone(),
            DecisionKind::Accept,
            basis,
            NonEmptyExplanation::new("should reject — superseded terminal").unwrap(),
            SessionId("session:test".into()),
            OperatorId::new("test"),
            SystemTime::now(),
        );
        let err = AnchorStore::apply_decision(&mut store, app).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::NotPromotableFrom(DecisionStatus::SupersededAccepted)
            ),
            "NotPromotableFrom(SupersededAccepted) bekleniyordu, got {err:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Faz 8b (PR #49): apply_supersede tests — mutlu yol, zincir, consolidation,
    // projection, error-path matrisi (12), fingerprint, serde.
    // ═══════════════════════════════════════════════════════════════════════════════

    use crate::anchoring::gate::SupersedeAuthority;
    use crate::anchoring::store::StoreError;
    use crate::anchoring::types::ConceptEdge;
    use crate::anchoring::ConceptEdgeKind;
    use std::collections::BTreeSet;

    /// Test yardımcı: belirli bir statüde bir node seed'le (Accepted için).
    fn node_with(id: &str, status: DecisionStatus) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: status,
            position_family: PositionFamily::ConceptualIntent,
        }
    }

    /// Test yardımcı: iki Accepted node'lu store (supersede için).
    fn store_with_two_accepted(superseded: &str, successor: &str) -> InMemoryAnchorStore {
        let mut seed = GraphSeed::default();
        seed.rule_candidates
            .push(node_with(superseded, DecisionStatus::Accepted));
        seed.rule_candidates
            .push(node_with(successor, DecisionStatus::Accepted));
        InMemoryAnchorStore::with_seed(seed)
    }

    /// Test factory: geçerli SupersedeApplication (authority test ctor ile).
    fn supersede_app(
        store: &InMemoryAnchorStore,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
    ) -> SupersedeApplication {
        let basis =
            PresentedSupersedeBasis::compile(store, superseded, successor).expect("basis compile");
        SupersedeApplication::new(
            superseded.clone(),
            successor.clone(),
            SupersedeAuthority::issue_operator_for_tests(),
            basis,
            NonEmptyExplanation::new("test supersede").unwrap(),
            SessionId("session:test".into()),
            OperatorId::new("test"),
            SystemTime::now(),
        )
    }

    /// INV-C15 "On Err unchanged" kanıtı: error-path testleri için **tam store-state** snapshot.
    /// ConceptGraph (Clone+PartialEq — node/edge *içeriği*: status, from/to/kind, aliases),
    /// iki ledger Vec'leri (kayıt *içeriği*: operator/reason/digest), audit_seq.
    /// Review PR #49 tur 6: count/length değil gerçek içerik — status/from-to/kayıt mutasyonu
    /// yakalanır (sayılar aynı kalıp içerik değişirse bu snapshot kırılır).
    #[derive(Debug, Clone, PartialEq)]
    struct StoreSnapshot {
        graph: crate::anchoring::types::ConceptGraph,
        decision_ledger: Vec<DecisionRecord>,
        supersede_ledger: Vec<SupersedeRecord>,
        audit_seq: u64,
    }

    fn snapshot_store(store: &InMemoryAnchorStore) -> StoreSnapshot {
        StoreSnapshot {
            graph: store.graph().clone(),
            decision_ledger: store.decision_ledger(),
            supersede_ledger: store.supersede_ledger(),
            audit_seq: store.audit_seq_for_tests(),
        }
    }

    /// "On Err unchanged" assertion: apply_supersede hatadan sonra store değişmemeli.
    /// Tam graph + iki ledger + audit_seq (Review PR #49 tur 6: içerik karşılaştırması).
    fn assert_store_unchanged_by_supersede_error(
        before: StoreSnapshot,
        store: &InMemoryAnchorStore,
        ctx: &str,
    ) {
        let after = snapshot_store(store);
        assert_eq!(after, before, "{ctx}: store unchanged after error");
    }

    /// Test-only malformed factory: basis private alanlarına doğrudan erişim.
    /// NodeNotFound / SelfSupersede gibi defense-in-depth dallarını exercise etmek için
    /// (basis compile bunları reddeder, bu yüzden private alanlara doğrudan erişim).
    fn app_with_basis_for_tests(
        superseded: ConceptNodeId,
        successor: ConceptNodeId,
        sup_digest: NodeDigest,
        suc_digest: NodeDigest,
    ) -> SupersedeApplication {
        let basis = PresentedSupersedeBasis {
            superseded_id: superseded.clone(),
            successor_id: successor.clone(),
            superseded_digest: sup_digest,
            successor_digest: suc_digest,
            compiled_at: SystemTime::UNIX_EPOCH,
        };
        SupersedeApplication::new(
            superseded,
            successor,
            SupersedeAuthority::issue_operator_for_tests(),
            basis,
            NonEmptyExplanation::new("malformed test").unwrap(),
            SessionId("session:malformed".into()),
            OperatorId::new("test"),
            SystemTime::now(),
        )
    }

    /// Mutlu yol: A (Accepted) supersede B (Accepted) → A SupersededAccepted,
    /// B Accepted kalır, successor→superseded edge (committed/Accepted), record seq global.
    #[test]
    fn apply_supersede_creates_one_committed_incoming_edge() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let app = supersede_app(&store, &old, &new);

        let record = store.apply_supersede(app).expect("supersede");

        // Record doğrulaması.
        assert_eq!(record.prior_status, DecisionStatus::Accepted);
        assert_eq!(record.new_status, DecisionStatus::SupersededAccepted);
        assert_eq!(record.superseded, old);
        assert_eq!(record.successor, new);
        assert_eq!(record.authority_level, SupersedeAuthorityLevel::Operator);
        assert_eq!(record.seq, 1, "ilk supersede → audit_seq 1");

        // Graph durumu: old SupersededAccepted, new Accepted.
        assert_eq!(
            store.graph().node(&old).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&new).unwrap().decision_status,
            DecisionStatus::Accepted
        );

        // Edge: successor → superseded (Accepted/committed). YÖN: new→old.
        let edges: Vec<_> = store
            .graph()
            .edges()
            .filter(|e| e.kind == ConceptEdgeKind::Supersedes)
            .collect();
        assert_eq!(edges.len(), 1, "tam olarak bir committed Supersedes edge");
        assert_eq!(edges[0].from, new, "edge from=successor");
        assert_eq!(edges[0].to, old, "edge to=superseded");
        assert_eq!(edges[0].decision_status, DecisionStatus::Accepted);

        // Ledger.
        assert_eq!(store.supersede_ledger().len(), 1);
        assert_eq!(store.supersede_ledger()[0].seq, record.seq);
    }

    /// Zincir: A→B→C. B A'yı supersede (B→A), sonra C B'yi supersede (C→B).
    /// A ve B SupersededAccepted, C Accepted. İki ayrı invariant örneği.
    #[test]
    fn a_replaced_by_b_replaced_by_c_lineage() {
        let mut store = {
            let mut seed = GraphSeed::default();
            for id in ["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"] {
                seed.rule_candidates
                    .push(node_with(id, DecisionStatus::Accepted));
            }
            InMemoryAnchorStore::with_seed(seed)
        };
        let a = ConceptNodeId("RuleCandidate:A".into());
        let b = ConceptNodeId("RuleCandidate:B".into());
        let c = ConceptNodeId("RuleCandidate:C".into());

        // B → A (B A'yı supersede).
        store
            .apply_supersede(supersede_app(&store, &a, &b))
            .unwrap();
        // C → B (C B'yi supersede).
        store
            .apply_supersede(supersede_app(&store, &b, &c))
            .unwrap();

        // Statüler.
        assert_eq!(
            store.graph().node(&a).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&b).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&c).unwrap().decision_status,
            DecisionStatus::Accepted
        );

        // Lineage: C→B→A (güncelden geçmişe doğal yürüyüş).
        let sup_edges: Vec<_> = store
            .graph()
            .edges()
            .filter(|e| {
                e.kind == ConceptEdgeKind::Supersedes
                    && e.decision_status == DecisionStatus::Accepted
            })
            .collect();
        assert_eq!(sup_edges.len(), 2, "iki committed Supersedes edge");

        // INV-C15 cardinalite: her SupersededAccepted'ın tam bir incoming committed edge'i.
        let incoming_a = sup_edges.iter().filter(|e| e.to == a).count();
        let incoming_b = sup_edges.iter().filter(|e| e.to == b).count();
        assert_eq!(incoming_a, 1, "A incoming = 1 (B→A)");
        assert_eq!(incoming_b, 1, "B incoming = 1 (C→B)");

        // audit_seq total order: iki kayıt, seq 1 ve 2.
        let seqs: Vec<u64> = store.supersede_ledger().iter().map(|r| r.seq).collect();
        assert_eq!(seqs, vec![1, 2]);
    }

    /// Consolidation: C hem A'yı hem B'yi supersede (outgoing sınırı yok).
    /// INV-C15 incoming-only cardinalite — successor'un outgoing sayısı serbest.
    #[test]
    fn one_successor_may_consolidate_multiple_old_nodes() {
        let mut store = {
            let mut seed = GraphSeed::default();
            for id in ["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"] {
                seed.rule_candidates
                    .push(node_with(id, DecisionStatus::Accepted));
            }
            InMemoryAnchorStore::with_seed(seed)
        };
        let a = ConceptNodeId("RuleCandidate:A".into());
        let b = ConceptNodeId("RuleCandidate:B".into());
        let c = ConceptNodeId("RuleCandidate:C".into());

        store
            .apply_supersede(supersede_app(&store, &a, &c))
            .unwrap();
        store
            .apply_supersede(supersede_app(&store, &b, &c))
            .unwrap();

        // A ve B SupersededAccepted, C Accepted (consolidation).
        assert_eq!(
            store.graph().node(&a).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&b).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&c).unwrap().decision_status,
            DecisionStatus::Accepted
        );

        // C'den iki outgoing edge (C→A, C→B) — izinli.
        let outgoing_c = store
            .graph()
            .edges()
            .filter(|e| {
                e.kind == ConceptEdgeKind::Supersedes
                    && e.decision_status == DecisionStatus::Accepted
                    && e.from == c
            })
            .count();
        assert_eq!(outgoing_c, 2, "consolidation: C outgoing = 2");
    }

    /// INV-C3/C14 projection: superseded mainline_query'de yok, mainline_history'de var.
    #[test]
    fn superseded_accepted_excluded_from_mainline_query_in_history() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        store
            .apply_supersede(supersede_app(&store, &old, &new))
            .unwrap();

        // mainline_query: sadece Accepted (new).
        let current: BTreeSet<String> = store
            .mainline_query()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        assert!(current.contains("RuleCandidate:New"));
        assert!(
            !current.contains("RuleCandidate:Old"),
            "superseded mainline_query'de değil"
        );

        // mainline_history: Accepted + SupersededAccepted (her ikisi).
        let history: BTreeSet<String> = store
            .mainline_history()
            .unwrap()
            .into_iter()
            .map(|n| n.id.0)
            .collect();
        assert!(history.contains("RuleCandidate:New"));
        assert!(
            history.contains("RuleCandidate:Old"),
            "superseded mainline_history'de (provenance)"
        );
    }

    /// İkinci committed supersede → AlreadySuperseded. Bu test seed'li committed edge ile
    /// kurulur (basis compile A Acceptedken yapılır, sonra A'ya committed edge seed'lenir,
    /// böylece store AlreadySuperseded dalına ulaşır — basis compile patlamadan).
    #[test]
    fn second_committed_successor_rejected_already_superseded() {
        let mut store = {
            let mut seed = GraphSeed::default();
            for id in ["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"] {
                seed.rule_candidates
                    .push(node_with(id, DecisionStatus::Accepted));
            }
            InMemoryAnchorStore::with_seed(seed)
        };
        let a = ConceptNodeId("RuleCandidate:A".into());
        let c = ConceptNodeId("RuleCandidate:C".into());

        // A'ya committed Supersedes edge seed'le (B→A, Accepted) — apply_supersede'nin
        // değil, mevcut bir supersede simülasyonu. Böylece store AlreadySuperseded kontrolü
        // ateşlenir; basis compile (A Acceptedken) daha önce yapıldı.
        store.graph_mut().insert_edge(ConceptEdge {
            from: ConceptNodeId("RuleCandidate:B".into()),
            to: a.clone(),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Accepted,
            explanation: Some(NonEmptyExplanation::new("seeded committed").unwrap()),
        });

        let app = supersede_app(&store, &a, &c);
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::AlreadySuperseded(_)),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "AlreadySuperseded");
    }

    /// Candidate Supersedes edge (apply_plan proposal) cardinality'ye katılmaz — gerçek
    /// supersession'ı engellemez. Post-state: candidate edge hâlâ orada, committed edge de var.
    #[test]
    fn candidate_supersedes_edge_does_not_count_as_committed() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());

        // Candidate Supersedes edge seed'le (apply_plan proposal simülasyonu).
        store.graph_mut().insert_edge(ConceptEdge {
            from: new.clone(),
            to: old.clone(),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Candidate,
            explanation: Some(NonEmptyExplanation::new("candidate proposal").unwrap()),
        });

        // apply_supersede hâlâ başarılı (Candidate edge committed sayılmaz).
        store
            .apply_supersede(supersede_app(&store, &old, &new))
            .expect("Candidate edge engellemedi");

        // Post-state: hem Candidate edge hem committed edge var (Review PR #49 tur 2).
        let sup_edges: Vec<_> = store
            .graph()
            .edges()
            .filter(|e| e.kind == ConceptEdgeKind::Supersedes && e.from == new && e.to == old)
            .collect();
        assert_eq!(
            sup_edges.len(),
            2,
            "candidate + committed edge ikisi de mevcut"
        );
        assert_eq!(
            sup_edges
                .iter()
                .filter(|e| e.decision_status == DecisionStatus::Candidate)
                .count(),
            1
        );
        assert_eq!(
            sup_edges
                .iter()
                .filter(|e| e.decision_status == DecisionStatus::Accepted)
                .count(),
            1
        );
    }

    /// Endpoint compatibility: farklı node_kind → IncompatibleSupersedeEndpoints.
    #[test]
    fn incompatible_endpoints_rejected() {
        let mut store = {
            let mut seed = GraphSeed::default();
            seed.rule_candidates.push(ConceptNode {
                id: ConceptNodeId("RuleCandidate:Old".into()),
                canonical: "Old".into(),
                aliases: vec![],
                node_kind: ConceptNodeKind::RuleCandidate,
                decision_status: DecisionStatus::Accepted,
                position_family: PositionFamily::ConceptualIntent,
            });
            seed.task_candidates.push(ConceptNode {
                id: ConceptNodeId("TaskCandidate:New".into()),
                canonical: "New".into(),
                aliases: vec![],
                node_kind: ConceptNodeKind::TaskCandidate,
                decision_status: DecisionStatus::Accepted,
                position_family: PositionFamily::ConceptualIntent,
            });
            InMemoryAnchorStore::with_seed(seed)
        };
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("TaskCandidate:New".into());
        let app = supersede_app(&store, &old, &new);
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::IncompatibleSupersedeEndpoints { .. }),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "IncompatibleSupersedeEndpoints");
    }

    /// Stale superseded basis: basis derle, superseded'ın canonical'ını değiştir → StaleSupersededBasis.
    /// (node_digest decision_status dışlar → status değil canonical değişimi tazelik kırar)
    #[test]
    fn stale_superseded_basis_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let app = supersede_app(&store, &old, &new);

        // TOCTOU: superseded canonical'ını değiştir.
        store.graph_mut().node_mut(&old).unwrap().canonical = "ChangedAfterBasis".into();

        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::StaleSupersededBasis { .. }),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "StaleSupersededBasis");
    }

    /// Stale successor basis: successor canonical'ını değiştir → StaleSuccessorBasis.
    #[test]
    fn stale_successor_basis_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let app = supersede_app(&store, &old, &new);

        store.graph_mut().node_mut(&new).unwrap().canonical = "ChangedSuccessor".into();

        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::StaleSuccessorBasis { .. }),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "StaleSuccessorBasis");
    }

    /// Non-accepted superseded: superseded'ı Candidate yap → NotSupersedeableFrom.
    /// (basis Acceptedken derlendi, sonra status değişti → digest hâlâ taze)
    #[test]
    fn non_accepted_superseded_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let app = supersede_app(&store, &old, &new);

        store.graph_mut().node_mut(&old).unwrap().decision_status = DecisionStatus::Candidate;

        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::NotSupersedeableFrom(DecisionStatus::Candidate)
            ),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "NotSupersedeableFrom");
    }

    /// Non-accepted successor: successor'ı Candidate yap → SuccessorNotAccepted.
    #[test]
    fn non_accepted_successor_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let app = supersede_app(&store, &old, &new);

        store.graph_mut().node_mut(&new).unwrap().decision_status = DecisionStatus::Candidate;

        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::SuccessorNotAccepted(DecisionStatus::Candidate)
            ),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "SuccessorNotAccepted");
    }

    /// Self-supersede: app superseded == successor → SelfSupersede. Malformed factory
    /// (basis A→A uyumlu, compile self-supersede'i reddeder bu yüzden private factory).
    #[test]
    fn self_supersede_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:A", "RuleCandidate:B");
        let a = ConceptNodeId("RuleCandidate:A".into());
        let a_node = store.graph().node(&a).unwrap();
        let a_digest = node_digest(a_node);
        // Basis A→A (uyumlu), app de A→A — self-supersede, ama precedence basis mismatch'i geçer.
        let app = app_with_basis_for_tests(a.clone(), a.clone(), a_digest, a_digest);
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(matches!(err, StoreError::SelfSupersede(_)), "got {err:?}");
        assert_store_unchanged_by_supersede_error(before, &store, "SelfSupersede");
    }

    /// Missing superseded node: app id graph'ta yok → NodeNotFound. Malformed factory
    /// (basis Ghost ID'sini taşır, ama compile edilemez çünkü Ghost mainline'da değil).
    #[test]
    fn missing_superseded_node_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let new = ConceptNodeId("RuleCandidate:New".into());
        let ghost = ConceptNodeId("RuleCandidate:Ghost".into());
        // Basis ghost→new (uyumlu), app ghost→new — ghost graph'ta yok → NodeNotFound.
        let new_digest = node_digest(store.graph().node(&new).unwrap());
        let app = app_with_basis_for_tests(ghost, new, new_digest, new_digest);
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(matches!(err, StoreError::NodeNotFound(_)), "got {err:?}");
        assert_store_unchanged_by_supersede_error(before, &store, "NodeNotFound(superseded)");
    }

    /// Missing successor node → NodeNotFound.
    #[test]
    fn missing_successor_node_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let ghost = ConceptNodeId("RuleCandidate:Ghost".into());
        let old_digest = node_digest(store.graph().node(&old).unwrap());
        // Basis old→ghost (uyumlu), app old→ghost — ghost graph'ta yok → NodeNotFound.
        let app = app_with_basis_for_tests(old, ghost, old_digest, old_digest);
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(matches!(err, StoreError::NodeNotFound(_)), "got {err:?}");
        assert_store_unchanged_by_supersede_error(before, &store, "NodeNotFound(successor)");
    }

    /// Basis endpoint mismatch: basis Old→New, app New→Old → SupersedeBasisMismatch.
    #[test]
    fn supersede_basis_endpoint_mismatch_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        // Basis old→new, app new→old (ters).
        let basis = PresentedSupersedeBasis::compile(&store, &old, &new).unwrap();
        let app = SupersedeApplication::new(
            new.clone(),
            old.clone(), // app ters
            SupersedeAuthority::issue_operator_for_tests(),
            basis,
            NonEmptyExplanation::new("mismatch").unwrap(),
            SessionId("session:t".into()),
            OperatorId::new("t"),
            SystemTime::now(),
        );
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::SupersedeBasisMismatch { .. }),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "SupersedeBasisMismatch");
    }

    /// Cycle: mevcut committed edge B→A; attempted edge A→B (superseded=B, successor=A).
    /// Cycle check: superseded(B) →* successor(A)? B→A zaten var → A→B eklemek cycle oluşturur.
    /// (Production API dizisiyle unreachable — her Supersedes hedefi atomik SupersededAccepted
    /// olur; seeded/deserialized adversarial graph savunması.)
    #[test]
    fn supersede_cycle_rejected() {
        let mut store = store_with_two_accepted("RuleCandidate:A", "RuleCandidate:B");
        let a = ConceptNodeId("RuleCandidate:A".into());
        let b = ConceptNodeId("RuleCandidate:B".into());
        // Mevcut committed edge: B→A (A henüz Accepted — gerçek apply yapmadık).
        store.graph_mut().insert_edge(ConceptEdge {
            from: b.clone(),
            to: a.clone(),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Accepted,
            explanation: Some(NonEmptyExplanation::new("seeded B→A").unwrap()),
        });
        // Attempted edge: A→B (superseded=B, successor=A). B→*A yolu zaten var → cycle.
        let app = supersede_app(&store, &b, &a);
        let before = snapshot_store(&store);
        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::SupersedeCycle { .. }),
            "got {err:?}"
        );
        assert_store_unchanged_by_supersede_error(before, &store, "SupersedeCycle");
    }

    /// Audit sequence exhaustion: audit_seq = u64::MAX → AuditSequenceExhausted.
    /// Audit sequence exhaustion: audit_seq = u64::MAX → AuditSequenceExhausted.
    /// Full graph, both ledgers, and audit_seq remain unchanged.
    #[test]
    fn audit_sequence_exhaustion_leaves_state_unchanged() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        store.set_audit_seq_for_tests(u64::MAX);

        let app = supersede_app(&store, &old, &new);
        let before = snapshot_store(&store);

        let err = store.apply_supersede(app).unwrap_err();
        assert!(
            matches!(err, StoreError::AuditSequenceExhausted),
            "got {err:?}"
        );
        // Full store-state unchanged (Review PR #49: node/edge counts + both ledgers + audit_seq).
        assert_store_unchanged_by_supersede_error(before, &store, "AuditSequenceExhausted");
    }

    /// Decision ve supersede record'lar paylaşımlı monotonik audit_seq paylaşır.
    #[test]
    fn decision_and_supersede_records_share_monotonic_audit_sequence() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        // Önce supersede (seq 1).
        let sup_rec = store
            .apply_supersede(supersede_app(&store, &old, &new))
            .unwrap();
        assert_eq!(sup_rec.seq, 1);
        // Sonra decision (seq 2) — ama artık old SupersededAccepted. new hâlâ Accepted,
        // Candidate node ekleyip decision yapalım.
        {
            let mut seed = GraphSeed::default();
            seed.rule_candidates
                .push(node_with("RuleCandidate:Cand", DecisionStatus::Candidate));
            store.seed_trusted(&seed).unwrap();
        }
        let cand = ConceptNodeId("RuleCandidate:Cand".into());
        let basis = crate::anchoring::review::PresentedBasis::compile(&store, &cand).unwrap();
        let dec_app = crate::anchoring::review::DecisionApplication::new(
            cand.clone(),
            crate::anchoring::review::DecisionKind::Accept,
            basis,
            NonEmptyExplanation::new("accept").unwrap(),
            SessionId("s".into()),
            OperatorId::new("o"),
            SystemTime::now(),
        );
        let dec_rec = store.apply_decision(dec_app).unwrap();
        assert_eq!(
            dec_rec.seq, 2,
            "decision seq supersede'den sonra (global audit_seq)"
        );
        assert!(
            dec_rec.seq > sup_rec.seq,
            "total order: decision > supersede"
        );
    }

    /// Fingerprint stabil: aynı basis → aynı fingerprint.
    #[test]
    fn supersede_basis_fingerprint_is_stable() {
        let store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let b1 = PresentedSupersedeBasis::compile(&store, &old, &new).unwrap();
        let b2 = PresentedSupersedeBasis::compile(&store, &old, &new).unwrap();
        // compiled_at farklı olabilir ama fingerprint aynı (compiled_at hariç).
        let fp1 = supersede_basis_fingerprint(&b1);
        let fp2 = supersede_basis_fingerprint(&b2);
        assert_eq!(
            fp1, fp2,
            "same basis → same fingerprint (compiled_at excluded)"
        );
    }

    /// Fingerprint direction-sensitive: fp(A←B) != fp(B←A).
    #[test]
    fn supersede_basis_fingerprint_is_direction_sensitive() {
        let store = store_with_two_accepted("RuleCandidate:A", "RuleCandidate:B");
        let a = ConceptNodeId("RuleCandidate:A".into());
        let b = ConceptNodeId("RuleCandidate:B".into());
        let fp_ab =
            supersede_basis_fingerprint(&PresentedSupersedeBasis::compile(&store, &a, &b).unwrap());
        let fp_ba =
            supersede_basis_fingerprint(&PresentedSupersedeBasis::compile(&store, &b, &a).unwrap());
        assert_ne!(fp_ab, fp_ba, "direction-sensitive: A←B != B←A");
    }

    /// SupersedeRecord serde round-trip.
    #[test]
    fn supersede_record_serde_roundtrip() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let record = store
            .apply_supersede(supersede_app(&store, &old, &new))
            .unwrap();
        let json = serde_json::to_string(&record).unwrap();
        let back: SupersedeRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, record.seq);
        assert_eq!(back.superseded, record.superseded);
        assert_eq!(back.successor, record.successor);
        assert_eq!(back.new_status, DecisionStatus::SupersededAccepted);
    }

    /// PresentedSupersedeBasis self-supersede'i compile aşamasında reddeder.
    #[test]
    fn presented_supersede_basis_rejects_self_supersede_at_compile() {
        let store = store_with_two_accepted("RuleCandidate:A", "RuleCandidate:B");
        let a = ConceptNodeId("RuleCandidate:A".into());
        let err = PresentedSupersedeBasis::compile(&store, &a, &a).unwrap_err();
        assert!(
            matches!(err, SupersedeError::SelfSupersede(_)),
            "got {err:?}"
        );
    }

    /// Compile: superseded mainline'da değilse (Candidate) → SupersededNotCurrent.
    /// (`SupersedeSession::supersede` aynı kontrolü tekrar eder — compile-aşaması reddi
    /// session'a ulaşmadan presentation'da yakalar. Review PR #49 tur 5.)
    #[test]
    fn presented_supersede_basis_rejects_non_accepted_superseded_at_compile() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        store.graph_mut().node_mut(&old).unwrap().decision_status = DecisionStatus::Candidate;
        let err = PresentedSupersedeBasis::compile(&store, &old, &new).unwrap_err();
        assert!(
            matches!(err, SupersedeError::SupersededNotCurrent(_)),
            "got {err:?}"
        );
    }

    /// Compile: successor mainline'da değilse (Candidate) → SuccessorNotCurrent.
    #[test]
    fn presented_supersede_basis_rejects_non_accepted_successor_at_compile() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        store.graph_mut().node_mut(&new).unwrap().decision_status = DecisionStatus::Candidate;
        let err = PresentedSupersedeBasis::compile(&store, &old, &new).unwrap_err();
        assert!(
            matches!(err, SupersedeError::SuccessorNotCurrent(_)),
            "got {err:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Faz 8b (PR #50): SupersedeSession tests — production invocation boundary.
    // Mutlu yol + stale TOCTOU (çift) + endpoint mismatch + store-rejection passthrough
    // (downcast) + close summary (iki varyant) + zincir + candidate-edge preserved +
    // counter exhaustion. Her hata testi: full store snapshot unchanged + counter==0.
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Test yardımcı: SupersedeSession + basis compile (mutlu yol seed).
    fn session_with_basis(
        store: &InMemoryAnchorStore,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
    ) -> (SupersedeSession, PresentedSupersedeBasis) {
        let session = SupersedeSession::open_for_operator(OperatorId::new("test-operator"));
        let basis = PresentedSupersedeBasis::compile(store, superseded, successor).unwrap();
        (session, basis)
    }

    /// Mutlu yol: session.supersede (authority parametresiz) → SupersededAccepted +
    /// committed edge + record seq=1 + authority_level == Operator (internal issuance kanıtı).
    #[test]
    fn supersede_session_creates_committed_edge() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("session supersede").unwrap();

        let record = session
            .supersede(&mut store, &old, &new, basis, reason)
            .expect("supersede");

        // Record: prior Accepted → new SupersededAccepted, authority Operator (internal mint).
        assert_eq!(record.prior_status, DecisionStatus::Accepted);
        assert_eq!(record.new_status, DecisionStatus::SupersededAccepted);
        assert_eq!(record.superseded, old);
        assert_eq!(record.successor, new);
        assert_eq!(record.authority_level, SupersedeAuthorityLevel::Operator);
        assert_eq!(record.seq, 1);

        // Graph: old SupersededAccepted, new Accepted.
        assert_eq!(
            store.graph().node(&old).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&new).unwrap().decision_status,
            DecisionStatus::Accepted
        );

        // Edge: successor → superseded (committed).
        let edges: Vec<_> = store
            .graph()
            .edges()
            .filter(|e| e.kind == ConceptEdgeKind::Supersedes && e.from == new && e.to == old)
            .collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].decision_status, DecisionStatus::Accepted);

        // Ledger + counter.
        assert_eq!(store.supersede_ledger().len(), 1);
        let summary = session.close();
        assert_eq!(summary.supersedes, 1);
    }

    /// Stale superseded basis (TOCTOU): basis derle, superseded canonical'ını değiştir →
    /// StaleSupersededBasis + store snapshot unchanged + counter==0.
    #[test]
    fn supersede_session_stale_superseded_basis_rejects_touctou() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("stale").unwrap();

        // TOCTOU: superseded canonical'ını değiştir.
        store.graph_mut().node_mut(&old).unwrap().canonical = "ChangedAfterBasis".into();

        let before = snapshot_store(&store);
        let err = session
            .supersede(&mut store, &old, &new, basis, reason)
            .unwrap_err();
        assert!(
            matches!(err, SupersedeError::StaleSupersededBasis { .. }),
            "got {err:?}"
        );
        assert_eq!(snapshot_store(&store), before, "store unchanged");
        let summary = session.close();
        assert_eq!(summary.supersedes, 0, "failed attempt counter'ı artırmaz");
    }

    /// Stale successor basis (TOCTOU): successor canonical'ını değiştir → StaleSuccessorBasis.
    #[test]
    fn supersede_session_stale_successor_basis_rejects_touctou() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("stale succ").unwrap();

        store.graph_mut().node_mut(&new).unwrap().canonical = "ChangedSuccessor".into();

        let before = snapshot_store(&store);
        let err = session
            .supersede(&mut store, &old, &new, basis, reason)
            .unwrap_err();
        assert!(
            matches!(err, SupersedeError::StaleSuccessorBasis { .. }),
            "got {err:?}"
        );
        assert_eq!(snapshot_store(&store), before, "store unchanged");
        let summary = session.close();
        assert_eq!(summary.supersedes, 0);
    }

    /// Basis endpoint mismatch: session'a farklı endpoint'ler → SupersedeBasisMismatch.
    #[test]
    fn supersede_session_supersede_basis_mismatch_rejects() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("mismatch").unwrap();

        // Session'a ters endpoint'ler (basis old→new, session new→old).
        let before = snapshot_store(&store);
        let err = session
            .supersede(&mut store, &new, &old, basis, reason)
            .unwrap_err();
        assert!(
            matches!(err, SupersedeError::SupersedeBasisMismatch { .. }),
            "got {err:?}"
        );
        assert_eq!(snapshot_store(&store), before, "store unchanged");
        let summary = session.close();
        assert_eq!(summary.supersedes, 0);
    }

    /// Store-rejection passthrough (Tur 3+4 §3): session'ın kontrol ETMEDİĞİ bir store kuralı
    /// → AlreadySuperseded. Seed committed incoming edge (B→A), session.supersede(A, C) →
    /// session validation geçer, store reject eder → `SupersedeError::Store(...)` yüzeye çıkar
    /// (downcast AlreadySuperseded) + snapshot unchanged + counter==0. Katmanlama iddiası +
    /// Box-sarmalı hata ergonomisi kanıtı.
    #[test]
    fn supersede_session_store_rejection_does_not_increment_summary() {
        let mut store = {
            let mut seed = GraphSeed::default();
            for id in ["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"] {
                seed.rule_candidates
                    .push(node_with(id, DecisionStatus::Accepted));
            }
            InMemoryAnchorStore::with_seed(seed)
        };
        let a = ConceptNodeId("RuleCandidate:A".into());
        let c = ConceptNodeId("RuleCandidate:C".into());

        // A'ya committed Supersedes edge seed'le (B→A, Accepted) — store AlreadySuperseded
        // ateşlenir; session bu kontrolü yapmaz (incoming-edge cardinality store katmanı).
        store.graph_mut().insert_edge(ConceptEdge {
            from: ConceptNodeId("RuleCandidate:B".into()),
            to: a.clone(),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Accepted,
            explanation: Some(NonEmptyExplanation::new("seeded committed").unwrap()),
        });

        let (mut session, basis) = session_with_basis(&store, &a, &c);
        let reason = NonEmptyExplanation::new("should hit store AlreadySuperseded").unwrap();

        let before = snapshot_store(&store);
        let err = session
            .supersede(&mut store, &a, &c, basis, reason)
            .unwrap_err();

        // Downcast: generic Store(_) her hatayı geçirir; AlreadySuperseded türünü doğrula.
        match err {
            SupersedeError::Store(source) => {
                assert!(
                    matches!(
                        source.downcast_ref::<StoreError>(),
                        Some(StoreError::AlreadySuperseded(_))
                    ),
                    "expected wrapped AlreadySuperseded, got {:?}",
                    source
                );
            }
            other => panic!("expected wrapped AlreadySuperseded, got {other:?}"),
        }
        assert_eq!(
            snapshot_store(&store),
            before,
            "store unchanged after store-rejection"
        );
        let summary = session.close();
        assert_eq!(summary.supersedes, 0);
    }

    /// Close summary: başarılı supersede sonrası summary.supersedes == 1.
    #[test]
    fn supersede_session_close_returns_summary() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("ok").unwrap();

        session
            .supersede(&mut store, &old, &new, basis, reason)
            .unwrap();

        let summary = session.close();
        assert_eq!(summary.supersedes, 1);
        assert_eq!(summary.operator.as_str(), "test-operator");
    }

    /// Zero-supersede close summary (Tur 2 küçük): supersede yapmadan close → supersedes == 0.
    #[test]
    fn supersede_session_zero_supersede_close_summary() {
        let session = SupersedeSession::open_for_operator(OperatorId::new("op"));
        let summary = session.close();
        assert_eq!(summary.supersedes, 0);
    }

    /// A→B→C zincir: iki supersede, summary.supersedes == 2, her SupersededAccepted'ın
    /// 1 incoming committed edge (INV-C15 cardinalite).
    #[test]
    fn supersede_session_chain_a_b_c() {
        let mut store = {
            let mut seed = GraphSeed::default();
            for id in ["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"] {
                seed.rule_candidates
                    .push(node_with(id, DecisionStatus::Accepted));
            }
            InMemoryAnchorStore::with_seed(seed)
        };
        let a = ConceptNodeId("RuleCandidate:A".into());
        let b = ConceptNodeId("RuleCandidate:B".into());
        let c = ConceptNodeId("RuleCandidate:C".into());
        let mut session = SupersedeSession::open_for_operator(OperatorId::new("op"));

        // B → A (B A'yı supersede).
        let basis1 = PresentedSupersedeBasis::compile(&store, &a, &b).unwrap();
        session
            .supersede(
                &mut store,
                &a,
                &b,
                basis1,
                NonEmptyExplanation::new("b>a").unwrap(),
            )
            .unwrap();
        // C → B (C B'yi supersede).
        let basis2 = PresentedSupersedeBasis::compile(&store, &b, &c).unwrap();
        session
            .supersede(
                &mut store,
                &b,
                &c,
                basis2,
                NonEmptyExplanation::new("c>b").unwrap(),
            )
            .unwrap();

        // Statüler.
        assert_eq!(
            store.graph().node(&a).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&b).unwrap().decision_status,
            DecisionStatus::SupersededAccepted
        );
        assert_eq!(
            store.graph().node(&c).unwrap().decision_status,
            DecisionStatus::Accepted
        );

        // INV-C15 cardinalite: her SupersededAccepted'ın 1 incoming committed edge.
        let sup_edges: Vec<_> = store
            .graph()
            .edges()
            .filter(|e| {
                e.kind == ConceptEdgeKind::Supersedes
                    && e.decision_status == DecisionStatus::Accepted
            })
            .collect();
        assert_eq!(sup_edges.iter().filter(|e| e.to == a).count(), 1);
        assert_eq!(sup_edges.iter().filter(|e| e.to == b).count(), 1);

        let summary = session.close();
        assert_eq!(summary.supersedes, 2);
    }

    /// Candidate proposal edge preserved (opsiyon (a) lock): seed'li Candidate Supersedes
    /// edge, başarılı session supersede sonrası hâlâ orada (coexistence; lane-sensitive).
    #[test]
    fn supersede_session_candidate_edge_preserved() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());

        // Candidate Supersedes edge seed'le (apply_plan proposal simülasyonu).
        store.graph_mut().insert_edge(ConceptEdge {
            from: new.clone(),
            to: old.clone(),
            kind: ConceptEdgeKind::Supersedes,
            decision_status: DecisionStatus::Candidate,
            explanation: Some(NonEmptyExplanation::new("candidate proposal").unwrap()),
        });

        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("committed supersede").unwrap();
        session
            .supersede(&mut store, &old, &new, basis, reason)
            .expect("Candidate edge engellemedi");

        // Post-state: candidate edge hâlâ orada + committed edge de var (coexistence).
        let sup_edges: Vec<_> = store
            .graph()
            .edges()
            .filter(|e| e.kind == ConceptEdgeKind::Supersedes && e.from == new && e.to == old)
            .collect();
        assert_eq!(
            sup_edges
                .iter()
                .filter(|e| e.decision_status == DecisionStatus::Candidate)
                .count(),
            1,
            "candidate proposal edge preserved"
        );
        assert_eq!(
            sup_edges
                .iter()
                .filter(|e| e.decision_status == DecisionStatus::Accepted)
                .count(),
            1,
            "committed lineage edge appended"
        );
    }

    /// Counter exhaustion: supersedes=u64::MAX set → SessionCounterExhausted + store unchanged.
    /// checked_add overflow mutation öncesi yakalanır; counter yalnız başarılı store op sonrası assign edilir.
    #[test]
    fn supersede_session_counter_exhaustion_leaves_store_unchanged() {
        let mut store = store_with_two_accepted("RuleCandidate:Old", "RuleCandidate:New");
        let old = ConceptNodeId("RuleCandidate:Old".into());
        let new = ConceptNodeId("RuleCandidate:New".into());
        let (mut session, basis) = session_with_basis(&store, &old, &new);
        let reason = NonEmptyExplanation::new("exhausted").unwrap();

        // Test modülü aynı dosyada olduğundan private field'a doğrudan erişilir.
        session.supersedes = u64::MAX;

        let before = snapshot_store(&store);
        let err = session
            .supersede(&mut store, &old, &new, basis, reason)
            .unwrap_err();
        assert!(
            matches!(err, SupersedeError::SessionCounterExhausted),
            "got {err:?}"
        );
        assert_eq!(snapshot_store(&store), before, "store unchanged");
        assert_eq!(session.supersedes, u64::MAX, "counter unchanged on error");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // PR E — CodeEntityResolutionSession unit tests
    // ═══════════════════════════════════════════════════════════════════════════

    use crate::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};

    /// Accepted CodeEntityCandidate node (PhysicalCode family).
    fn accepted_code_entity_candidate(path: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("CodeEntityCandidate:{path}")),
            canonical: path.into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntityCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        }
    }

    /// Store with Accepted CodeEntityCandidate + identity binding seed'li.
    fn store_with_resolvable_candidate(path: &str) -> InMemoryAnchorStore {
        let node = accepted_code_entity_candidate(path);
        let mut seed = GraphSeed::default();
        seed.code_entities.push(node);
        let mut store = InMemoryAnchorStore::with_seed(seed);
        // Identity binding bootstrap.
        let key = CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
            },
            path,
        )
        .unwrap();
        let binding = crate::anchoring::types::CodeIdentityBinding {
            node_id: ConceptNodeId(format!("CodeEntityCandidate:{path}")),
            identity_key: key,
        };
        use crate::anchoring::store::AnchorStore;
        store
            .seed_code_identity_bindings_trusted(&[binding])
            .unwrap();
        store
    }

    fn insensitive_key(path: &str) -> CodeIdentityKey {
        CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
            },
            path,
        )
        .unwrap()
    }

    /// Happy path: Accepted candidate resolves → Created CodeEntity.
    #[test]
    fn accepted_candidate_resolves_to_newly_created_entity() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let record = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("resolution test").unwrap(),
            )
            .unwrap();
        // Outcome Created.
        assert!(matches!(
            record.outcome,
            crate::anchoring::review::ResolutionOutcome::Created { .. }
        ));
        // Entity node oluşturuldu (Candidate status).
        let entity_id = record.entity_id.clone();
        let entity = store.graph().node(&entity_id).expect("entity created");
        assert_eq!(entity.node_kind, ConceptNodeKind::CodeEntity);
        assert_eq!(entity.decision_status, DecisionStatus::Candidate);
    }

    #[test]
    fn entity_initial_status_pinned_candidate() {
        let mut store = store_with_resolvable_candidate("src/x.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/x.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let record = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test").unwrap(),
            )
            .unwrap();
        let entity = store.graph().node(&record.entity_id).unwrap();
        assert_eq!(
            entity.decision_status,
            DecisionStatus::Candidate,
            "entity initial = Candidate (otomatik mainline'a alınmaz)"
        );
    }

    #[test]
    fn entity_material_deterministic_from_key() {
        // tur 3 P2-C: canonical = key.canonical_key(), aliases = [].
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let record = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test").unwrap(),
            )
            .unwrap();
        let entity = store.graph().node(&record.entity_id).unwrap();
        assert_eq!(entity.canonical, "src/auth.rs", "canonical = key.canonical_key()");
        assert!(entity.aliases.is_empty(), "aliases = []");
    }

    #[test]
    fn resolves_to_edge_accepted_with_explanation() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let record = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("resolution reason").unwrap(),
            )
            .unwrap();
        // Committed ResolvesTo edge (Accepted + explanation).
        let edge = store
            .graph()
            .edges()
            .find(|e| {
                e.kind == crate::anchoring::ConceptEdgeKind::ResolvesTo
                    && e.from == candidate_id
                    && e.to == record.entity_id
            })
            .expect("ResolvesTo edge mevcut");
        assert_eq!(edge.decision_status, DecisionStatus::Accepted);
        assert!(edge.explanation.is_some(), "INV-C7 explanation");
    }

    #[test]
    fn record_outcome_created() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let record = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test").unwrap(),
            )
            .unwrap();
        assert!(matches!(
            record.outcome,
            crate::anchoring::review::ResolutionOutcome::Created { entity_id: _ }
        ));
    }

    /// N:1 reuse: ikinci candidate aynı key → mevcut entity'ye resolve.
    #[test]
    fn second_candidate_same_key_resolves_to_existing_entity() {
        // İlk candidate resolve → Created entity.
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate1 = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis1 = PresentedResolutionBasis::compile(&store, &candidate1).unwrap();
        let record1 = session
            .resolve(
                &mut store,
                &candidate1,
                basis1,
                NonEmptyExplanation::new("first").unwrap(),
            )
            .unwrap();
        let entity_id = record1.entity_id.clone();

        // İkinci candidate (farklı path, aynı identity key) seed + resolve → Reused.
        let candidate2 = accepted_code_entity_candidate("src/Auth.rs"); // case-insensitive aynı key
        let mut seed2 = GraphSeed::default();
        seed2.code_entities.push(candidate2);
        store.seed_trusted(&seed2).unwrap();
        let binding2 = crate::anchoring::types::CodeIdentityBinding {
            node_id: ConceptNodeId("CodeEntityCandidate:src/Auth.rs".into()),
            identity_key: insensitive_key("src/auth.rs"),
        };
        store.seed_code_identity_bindings_trusted(&[binding2]).unwrap();
        let candidate2_id = ConceptNodeId("CodeEntityCandidate:src/Auth.rs".into());
        let basis2 = PresentedResolutionBasis::compile(&store, &candidate2_id).unwrap();
        let record2 = session
            .resolve(
                &mut store,
                &candidate2_id,
                basis2,
                NonEmptyExplanation::new("second").unwrap(),
            )
            .unwrap();
        // Outcome Reused → aynı entity.
        assert!(matches!(
            record2.outcome,
            crate::anchoring::review::ResolutionOutcome::Reused { entity_id: _ }
        ));
        assert_eq!(record2.entity_id, entity_id, "reuse → aynı entity ID");
    }

    #[test]
    fn no_duplicate_entity_created_on_reuse() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate1 = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis1 = PresentedResolutionBasis::compile(&store, &candidate1).unwrap();
        session
            .resolve(
                &mut store,
                &candidate1,
                basis1,
                NonEmptyExplanation::new("first").unwrap(),
            )
            .unwrap();
        let entity_count_before: usize = store
            .graph()
            .nodes_iter()
            .filter(|n| n.node_kind == ConceptNodeKind::CodeEntity)
            .count();
        // İkinci candidate reuse.
        let candidate2 = accepted_code_entity_candidate("src/Auth.rs");
        let mut seed2 = GraphSeed::default();
        seed2.code_entities.push(candidate2);
        store.seed_trusted(&seed2).unwrap();
        store
            .seed_code_identity_bindings_trusted(&[crate::anchoring::types::CodeIdentityBinding {
                node_id: ConceptNodeId("CodeEntityCandidate:src/Auth.rs".into()),
                identity_key: insensitive_key("src/auth.rs"),
            }])
            .unwrap();
        let candidate2_id = ConceptNodeId("CodeEntityCandidate:src/Auth.rs".into());
        let basis2 = PresentedResolutionBasis::compile(&store, &candidate2_id).unwrap();
        session
            .resolve(
                &mut store,
                &candidate2_id,
                basis2,
                NonEmptyExplanation::new("second").unwrap(),
            )
            .unwrap();
        let entity_count_after: usize = store
            .graph()
            .nodes_iter()
            .filter(|n| n.node_kind == ConceptNodeKind::CodeEntity)
            .count();
        assert_eq!(
            entity_count_before, entity_count_after,
            "reuse → yeni entity YOK"
        );
    }

    #[test]
    fn candidate_not_accepted_rejects() {
        // Candidate node (Candidate status, Accepted değil).
        let mut store = InMemoryAnchorStore::new();
        let candidate = ConceptNode {
            id: ConceptNodeId("CodeEntityCandidate:c.rs".into()),
            canonical: "c.rs".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntityCandidate,
            decision_status: DecisionStatus::Candidate, // Accepted değil
            position_family: PositionFamily::PhysicalCode,
        };
        let mut seed = GraphSeed::default();
        seed.code_entities.push(candidate);
        store.seed_trusted(&seed).unwrap();
        store
            .seed_code_identity_bindings_trusted(&[crate::anchoring::types::CodeIdentityBinding {
                node_id: ConceptNodeId("CodeEntityCandidate:c.rs".into()),
                identity_key: insensitive_key("c.rs"),
            }])
            .unwrap();
        let candidate_id = ConceptNodeId("CodeEntityCandidate:c.rs".into());
        // compile Accepted olmadığı için basis view error verir (NotPromotableFrom store'dan).
        let err = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap_err();
        // Store hatası sarmalanmış gelir (ResolutionError::Store).
        assert!(
            matches!(err, ResolutionError::Store(_)),
            "Candidate status compile reject — got {err:?}"
        );
    }

    #[test]
    fn already_resolved_candidate_rejects() {
        // R6: candidate başına ≤1 outgoing ResolvesTo.
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        // İlk resolve → Created.
        let basis1 = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        session
            .resolve(
                &mut store,
                &candidate_id,
                basis1,
                NonEmptyExplanation::new("first").unwrap(),
            )
            .unwrap();
        // İkinci resolve → R6 AlreadyResolved (store step 8; basis compile Reuse üretir ama
        // apply_resolution step 8 outgoing ResolvesTo var → reject).
        let basis2 = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let err = session
            .resolve(
                &mut store,
                &candidate_id,
                basis2,
                NonEmptyExplanation::new("second").unwrap(),
            )
            .unwrap_err();
        // Store-level AlreadyResolved → ResolutionError::Store sarmalı.
        assert!(
            matches!(err, ResolutionError::Store(_)),
            "R6 already resolved reject — got {err:?}"
        );
    }

    #[test]
    fn every_failure_leaves_store_unchanged() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        // Stale basis: basis compile et, sonra node canonical değiştir → digest mismatch.
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        store.graph_mut().node_mut(&candidate_id).unwrap().canonical = "changed.rs".into();
        let before = store.node_count().unwrap() + store.edge_count().unwrap();
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let _err = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test").unwrap(),
            )
            .unwrap_err();
        let after = store.node_count().unwrap() + store.edge_count().unwrap();
        assert_eq!(before, after, "failure leaves graph unchanged");
    }

    #[test]
    fn seed_bindings_rejects_wrong_kind() {
        let mut store = InMemoryAnchorStore::new();
        // Concept node (CodeEntityCandidate/CodeEntity değil).
        let node = ConceptNode {
            id: ConceptNodeId("Concept:X".into()),
            canonical: "X".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.concepts.push(node);
        store.seed_trusted(&seed).unwrap();
        let err = store
            .seed_code_identity_bindings_trusted(&[crate::anchoring::types::CodeIdentityBinding {
                node_id: ConceptNodeId("Concept:X".into()),
                identity_key: insensitive_key("x"),
            }])
            .unwrap_err();
        assert!(matches!(
            err,
            crate::anchoring::store::StoreError::BindingWrongKind { .. }
        ));
    }

    #[test]
    fn seed_bindings_rejects_duplicate_binding() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        // Aynı node'ya ikinci binding → duplicate.
        let err = store
            .seed_code_identity_bindings_trusted(&[crate::anchoring::types::CodeIdentityBinding {
                node_id: ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
                identity_key: insensitive_key("src/auth.rs"),
            }])
            .unwrap_err();
        assert!(matches!(
            err,
            crate::anchoring::store::StoreError::DuplicateBinding(_)
        ));
    }

    /// Snapshot restore validation: resolution records + bindings INV-C16 triangulation.
    #[test]
    fn snapshot_restore_validates_resolution_triangulation() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test").unwrap(),
            )
            .unwrap();
        // Export → restore → validation geçerli.
        let snapshot = store.export_snapshot();
        let restored = InMemoryAnchorStore::restore_snapshot(snapshot).expect("restore valid");
        assert_eq!(
            restored.resolution_ledger().len(),
            1,
            "resolution record restore edildi"
        );
    }

    #[test]
    fn deterministic_export_ordering() {
        let mut store = store_with_resolvable_candidate("src/auth.rs");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test").unwrap(),
            )
            .unwrap();
        let snap1 = store.export_snapshot();
        let snap2 = store.export_snapshot();
        assert_eq!(snap1, snap2, "deterministic export");
        // code_identity_bindings sorted by node_id.
        let binding_ids: Vec<_> = snap1
            .code_identity_bindings
            .iter()
            .map(|b| &b.node_id.0)
            .collect();
        let mut sorted = binding_ids.clone();
        sorted.sort();
        assert_eq!(binding_ids, sorted, "bindings sorted by node_id");
    }
}
