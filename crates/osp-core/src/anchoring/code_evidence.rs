//! CodeEvidenceProvider trait + InMemoryCodeEvidenceProvider (Faz 4, §11, INV-C6).
//!
//! # Faz 4 — Code evidence integration
//! `CodeEvidenceProvider` trait'i, bir CodeEntity için **observed (ölçülmüş)** kod kanıtı
//! arar. [`crate::anchoring::gate::AnchorGate`] `ImplementedBy` edge'ini ancak bu provider
//! gerçek `ObservedCodeEvidence` object döndürürse kabul eder (Not 5 — `evidence_strength > 0`
//! tek başına açmaz; gerçek object gerekir).
//!
//! # Not 5 güçlenme (PR C)
//! Önceki modelde "evidence object var, `confidence=0`" temsil edilebiliyordu ve
//! gate (object presence) / scorer (strength > 0) ayrımı bu kenar duruma dayanıyordu.
//! PR C axis-granular modeli zero-strength reject uygular (`ObservedPhysicalMetric::new`
//! strength=0 → error), bu yüzden "strength=0 evidence" artık oluşamaz. Gate hâlâ object
//! presence kontrolü yapar, scorer hâlâ `minimum_observed_strength()` skalarını kullanır;
//! ama korunmuş kenar durum ortadan kalktı — gate/scorer ayrımı korunur, korumaya gerek
//! kalmaz.
//!
//! # D7-abstraction (osp-core analyzer-agnostic)
//! [`AnchorStore`] (Faz 3) gibi, `CodeEvidenceProvider` de `osp-core`'u analyzer-agnostic
//! tutar. Gerçek `osp-analyzer` bridge'i (symbol index) ayrı bir PR'da / crate'te impl
//! edilir — bu faz sadece deterministik stub ile **mechanism proof**.
//!
//! # INV-C6 epistemik ayrımı (D15 — provenance yorumu)
//! - `DecisionStatus` = graph acceptance lane (Candidate→Accepted)
//! - `ObservedCodeEvidence` = epistemik provenance lane (MetricSource'tan)
//!
//! "Observed code reality is evidence, not acceptance." Provider observed evidence döndürür
//! ama node'un acceptance status'unu değiştirmez. `GraphSeed.code_entities` varlığı **kanıt
//! sayılmaz** (Patch 6) — explicit `ObservedCodeEvidence` seed gerekir.

use crate::anchoring::types::{ConceptNodeId, EvidenceStrength, ObservedCodeEvidence};
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// CodeEvidenceError — thiserror + serde (Patch 4 — tek error, object-safe trait)
// ═══════════════════════════════════════════════════════════════════════════════

