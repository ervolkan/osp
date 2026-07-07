//! AnchorGate — threshold + INV-C7/C8 + §6.4.1/§8.6 + ImplementedBy/Supersedes.
//!
//! `Vec<AnchorCandidate>` → `AnchorPlan`. Faz 1 (§8.2, §8.4, §6.4.1, §8.6, INV-C7, INV-C8).
//!
//! # Kural özeti
//! - **§8.2 threshold** (`total_clamped`): ≥0.80 StrongLink, [0.60,0.80) TentativeLink,
//!   [0.40,0.60) CreateNode, <0.40 MarkUnanchored.
//! - **INV-C7**: high-stake edge explanation zorunlu.
//! - **INV-C8 canon gate** (CreateNode öncesi): exact/alias/edit≤2 → redirect (hata değil).
//! - **§6.4.1**: Contradicts → MarkContradiction + negative_assertions.
//! - **§8.6**: yüksek abstraction → doğrudan CodeEntity yasak.
//! - **ImplementedBy**: Faz 1'de code evidence yok → yasak.
//! - **Supersedes**: authority gerektirir (INV-C4).

use crate::anchoring::classifier::Glossary;
use crate::anchoring::edit_distance::within_edit_distance_2;
use crate::anchoring::types::{
    AnchorCandidate, AnchorPlan, CanonicalRedirect, CanonicalRedirectReason, ConceptGraph,
    ConceptNodeId, ConceptPacketId,
};
use crate::anchoring::{AnchorDecisionKind, ConceptEdgeKind, ThresholdBand};

/// Anchor gate hatası — invariant ihlalleri.
#[derive(Debug, Clone, PartialEq)]
pub enum GateError {
    /// INV-C7: high-stake edge explanation yok.
    MissingExplanation { edge_kind: ConceptEdgeKind },
    /// §8.6: yüksek abstraction → doğrudan CodeEntity.
    IllegalDirectCodeBinding {
        from: ConceptNodeId,
        to: ConceptNodeId,
    },
    /// Faz 1: ImplementedBy code evidence (Faz 4) olmadan üretilemez.
    ImplementedByRequiresCodeEvidence,
    /// INV-C4: Supersedes authority yok.
    SupersedeAuthorityRequired,
}

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExplanation { edge_kind } => {
                write!(
                    f,
                    "INV-C7: high-stake edge {:?} explanation zorunlu",
                    edge_kind
                )
            }
            Self::IllegalDirectCodeBinding { from, to } => {
                write!(
                    f,
                    "§8.6: yasak doğrudan kod bağlantısı {} → {}",
                    from.0, to.0
                )
            }
            Self::ImplementedByRequiresCodeEvidence => {
                write!(f, "ImplementedBy code evidence (Faz 4) gerektirir")
            }
            Self::SupersedeAuthorityRequired => {
                write!(f, "INV-C4: Supersedes authority gerekli")
            }
        }
    }
}

impl std::error::Error for GateError {}

// ═══════════════════════════════════════════════════════════════════════════════
// SupersedeAuthority — INV-C4 capability token (§6.4, OperatorAcceptance pattern mirror)
// ═══════════════════════════════════════════════════════════════════════════════

/// Accepted kararları geçersiz kılma yetkisi (§6.4 hiyerarşi).
///
/// # INV-C4 type-level (yapısal garanti)
/// **Private field** (`_private: ()`) sayesinde external crate struct literal ile
/// üretemez. `AnchorGateContext { supersede_authority: Some(SupersedeAuthority::Operator) }`
/// dışarıdan yazılamaz — `SupersedeAuthority::Operator` bir enum varyantı DEĞİL, private
/// field'lı struct'tır. Sadece `pub(crate) fn issue_*()` constructor'ları (TCB içi)
/// üretebilir. Faz 8 operator console bu constructor'ları gerçek API ile çağırır.
///
/// `SupersedeAuthorityLevel` public enum sadece **bilgi amaçlı** (level okuma);
/// yeni authority üretmek için kullanılamaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersedeAuthority {
    level: SupersedeAuthorityLevel,
    _private: (),
}

