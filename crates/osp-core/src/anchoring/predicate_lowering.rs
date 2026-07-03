//! Predicate lowering — RuleCandidate → PredicateStub (Faz 5a, INV-P1, D16).
//!
//! # Ana tez
//! *A rule is not a predicate. A predicate is a rule whose measurable slots have
//! been bound.* — `RuleCandidate` insan niyeti seviyesinde, `PredicateSet` (Paper 2)
//! çalıştırılabilir ölçüm seviyesinde. Arada `PredicateStub` epistemik tampon.
//!
//! # INV-P1 (yeni, D16)
//! Ölçülebilir slotları bağlanmamış RuleCandidate, ExecutablePredicateSet üretemez.
//! - **INV-P1a (PR33a):** RuleCandidate lowering `PredicateStub` üretir,
//!   ExecutablePredicateSet **DEĞİL**.
//! - **INV-P1b (PR33b):** PredicateStub → ExecutablePredicateSet sadece slot binding
//!   (operator/evidence-backed) ile.
//!
//! # Structured uncertainty
//! `PredicateStub` boş bir "bilmiyorum" DEĞİL — neyi bilmediğini (`unresolved_slots`),
//! neden bilmediğini (`reason`), hangi kalıplara uyabileceğini (`suggested_templates`)
//! ölçülü şekilde temsil eder. *"A PredicateStub is not absence of knowledge; it is
//! structured uncertainty."*
//!
//! # PR33a kapsamı
//! Bu modül sadece `PredicateStub` üretir. Navigator bağlantısı, executable predicate,
//! slot binding hepsi PR33b'ye. `lower_rule_to_predicate_stub` her zaman `Stub` döner.

use crate::anchoring::types::{ConceptNode, ConceptNodeId};
use crate::anchoring::ConceptNodeKind;

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateSlot — ölçülebilir slot (Patch 5 serde: Serialize + Deserialize)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir predicate'in ölçülebilir slot'u (henüz bağlı olmayan parametre).
///
/// Patch 5 serde politikası: `Serialize + Deserialize` (operator console slot seçimi
/// JSON ile gelebilir). `PredicateStub` ise Serialize-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateSlot {
    /// Hangi metric? (coupling/cohesion/instability/...)
    Metric,
    /// Hangi eşik? (0.55 / repo-average / ...)
    Threshold,
    /// Hangi kapsam? (hangi modül/node/subgraph)
    Scope,
    /// Hangi karşılaştırma? (< / ≤ / > / ≥)
    Comparator,
}

/// Tüm slot evreni (PR33a — 4 slot). `completeness()` için sabit.
pub const ALL_SLOTS: [PredicateSlot; 4] = [
    PredicateSlot::Metric,
    PredicateSlot::Threshold,
    PredicateSlot::Scope,
    PredicateSlot::Comparator,
];

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateTemplateId — önerilen template (Patch 5 serde: Serialize + Deserialize)
// ═══════════════════════════════════════════════════════════════════════════════

/// PR33a'da sadece ID/stub — executable logic PR33b. Rule canonical'ından keyword
/// mapping ile önerilir; ama **executable predicate üretmez** (sadece "bu template
/// önerildi" der).
///
/// Patch 5 serde politikası: `Serialize + Deserialize` (operator console template
/// seçimi JSON ile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateTemplateId {
    /// `metric(target, coupling) < threshold` — coupling/cohesion/instability eşik.
    MetricThreshold,
    /// `metric_after < metric_before` — progress checkpoint (Paper 2 loss azalma).
    MetricDelta,
    /// edge/claim için evidence var mı (Faz 4 ObservedCodeEvidence'e bağlanır).
    EvidenceRequired,
    /// `Concept --ImplementedBy--> CodeEntity` var mı (Faz 4'e bağlanır).
    RelationExists,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateStubReason — neden executable değil
// ═══════════════════════════════════════════════════════════════════════════════