/// Code evidence provider hatası. Object-safe trait için associated `Error` yerine tek
/// concrete error (Patch 4 — Seçenek A).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum CodeEvidenceError {
    #[error("evidence bulunamadı: {0}")]
    NotFound(String),
    #[error("internal provider hatası: {0}")]
    Internal(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeEvidenceProvider — trait (object-safe, &dyn)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir CodeEntity için observed kod kanıtı arar (INV-C6, Faz 4).
///
/// # İki method — iki kullanım (Not 5)
/// - `find_evidence` → [`AnchorGate`] `ImplementedBy` için **evidence object varlığını**
///   kontrol eder. Sadece `evidence_strength > 0` ImplementedBy açmaz; gerçek object olmalı.
/// - `evidence_strength` → [`AnchorScorer`](crate::anchoring::scorer::AnchorScorer)
///   `code_evidence_score` (weight 0.10) için skalar gücü döndürür (PR C:
///   `minimum_observed_strength()` — normative min-over-axes).
///
/// # Not 5 güçlenme (PR C)
/// Önceki modelde "evidence object var, `confidence=0`" temsil edilebiliyordu ve
/// gate (object presence) / scorer (strength > 0) ayrımı bu kenar duruma dayanıyordu.
/// PR C axis-granular modeli zero-strength reject uygular, bu yüzden "strength=0 evidence"
/// artık oluşamaz. Gate/scorer ayrımı korunur ama korunan kenar durum yok.
///
/// # Object-safe
/// Associated `Error` yerine tek concrete [`CodeEvidenceError`] → `&dyn CodeEvidenceProvider`
/// ile kullanılabilir; pipeline/gate/scorer imzalarını büyütmez.
pub trait CodeEvidenceProvider {
    /// CodeEntity için observed evidence object'i (varsa). Gate `ImplementedBy` bunu ister.
    fn find_evidence(
        &self,
        code_entity_id: &ConceptNodeId,
    ) -> Result<Option<ObservedCodeEvidence>, CodeEvidenceError>;

    /// Evidence gücü `[0,1]` (EvidenceStrength). Scorer `code_evidence_score` için.
    /// Evidence yoksa `EvidenceStrength::zero()`.
    fn evidence_strength(
        &self,
        code_entity_id: &ConceptNodeId,
    ) -> Result<EvidenceStrength, CodeEvidenceError>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// InMemoryCodeEvidenceProvider — deterministik stub (Faz 4 mechanism proof)
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory, seeded, deterministik code evidence provider.
///
/// # Patch 6 — GraphSeed.code_entities otomatik evidence sayılmaz
/// Bu provider **sadece explicit `ObservedCodeEvidence` seed** ile beslenir. Bir
/// `CodeEntity` node'unun [`GraphSeed`] üzerinden seed edilmiş olması kanıt üretmez.
/// Bu, INV-C6 boundary'yi korur: `CodeEntity` node varlığı ≠ observed code evidence.
///
/// # Backward-compat
/// `empty()` default ile tüm lookups `None`/zero döner → mevcut davranış korunur
/// (`code_evidence_score=0`, gate `ImplementedBy` reject).
#[derive(Debug, Clone, Default)]
pub struct InMemoryCodeEvidenceProvider {
    evidence: HashMap<ConceptNodeId, ObservedCodeEvidence>,
}

impl InMemoryCodeEvidenceProvider {
    /// Boş provider — tüm lookups kanıt yok (default/Faz 1-2 backward-compat).
    pub fn empty() -> Self {
        Self {
            evidence: HashMap::new(),
        }
    }

    /// Explicit observed evidence seed ile provider oluştur.
    pub fn from_evidence(evidence: Vec<ObservedCodeEvidence>) -> Self {
        let map = evidence
            .into_iter()
            .map(|e| (e.code_entity_id().clone(), e))
            .collect();
        Self { evidence: map }
    }

    /// Explicit evidence ekle (builder pattern). Aynı CodeEntity için overwrite.
    pub fn with_evidence(mut self, evidence: ObservedCodeEvidence) -> Self {
        self.evidence
            .insert(evidence.code_entity_id().clone(), evidence);
        self
    }

    /// Seed'deki evidence sayısı (test/diagnostic).
    pub fn evidence_count(&self) -> usize {
        self.evidence.len()
    }
}

impl CodeEvidenceProvider for InMemoryCodeEvidenceProvider {
    fn find_evidence(
        &self,
        code_entity_id: &ConceptNodeId,
    ) -> Result<Option<ObservedCodeEvidence>, CodeEvidenceError> {
        Ok(self.evidence.get(code_entity_id).cloned())
    }

    fn evidence_strength(
        &self,
        code_entity_id: &ConceptNodeId,
    ) -> Result<EvidenceStrength, CodeEvidenceError> {
        Ok(match self.evidence.get(code_entity_id) {
            Some(ev) => ev.observations().minimum_observed_strength(),
            None => EvidenceStrength::zero(),
        })
    }
}

#[cfg(test)]
mod tests {
    //! code_evidence.rs unit testleri — provider lookup, evidence_strength,
    //! empty provider backward-compat, ObservedCodeEvidence constructor validasyon.

    use super::*;
    use crate::anchoring::PhysicalCodeMetricAxis;
    use crate::anchoring::types::{
        EvidenceCoverage, EvidenceStrength, ObservedCodeEvidence, ObservedCodeMetricSource,
        ObservedPhysicalMetric, ObservedPhysicalMetrics,
    };

    /// auth_service observation'ları — entropy/witness **representative normalized**
    /// (PR C: 1.1/5.0 raw → 0.52/0.68). Eski `PhysicalCodeVector::new(0.42, 0.78, 0.30, 1.1, 5.0)`
    /// raw hatası düzeltildi; 5 eksende de uniform [0,1].
    fn auth_service_observations() -> ObservedPhysicalMetrics {
        let strength = EvidenceStrength::new(0.85).unwrap();
        let coverage = EvidenceCoverage::new(1.0).unwrap();
        let scip = ObservedCodeMetricSource::Scip;
        ObservedPhysicalMetrics::try_new(vec![
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Coupling,
                0.42,
                scip,
                strength,
                coverage,
            )
            .unwrap(),
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Cohesion,
                0.78,
                scip,
                strength,
                coverage,
            )
            .unwrap(),
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Instability,
                0.30,
                scip,
                strength,
                coverage,
            )
            .unwrap(),
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Entropy,
                0.52,
                scip,
                strength,
                coverage,
            )
            .unwrap(),
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::WitnessDepth,
                0.68,
                scip,
                strength,
                coverage,
            )
            .unwrap(),
        ])
        .unwrap()
    }

    fn auth_service_evidence() -> ObservedCodeEvidence {
        ObservedCodeEvidence::new(
            ConceptNodeId("CodeEntity:AuthService".into()),
            auth_service_observations(),
            1_700_000_000,
        )
    }

    /// Tek-eksen observation helper (builder-pattern overwrite testi için).
    fn single_axis_observations(
        axis: PhysicalCodeMetricAxis,
        value: f64,
        source: ObservedCodeMetricSource,
        strength: EvidenceStrength,
    ) -> ObservedPhysicalMetrics {
        ObservedPhysicalMetrics::try_new(vec![ObservedPhysicalMetric::new(
            axis,
            value,
            source,
            strength,
            EvidenceCoverage::new(1.0).unwrap(),
        )
        .unwrap()])
        .unwrap()
    }

    #[test]
    fn empty_provider_returns_none_and_zero() {
        let p = InMemoryCodeEvidenceProvider::empty();
        let id = ConceptNodeId("CodeEntity:X".into());
        assert_eq!(p.evidence_count(), 0);
        assert!(p.find_evidence(&id).unwrap().is_none());
        assert_eq!(
            p.evidence_strength(&id).unwrap().get(),
            0.0,
            "empty provider → evidence_strength zero (backward-compat)"
        );
    }

    #[test]
    fn seeded_provider_finds_evidence_by_id() {
        let p = InMemoryCodeEvidenceProvider::from_evidence(vec![auth_service_evidence()]);
        let id = ConceptNodeId("CodeEntity:AuthService".into());
        assert_eq!(p.evidence_count(), 1);

        let ev = p.find_evidence(&id).unwrap().expect("evidence mevcut");
        assert_eq!(ev.code_entity_id(), &id);
        assert_eq!(ev.measured_at(), 1_700_000_000);
        // PR C: axis-granular observations. Cohesion axis değeri korundu (0.78).
        let cohesion = ev
            .observations()
            .values()
            .iter()
            .find(|o| o.axis() == PhysicalCodeMetricAxis::Cohesion)
            .expect("Cohesion axis mevcut");
        assert_eq!(cohesion.value().get(), 0.78);
        assert_eq!(cohesion.source(), ObservedCodeMetricSource::Scip);
    }

    #[test]
    fn evidence_strength_matches_minimum_observed_when_present() {
        let p = InMemoryCodeEvidenceProvider::from_evidence(vec![auth_service_evidence()]);
        let id = ConceptNodeId("CodeEntity:AuthService".into());
        // Not 5 (PR C): strength = minimum_observed_strength (normative min-over-axes).
        // auth_service tüm eksenlerde 0.85 strength → min 0.85.
        assert_eq!(p.evidence_strength(&id).unwrap().get(), 0.85);
    }

    #[test]
    fn graphseed_code_entities_varligi_evidence_uretmez() {
        // Patch 6: CodeEntity node seed edilmiş olabilir ama explicit evidence yoksa
        // ImplementedBy açılmamalı. InMemoryCodeEvidenceProvider explicit seed ister.
        let p = InMemoryCodeEvidenceProvider::empty();
        // GraphSeed.code_entities'e ConceptNode eklendi varsayalım — provider bilmez.
        let id = ConceptNodeId("CodeEntity:PaymentModule".into());
        assert!(
            p.find_evidence(&id).unwrap().is_none(),
            "GraphSeed.code_entities varlığı evidence sayılmaz (Patch 6)"
        );
    }

    #[test]
    fn with_evidence_builder_pattern_overwrites() {
        // Değer seti 2 (0.1..1.0) + değer seti 3 (witness 9.0→0.9 soft-norm).
        let ev1 = ObservedCodeEvidence::new(
            ConceptNodeId("CodeEntity:X".into()),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.1,
                ObservedCodeMetricSource::TreeSitter,
                EvidenceStrength::new(0.5).unwrap(),
            ),
            100,
        );
        let ev2 = ObservedCodeEvidence::new(
            ConceptNodeId("CodeEntity:X".into()),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.9,
                ObservedCodeMetricSource::Scip,
                EvidenceStrength::new(0.95).unwrap(),
            ),
            200,
        );
        let p = InMemoryCodeEvidenceProvider::empty()
            .with_evidence(ev1)
            .with_evidence(ev2);
        let id = ConceptNodeId("CodeEntity:X".into());
        let found = p.find_evidence(&id).unwrap().unwrap();
        // overwrite → son evidence kazanır (Coupling axis Scip source).
        let coupling = found
            .observations()
            .values()
            .iter()
            .find(|o| o.axis() == PhysicalCodeMetricAxis::Coupling)
            .unwrap();
        assert_eq!(coupling.source(), ObservedCodeMetricSource::Scip);
        assert_eq!(found.observations().minimum_observed_strength().get(), 0.95);
    }

    #[test]
    fn evidence_strength_out_of_range_rejects_nan_inf_negative() {
        // Not 1: is_finite() + [0,1] range — NaN, ±∞, -1.0, 2.0 hepsi reject
        assert!(EvidenceStrength::new(f64::NAN).is_err());
        assert!(EvidenceStrength::new(f64::INFINITY).is_err());
        assert!(EvidenceStrength::new(f64::NEG_INFINITY).is_err());
        assert!(EvidenceStrength::new(-0.01).is_err());
        assert!(EvidenceStrength::new(1.01).is_err());
        // Boundary: 0.0 ve 1.0 geçerli
        assert!(EvidenceStrength::new(0.0).is_ok());
        assert!(EvidenceStrength::new(1.0).is_ok());
    }

    #[test]
    fn observed_code_evidence_accessors() {
        let ev = auth_service_evidence();
        assert_eq!(ev.code_entity_id().0, "CodeEntity:AuthService");
        assert_eq!(ev.measured_at(), 1_700_000_000);
        // PR C: axis-granular observations. Coupling axis değeri 0.42, source Scip.
        let coupling = ev
            .observations()
            .values()
            .iter()
            .find(|o| o.axis() == PhysicalCodeMetricAxis::Coupling)
            .unwrap();
        assert_eq!(coupling.value().get(), 0.42);
        assert_eq!(coupling.source(), ObservedCodeMetricSource::Scip);
        assert_eq!(coupling.strength().get(), 0.85);
        assert_eq!(coupling.coverage().get(), 1.0);
        // minimum_observed_strength = 0.85 (tüm eksenlerde 0.85).
        assert_eq!(
            ev.observations().minimum_observed_strength().get(),
            0.85
        );
    }

    #[test]
    fn evidence_strength_serde_rejects_out_of_range() {
        // R1 review patch: Deserialize new() üzerinden range-check yapar.
        // serde_json::from_str("2.0") / "-1.0" reject — constructor bypass edilemez.
        assert!(serde_json::from_str::<EvidenceStrength>("2.0").is_err());
        assert!(serde_json::from_str::<EvidenceStrength>("-1.0").is_err());
        // NaN/inf JSON'da standart temsil edilmez ama emin olalım.
        assert!(serde_json::from_str::<EvidenceStrength>("\"NaN\"").is_err());
    }

    #[test]
    fn evidence_strength_serde_roundtrip_valid() {
        // Geçerli değer round-trip: serialize → deserialize aynı kalır.
        let original = EvidenceStrength::new(0.85).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: EvidenceStrength = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
        assert_eq!(restored.get(), 0.85);
    }
}