/// Authority seviyesi (§6.2 hiyerarşi) — public, ama yeni `SupersedeAuthority`
/// üretmek için kullanılamaz (sadece `SupersedeAuthority::level()` ile okunur).
///
/// Faz 8b (PR #49): serde derive eklendi — `SupersedeRecord` audit için `authority_level`
/// taşır. Güvenlik açığı yok: level informational'dır, `SupersedeAuthority` capability'si
/// değil (capability hala private-field + `pub(crate)` ctor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SupersedeAuthorityLevel {
    /// §6.2 seviye 1 — her şeyi supersede edebilir.
    Operator,
    /// §6.2 seviye 2 — architect decision ve altını.
    ExplicitUser,
    /// §6.2 seviye 3 — documentation ve altını.
    WitnessedArchitectDecision,
}

impl SupersedeAuthority {
    /// Authority seviyesini oku (bilgi amaçlı — yeni authority üretmez).
    pub fn level(&self) -> SupersedeAuthorityLevel {
        self.level
    }

    #[cfg(test)]
    pub(crate) fn issue_operator_for_tests() -> Self {
        Self {
            level: SupersedeAuthorityLevel::Operator,
            _private: (),
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn issue_explicit_user_for_tests() -> Self {
        Self {
            level: SupersedeAuthorityLevel::ExplicitUser,
            _private: (),
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn issue_witnessed_architect_for_tests() -> Self {
        Self {
            level: SupersedeAuthorityLevel::WitnessedArchitectDecision,
            _private: (),
        }
    }
}

/// Gate context — INV-C4 authority + INV-C6 code evidence + ileride diğer capability'ler.
///
/// `decide` çağrılırken verilir. `Supersedes` candidate varsa `supersede_authority`
/// kontrol edilir; yoksa `GateError::SupersedeAuthorityRequired`. Faz 8 operator console
/// bu context'i doldurur.
///
/// # Faz 4 — code evidence provider (Not 5)
/// `code_evidence: Option<&dyn CodeEvidenceProvider>` — gate `ImplementedBy` için
/// `find_evidence()` ile **evidence object varlığını** kontrol eder (strength değil).
/// `None` → Faz 1-2 backward-compat (ImplementedBy reject).
///
/// # Clone/Copy/Default (review patch R3/R4)
/// `&dyn Trait` ve `SupersedeAuthority` ikisi de `Copy` → context `Clone + Copy`.
/// `Debug` custom kalır (dyn provider Debug olmayabilir). `Default = no_authority()`
/// (downstream compat — Faz 1-2'de `Default` vardı).
#[derive(Clone, Copy)]
pub struct AnchorGateContext<'a> {
    pub supersede_authority: Option<SupersedeAuthority>,
    pub code_evidence: Option<&'a dyn crate::anchoring::code_evidence::CodeEvidenceProvider>,
}

impl<'a> std::fmt::Debug for AnchorGateContext<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnchorGateContext")
            .field("supersede_authority", &self.supersede_authority)
            .field(
                "code_evidence",
                &self.code_evidence.map(|_| "<dyn CodeEvidenceProvider>"),
            )
            .finish()
    }
}

impl<'a> Default for AnchorGateContext<'a> {
    /// Faz 1-2 backward-compat: `no_authority()` ile eşdeğer (provider None).
    fn default() -> Self {
        Self::no_authority()
    }
}

impl<'a> AnchorGateContext<'a> {
    /// Faz 1-2 default: hiçbir authority yok, code evidence provider yok (Faz 4).
    /// ImplementedBy reject edilir (backward-compat), Supersedes authority ister.
    pub fn no_authority() -> Self {
        Self {
            supersede_authority: None,
            code_evidence: None,
        }
    }

