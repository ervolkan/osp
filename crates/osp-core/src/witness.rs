//! Şahitlik zinciri — Evidence → WitnessSet → Status → Result (§4).
//!
//! **Faz 1.5:** Workflow-agnostik witness modeli.
//! - **inv #1** author-witness rejection → `canonicalize_for(author)` author'ı çıkarır
//! - **inv #2** EvidenceEvent dedup → `(source, actor, claim)` HashMap, max-weight
//! - **inv #3** tri-state `WitnessStatus::{Witnessed, Unwitnessed, UnobservableLocally}`
//! - **inv #9** WitnessSet tabanlı `W(C, Ω)` → `evaluate(claim, &WitnessSet) -> WitnessResult`
//!
//! `CanonicalWitnessSet` (parse-don't-validate): dedup + author-filter yapısal garanti.
//! `support()` / `approver_count()` yalnızca canonicalized set üzerinde → "dedup'i unutma" imkânsız.

use crate::coords::RawPosition;
use crate::space::{Edge, Node};

// ═══════════════════════════════════════════════════════════════════════════════
// Tanımlayıcılar
// ═══════════════════════════════════════════════════════════════════════════════

pub type AgentId = u64;
pub type ClaimId = u64;
pub type EvidenceId = u64;
/// Kanıt kaynağı: `"PR #42"`, `"<commit-sha>"`, `"trailer:Reviewed-by:alice"`.
pub type EvidenceSource = String;

// ═══════════════════════════════════════════════════════════════════════════════
// WitnessKind + ağırlıklar (§4.1 KARAR; kalibrasyon Faz 1.11)
// ═══════════════════════════════════════════════════════════════════════════════

/// Şahit türü. Ağırlık = güven seviyesi.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum WitnessKind {
    /// `git rev-list --merges` — kriptografik (imzalı merge). En güçlü.
    #[default]
    MergeCommit,
    /// `gh pr list --state merged` veya merge-imza metni — GitHub API + review-required.
    PRMerged,
    /// `Reviewed-by:` trailer'ı — imzalı ama daha zayıf.
    TrailerReviewed,
    /// `Co-authored-by:` trailer'ı — katkı, review değil. En zayıf.
    CoAuthored,
}