/// Stub'ın executable olmadığının nedeni.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum PredicateStubReason {
    /// "coupling" mi "instability" mi net değil.
    MetricUnresolved,
    /// 0.55 mi repo-average mi net değil.
    ThresholdUnresolved,
    /// Hangi modül/node net değil.
    ScopeUnresolved,
    /// < mi ≤ mi net değil.
    ComparatorUnresolved,
    /// Hiçbir template uymadı (suggested_templates boş olmalı).
    NoTemplateMatch,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateStub — structured uncertainty (Patch 1/2, Faz 4 ObservedCodeEvidence paterni)
// ═══════════════════════════════════════════════════════════════════════════════

/// Rule'ın predicate olmak için ne eksik olduğu — INV-P1 structured uncertainty.
///
/// # Yapısal garanti (Patch 1)
/// Private field'lar + public smart constructor `new`. Dış crate literal construct
/// edemez (trybuild `cP1_predicate_stub_literal`); ama `new()` ile geçerli stub
/// üretebilir (operator console / bridge). Faz 4 `ObservedCodeEvidence` paterni.
///
/// # Non-empty invariant (Patch 2 — structured uncertainty type-level)
/// Stub **gerçekten boş değil** — consistency kontrolü:
/// - `unresolved_slots` boş VE `reason != NoTemplateMatch` → `EmptyUnresolvedSlots`.
/// - `reason == NoTemplateMatch` VE `suggested_templates` dolu →
///   `NoTemplateMatchCannotSuggestTemplate` (çelişki).
/// *"A PredicateStub is not absence of knowledge; it is structured uncertainty."*
///
/// # Serde boundary (Patch 5)
/// `Serialize`-only (audit). `Deserialize` YOK — stub yeniden apply edilememeli
/// (PR30/Faz4 serde boundary paterni). `PredicateSlot`/`PredicateTemplateId` ayrı
/// (Serialize + Deserialize — operator console seçim).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PredicateStub {
    rule_id: ConceptNodeId,
    reason: PredicateStubReason,
    unresolved_slots: Vec<PredicateSlot>,
    suggested_templates: Vec<PredicateTemplateId>,
}

/// `PredicateStub::new` consistency hatası (Patch 2).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum PredicateStubError {
    #[error("unresolved_slots boş ama reason NoTemplateMatch değil — stub boş olamaz")]
    EmptyUnresolvedSlots,
    #[error("reason NoTemplateMatch ama suggested_templates dolu — çelişki")]
    NoTemplateMatchCannotSuggestTemplate,
}

impl PredicateStub {
    /// Public smart constructor — Patch 2 consistency kontrolü.
    ///
    /// # Non-empty invariant
    /// - `unresolved_slots` boş VE `reason != NoTemplateMatch` → hata (stub boş).
    /// - `reason == NoTemplateMatch` VE `suggested_templates` dolu → hata (çelişki).
    /// - `reason == NoTemplateMatch` VE `suggested_templates` boş → tek geçerli yol
    ///   (rule'un hiçbir template'e uymadığı durum).
    pub fn new(
        rule_id: ConceptNodeId,
        reason: PredicateStubReason,
        unresolved_slots: Vec<PredicateSlot>,
        suggested_templates: Vec<PredicateTemplateId>,
    ) -> Result<Self, PredicateStubError> {
        if unresolved_slots.is_empty() && !matches!(reason, PredicateStubReason::NoTemplateMatch) {
            return Err(PredicateStubError::EmptyUnresolvedSlots);
        }
        if matches!(reason, PredicateStubReason::NoTemplateMatch) && !suggested_templates.is_empty()
        {
            return Err(PredicateStubError::NoTemplateMatchCannotSuggestTemplate);
        }
        Ok(Self {
            rule_id,
            reason,
            unresolved_slots,
            suggested_templates,
        })
    }

    pub fn rule_id(&self) -> &ConceptNodeId {
        &self.rule_id
    }
    pub fn reason(&self) -> PredicateStubReason {
        self.reason
    }
    pub fn unresolved_slots(&self) -> &[PredicateSlot] {
        &self.unresolved_slots
    }
    pub fn suggested_templates(&self) -> &[PredicateTemplateId] {
        &self.suggested_templates
    }