    /// Code evidence provider ile context oluştur (INV-C6 — Faz 4).
    /// `ImplementedBy` evidence varsa kabul edilebilir.
    pub fn with_code_evidence(
        supersede_authority: Option<SupersedeAuthority>,
        code_evidence: &'a dyn crate::anchoring::code_evidence::CodeEvidenceProvider,
    ) -> Self {
        Self {
            supersede_authority,
            code_evidence: Some(code_evidence),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorGate
// ═══════════════════════════════════════════════════════════════════════════════

/// Anchor gate. Pipeline'ın son aşaması — candidate'leri plana dönüştürür.
pub struct AnchorGate {
    glossary: Glossary,
}

impl AnchorGate {
    pub fn new(glossary: Glossary) -> Self {
        Self { glossary }
    }

    /// Skorlanmış adayları AnchorPlan'a dönüştür.
    ///
    /// Sıra: invariant kontrolleri → threshold karar → canon gate → negative assertions.
    /// `ctx` INV-C4 authority taşır (Faz 2'de `None`; Faz 8 operator console doldurur).
    pub fn decide(
        &self,
        packet_id: &ConceptPacketId,
        candidates: Vec<AnchorCandidate>,
        graph: &ConceptGraph,
        ctx: &AnchorGateContext,
    ) -> Result<AnchorPlan, GateError> {
        // 1. INV-C7: high-stake explanation kontrolü
        for c in &candidates {
            if c.edge_kind.is_high_stake() && c.explanation.is_none() {
                return Err(GateError::MissingExplanation {
                    edge_kind: c.edge_kind,
                });
            }
        }

        // 2. §8.6 + ImplementedBy + Supersedes kontrolleri
        for c in &candidates {
            self.validate_edge_kind(c, graph, ctx)?;
        }

        // 3. Threshold karar (en yüksek skorlu aday üzerinden)
        let max_score = candidates
            .iter()
            .map(|c| c.score.total_clamped())
            .fold(0.0_f64, f64::max);
        let (decision, band) = self.threshold_decision(max_score);

        // 4. INV-C8 canon gate (CreateNode ise redirect kontrolü)
        let mut redirects = Vec::new();
        let mut adjusted_candidates = candidates;
        if matches!(decision, AnchorDecisionKind::CreateNode) {
            for c in &mut adjusted_candidates {
                if let Some(redirect) = self.canon_gate_check(c, graph) {
                    // Redirect varsa: yeni node yerine mevcut'a TentativeLink
                    c.target_node_id = redirect.existing_node.clone();
                    redirects.push(redirect);
                }
            }
        }

        // 5. §6.4.1 Contradicts → MarkContradiction + negative assertions
        let has_contradicts = adjusted_candidates
            .iter()
            .any(|c| c.edge_kind == ConceptEdgeKind::Contradicts);
        let final_decision =
            if has_contradicts && !matches!(decision, AnchorDecisionKind::MarkUnanchored) {
                AnchorDecisionKind::MarkContradiction
            } else {
                decision
            };
        let final_band = if has_contradicts && !matches!(band, ThresholdBand::Unanchored) {
            // Contradiction band'ı düşürme yok; ama decision değişti
            band
        } else {
            band
        };

        // 6. Negative assertions (fix_007 — §6.4.1 mapping)
        let negative_assertions = if has_contradicts {
            self.contradiction_negative_assertions(&adjusted_candidates)
        } else {
            Vec::new()
        };

        // 7. requires_operator_review (INV-C7 ↔ D6)
        let requires_operator_review = adjusted_candidates
            .iter()
            .any(|c| c.edge_kind.is_high_stake())
            || matches!(
                final_decision,
                AnchorDecisionKind::RequireOperatorReview
                    | AnchorDecisionKind::TentativeLink
                    | AnchorDecisionKind::MarkContradiction
            );

        Ok(AnchorPlan::from_gate(
            packet_id.clone(),
            adjusted_candidates,
            final_decision,
            final_band,
            requires_operator_review,
            negative_assertions,
            redirects,
        ))
    }

    /// §8.2 threshold → (decision, band).
    fn threshold_decision(&self, score: f64) -> (AnchorDecisionKind, ThresholdBand) {
        if score >= 0.80 {
            (AnchorDecisionKind::StrongLink, ThresholdBand::Strong)
        } else if score >= 0.60 {
            (AnchorDecisionKind::TentativeLink, ThresholdBand::Tentative)
        } else if score >= 0.40 {
            (AnchorDecisionKind::CreateNode, ThresholdBand::Weak)
        } else {
            (
                AnchorDecisionKind::MarkUnanchored,
                ThresholdBand::Unanchored,
            )
        }
    }

    /// §8.6 + ImplementedBy (evidence-gated) + Supersedes validation.
    ///
    /// # Faz 4 — ImplementedBy evidence-gated (Not 4/5)
    /// ImplementedBy blanket-reject DEĞİL. `ctx.code_evidence` provider varsa ve
    /// `find_evidence()` gerçek `ObservedCodeEvidence` object döndürürse → kabul.
    /// Provider yok veya evidence object yok → `ImplementedByRequiresCodeEvidence`.
    ///
    /// **Not 5:** gate `find_evidence()` ile **object varlığını** kontrol eder;
    /// `evidence_strength > 0` tek başına açmaz (scorer strength kullanır, gate object).
    ///
    /// **Not 4 (sıra garantisi):** INV-C7 explanation kontrolü `decide()`'de bu metodtan
    /// ÖNCE yapılır (line 176-183). Yani pozitif ImplementedBy = evidence VAR **ve**
    /// explanation VAR. Evidence explanation requirement'ı bypass etmez.
    fn validate_edge_kind(
        &self,
        c: &AnchorCandidate,
        _graph: &ConceptGraph,
        ctx: &AnchorGateContext,
    ) -> Result<(), GateError> {
        // Faz 4: ImplementedBy — evidence-gated (provider + object required).
        if c.edge_kind == ConceptEdgeKind::ImplementedBy {
            match ctx.code_evidence {
                Some(provider) => {
                    // Not 5: object varlığı kontrolü (strength değil).
                    let evidence = provider
                        .find_evidence(&c.target_node_id)
                        .map_err(|_| GateError::ImplementedByRequiresCodeEvidence)?;
                    if evidence.is_none() {
                        return Err(GateError::ImplementedByRequiresCodeEvidence);
                    }
                    // Evidence object mevcut → kabul (explanation zaten decide() adım 1'de).
                }
                None => {
                    // Provider yok → Faz 1-2 backward-compat (reject).
                    return Err(GateError::ImplementedByRequiresCodeEvidence);
                }
            }
        }
        // INV-C4: Supersedes authority gerekli. ctx.supersede_authority yoksa reject.
        // Faz 2'de context default None → reject. Faz 8 operator console authority verir.
        if c.edge_kind == ConceptEdgeKind::Supersedes && ctx.supersede_authority.is_none() {
            return Err(GateError::SupersedeAuthorityRequired);
        }
        // §8.6: ConceptPacket → doğrudan CodeEntity (Candidate değil) yasak
        if c.edge_kind == ConceptEdgeKind::ExpectedImplementation
            && c.target_node_id.0.starts_with("CodeEntity:")
        {
            return Err(GateError::IllegalDirectCodeBinding {
                from: c.packet_id.clone().into_node_id(),
                to: c.target_node_id.clone(),
            });
        }
        Ok(())
    }

    /// INV-C8 canon gate: CreateNode öncesi mevcut node redirect kontrolü (3 katman).
    /// Match varsa Some(redirect) — hata değil, başarılı redirect.
    fn canon_gate_check(
        &self,
        c: &AnchorCandidate,
        graph: &ConceptGraph,
    ) -> Option<CanonicalRedirect> {
        // Sadece yeni Concept node adayları için (target Concept: prefix)
        let target = &c.target_node_id.0;
        let (_, name) = target.split_once(':')?;
        let name = name.trim();

        // 1. Exact canonical match
        let exact = graph.find_concept_by_canonical(name);
        if let Some(existing) = exact.first() {
            return Some(CanonicalRedirect {
                attempted: name.to_string(),
                existing_node: existing.id.clone(),
                reason: CanonicalRedirectReason::ExactCanonicalMatch,
            });
        }

        // 2. Glossary alias match
        if let Some(canonical) = self.glossary.canonical_for(name) {
            let canonical_id = ConceptNodeId(format!("Concept:{canonical}"));
            if graph.node(&canonical_id).is_some() {
                return Some(CanonicalRedirect {
                    attempted: name.to_string(),
                    existing_node: canonical_id,
                    reason: CanonicalRedirectReason::GlossaryAliasMatch,
                });
            }
        }

        // 3. Edit distance ≤2 — mevcut concept canonical'larına karşı
        for node in graph.nodes_iter() {
            if matches!(
                node.node_kind,
                crate::anchoring::types::ConceptNodeKind::Concept
            ) {
                if let Some(distance) = within_edit_distance_2(name, &node.canonical) {
                    return Some(CanonicalRedirect {
                        attempted: name.to_string(),
                        existing_node: node.id.clone(),
                        reason: CanonicalRedirectReason::EditDistanceLe2 { distance },
                    });
                }
            }
        }

        None
    }

    /// §6.4.1 contradiction negative assertions (fix_007).
    fn contradiction_negative_assertions(&self, candidates: &[AnchorCandidate]) -> Vec<String> {
        let mut out = Vec::new();
        let has_code_entity = candidates.iter().any(|c| {
            c.target_node_id.0.starts_with("CodeEntity:")
                || c.target_node_id.0.starts_with("CodeEntityCandidate:")
        });
        if has_code_entity {
            out.push(
                "CodeEntity --SUPERSEDES--> Decision yasak (INV-C4, §6.4.1: kod güçlüdür ama karar değildir)".into(),
            );
            out.push("CodeEntity sadece --DRIFTS_FROM--> / --CONTRADICTS?--> üretebilir".into());
        }
        out.push(
            "Agent kaynağı --SUPERSEDES--> AcceptedDecision yapamaz; sadece CONTRADICTS? önerir (INV-C4 capability gate)".into(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::types::{
        AnchorScoreBreakdown, ConceptNode, ConceptNodeId, ConceptNodeKind, ConceptPacketId,
    };
    use crate::anchoring::{ConceptEdgeKind, DecisionStatus, PositionFamily};

    fn glossary() -> Glossary {
        Glossary::seed_default()
    }

    fn candidate(
        target: &str,
        kind: ConceptEdgeKind,
        score: f64,
        expl: Option<&str>,
    ) -> AnchorCandidate {
        // Test helper: tüm pozitif bileşenleri `score`'a eşitle → raw_total ≈ score
        // (ağırlık toplamı 1.0). penalty'ler 0. Böylece threshold testleri `score` ile
        // direkt ilişki kurabilir (0.90 → StrongLink, 0.70 → TentativeLink, vb.).
        let b = AnchorScoreBreakdown {
            semantic_similarity: crate::anchoring::types::ScalarSimilarity::new(score)
                .expect("test score [0,1]"),
            ontology_type_compatibility: score,
            graph_context_score: score,
            domain_term_match: score,
            code_evidence_score: score,
            temporal_trust_score: score,
            decision_status_score: score,
            contradiction_penalty: 0.0,
            staleness_penalty: 0.0,
        };
        AnchorCandidate {
            packet_id: ConceptPacketId("pkt:1".into()),
            target_node_id: ConceptNodeId(target.into()),
            edge_kind: kind,
            score: b,
            explanation: expl
                .map(|e| crate::anchoring::types::NonEmptyExplanation::from_validated(e.into())),
        }
    }

    fn empty_graph() -> ConceptGraph {
        ConceptGraph::new()
    }

    #[test]
    fn threshold_strong() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.90, None);
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::StrongLink);
        assert_eq!(plan.threshold_band, ThresholdBand::Strong);
    }

    #[test]
    fn threshold_tentative() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.70, None);
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::TentativeLink);
        assert_eq!(plan.threshold_band, ThresholdBand::Tentative);
    }