impl WitnessKind {
    /// §4.1 başlangıç ağırlıkları (kalibrasyon: Faz 1.11, 15-20 repo korpusu).
    pub fn default_weight(&self) -> f64 {
        match self {
            Self::MergeCommit => 1.0,
            Self::PRMerged => 0.8,
            Self::TrailerReviewed => 0.7,
            Self::CoAuthored => 0.4,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EvidenceEvent (inv #2 dedup birimi)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek gözlemlenen kanıt. Pozitif evidence (approval). Dedup anahtarı:
/// `(source, actor, claim)` üçlüsü (inv #2).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceEvent {
    pub id: EvidenceId,
    pub source: EvidenceSource,
    pub witness_kind: WitnessKind,
    pub actor: AgentId,
    pub claim: ClaimId,
    pub weight: f64,
}

impl EvidenceEvent {
    pub fn new(
        id: EvidenceId,
        source: impl Into<EvidenceSource>,
        kind: WitnessKind,
        actor: AgentId,
        claim: ClaimId,
    ) -> Self {
        Self {
            id,
            source: source.into(),
            weight: kind.default_weight(),
            witness_kind: kind,
            actor,
            claim,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// WitnessSet (raw — ham kanıtlar) + canonicalize_for
// ═══════════════════════════════════════════════════════════════════════════════

/// W(C, Ω)'nın Ω'sı. Ham kanıt kümesi.
///
/// **DİKKAT:** `support()` / approver sayısı doğrudan HESAPLANMAZ — önce
/// `canonicalize_for(author)` ile `CanonicalWitnessSet`'e çevir (inv #1 + #2 yapısal).
#[derive(Debug, Clone, Default)]
pub struct WitnessSet {
    pub events: Vec<EvidenceEvent>,
    pub min_approvers: usize,
    pub quorum_threshold: f64,
}

impl WitnessSet {
    /// Default: `min_approvers=2`, `quorum_threshold=1.5` (§4.2).
    pub fn new(events: Vec<EvidenceEvent>) -> Self {
        Self {
            events,
            min_approvers: 2,
            quorum_threshold: 1.5,
        }
    }

    /// Builder: quorum parametrelerini override et (kalibrasyon, Faz 1.11).
    pub fn with_quorum(mut self, min_approvers: usize, quorum_threshold: f64) -> Self {
        self.min_approvers = min_approvers;
        self.quorum_threshold = quorum_threshold;
        self
    }

    /// **inv #1 + #2 — tek giriş noktası.** Author'ı çıkar + `(source, actor, claim)`
    /// dedup (en güçlü kalır) → `CanonicalWitnessSet`.
    ///
    /// "dedup'i unutma" hatası imkânsız: `support()`/`approver_count()` sadece
    /// `CanonicalWitnessSet`'te, o da sadece bu metodla oluşur.
    pub fn canonicalize_for(&self, author: AgentId) -> CanonicalWitnessSet {
        use std::collections::HashMap;
        let mut deduped: HashMap<(EvidenceSource, AgentId, ClaimId), EvidenceEvent> =
            HashMap::new();
        for e in &self.events {
            if e.actor == author {
                continue; // inv #1 — author witness rejection
            }
            let key = (e.source.clone(), e.actor, e.claim);
            match deduped.get(&key) {
                Some(existing) if existing.weight >= e.weight => {} // inv #2 — strongest stays
                _ => {
                    deduped.insert(key, e.clone());
                }
            }
        }
        let mut events: Vec<EvidenceEvent> = deduped.into_values().collect();
        events.sort_by_key(|e| e.id); // stable ordering for deterministic tests
        CanonicalWitnessSet {
            events,
            min_approvers: self.min_approvers,
            quorum_threshold: self.quorum_threshold,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CanonicalWitnessSet (dedup'lı + author-filtered — yapısal garanti)
// ═══════════════════════════════════════════════════════════════════════════════

/// Dedup'lı + author-filtered witness set. Sadece `WitnessSet::canonicalize_for` ile
/// oluşur → inv #1 (author excluded) ve inv #2 (dedup) **yapısal garantili**
/// (runtime-check değil, type-level).
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalWitnessSet {
    events: Vec<EvidenceEvent>,
    min_approvers: usize,
    quorum_threshold: f64,
}

impl CanonicalWitnessSet {
    /// Q1: distinct non-author approver sayısı.
    pub fn approver_count(&self) -> usize {
        self.events.len()
    }

    /// Q2: Σ weight (zaten dedup'lı — ekstra çağrı gerekmez).
    pub fn support(&self) -> f64 {
        self.events.iter().map(|e| e.weight).sum()
    }

    pub fn min_approvers(&self) -> usize {
        self.min_approvers
    }
    pub fn quorum_threshold(&self) -> f64 {
        self.quorum_threshold
    }
    pub fn events(&self) -> &[EvidenceEvent] {
        &self.events
    }
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// WitnessStatus (inv #3 tri-state)
// ═══════════════════════════════════════════════════════════════════════════════

/// Epistemolojik üçlü durum. "Görünmüyor" ≠ "yok" (inv #3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessStatus {
    /// Lokalde dedup'lı `support ≥ θ_quorum` — nesnel uzaya kabul için yeterli.
    Witnessed,
    /// Lokalde observable ama quorum yok — foam sinyali.
    Unwitnessed,
    /// Squash/rebase + trailersız — lokalde karar verilemez; provider API opsiyonel.
    UnobservableLocally,
}

// ═══════════════════════════════════════════════════════════════════════════════
// WitnessResult + Reason (inv #7, #9)
// ═══════════════════════════════════════════════════════════════════════════════

/// W(C, Ω) sonucu. Sadece witness-based Q1-Q3 karar.
///
/// Claim-based Q4-Q6 gate'ler `SpaceEngine::commit()` Phase 0'da evaluate()'den ÖNCE koşar
/// (space-engine-design.md §4, osp-core-design.md §3.2). Bu yüzden evaluate() yalnızca
/// Q1-Q3'ü kontrol eder — Claim'in Q4-Q6'yı geçtiği varsayılır.
#[derive(Debug, Clone, PartialEq)]
pub enum WitnessResult {
    /// Quorum + vision-bound sağlandı → uzay genişler (§6).
    Commit {
        delta: crate::bigbang::Delta,
        /// inv #7 — admin override kullanıldıysa true (asla sessiz).
        safety_weakened: bool,
        override_reason: Option<String>,
    },
    /// Q3: honest-reject (witness-based).
    Reject(Reason),
    /// Q1/Q2 yetersiz — beklemeye alınır.
    Hold(Reason),
}

/// Sadece witness-based gate failure'ları (Q1-Q3). Claim-based Q4-Q6 failure'ları
/// `EngineCommitError`'da (engine.rs §6.1) — burada tekrar tanımlanmaz
/// (single-source-of-truth, duplication drift risk — osp-core-design.md §3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// Q2: `support < θ_quorum`.
    QuorumInsufficient { support: f64, threshold: f64 },
    /// Q1: `distinct_non_author_approvers < min_approvers`.
    MinApproversNotMet { distinct: usize, required: usize },
    /// Q3: honest witness reddetti (Faz 1.7+ explicit-reject sinyalleri).
    HonestReject { witness: AgentId },
    /// inv #3 tri-state — lokalde gözlemlenemeyen (squash/rebase + trailersız).
    UnobservableLocally { hint: String },
}

// ═══════════════════════════════════════════════════════════════════════════════
// Intent + Claim (witness-domain; space'e NodeId ile bağlı)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agent'a verilen görev. **`t_f` (Gelecek) katmanında yaşar** (potansiyel gradyan —
/// agent-prompt-semantics.md §0 ontolojik harita, OSP-formalism.md §1.2 + §3.1).
///
/// Intent uzayı mutasyona uğratmaz; `t_f`'den `t_m`'ye gradyan `OspPrompt` projeksiyonu
/// üzerinden yayılır (Faz 5).
///
/// **Invariant (yapısal):** `time_layer` private + `#[serde(skip)]` → her zaman `Gelecek`.
/// Struct literal bypass imkânsız (field private); `Intent::new()` tek constructor.
/// Serde deserialize'da da `default_time_layer_gelecek()` ile Gelecek set edilir.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Intent {
    pub agent: AgentId,
    /// Raw hedef konum — derived değil (inv #4).
    pub target_raw: RawPosition,
    #[serde(skip, default = "default_time_layer_gelecek")]
    time_layer: crate::space::TimeLayer,
}

fn default_time_layer_gelecek() -> crate::space::TimeLayer {
    crate::space::TimeLayer::Gelecek
}

impl Intent {
    /// Intent oluştur — time_layer her zaman Gelecek (constructor invariant).
    pub fn new(agent: AgentId, target_raw: RawPosition) -> Self {
        Self {
            agent,
            target_raw,
            time_layer: crate::space::TimeLayer::Gelecek,
        }
    }

    /// Zaman katmanı — her zaman `TimeLayer::Gelecek` (yapısal invariant).
    pub fn time_layer(&self) -> crate::space::TimeLayer {
        self.time_layer
    }
}

/// Agent'ın ürettiği iş (PR). `t_m`'de Belief → `t_c`'de Knowledge (witness sonrası).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub intent: Intent,
    /// inv #1 — evaluate'de approvers'tan çıkarılır.
    pub author: AgentId,
    /// Engine tarafından DeltaProposal'dan **compute edilmiş** raw position.
    /// LLM tarafından declare EDİLMEZ (inv #4, agent-prompt-semantics.md §2.2) —
    /// coord_system/analyzer'ın ΔS'i uygulayıp node'ları yeniden ölçmesinin sonucu.
    pub computed_raw: RawPosition,
    pub delta_nodes: Vec<Node>,
    pub delta_edges: Vec<Edge>,
    /// INV-T5 — Trajectory binding. `None` = standalone claim (Paper 1 static flow,
    /// legacy, baseline analiz). `Some(id)` = trajectory-bound (Q5.b Predicate Gate
    /// bu claim'i değerlendirir). `#[serde(default)]` ile backward-compat — eski
    /// snapshot'lar (task_id yok) None ile deserialize olur.
    ///
    /// **Q5.b kuralı:** çıplak Claim ile çalışmaz; `bind_task_claim()` ile
    /// `TaskBoundClaim`'e dönüştürülmeli (INV-T5 — static Claim taskless olabilir,
    /// ama Q5.b sadece TaskBoundClaim kabul eder).
    #[serde(default)]
    pub task_id: Option<crate::trajectory::TaskId>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// evaluate — W(C, Ω) pure karar fonksiyonu (inv #9)
// ═══════════════════════════════════════════════════════════════════════════════

/// W(C, Ω) — pure karar (mutasyon yok). Sadece witness-based Q1-Q3.
/// Claim-based Q4-Q6 (`SpaceEngine::commit()` Phase 0) zaten geçti varsayılır.
///
/// Faz 1.5: Q1 (min_approvers) + Q2 (quorum) uygular. Q3 (honest-reject) Faz 1.7+'te
/// explicit sinyallerle gelir; o zamana kadar Q1+Q2 geçer → Commit.
pub fn evaluate(claim: &Claim, omega: &WitnessSet) -> WitnessResult {
    let canon = omega.canonicalize_for(claim.author);

    // Q1: distinct non-author approvers
    if canon.approver_count() < canon.min_approvers() {
        return WitnessResult::Hold(Reason::MinApproversNotMet {
            distinct: canon.approver_count(),
            required: canon.min_approvers(),
        });
    }

    // Q2: support >= quorum
    let support = canon.support();
    if support < canon.quorum_threshold() {
        return WitnessResult::Hold(Reason::QuorumInsufficient {
            support,
            threshold: canon.quorum_threshold(),
        });
    }

    // Q3 (honest-reject): Faz 1.7+ — sinyal yok, skip.
    // (Q4-Q6 claim-based — engine seviyesinde, evaluate()'den önce kontrol edildi)

    // Commit — prospective delta (gerçek mutasyon Faz 1.8 commit() içinde).
    let delta = crate::bigbang::Delta {
        new_nodes: claim.delta_nodes.clone(), // Faz 2.4: full Node objects (replay için)
        new_edges: claim.delta_edges.clone(),
        repositioned: vec![], // Faz 1.8: ΔV ∪ N₁(ΔV) hesaplar
    };

    WitnessResult::Commit {
        delta,
        safety_weakened: false,
        override_reason: None,
    }
}

/// Repo/claim seviyesinde tri-state sınıflama (inv #3).
///
/// Faz 1.10 re-spike'ta `osp-spike` `WitnessProfile`'ından beslenir. `locally_observable=false`
/// (`squash/rebase + trailersız`) → `UnobservableLocally` (`Unwitnessed` ile karıştırma).
pub fn classify_status(
    support: f64,
    quorum_threshold: f64,
    locally_observable: bool,
) -> WitnessStatus {
    if !locally_observable {
        WitnessStatus::UnobservableLocally
    } else if support >= quorum_threshold {
        WitnessStatus::Witnessed
    } else {
        WitnessStatus::Unwitnessed
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::RawPosition;

    fn claim_with_author(author: AgentId) -> Claim {
        Claim {
            id: 1,
            intent: Intent::new(author, RawPosition::default()),
            author,
            computed_raw: RawPosition::default(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id: None, // standalone (Paper 1 static flow, INV-T5)
        }
    }

    fn ev(id: EvidenceId, source: &str, kind: WitnessKind, actor: AgentId) -> EvidenceEvent {
        EvidenceEvent::new(id, source, kind, actor, 1)
    }

    // --- inv #1: author exclusion ---

    #[test]
    fn canonicalize_excludes_author_events() {
        let omega = WitnessSet::new(vec![
            ev(1, "PR#1", WitnessKind::MergeCommit, 200),
            ev(2, "PR#2", WitnessKind::MergeCommit, 100), // author!
            ev(3, "PR#3", WitnessKind::MergeCommit, 300),
        ]);
        let canon = omega.canonicalize_for(100);
        // author (100) çıkarıldı → 2 approver (200, 300)
        assert_eq!(canon.approver_count(), 2);
        assert!(!canon.events().iter().any(|e| e.actor == 100));
    }

    #[test]
    fn evaluate_author_self_approval_ignored() {
        // author 100 iki self-event → 0 distinct non-author → Hold
        let claim = claim_with_author(100);
        let omega = WitnessSet::new(vec![
            ev(1, "PR#1", WitnessKind::MergeCommit, 100),
            ev(2, "PR#2", WitnessKind::MergeCommit, 100),
        ]);
        let result = evaluate(&claim, &omega);
        assert!(matches!(
            result,
            WitnessResult::Hold(Reason::MinApproversNotMet { distinct: 0, .. })
        ));
    }

    // --- inv #2: dedup ---

    #[test]
    fn dedup_keeps_strongest_per_key() {
        // Aynı (source, actor, claim) — MergeCommit (1.0) vs CoAuthored (0.4) → 1.0 kalmalı
        let omega = WitnessSet::new(vec![
            ev(1, "PR#42", WitnessKind::MergeCommit, 200),
            ev(2, "PR#42", WitnessKind::CoAuthored, 200), // same source+actor+claim
        ]);
        let canon = omega.canonicalize_for(999); // author yok
        assert_eq!(canon.approver_count(), 1, "aynı key → 1 event");
        assert!(
            (canon.support() - 1.0).abs() < 1e-9,
            "strongest (1.0) kalmalı"
        );
    }

    #[test]
    fn dedup_different_source_kept() {
        // Farklı source → distinct evidence → ikisi de kalır
        let omega = WitnessSet::new(vec![
            ev(1, "PR#42", WitnessKind::PRMerged, 200),
            ev(
                2,
                "trailer: Reviewed-by:200",
                WitnessKind::TrailerReviewed,
                200,
            ),
        ]);
        let canon = omega.canonicalize_for(999);
        assert_eq!(canon.approver_count(), 2, "farklı source → 2 event");
    }

    #[test]
    fn dedup_different_actor_kept() {
        // Aynı PR, farklı approver → ikisi de kalır (iki kişi)
        let omega = WitnessSet::new(vec![
            ev(1, "PR#42", WitnessKind::MergeCommit, 200),
            ev(2, "PR#42", WitnessKind::MergeCommit, 300),
        ]);
        let canon = omega.canonicalize_for(999);
        assert_eq!(canon.approver_count(), 2);
    }

    // --- inv #9: evaluate quorum combinations ---

    #[test]
    fn evaluate_two_strong_witnesses_commit() {
        let claim = claim_with_author(100);
        let omega = WitnessSet::new(vec![
            ev(1, "PR#1", WitnessKind::MergeCommit, 200),
            ev(2, "PR#2", WitnessKind::MergeCommit, 300),
        ]);
        // support = 2.0 ≥ 1.5, 2 distinct ≥ 2 → Commit
        let result = evaluate(&claim, &omega);
        assert!(matches!(
            result,
            WitnessResult::Commit {
                safety_weakened: false,
                ..
            }
        ));
    }

    #[test]
    fn evaluate_three_weak_witnesses_commit() {
        // 3 × TrailerReviewed = 2.1 ≥ 1.5, 3 distinct ≥ 2 → Commit
        let claim = claim_with_author(100);
        let omega = WitnessSet::new(vec![
            ev(1, "t1", WitnessKind::TrailerReviewed, 200),
            ev(2, "t2", WitnessKind::TrailerReviewed, 300),
            ev(3, "t3", WitnessKind::TrailerReviewed, 400),
        ]);
        let result = evaluate(&claim, &omega);
        assert!(matches!(result, WitnessResult::Commit { .. }));
    }

    #[test]
    fn evaluate_single_strong_witness_holds_on_min_approvers() {
        // 1 MergeCommit: support=1.0 ama distinct=1 < 2 → Hold (Q1 önce)
        let claim = claim_with_author(100);
        let omega = WitnessSet::new(vec![ev(1, "PR#1", WitnessKind::MergeCommit, 200)]);
        let result = evaluate(&claim, &omega);
        assert!(matches!(
            result,
            WitnessResult::Hold(Reason::MinApproversNotMet {
                distinct: 1,
                required: 2
            })
        ));
    }

    #[test]
    fn evaluate_quorum_insufficient_hold() {
        // 2 CoAuthored = 0.8 < 1.5 → Hold (Q2). distinct=2 ≥ 2 (Q1 geçer).
        let claim = claim_with_author(100);
        let omega = WitnessSet::new(vec![
            ev(1, "c1", WitnessKind::CoAuthored, 200),
            ev(2, "c2", WitnessKind::CoAuthored, 300),
        ]);
        let result = evaluate(&claim, &omega);
        assert!(matches!(
            result,
            WitnessResult::Hold(Reason::QuorumInsufficient { support, threshold })
            if (support - 0.8).abs() < 1e-9 && (threshold - 1.5).abs() < 1e-9
        ));
    }

    #[test]
    fn evaluate_empty_set_holds() {
        let claim = claim_with_author(100);
        let omega = WitnessSet::new(vec![]);
        let result = evaluate(&claim, &omega);
        assert!(matches!(result, WitnessResult::Hold(_)));
    }

    // --- custom quorum (kalibrasyon) ---

    #[test]
    fn evaluate_custom_quorum() {
        // min_approvers=1, quorum=1.0 → tek MergeCommit Commit eder
        let claim = claim_with_author(100);
        let omega =
            WitnessSet::new(vec![ev(1, "PR#1", WitnessKind::MergeCommit, 200)]).with_quorum(1, 1.0);
        let result = evaluate(&claim, &omega);
        assert!(matches!(result, WitnessResult::Commit { .. }));
    }

    // --- commit delta prospective ---

    #[test]
    fn evaluate_commit_carries_prospective_delta() {
        let mut claim = claim_with_author(100);
        claim.delta_nodes = vec![Node {
            id: 42,
            ..Default::default()
        }];
        let omega = WitnessSet::new(vec![
            ev(1, "PR#1", WitnessKind::MergeCommit, 200),
            ev(2, "PR#2", WitnessKind::MergeCommit, 300),
        ]);
        if let WitnessResult::Commit { delta, .. } = evaluate(&claim, &omega) {
            assert_eq!(delta.new_nodes.len(), 1);
            assert_eq!(delta.new_nodes[0].id, 42);
        } else {
            panic!("Commit bekleniyordu");
        }
    }

    // --- inv #3: tri-state classify ---

    #[test]
    fn classify_witnessed_when_observable_and_quorum_met() {
        assert_eq!(classify_status(2.0, 1.5, true), WitnessStatus::Witnessed);
    }

    #[test]
    fn classify_unwitnessed_when_observable_but_quorum_low() {
        assert_eq!(classify_status(0.5, 1.5, true), WitnessStatus::Unwitnessed);
    }

    #[test]
    fn classify_unobservable_when_not_observable_even_if_support_high() {
        // inv #3 kritik: "görünmüyor" ≠ "yok" — yüksek support bile UnobservableLocally
        assert_eq!(
            classify_status(10.0, 1.5, false),
            WitnessStatus::UnobservableLocally
        );
    }

    // --- WitnessKind ağırlıkları ---

    #[test]
    fn witness_kind_weights_ordered() {
        assert!(WitnessKind::MergeCommit.default_weight() > WitnessKind::PRMerged.default_weight());
        assert!(
            WitnessKind::PRMerged.default_weight() > WitnessKind::TrailerReviewed.default_weight()
        );
        assert!(
            WitnessKind::TrailerReviewed.default_weight()
                > WitnessKind::CoAuthored.default_weight()
        );
    }

    // --- Intent invariant (yapısal — time_layer her zaman Gelecek) ---

    #[test]
    fn intent_new_sets_time_layer_gelecek() {
        use crate::space::TimeLayer;
        let intent = Intent::new(100, RawPosition::default());
        assert_eq!(intent.time_layer(), TimeLayer::Gelecek);
    }

    #[test]
    fn intent_serde_roundtrip_preserves_gelecek() {
        // serde #[serde(skip)] ile time_layer serialize edilmez, deserialize'da default (Gelecek)
        use crate::space::TimeLayer;
        let intent = Intent::new(
            42,
            RawPosition {
                x: 0.5,
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&intent).expect("serialize");
        // time_layer skip edildi → JSON'da yok
        assert!(!json.contains("time_layer"));
        let restored: Intent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            restored.time_layer(),
            TimeLayer::Gelecek,
            "deserialize her zaman Gelecek"
        );
        assert_eq!(restored.agent, 42);
        assert!((restored.target_raw.x - 0.5).abs() < 1e-9);
    }

    #[test]
    fn intent_serdeskip_ignores_time_layer_in_input() {
        // Kötü niyetli serialize edilmiş veri time_layer=Simdiki içerse bile → Gelecek olur
        use crate::space::TimeLayer;
        let malicious_json = r#"{"agent":1,"target_raw":{"x":0.0,"y":0.0,"z":0.0,"w":0.0,"v":0.0},"time_layer":"Simdiki"}"#;
        let restored: Intent = serde_json::from_str(malicious_json).expect("deserialize");
        assert_eq!(
            restored.time_layer(),
            TimeLayer::Gelecek,
            "serde #[skip] input'taki time_layer'ı yok sayar → invariant korundu"
        );
    }
}