    /// Çözülmüş slot oranı `[0,1]` (D2 öneri 1, Patch 4 sabit formül).
    ///
    /// ```text
    /// NoTemplateMatch → 0.0
    /// otherwise → 1.0 - (unresolved_slots.len() / ALL_SLOTS.len())
    /// ```
    /// Tüm slot'lar unresolved → 0.0; 2 slot unresolved → 0.5. Operator önceliklendirme
    /// için. PR33b'de template-specific slot universe gelebilir.
    pub fn completeness(&self) -> f64 {
        if matches!(self.reason, PredicateStubReason::NoTemplateMatch) {
            return 0.0;
        }
        let total = ALL_SLOTS.len() as f64;
        let unresolved = self.unresolved_slots.len() as f64;
        (1.0 - unresolved / total).clamp(0.0, 1.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateLoweringOutcome — PR33a'da sadece Stub (Patch 3)
// ═══════════════════════════════════════════════════════════════════════════════

/// RuleCandidate lowering sonucu. PR33a'da **her zaman `Stub`** (Patch 3).
/// `RequiresOperatorBinding(UnresolvedPredicateBinding)` PR33b'ye.
#[derive(Debug, Clone, PartialEq)]
pub enum PredicateLoweringOutcome {
    /// PR33a — Rule'ın predicate olmak için eksikleri (structured uncertainty).
    Stub(PredicateStub),
    // PR33b: RequiresOperatorBinding(UnresolvedPredicateBinding),
}

/// `lower_rule_to_predicate_stub` hatası (Son Patch 1).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum PredicateLoweringError {
    #[error("node RuleCandidate değil: {node_id}")]
    NotRuleCandidate { node_id: ConceptNodeId },
    #[error("stub construct hatası: {0}")]
    InvalidStub(PredicateStubError),
}

// ═══════════════════════════════════════════════════════════════════════════════
// lower_rule_to_predicate_stub — lowering fonksiyonu (Son Patch 1: Result döner)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir RuleCandidate node'unu PredicateStub'a lower et (INV-P1a).
///
/// # INV-P1 (Son Patch 1/2)
/// - Sadece `ConceptNodeKind::RuleCandidate` lowering'e girebilir. Başka kind verilirse
///   `NotRuleCandidate` hatası. *"Sadece RuleCandidate lowering'e girebilir.
///   RuleCandidate bile PredicateSet üretemez; sadece PredicateStub üretir."*
/// - PR33a'da **her zaman Stub** döner — executable predicate üretmez (INV-P1a).
///
/// # Deterministic suggested_templates (cross-family translation yok)
/// Rule canonical'ından keyword'lere göre template **önerilir** (coupling →
/// MetricThreshold, evidence → EvidenceRequired, decrease → MetricDelta, implemented →
/// RelationExists). Ama executable predicate üretmez — sadece "bu template önerildi".
/// Tüm slot'lar (metric/threshold/scope/comparator) unresolved kalır; operator
/// bağlayacak (PR33b).
///
/// # Scope (PR33a)
/// NLP yok — sadece canonical string keyword eşleştirme. No template match ise
/// `reason: NoTemplateMatch`, `suggested_templates: []` (tek geçerli boş durum).
pub fn lower_rule_to_predicate_stub(
    rule_candidate: &ConceptNode,
) -> Result<PredicateLoweringOutcome, PredicateLoweringError> {
    // Son Patch 1: kind kontrolü — sadece RuleCandidate.
    if !matches!(rule_candidate.node_kind, ConceptNodeKind::RuleCandidate) {
        return Err(PredicateLoweringError::NotRuleCandidate {
            node_id: rule_candidate.id.clone(),
        });
    }

    let canonical_lower = rule_candidate.canonical.to_lowercase();

    // Deterministic keyword → suggested_templates (öneri, executable değil).
    let mut suggested: Vec<PredicateTemplateId> = Vec::new();
    if canonical_lower.contains("coupling")
        || canonical_lower.contains("cohesion")
        || canonical_lower.contains("instability")
        || canonical_lower.contains("bağıml")
    {
        suggested.push(PredicateTemplateId::MetricThreshold);
    }
    if canonical_lower.contains("decrease")
        || canonical_lower.contains("reduce")
        || canonical_lower.contains("azalt")
        || canonical_lower.contains("düşür")
    {
        suggested.push(PredicateTemplateId::MetricDelta);
    }
    if canonical_lower.contains("evidence")
        || canonical_lower.contains("kanıt")
        || canonical_lower.contains("witness")
    {
        suggested.push(PredicateTemplateId::EvidenceRequired);
    }
    if canonical_lower.contains("implement")
        || canonical_lower.contains("implemente")
        || canonical_lower.contains("relation")
    {
        suggested.push(PredicateTemplateId::RelationExists);
    }

    // Tüm slot'lar unresolved (operator bağlayacak — PR33b). Metric/Threshold/Scope/
    // Comparator hepsi net değil; sadece template önerildi. NoTemplateMatch durumunda
    // suggested boş → unresolved_slots da boş (smart ctor consistency: NoTemplateMatch
    // + boş templates tek geçerli boş durum).
    let reason = if suggested.is_empty() {
        PredicateStubReason::NoTemplateMatch
    } else {
        // Template önerildi ama slot'lar unresolved — MetricUnresolved en genel.
        // (Diğer reason'lar PR33b'de slot-specific lowering ile ayrışır.)
        PredicateStubReason::MetricUnresolved
    };

    let unresolved_slots = if matches!(reason, PredicateStubReason::NoTemplateMatch) {
        Vec::new()
    } else {
        vec![
            PredicateSlot::Metric,
            PredicateSlot::Threshold,
            PredicateSlot::Scope,
            PredicateSlot::Comparator,
        ]
    };

    let stub = PredicateStub::new(
        rule_candidate.id.clone(),
        reason,
        unresolved_slots,
        suggested,
    )
    .map_err(PredicateLoweringError::InvalidStub)?;

    Ok(PredicateLoweringOutcome::Stub(stub))
}

#[cfg(test)]
mod tests {
    //! predicate_lowering.rs unit testleri — smart ctor consistency (3), non-RuleCandidate
    //! reject, completeness formül, lowering outcome, serde boundary.

    use super::*;
    use crate::anchoring::ConceptNodeKind;

    fn rule_candidate(canonical: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("RuleCandidate:{canonical}")),
            canonical: canonical.into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: crate::anchoring::DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        }
    }