    #[test]
    fn threshold_unanchored() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.20, None);
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::MarkUnanchored);
        assert_eq!(plan.threshold_band, ThresholdBand::Unanchored);
    }

    #[test]
    fn inv_c7_high_stake_requires_explanation() {
        let g = AnchorGate::new(glossary());
        // DerivesRisk high-stake, explanation yok → error
        let c = candidate("RiskCandidate:X", ConceptEdgeKind::DerivesRisk, 0.70, None);
        let err = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap_err();
        assert!(matches!(err, GateError::MissingExplanation { .. }));
    }

    #[test]
    fn inv_c7_high_stake_with_explanation_ok() {
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "RiskCandidate:X",
            ConceptEdgeKind::DerivesRisk,
            0.70,
            Some("risk derived"),
        );
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert!(plan.requires_operator_review, "high-stake → review");
    }

    #[test]
    fn gate_rejects_implemented_by_without_code_evidence() {
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "CodeEntity:X",
            ConceptEdgeKind::ImplementedBy,
            0.90,
            Some("impl"),
        );
        let err = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap_err();
        assert_eq!(err, GateError::ImplementedByRequiresCodeEvidence);
    }

    #[test]
    fn gate_accepts_implemented_by_with_evidence() {
        // Faz 4 pozitif yol: ImplementedBy + provider + evidence object → kabul.
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "CodeEntity:AuthService",
            ConceptEdgeKind::ImplementedBy,
            0.90,
            Some("SCIP-observed implementation"),
        );
        let evidence = crate::anchoring::types::ObservedCodeEvidence::new(
            ConceptNodeId("CodeEntity:AuthService".into()),
            crate::anchoring::types::PhysicalCodeVector::new(0.42, 0.78, 0.30, 1.1, 5.0),
            crate::anchoring::types::ObservedCodeMetricSource::Scip,
            crate::anchoring::types::EvidenceStrength::new(0.85).unwrap(),
            1_700_000_000,
        );
        let provider =
            crate::anchoring::code_evidence::InMemoryCodeEvidenceProvider::from_evidence(vec![
                evidence,
            ]);
        let ctx = AnchorGateContext::with_code_evidence(None, &provider);
        let plan = g
            .decide(&ConceptPacketId("p".into()), vec![c], &empty_graph(), &ctx)
            .expect("evidence + explanation → ImplementedBy kabul");
        assert!(plan
            .candidates()
            .iter()
            .any(|c| c.edge_kind() == ConceptEdgeKind::ImplementedBy));
    }

    #[test]
    fn implemented_by_evidence_does_not_bypass_explanation() {
        // Not 4: evidence VAR ama explanation YOK → MissingExplanation (INV-C7 önce).
        // Evidence explanation requirement'ı bypass etmez.
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "CodeEntity:AuthService",
            ConceptEdgeKind::ImplementedBy,
            0.90,
            None, // explanation yok → INV-C7 önce reject
        );
        let evidence = crate::anchoring::types::ObservedCodeEvidence::new(
            ConceptNodeId("CodeEntity:AuthService".into()),
            crate::anchoring::types::PhysicalCodeVector::new(0.42, 0.78, 0.30, 1.1, 5.0),
            crate::anchoring::types::ObservedCodeMetricSource::Scip,
            crate::anchoring::types::EvidenceStrength::one(),
            1_700_000_000,
        );
        let provider =
            crate::anchoring::code_evidence::InMemoryCodeEvidenceProvider::from_evidence(vec![
                evidence,
            ]);
        let ctx = AnchorGateContext::with_code_evidence(None, &provider);
        let err = g
            .decide(&ConceptPacketId("p".into()), vec![c], &empty_graph(), &ctx)
            .unwrap_err();
        assert!(
            matches!(err, GateError::MissingExplanation { .. }),
            "evidence explanation'ı bypass etmez (Not 4)"
        );
    }

    #[test]
    fn implemented_by_rejects_when_provider_has_no_evidence_object() {
        // Not 5: gate find_evidence() ile OBJECT varlığını kontrol eder.
        // Provider mevcut ama bu CodeEntity için evidence object yok → reject.
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "CodeEntity:NotInProvider",
            ConceptEdgeKind::ImplementedBy,
            0.90,
            Some("impl"),
        );
        // Provider var ama CodeEntity:NotInProvider için evidence seed'lenmedi.
        let provider = crate::anchoring::code_evidence::InMemoryCodeEvidenceProvider::empty();
        let ctx = AnchorGateContext::with_code_evidence(None, &provider);
        let err = g
            .decide(&ConceptPacketId("p".into()), vec![c], &empty_graph(), &ctx)
            .unwrap_err();
        assert_eq!(
            err,
            GateError::ImplementedByRequiresCodeEvidence,
            "object yok → reject (Not 5 — strength değil object)"
        );
    }

    #[test]
    fn gate_rejects_supersedes_without_authority() {
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "Decision:X",
            ConceptEdgeKind::Supersedes,
            0.90,
            Some("super"),
        );
        let err = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap_err();
        assert_eq!(err, GateError::SupersedeAuthorityRequired);
    }

    #[test]
    fn gate_accepts_supersedes_with_authority() {
        // INV-C4: authority varsa Supersedes kabul edilir (Faz 8 operator console)
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "Decision:X",
            ConceptEdgeKind::Supersedes,
            0.90,
            Some("super"),
        );
        let ctx = AnchorGateContext {
            supersede_authority: Some(SupersedeAuthority::issue_operator_for_tests()),
            code_evidence: None,
        };
        let plan = g
            .decide(&ConceptPacketId("p".into()), vec![c], &empty_graph(), &ctx)
            .expect("authority ile Supersedes kabul");
        assert!(plan
            .candidates()
            .iter()
            .any(|c| c.edge_kind() == ConceptEdgeKind::Supersedes));
    }

    #[test]
    fn section_8_6_illegal_direct_code_binding() {
        let g = AnchorGate::new(glossary());
        // ExpectedImplementation → gerçek CodeEntity (Candidate değil) → §8.6 yasak
        let c = candidate(
            "CodeEntity:PaymentService",
            ConceptEdgeKind::ExpectedImplementation,
            0.90,
            Some("expected"),
        );
        let err = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap_err();
        assert!(matches!(err, GateError::IllegalDirectCodeBinding { .. }));
    }

    #[test]
    fn inv_c8_canon_gate_exact_match_redirect() {
        let g = AnchorGate::new(glossary());
        // Graph'ta Concept:Payment var; "Payment" için CreateNode → redirect
        let mut graph = empty_graph();
        graph.insert_node(ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        });
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.50, None);
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &graph,
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::CreateNode);
        assert_eq!(plan.redirects.len(), 1);
        assert_eq!(
            plan.redirects[0].reason,
            CanonicalRedirectReason::ExactCanonicalMatch
        );
    }

    #[test]
    fn inv_c8_canon_gate_alias_match_redirect() {
        let g = AnchorGate::new(glossary());
        let mut graph = empty_graph();
        graph.insert_node(ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec!["ödeme".into()],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        });
        // "ödeme" alias → Concept:Payment redirect
        let c = candidate("Concept:ödeme", ConceptEdgeKind::Mentions, 0.50, None);
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &graph,
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert!(!plan.redirects.is_empty());
    }

    #[test]
    fn section_6_4_1_contradiction_negative_assertions() {
        let g = AnchorGate::new(glossary());
        let c = candidate(
            "Decision:X",
            ConceptEdgeKind::Contradicts,
            0.50,
            Some("contradicts"),
        );
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::MarkContradiction);
        assert!(
            !plan.negative_assertions.is_empty(),
            "§6.4.1 negative assertions doldu"
        );
        assert!(plan
            .negative_assertions
            .iter()
            .any(|s| s.contains("SUPERSEDES")));
    }

    #[test]
    fn requires_review_false_when_only_low_stake() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.50, None);
        let plan = g
            .decide(
                &ConceptPacketId("p".into()),
                vec![c],
                &empty_graph(),
                &AnchorGateContext::no_authority(),
            )
            .unwrap();
        assert!(
            !plan.requires_operator_review,
            "düşük-stake Mentions → review gerekmez"
        );
    }
}