    fn concept_node(kind: ConceptNodeKind, canonical: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("{}:{canonical}", kind.as_prefix())),
            canonical: canonical.into(),
            aliases: Vec::new(),
            node_kind: kind,
            decision_status: crate::anchoring::DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        }
    }

    // ── Patch 2: smart ctor consistency (3 test) ──────────────────────────────

    #[test]
    fn predicate_stub_rejects_empty_uncertainty() {
        // unresolved_slots boş + reason NoTemplateMatch değil → hata.
        let result = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::MetricUnresolved,
            vec![],
            vec![PredicateTemplateId::MetricThreshold],
        );
        assert_eq!(
            result.unwrap_err(),
            PredicateStubError::EmptyUnresolvedSlots,
            "stub boş olamaz — structured uncertainty"
        );
    }

    #[test]
    fn predicate_stub_rejects_no_template_with_suggestions() {
        // NoTemplateMatch + suggested_templates dolu → çelişki → hata.
        let result = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],                                     // NoTemplateMatch için boş olabilir
            vec![PredicateTemplateId::MetricThreshold], // ama template önerilmiş → çelişki
        );
        assert_eq!(
            result.unwrap_err(),
            PredicateStubError::NoTemplateMatchCannotSuggestTemplate
        );
    }

    #[test]
    fn predicate_stub_allows_no_template_match_without_suggestions() {
        // NoTemplateMatch + boş templates → tek geçerli boş durum.
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],
            vec![],
        )
        .expect("NoTemplateMatch + boş templates geçerli");
        assert_eq!(stub.reason(), PredicateStubReason::NoTemplateMatch);
        assert!(stub.suggested_templates().is_empty());
    }

    // ── Son Patch 2: non-RuleCandidate reject ─────────────────────────────────

    #[test]
    fn lowering_rejects_non_rule_candidate() {
        // INV-P1: sadece RuleCandidate lowering'e girebilir.
        let concept = concept_node(ConceptNodeKind::Concept, "Payment");
        let err = lower_rule_to_predicate_stub(&concept).unwrap_err();
        assert!(
            matches!(err, PredicateLoweringError::NotRuleCandidate { .. }),
            "Concept lowering'e giremez"
        );

        let task = concept_node(ConceptNodeKind::TaskCandidate, "Refactor");
        let err = lower_rule_to_predicate_stub(&task).unwrap_err();
        assert!(
            matches!(err, PredicateLoweringError::NotRuleCandidate { .. }),
            "TaskCandidate lowering'e giremez"
        );

        let code = concept_node(ConceptNodeKind::CodeEntity, "AuthService");
        let err = lower_rule_to_predicate_stub(&code).unwrap_err();
        assert!(
            matches!(err, PredicateLoweringError::NotRuleCandidate { .. }),
            "CodeEntity lowering'e giremez"
        );
    }

    // ── Son Patch 4: completeness formül ──────────────────────────────────────

    #[test]
    fn completeness_all_slots_unresolved_is_zero() {
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::MetricUnresolved,
            vec![
                PredicateSlot::Metric,
                PredicateSlot::Threshold,
                PredicateSlot::Scope,
                PredicateSlot::Comparator,
            ],
            vec![PredicateTemplateId::MetricThreshold],
        )
        .unwrap();
        assert_eq!(stub.completeness(), 0.0, "tüm slot'lar unresolved → 0.0");
    }

    #[test]
    fn completeness_two_slots_unresolved_is_half() {
        // 4 slot'tan 2'si unresolved → 1.0 - 2/4 = 0.5
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::ThresholdUnresolved,
            vec![PredicateSlot::Threshold, PredicateSlot::Scope],
            vec![PredicateTemplateId::MetricThreshold],
        )
        .unwrap();
        assert_eq!(stub.completeness(), 0.5);
    }

    #[test]
    fn completeness_no_template_match_is_zero() {
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(stub.completeness(), 0.0, "NoTemplateMatch → 0.0");
    }

    // ── Lowering outcome (INV-P1a — her zaman Stub) ───────────────────────────

    #[test]
    fn lowering_coupling_rule_suggests_metric_threshold() {
        let rule = rule_candidate("NoHighCouplingDependency");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert!(stub
                    .suggested_templates()
                    .contains(&PredicateTemplateId::MetricThreshold));
                assert_eq!(stub.reason(), PredicateStubReason::MetricUnresolved);
                // Tüm slot'lar unresolved (operator bağlayacak — PR33b)
                assert_eq!(stub.unresolved_slots().len(), 4);
                // Executable predicate YOK (INV-P1a)
            }
        }
    }

    #[test]
    fn lowering_no_keyword_rule_yields_no_template_match() {
        let rule = rule_candidate("SomeAbstractConcern");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert_eq!(stub.reason(), PredicateStubReason::NoTemplateMatch);
                assert!(stub.suggested_templates().is_empty());
                // NoTemplateMatch → unresolved_slots boş (tek geçerli boş durum)
                assert!(stub.unresolved_slots().is_empty());
            }
        }
    }

    #[test]
    fn lowering_always_produces_stub_never_executable() {
        // INV-P1a: PR33a'da her zaman Stub — executable predicate yok.
        for canonical in [
            "CouplingRule",
            "EvidenceRule",
            "DecreaseCoupling",
            "AbstractRule",
        ] {
            let rule = rule_candidate(canonical);
            let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
            assert!(
                matches!(outcome, PredicateLoweringOutcome::Stub(_)),
                "PR33a her zaman Stub: {canonical}"
            );
        }
    }
}
