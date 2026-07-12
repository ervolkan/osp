//! Evidence identity layer (PR F) — anti-corruption boundary between graph world
//! (`ConceptNodeId`) and identity/evidence world (`CodeIdentityKey`).
//!
//! # Mimari merkez (PR F — 3 tur plan review sonucu, sabitlenen ontolojik sözleşme)
//! ```text
//! Graph dünyası                Identity/evidence dünyası
//! ConceptNodeId                CodeIdentityKey
//!        │                             │
//!        └── CodeIdentityBindingLookup ┘  (dar public read-only capability)
//!                     │
//!                     ▼
//!            ResolvedCodeIdentity
//!                     │
//!                     ▼
//!            CodeEvidenceSource (key-facing)
//!                     │
//!                     ▼
//!            ObservedCodeEvidence
//! ```
//!
//! Tek truth source: `HashMap<CodeIdentityKey, ObservedCodeEvidence>`. `ConceptNodeId`-keyed
//! evidence storage **oluşturulmaz** (EI1 mimari garanti). Node-facing adapter
//! ([`ResolvedCodeEvidenceProvider`]) graph dünyasından identity dünyasına geçişin tek noktasıdır.
//!
//! # Not 5 güçlenme (PR C — korunur)
//! PR C axis-granular modeli zero-strength reject uygular (`ObservedPhysicalMetric::new`
//! strength=0 → error). Gate hâlâ object presence kontrolü yapar, scorer hâlâ
//! `minimum_observed_strength()` skalarını kullanır.
//!
//! # EI1-EI8 evidence identity invariantları (PR F)
//! Clause-bazlı enforcement — her invariant'ın temsil edilebilirlik (TYPE) ve duruma bağlı
//! davranış (RUNTIME) parçaları ayrı enforce edilir:
//! - **EI1-a (TYPE):** resolved value exactly one key taşır (private fields + fixed struct shape)
//! - **EI1-b (RUNTIME):** bound node store'da tek binding'e resolve
//! - **EI2 (RUNTIME):** candidate+entity aynı evidence
//! - **EI3-a (TYPE/API):** Resolution API evidence-source/mutator capability taşımaz
//! - **EI3-b (RUNTIME):** resolution source cardinality değiştirmez (regression witness)
//! - **EI4-a (RUNTIME):** one node → conflicting keys reject
//! - **EI4-b (RUNTIME):** materialization-zamanı — one key → multiple live CodeEntity reject (R7, PR E)
//! - **EI4-c (RUNTIME):** resolution-zamanı — multiple candidates same key → converge (N:1 reuse)
//! - **EI5-a (TYPE):** resolver NodeNotFound/Unbound typed ayırır
//! - **EI5-b (TYPE + pin test):** adapter explicit semantic mapping (Unbound→Ok(None), NodeNotFound→IdentityLookup)
//! - **EI6 (RUNTIME):** same snapshot → consumer-bazlı eşitlikler
//! - **EI7 (RUNTIME):** candidate/entity strength equality (shared key ownership)
//! - **EI8-V1 (RUNTIME):** graph absence/unbound → key-owned evidence mutasyonu YOK

use crate::anchoring::identity::CodeIdentityKey;
use crate::anchoring::types::{ConceptNodeId, EvidenceStrength, ObservedCodeEvidence};
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// CodeIdentityLookupError — dar read-only capability hatası (EI5-a typed distinction)
// ═══════════════════════════════════════════════════════════════════════════════

/// `CodeIdentityBindingLookup` capability hatası.
///
/// **EI5-a:** İki ayrı typed durum — `NodeNotFound` (structural inconsistency: node grafta yok)
/// ve `Unbound` (node mevcut ama fiziksel identity binding yok — normal evidence absence).
/// Adapter ([`ResolvedCodeEvidenceProvider`]) bu ayrımı explicit semantic mapping ile kullanır.
///
/// `Eq` derive YOK — PR E pattern'i (`CodeIdentityKeyError` Eq değil) ile tutarlı; future
/// genişlemede (`Ambiguous`/`SupersededBinding`/`SchemeMismatch`) Eq kırılabilir. PartialEq yeterli.
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum CodeIdentityLookupError {
    /// Structural inconsistency — node grafta yok. Adapter bunu `IdentityLookup`'a map eder.
    #[error("node not found: {0}")]
    NodeNotFound(ConceptNodeId),
    /// Node mevcut ama fiziksel identity binding yok — normal evidence absence.
    /// Adapter bunu `Ok(None)`'a map eder (gate reject, scorer zero).
    #[error("node not bound to any code identity: {0}")]
    Unbound(ConceptNodeId),
}

// ═══════════════════════════════════════════════════════════════════════════════
// ResolvedCodeIdentity — resolved value (EI1-a: exactly one key taşır)
// ═══════════════════════════════════════════════════════════════════════════════

/// `ConceptNodeId` ↔ `CodeIdentityKey` resolve edildi — attested read-model.
///
/// **EI1-a (TYPE — shape, not provenance):** Private fields + fixed struct shape → resolved value
/// exactly one key taşır. Struct literal dışarıdan kurulamaz (compile-fail
/// `cF1_resolved_code_identity_literal`). **Ancak** tip pairing'in gerçekten store'dan resolve
/// edildiğini garanti ETMEZ — trust, lookup implementation'ına ( [`CodeIdentityBindingLookup`] )
/// dayanır. Public ctor external backend'lerin trait'i implement edebilmesi için (tur 2 P1-1);
/// bu bir "verified" değil "attested" read-model'dir.
///
/// **No Deserialize:** PR E `ResolutionApplication` opacity pattern mirror.
///
/// V1 iki alan (`binding_digest`/`scheme_version`/`path_case_policy` future — kullanıcı: "sahte
/// alan ekleme"; metadata mevcut ve güvenilir değilse eklenmez).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ResolvedCodeIdentity {
    node_id: ConceptNodeId,
    identity_key: CodeIdentityKey,
}

impl ResolvedCodeIdentity {
    /// Public smart constructor — `node_id` + `identity_key` resolve edildi.
    ///
    /// Private fields sayesinde struct literal dışarıdan kurulamaz; ama capability trait
    /// implementasyonu (external backend) bu ctor'u kullanabilir.
    pub fn new(node_id: ConceptNodeId, identity_key: CodeIdentityKey) -> Self {
        Self {
            node_id,
            identity_key,
        }
    }

    /// Graph dünyası referansı (ConceptNodeId — bu resolve edilen node).
    pub fn node_id(&self) -> &ConceptNodeId {
        &self.node_id
    }

    /// Identity/evidence dünyası anahtarı (CodeIdentityKey — evidence storage key).
    pub fn identity_key(&self) -> &CodeIdentityKey {
        &self.identity_key
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeIdentityBindingLookup — dar public read-only capability (graph → identity)
// ═══════════════════════════════════════════════════════════════════════════════

/// Dar read-only capability: `ConceptNodeId → ResolvedCodeIdentity`.
///
/// **Anti-corruption boundary'nin graph tarafı.** `AnchorStore` supertrait genişletmesi DEĞİL —
/// ayrı dar port. Gate/scorer/adapter sadece bu trait'i görür; tam `AnchorStore` authority'si
/// almaz (least-authority principle).
///
/// **Fail-closed:** Node bulunamazsa `NodeNotFound`; node mevcut ama binding yoksa `Unbound`.
/// Normalize ETMEZ — bozuk/unbound state'te typed error döner.
///
/// **External extensibility:** Public trait + [`ResolvedCodeIdentity::new`] public ctor →
/// dış backend'ler (alternative store impl) bu trait'i implement edebilir.
pub trait CodeIdentityBindingLookup {
    /// `ConceptNodeId` → `ResolvedCodeIdentity` resolve et.
    ///
    /// Returns:
    /// - `Ok(ResolvedCodeIdentity)` — node mevcut + binding var
    /// - `Err(NodeNotFound)` — node grafta yok (structural inconsistency)
    /// - `Err(Unbound)` — node mevcut ama binding yok (normal evidence absence)
    fn resolve_code_identity(
        &self,
        node_id: &ConceptNodeId,
    ) -> Result<ResolvedCodeIdentity, CodeIdentityLookupError>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeEvidenceError — thiserror + serde (Patch 4 + PR F IdentityLookup varyantı)
// ═══════════════════════════════════════════════════════════════════════════════

/// Code evidence provider hatası. Object-safe trait için associated `Error` yerine tek
/// concrete error (Patch 4 — Seçenek A).
///
/// **PR F — `IdentityLookup` varyantı (EI5-b):** Adapter, `CodeIdentityLookupError`'u typed
/// propagate eder. `#[from]` hem `From<CodeIdentityLookupError>` üretür hem `Display`'de iç
/// hatayı korur. Adapter explicit `match` ile `Unbound → Ok(None)` mapping yaptığından,
/// `#[from]` footgun'unu `unbound_maps_to_none` regression-guard test pinler.
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum CodeEvidenceError {
    #[error("evidence bulunamadı: {0}")]
    NotFound(String),
    /// PR F — code identity lookup hatası (NodeNotFound structural inconsistency).
    /// `Unbound` adapter'da `Ok(None)`'a map edildiği için buraya sadece `NodeNotFound` ulaşır.
    #[error("code identity lookup failed: {0}")]
    IdentityLookup(#[from] CodeIdentityLookupError),
    #[error("internal provider hatası: {0}")]
    Internal(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeEvidenceProvider — node-facing trait (AYNEN KORUNUR — gate/scorer consumer)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir CodeEntity için observed kod kanıtı arar (INV-C6, Faz 4).
///
/// **PR F:** Trait imzası AYNEN KORUNUR — gate.rs:377 + scorer.rs:186 dokunulmaz. Node-facing
/// adapter ([`ResolvedCodeEvidenceProvider`]) bu trait'i implement eder; graph dünyasından
/// identity dünyasına geçiş adapter içinde tek noktada yapılır.
///
/// # İki method — iki kullanım (Not 5)
/// - `find_evidence` → [`AnchorGate`](crate::anchoring::gate::AnchorGate) `ImplementedBy` için
///   **evidence object varlığını** kontrol eder.
/// - `evidence_strength` → [`AnchorScorer`](crate::anchoring::scorer::AnchorScorer)
///   `code_evidence_score` (weight 0.10) için skalar gücü döndürür (PR C:
///   `minimum_observed_strength()` — normative min-over-axes).
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
// CodeEvidenceSource — key-facing evidence storage (identity dünyası)
// ═══════════════════════════════════════════════════════════════════════════════

/// Key-facing evidence storage — `CodeIdentityKey → Option<ObservedCodeEvidence>`.
///
/// **Anti-corruption boundary'nin identity tarafı.** `ConceptNodeId` kabul ETMEZ (trait boundary
/// — EI1 mimari garanti). Tek truth source: `HashMap<CodeIdentityKey, ObservedCodeEvidence>`.
///
/// Tek metod (`load`). Strength lookup `load → map(minimum_observed_strength)` ile adapter'da
/// türetilir — duplicate metod YOK.
pub trait CodeEvidenceSource {
    /// `CodeIdentityKey` için observed evidence object'i (varsa).
    fn load(
        &self,
        key: &CodeIdentityKey,
    ) -> Result<Option<ObservedCodeEvidence>, CodeEvidenceError>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeEvidenceSourceBuildError — builder fail-closed (R1a P1-2)
// ═══════════════════════════════════════════════════════════════════════════════

/// `InMemoryCodeEvidenceSource` builder hatası.
///
/// **Fail-closed (R1a P1-2):** Duplicate identity key reject — sessiz overwrite YOK.
/// `CodeIdentityKey` `Display` implement etmediği için `{0:?}` (Debug) kullanılır.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CodeEvidenceSourceBuildError {
    #[error("duplicate evidence for code identity: {0:?}")]
    DuplicateIdentity(CodeIdentityKey),
}

// ═══════════════════════════════════════════════════════════════════════════════
// InMemoryCodeEvidenceSource — key-faced evidence storage (deterministik stub)
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory, seeded, deterministik code evidence source (key-faced).
///
/// **PR F migration:** Eski `InMemoryCodeEvidenceProvider` (`HashMap<ConceptNodeId, _>`)
/// yerine key-faced storage. Tek truth source: `HashMap<CodeIdentityKey, ObservedCodeEvidence>`.
///
/// # Patch 6 — GraphSeed.code_entities otomatik evidence sayılmaz
/// Bu source **sadece explicit `ObservedCodeEvidence` seed** ile beslenir. Bir `CodeEntity`
/// node'unun [`GraphSeed`] üzerinden seed edilmiş olması kanıt üretmez. Bu, INV-C6 boundary'yi
/// korur: `CodeEntity` node varlığı ≠ observed code evidence.
///
/// # Fail-closed builder'lar (R1a P1-2)
/// `try_from_evidence` / `try_with_evidence` duplicate identity key'de `DuplicateIdentity` reject
/// eder. Explicit loop + `contains_key` kontrolü — `collect()` sessiz overwrite eder (kullanılmaz).
#[derive(Debug, Clone, Default)]
pub struct InMemoryCodeEvidenceSource {
    evidence: HashMap<CodeIdentityKey, ObservedCodeEvidence>,
}

impl InMemoryCodeEvidenceSource {
    /// Boş source — tüm lookups kanıt yok (default/Faz 1-2 backward-compat).
    pub fn empty() -> Self {
        Self {
            evidence: HashMap::new(),
        }
    }

    /// Explicit observed evidence seed ile source oluştur (fail-closed).
    ///
    /// Duplicate identity key → `DuplicateIdentity` reject. Explicit loop + `contains_key`
    /// (R1a P1-2 — `collect()` sessiz overwrite eder, kullanılmaz).
    pub fn try_from_evidence(
        evidence: Vec<ObservedCodeEvidence>,
    ) -> Result<Self, CodeEvidenceSourceBuildError> {
        let mut map: HashMap<CodeIdentityKey, ObservedCodeEvidence> =
            HashMap::with_capacity(evidence.len());
        for item in evidence {
            let key = item.code_identity_key().clone();
            if map.contains_key(&key) {
                return Err(CodeEvidenceSourceBuildError::DuplicateIdentity(key));
            }
            map.insert(key, item);
        }
        Ok(Self { evidence: map })
    }

    /// Explicit evidence ekle (builder pattern, fail-closed).
    ///
    /// Duplicate identity key → `DuplicateIdentity` reject. Builder semantiği: duplicate
    /// successful overwritten source üretmez (R1a P2-4 — unchanged-on-error iddiası Builder'da
    /// doğrulanamaz ama yanlış da üretmez).
    pub fn try_with_evidence(
        mut self,
        evidence: ObservedCodeEvidence,
    ) -> Result<Self, CodeEvidenceSourceBuildError> {
        let key = evidence.code_identity_key().clone();
        if self.evidence.contains_key(&key) {
            return Err(CodeEvidenceSourceBuildError::DuplicateIdentity(key));
        }
        self.evidence.insert(key, evidence);
        Ok(self)
    }

    /// Seed'deki evidence sayısı (test/diagnostic).
    pub fn evidence_count(&self) -> usize {
        self.evidence.len()
    }
}

impl CodeEvidenceSource for InMemoryCodeEvidenceSource {
    fn load(
        &self,
        key: &CodeIdentityKey,
    ) -> Result<Option<ObservedCodeEvidence>, CodeEvidenceError> {
        Ok(self.evidence.get(key).cloned())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ResolvedCodeEvidenceProvider — node-facing adapter (graph → identity compose)
// ═══════════════════════════════════════════════════════════════════════════════

/// Node-facing adapter — [`CodeIdentityBindingLookup`] + [`CodeEvidenceSource`] compose eder.
///
/// **Anti-corruption boundary'nin compose noktası.** Graph dünyası (`ConceptNodeId`) ile
/// identity/evidence dünyası (`CodeIdentityKey`) arasındaki geçiş tek noktada yapılır:
/// 1. `lookup.resolve_code_identity(node_id)` → `ResolvedCodeIdentity` (EI5 typed error)
/// 2. `source.load(&resolved.identity_key())` → evidence
///
/// **EI5-b explicit semantic mapping:**
/// - `Unbound → Ok(None)` (normal evidence absence — gate reject, scorer zero)
/// - `NodeNotFound → Err(IdentityLookup)` (structural inconsistency)
///
/// **Public ctor (tur 2 P1-4):** `pub fn new(lookup, source)` — osp-cli ve dış integration
/// testleri adapter oluşturabilir.
///
/// **Object-safety:** Generic adapter monomorphization'da `&dyn CodeEvidenceProvider`'a coerce
/// olur (tur 2 P2-2). V1 için `?Sized` gerekmez.
pub struct ResolvedCodeEvidenceProvider<'a, L, S>
where
    L: CodeIdentityBindingLookup,
    S: CodeEvidenceSource,
{
    lookup: &'a L,
    source: &'a S,
}

impl<'a, L, S> ResolvedCodeEvidenceProvider<'a, L, S>
where
    L: CodeIdentityBindingLookup,
    S: CodeEvidenceSource,
{
    /// Adapter oluştur — lookup (graph → identity) + source (identity → evidence).
    pub fn new(lookup: &'a L, source: &'a S) -> Self {
        Self { lookup, source }
    }
}

impl<'a, L, S> CodeEvidenceProvider for ResolvedCodeEvidenceProvider<'a, L, S>
where
    L: CodeIdentityBindingLookup,
    S: CodeEvidenceSource,
{
    fn find_evidence(
        &self,
        code_entity_id: &ConceptNodeId,
    ) -> Result<Option<ObservedCodeEvidence>, CodeEvidenceError> {
        // EI5-b: explicit semantic mapping. Unbound → Ok(None) (normal evidence absence);
        // NodeNotFound → IdentityLookup (structural inconsistency).
        // NOT: `?` KULLANMA — `#[from]` footgun (Unbound → IdentityLookup sessiz collapse).
        // `unbound_maps_to_none` regression-guard test bu mapping'i pinler.
        match self.lookup.resolve_code_identity(code_entity_id) {
            Ok(resolved) => self.source.load(resolved.identity_key()),
            Err(CodeIdentityLookupError::Unbound(_)) => Ok(None),
            Err(e @ CodeIdentityLookupError::NodeNotFound(_)) => {
                Err(CodeEvidenceError::IdentityLookup(e))
            }
        }
    }

    fn evidence_strength(
        &self,
        code_entity_id: &ConceptNodeId,
    ) -> Result<EvidenceStrength, CodeEvidenceError> {
        Ok(
            self.find_evidence(code_entity_id)?
                .map_or_else(EvidenceStrength::zero, |ev| {
                    ev.observations().minimum_observed_strength()
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    //! code_evidence.rs unit testleri — PR F evidence identity layer.
    //!
    //! Katman 2 (smart-constructor + EI5-b footgun guard):
    //! - `ResolvedCodeIdentity::new` + accessor
    //! - `InMemoryCodeEvidenceSource` builders (try_from_evidence, try_with_evidence, fail-closed)
    //! - `CodeIdentityBindingLookup` trait stub (NodeNotFound/Unbound)
    //! - `ResolvedCodeEvidenceProvider` adapter delegation + EI5-b mapping
    //! - `unbound_maps_to_none` pin test (R2 P2-A `#[from]` footgun guard)
    //! - EvidenceStrength serde boundary (PR C preserved)

    use super::*;
    use crate::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
    use crate::anchoring::types::{
        ConceptNodeId, EvidenceCoverage, EvidenceStrength, ObservedCodeEvidence,
        ObservedCodeMetricSource, ObservedPhysicalMetric, ObservedPhysicalMetrics,
    };
    use crate::anchoring::PhysicalCodeMetricAxis;

    // ─────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Test identity key üret (CaseSensitive — key olduğu gibi).
    fn identity_key(key: &str) -> CodeIdentityKey {
        CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::CaseSensitive,
            },
            key,
        )
        .expect("test key geçerli")
    }

    /// auth_service observation'ları — entropy/witness **representative normalized**
    /// (PR C: 1.1/5.0 raw → 0.52/0.68). 5 eksende de uniform [0,1].
    fn auth_service_observations() -> ObservedPhysicalMetrics {
        let strength = EvidenceStrength::new(0.85).unwrap();
        let coverage = EvidenceCoverage::new(1.0).unwrap();
        let scip = ObservedCodeMetricSource::Scip;
        ObservedPhysicalMetrics::try_new(vec![
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Coupling, 0.42, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Cohesion, 0.78, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Instability, 0.30, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Entropy, 0.52, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::WitnessDepth, 0.68, scip, strength, coverage).unwrap(),
        ])
        .unwrap()
    }

    fn auth_service_evidence() -> ObservedCodeEvidence {
        ObservedCodeEvidence::new(
            identity_key("CodeEntity:AuthService"),
            auth_service_observations(),
            1_700_000_000,
        )
    }

    /// Tek-eksen observation helper.
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

    /// Stub lookup — test lookup davranışları için.
    struct StubLookup {
        node_exists: bool,
        binding: Option<CodeIdentityKey>,
    }

    impl CodeIdentityBindingLookup for StubLookup {
        fn resolve_code_identity(
            &self,
            node_id: &ConceptNodeId,
        ) -> Result<ResolvedCodeIdentity, CodeIdentityLookupError> {
            if !self.node_exists {
                return Err(CodeIdentityLookupError::NodeNotFound(node_id.clone()));
            }
            match &self.binding {
                Some(key) => Ok(ResolvedCodeIdentity::new(node_id.clone(), key.clone())),
                None => Err(CodeIdentityLookupError::Unbound(node_id.clone())),
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ResolvedCodeIdentity (EI1-a)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn resolved_code_identity_new_and_accessors() {
        let node_id = ConceptNodeId("CodeEntity:AuthService".into());
        let key = identity_key("CodeEntity:AuthService");
        let resolved = ResolvedCodeIdentity::new(node_id.clone(), key.clone());
        assert_eq!(resolved.node_id(), &node_id);
        assert_eq!(resolved.identity_key(), &key);
    }

    #[test]
    fn resolved_code_identity_partial_eq_same_key() {
        let node_id = ConceptNodeId("CodeEntity:X".into());
        let key = identity_key("k");
        let a = ResolvedCodeIdentity::new(node_id.clone(), key.clone());
        let b = ResolvedCodeIdentity::new(node_id, key);
        assert_eq!(a, b);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // InMemoryCodeEvidenceSource builders (fail-closed — R1a P1-2)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn empty_source_loads_none_and_zero_strength() {
        let source = InMemoryCodeEvidenceSource::empty();
        let key = identity_key("CodeEntity:X");
        assert_eq!(source.evidence_count(), 0);
        assert!(source.load(&key).unwrap().is_none());
    }

    /// Empty source — hiç evidence yüklenmediyse load her zaman None.
    ///
    /// Bu, INV-C6 Patch 6 boundary'nin source katmanı: explicit `ObservedCodeEvidence` seed
    /// gerekir. Gerçek graph presence (GraphSeed.code_entities varlığı ≠ evidence) test'i
    /// `resolution_identity_integration_tests::graphseed_presence_does_not_produce_evidence`
    /// içinde gerçek `InMemoryAnchorStore` + adapter ile doğrulanır.
    #[test]
    fn empty_source_does_not_infer_evidence() {
        let source = InMemoryCodeEvidenceSource::empty();
        let key = identity_key("CodeEntity:PaymentModule");
        assert!(
            source.load(&key).unwrap().is_none(),
            "empty source → load None (explicit evidence seed gerekir)"
        );
        assert_eq!(source.evidence_count(), 0);
    }

    #[test]
    fn try_from_evidence_loads_by_identity_key() {
        let source = InMemoryCodeEvidenceSource::try_from_evidence(vec![auth_service_evidence()])
            .expect("mutlu yol");
        assert_eq!(source.evidence_count(), 1);
        let key = identity_key("CodeEntity:AuthService");
        let ev = source.load(&key).unwrap().expect("evidence mevcut");
        assert_eq!(ev.code_identity_key(), &key);
        assert_eq!(ev.measured_at(), 1_700_000_000);
        // PR C: axis-granular observations.
        let cohesion = ev
            .observations()
            .values()
            .iter()
            .find(|o| o.axis() == PhysicalCodeMetricAxis::Cohesion)
            .expect("Cohesion axis mevcut");
        assert_eq!(cohesion.value().get(), 0.78);
    }

    #[test]
    fn try_from_evidence_rejects_duplicate_identity() {
        // İki evidence aynı identity key → fail-closed reject.
        let ev1 = ObservedCodeEvidence::new(
            identity_key("CodeEntity:X"),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.1,
                ObservedCodeMetricSource::TreeSitter,
                EvidenceStrength::new(0.5).unwrap(),
            ),
            100,
        );
        let ev2 = ObservedCodeEvidence::new(
            identity_key("CodeEntity:X"),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.9,
                ObservedCodeMetricSource::Scip,
                EvidenceStrength::new(0.95).unwrap(),
            ),
            200,
        );
        let result = InMemoryCodeEvidenceSource::try_from_evidence(vec![ev1, ev2]);
        assert!(
            matches!(result, Err(CodeEvidenceSourceBuildError::DuplicateIdentity(_))),
            "duplicate identity key → fail-closed reject (sessiz overwrite YOK)"
        );
    }

    #[test]
    fn try_with_evidence_builder_rejects_duplicate() {
        let ev1 = ObservedCodeEvidence::new(
            identity_key("CodeEntity:X"),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.1,
                ObservedCodeMetricSource::TreeSitter,
                EvidenceStrength::new(0.5).unwrap(),
            ),
            100,
        );
        let ev2 = ObservedCodeEvidence::new(
            identity_key("CodeEntity:X"),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.9,
                ObservedCodeMetricSource::Scip,
                EvidenceStrength::new(0.95).unwrap(),
            ),
            200,
        );
        let result = InMemoryCodeEvidenceSource::empty().try_with_evidence(ev1).unwrap();
        let duplicate = result.try_with_evidence(ev2);
        assert!(
            matches!(duplicate, Err(CodeEvidenceSourceBuildError::DuplicateIdentity(_))),
            "builder duplicate → fail-closed reject"
        );
    }

    #[test]
    fn try_with_evidence_builder_distinct_keys_succeeds() {
        let ev1 = ObservedCodeEvidence::new(
            identity_key("CodeEntity:A"),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.1,
                ObservedCodeMetricSource::TreeSitter,
                EvidenceStrength::new(0.5).unwrap(),
            ),
            100,
        );
        let ev2 = ObservedCodeEvidence::new(
            identity_key("CodeEntity:B"),
            single_axis_observations(
                PhysicalCodeMetricAxis::Coupling,
                0.9,
                ObservedCodeMetricSource::Scip,
                EvidenceStrength::new(0.95).unwrap(),
            ),
            200,
        );
        let source = InMemoryCodeEvidenceSource::empty()
            .try_with_evidence(ev1)
            .unwrap()
            .try_with_evidence(ev2)
            .unwrap();
        assert_eq!(source.evidence_count(), 2);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Adapter delegation + EI5-b mapping + footgun guard
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn adapter_finds_evidence_through_lookup_and_source() {
        // Mutlu yol: node exists + binding + evidence loaded.
        let key = identity_key("CodeEntity:AuthService");
        let lookup = StubLookup {
            node_exists: true,
            binding: Some(key.clone()),
        };
        let source = InMemoryCodeEvidenceSource::try_from_evidence(vec![auth_service_evidence()])
            .unwrap();
        let adapter = ResolvedCodeEvidenceProvider::new(&lookup, &source);
        let node_id = ConceptNodeId("CodeEntity:AuthService".into());
        let ev = adapter
            .find_evidence(&node_id)
            .unwrap()
            .expect("evidence mevcut");
        assert_eq!(ev.code_identity_key(), &key);
        // strength = minimum_observed_strength = 0.85 (tüm eksenlerde).
        assert_eq!(adapter.evidence_strength(&node_id).unwrap().get(), 0.85);
    }

    #[test]
    fn adapter_unbound_maps_to_none() {
        // EI5-b: Unbound → Ok(None). R2 P2-A footgun guard — `#[from]` ile `?` kullanılsa
        // Idessiz IdentityLookup'a collapse ederdi; explicit match pinler.
        let lookup = StubLookup {
            node_exists: true,
            binding: None, // node mevcut ama binding yok
        };
        let source = InMemoryCodeEvidenceSource::empty();
        let adapter = ResolvedCodeEvidenceProvider::new(&lookup, &source);
        let node_id = ConceptNodeId("CodeEntity:Unbound".into());
        assert!(
            adapter.find_evidence(&node_id).unwrap().is_none(),
            "Unbound → Ok(None) (normal evidence absence)"
        );
        assert_eq!(
            adapter.evidence_strength(&node_id).unwrap().get(),
            0.0,
            "Unbound → evidence_strength zero"
        );
    }

    #[test]
    fn adapter_node_not_found_maps_to_identity_lookup_error() {
        // EI5-b: NodeNotFound → Err(IdentityLookup) (structural inconsistency).
        let lookup = StubLookup {
            node_exists: false,
            binding: None,
        };
        let source = InMemoryCodeEvidenceSource::empty();
        let adapter = ResolvedCodeEvidenceProvider::new(&lookup, &source);
        let node_id = ConceptNodeId("CodeEntity:Ghost".into());
        let result = adapter.find_evidence(&node_id);
        assert!(
            matches!(result, Err(CodeEvidenceError::IdentityLookup(_))),
            "NodeNotFound → IdentityLookup (structural inconsistency)"
        );
    }

    #[test]
    fn adapter_evidence_strength_zero_when_source_has_no_evidence() {
        // Node bound ama source'ta evidence yok → Ok(None) → strength zero.
        let key = identity_key("CodeEntity:Bound");
        let lookup = StubLookup {
            node_exists: true,
            binding: Some(key),
        };
        let source = InMemoryCodeEvidenceSource::empty();
        let adapter = ResolvedCodeEvidenceProvider::new(&lookup, &source);
        let node_id = ConceptNodeId("CodeEntity:Bound".into());
        assert_eq!(
            adapter.evidence_strength(&node_id).unwrap().get(),
            0.0,
            "bound node + no evidence → strength zero"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CodeEvidenceError — IdentityLookup #[from] propagation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn identity_lookup_error_converts_via_from() {
        // #[from] üretir: CodeIdentityLookupError → CodeEvidenceError::IdentityLookup.
        let node_id = ConceptNodeId("CodeEntity:X".into());
        let lookup_err = CodeIdentityLookupError::NodeNotFound(node_id);
        let provider_err: CodeEvidenceError = lookup_err.into();
        assert!(matches!(provider_err, CodeEvidenceError::IdentityLookup(_)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EvidenceStrength serde boundary (PR C preserved — regression guard)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn evidence_strength_out_of_range_rejects_nan_inf_negative() {
        assert!(EvidenceStrength::new(f64::NAN).is_err());
        assert!(EvidenceStrength::new(f64::INFINITY).is_err());
        assert!(EvidenceStrength::new(f64::NEG_INFINITY).is_err());
        assert!(EvidenceStrength::new(-0.01).is_err());
        assert!(EvidenceStrength::new(1.01).is_err());
        assert!(EvidenceStrength::new(0.0).is_ok());
        assert!(EvidenceStrength::new(1.0).is_ok());
    }

    #[test]
    fn evidence_strength_serde_rejects_out_of_range() {
        assert!(serde_json::from_str::<EvidenceStrength>("2.0").is_err());
        assert!(serde_json::from_str::<EvidenceStrength>("-1.0").is_err());
        assert!(serde_json::from_str::<EvidenceStrength>("\"NaN\"").is_err());
    }

    #[test]
    fn evidence_strength_serde_roundtrip_valid() {
        let original = EvidenceStrength::new(0.85).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: EvidenceStrength = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
        assert_eq!(restored.get(), 0.85);
    }

    #[test]
    fn observed_code_evidence_accessors() {
        let ev = auth_service_evidence();
        assert_eq!(ev.code_identity_key(), &identity_key("CodeEntity:AuthService"));
        assert_eq!(ev.measured_at(), 1_700_000_000);
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
        assert_eq!(
            ev.observations().minimum_observed_strength().get(),
            0.85
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PR F review P1-2 — N:1 resolution + evidence identity integration tests.
//
// Bu modül gerçek `InMemoryAnchorStore` kullanır (StubLookup DEĞİL). EI1-b, EI2, EI3-b,
// EI4-c, EI6, EI7 invariant'larını tek bir resolution + evidence zincirinde kanıtlar.
// Setup helper'lar review.rs `store_with_resolvable_candidate` (line 2903-2938) mirror.
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod resolution_identity_integration_tests {
    use super::*;
    use crate::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
    use crate::anchoring::review::{
        CodeEntityResolutionSession, OperatorId, PresentedResolutionBasis, ResolutionOutcome,
    };
    use crate::anchoring::store::{AnchorStore, InMemoryAnchorStore};
    use crate::anchoring::types::{
        CodeIdentityBinding, ConceptNode, ConceptNodeKind, ConceptNodeId, EvidenceCoverage,
        EvidenceStrength, GraphSeed, ObservedCodeMetricSource, ObservedPhysicalMetric,
        ObservedPhysicalMetrics,
    };
    use crate::anchoring::{DecisionStatus, NonEmptyExplanation, PhysicalCodeMetricAxis, PositionFamily};

    /// Accepted CodeEntityCandidate node (PhysicalCode family).
    fn accepted_candidate(path: &str) -> ConceptNode {
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
    fn store_with_candidate(path: &str, case_policy: CodePathCasePolicy) -> InMemoryAnchorStore {
        let mut seed = GraphSeed::default();
        seed.code_entities.push(accepted_candidate(path));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        let key = CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 { case_policy },
            path,
        )
        .unwrap();
        store
            .seed_code_identity_bindings_trusted(&[CodeIdentityBinding {
                node_id: ConceptNodeId(format!("CodeEntityCandidate:{path}")),
                identity_key: key,
            }])
            .unwrap();
        store
    }

    /// 5-axis observation helper (uniform strength).
    fn full_observations(strength_val: f64) -> ObservedPhysicalMetrics {
        let strength = EvidenceStrength::new(strength_val).unwrap();
        let coverage = EvidenceCoverage::new(1.0).unwrap();
        let scip = ObservedCodeMetricSource::Scip;
        ObservedPhysicalMetrics::try_new(vec![
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Coupling, 0.4, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Cohesion, 0.5, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Instability, 0.3, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::Entropy, 0.5, scip, strength, coverage).unwrap(),
            ObservedPhysicalMetric::new(PhysicalCodeMetricAxis::WitnessDepth, 0.6, scip, strength, coverage).unwrap(),
        ])
        .unwrap()
    }

    /// Test identity key üret.
    fn identity_key(key: &str) -> CodeIdentityKey {
        CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::CaseSensitive,
            },
            key,
        )
        .unwrap()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EI1-b: gerçek store binding lookup ile çözülür
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn real_store_resolves_bound_identity_key() {
        // EI1-b: InMemoryAnchorStore::resolve_code_identity gerçek binding döner.
        let store = store_with_candidate("src/auth.rs", CodePathCasePolicy::CaseSensitive);
        let node_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());

        let resolved = store.resolve_code_identity(&node_id).unwrap();
        assert_eq!(resolved.node_id(), &node_id);
        assert_eq!(
            resolved.identity_key().canonical_key(),
            "src/auth.rs",
            "identity key binding'den resolve edildi"
        );
    }

    #[test]
    fn real_store_unbound_node_returns_unbound_error() {
        // EI1-b negatif: node mevcut ama binding yok → Unbound.
        let mut seed = GraphSeed::default();
        seed.code_entities.push(accepted_candidate("src/bound.rs"));
        // binding seed'leme → Unbound beklenir.
        let store = InMemoryAnchorStore::with_seed(seed);
        let node_id = ConceptNodeId("CodeEntityCandidate:src/bound.rs".into());
        let err = store.resolve_code_identity(&node_id).unwrap_err();
        assert!(matches!(err, CodeIdentityLookupError::Unbound(_)));
    }

    #[test]
    fn real_store_missing_node_returns_node_not_found() {
        // EI1-b negatif: node grafta yok → NodeNotFound.
        let store = InMemoryAnchorStore::new();
        let ghost = ConceptNodeId("CodeEntityCandidate:ghost.rs".into());
        let err = store.resolve_code_identity(&ghost).unwrap_err();
        assert!(matches!(err, CodeIdentityLookupError::NodeNotFound(_)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EI2 + EI4-c + EI7: N:1 resolution + evidence identity triangulation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn n1_resolution_two_candidates_same_key_share_evidence() {
        // EI4-c + EI2 + EI7: iki Accepted candidate aynı identity key'e bağlı →
        // resolve sonrası her ikisi de aynı entity'ye converge ve aynı evidence görür.
        //
        // Setup: candidate-a + candidate-b aynı key (AsciiCaseInsensitive: src/auth.rs == src/Auth.rs).
        let mut store = store_with_candidate("src/auth.rs", CodePathCasePolicy::AsciiCaseInsensitive);

        // İkinci candidate ekle (aynı identity key, farklı path spelling).
        let candidate_b_path = "src/Auth.rs";
        let mut seed2 = GraphSeed::default();
        seed2.code_entities.push(accepted_candidate(candidate_b_path));
        store.seed_trusted(&seed2).unwrap();
        store
            .seed_code_identity_bindings_trusted(&[CodeIdentityBinding {
                node_id: ConceptNodeId(format!("CodeEntityCandidate:{candidate_b_path}")),
                identity_key: CodeIdentityKey::new(
                    CodeIdentityScheme::AnalysisPathV1 {
                        case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
                    },
                    "src/auth.rs", // canonical lowercase — aynı key
                )
                .unwrap(),
            }])
            .unwrap();

        // Shared identity key (canonical lowercase).
        let shared_key = CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
            },
            "src/auth.rs",
        )
        .unwrap();

        // Evidence source: tek evidence (shared key).
        let evidence = ObservedCodeEvidence::new(
            shared_key.clone(),
            full_observations(0.85),
            1_700_000_000,
        );
        let source = InMemoryCodeEvidenceSource::try_from_evidence(vec![evidence]).unwrap();

        // Adapter: store lookup + source → CodeEvidenceProvider.
        let adapter = ResolvedCodeEvidenceProvider::new(&store, &source);

        let candidate_a = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let candidate_b = ConceptNodeId("CodeEntityCandidate:src/Auth.rs".into());

        // EI2: candidate-a ve candidate-b aynı evidence object görür (identity-owned).
        let ev_a = adapter.find_evidence(&candidate_a).unwrap().expect("evidence a");
        let ev_b = adapter.find_evidence(&candidate_b).unwrap().expect("evidence b");
        assert_eq!(
            ev_a.code_identity_key(),
            ev_b.code_identity_key(),
            "EI2: iki candidate aynı identity-owned evidence görür"
        );
        assert_eq!(ev_a, ev_b, "EI2: evidence object birebir aynı");

        // EI7: strength eşit (shared key ownership).
        let strength_a = adapter.evidence_strength(&candidate_a).unwrap();
        let strength_b = adapter.evidence_strength(&candidate_b).unwrap();
        assert_eq!(
            strength_a, strength_b,
            "EI7: candidate/entity strength eşit (shared key ownership)"
        );
        assert_eq!(strength_a.get(), 0.85);
    }

    #[test]
    fn n1_resolution_creates_then_reuses_same_entity_and_evidence() {
        // EI4-c: resolve candidate-a → Created entity; resolve candidate-b → Reused same entity.
        // Resolution sonrası entity node da aynı identity key'e bound → aynı evidence.
        let mut store = store_with_candidate("src/auth.rs", CodePathCasePolicy::AsciiCaseInsensitive);

        // İkinci candidate (aynı key).
        let candidate_b_path = "src/Auth.rs";
        let mut seed2 = GraphSeed::default();
        seed2.code_entities.push(accepted_candidate(candidate_b_path));
        store.seed_trusted(&seed2).unwrap();
        store
            .seed_code_identity_bindings_trusted(&[CodeIdentityBinding {
                node_id: ConceptNodeId(format!("CodeEntityCandidate:{candidate_b_path}")),
                identity_key: CodeIdentityKey::new(
                    CodeIdentityScheme::AnalysisPathV1 {
                        case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
                    },
                    "src/auth.rs",
                )
                .unwrap(),
            }])
            .unwrap();

        // Shared key.
        let shared_key = CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
            },
            "src/auth.rs",
        )
        .unwrap();

        // Evidence source.
        let evidence = ObservedCodeEvidence::new(
            shared_key.clone(),
            full_observations(0.85),
            1_700_000_000,
        );
        let source = InMemoryCodeEvidenceSource::try_from_evidence(vec![evidence]).unwrap();

        let candidate_a = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let candidate_b = ConceptNodeId("CodeEntityCandidate:src/Auth.rs".into());

        // EI3-b: evidence_count before resolution.
        let count_before = source.evidence_count();

        // Resolve candidate-a → Created entity.
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis_a = PresentedResolutionBasis::compile(&store, &candidate_a).unwrap();
        let record_a = session
            .resolve(
                &mut store,
                &candidate_a,
                basis_a,
                NonEmptyExplanation::new("first").unwrap(),
            )
            .unwrap();
        assert!(matches!(record_a.outcome, ResolutionOutcome::Created { .. }));
        let entity_id = record_a.entity_id.clone();

        // Resolve candidate-b → Reused same entity.
        let basis_b = PresentedResolutionBasis::compile(&store, &candidate_b).unwrap();
        let record_b = session
            .resolve(
                &mut store,
                &candidate_b,
                basis_b,
                NonEmptyExplanation::new("second").unwrap(),
            )
            .unwrap();
        assert!(
            matches!(record_b.outcome, ResolutionOutcome::Reused { .. }),
            "EI4-c: ikinci candidate reuse ile aynı entity'ye converge"
        );
        assert_eq!(record_b.entity_id, entity_id, "N:1 → same entity ID");

        // EI3-b: evidence_count unchanged — resolution source'a erişmedi.
        let adapter = ResolvedCodeEvidenceProvider::new(&store, &source);
        let count_after = source.evidence_count();
        assert_eq!(
            count_before, count_after,
            "EI3-b: resolution source cardinality değiştirmez"
        );

        // EI2: candidate-a, candidate-b, ve resolved entity aynı evidence görür.
        let ev_candidate_a = adapter.find_evidence(&candidate_a).unwrap().unwrap();
        let ev_candidate_b = adapter.find_evidence(&candidate_b).unwrap().unwrap();
        let ev_entity = adapter.find_evidence(&entity_id).unwrap().unwrap();
        assert_eq!(
            ev_candidate_a.code_identity_key(),
            ev_entity.code_identity_key(),
            "EI2: candidate-a ve entity aynı identity-owned evidence"
        );
        assert_eq!(
            ev_candidate_b.code_identity_key(),
            ev_entity.code_identity_key(),
            "EI2: candidate-b ve entity aynı identity-owned evidence"
        );
        assert_eq!(ev_candidate_a, ev_entity);
        assert_eq!(ev_candidate_b, ev_entity);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EI6: snapshot restore sonrası candidate/entity/gate/scorer eşitliği
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn snapshot_restore_preserves_identity_lookup_and_evidence() {
        // EI6: snapshot export → restore sonrası binding lookup + evidence aynı kalır.
        let mut store = store_with_candidate("src/auth.rs", CodePathCasePolicy::CaseSensitive);
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());

        // Resolve → entity materialize.
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        let record = session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("resolve").unwrap(),
            )
            .unwrap();
        let entity_id = record.entity_id.clone();

        // Snapshot export → restore.
        let snapshot = store.export_snapshot();
        let restored = InMemoryAnchorStore::restore_snapshot(snapshot).expect("restore valid");

        // Shared key + evidence.
        let key = identity_key("src/auth.rs");
        let evidence = ObservedCodeEvidence::new(key.clone(), full_observations(0.85), 1_700_000_000);
        let source = InMemoryCodeEvidenceSource::try_from_evidence(vec![evidence]).unwrap();

        // Original store adapter.
        let adapter_orig = ResolvedCodeEvidenceProvider::new(&store, &source);
        // Restored store adapter.
        let adapter_restored = ResolvedCodeEvidenceProvider::new(&restored, &source);

        // EI6: candidate lookup aynı.
        let resolved_orig = adapter_orig
            .find_evidence(&candidate_id)
            .unwrap()
            .unwrap();
        let resolved_restored = adapter_restored
            .find_evidence(&candidate_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved_orig, resolved_restored,
            "EI6: candidate lookup restore sonrası aynı"
        );

        // EI6: entity lookup aynı.
        let entity_orig = adapter_orig.find_evidence(&entity_id).unwrap().unwrap();
        let entity_restored = adapter_restored.find_evidence(&entity_id).unwrap().unwrap();
        assert_eq!(
            entity_orig, entity_restored,
            "EI6: entity lookup restore sonrası aynı"
        );

        // EI6: binding resolve aynı identity key döner.
        let binding_orig = store.resolve_code_identity(&entity_id).unwrap();
        let binding_restored = restored.resolve_code_identity(&entity_id).unwrap();
        assert_eq!(
            binding_orig.identity_key(),
            binding_restored.identity_key(),
            "EI6: binding identity key restore sonrası aynı"
        );

        // EI6 (review P2-1 Seçenek A): evidence_strength equality — scorer consumer'ın
        // gördüğü skalar da restore sonrası aynı. find_evidence equality zaten yukarıda
        // kanıtlandı; strength türetilmiş skalar ama ayrı assertion matrisi dürüstleştirir.
        let strength_orig_candidate = adapter_orig.evidence_strength(&candidate_id).unwrap();
        let strength_restored_candidate = adapter_restored.evidence_strength(&candidate_id).unwrap();
        assert_eq!(
            strength_orig_candidate, strength_restored_candidate,
            "EI6: candidate evidence_strength restore sonrası aynı (scorer consumer)"
        );
        let strength_orig_entity = adapter_orig.evidence_strength(&entity_id).unwrap();
        let strength_restored_entity = adapter_restored.evidence_strength(&entity_id).unwrap();
        assert_eq!(
            strength_orig_entity, strength_restored_entity,
            "EI6: entity evidence_strength restore sonrası aynı (scorer consumer)"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EI8-V1: graph absence/unbound → key-owned evidence mutasyonu YOK
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn graph_absence_does_not_mutate_key_owned_evidence() {
        // EI8-V1: graph'ta olmayan node + unbound node lookup'ları source içeriğini değiştirmez.
        let store = store_with_candidate("src/auth.rs", CodePathCasePolicy::CaseSensitive);
        let key = identity_key("src/auth.rs");
        let evidence = ObservedCodeEvidence::new(key.clone(), full_observations(0.85), 1_700_000_000);
        let source = InMemoryCodeEvidenceSource::try_from_evidence(vec![evidence]).unwrap();
        let adapter = ResolvedCodeEvidenceProvider::new(&store, &source);

        let count_before = source.evidence_count();

        // Ghost node (grafta yok) → NodeNotFound → IdentityLookup error.
        let ghost = ConceptNodeId("CodeEntityCandidate:ghost.rs".into());
        let _ = adapter.find_evidence(&ghost);

        // Unbound node (grafta var, binding yok).
        let mut seed_unbound = GraphSeed::default();
        seed_unbound.code_entities.push(accepted_candidate("src/unbound.rs"));
        let store2 = InMemoryAnchorStore::with_seed(seed_unbound);
        // Binding seed'leme → unbound.
        let unbound = ConceptNodeId("CodeEntityCandidate:src/unbound.rs".into());
        let adapter2 = ResolvedCodeEvidenceProvider::new(&store2, &source);
        let _ = adapter2.find_evidence(&unbound);

        // EI8-V1: source içeriği değişmedi.
        assert_eq!(
            source.evidence_count(),
            count_before,
            "EI8-V1: graph absence/unbound lookup'ları key-owned evidence'ı mutasyona uğratmaz"
        );

        // Source hâlâ key ile lookup edilebilir.
        let ev = source.load(&key).unwrap();
        assert!(ev.is_some(), "key-owned evidence hâlâ mevcut");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // INV-C6 Patch 6: GraphSeed.code_entities presence ≠ evidence (review P2-3)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn graphseed_presence_does_not_produce_evidence() {
        // INV-C6 Patch 6 (review P2-3): candidate node + binding gerçekten var olduğu halde
        // source'a explicit evidence yüklenmediyse adapter None/zero döner. Graph presence
        // evidence üretmez — explicit ObservedCodeEvidence seed gerekir.
        let store = store_with_candidate("src/auth.rs", CodePathCasePolicy::CaseSensitive);
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());

        // Source boş — explicit evidence yok.
        let source = InMemoryCodeEvidenceSource::empty();
        let adapter = ResolvedCodeEvidenceProvider::new(&store, &source);

        // Adapter: node + binding var (resolve_code_identity Ok döner) ama source'ta evidence yok.
        assert_eq!(
            store.resolve_code_identity(&candidate_id).unwrap().identity_key().canonical_key(),
            "src/auth.rs",
            "node + binding mevcut (lookup Ok)"
        );
        assert!(
            adapter.find_evidence(&candidate_id).unwrap().is_none(),
            "INV-C6 Patch 6: graph presence ≠ evidence; explicit seed gerekir"
        );
        assert_eq!(
            adapter.evidence_strength(&candidate_id).unwrap().get(),
            0.0,
            "INV-C6 Patch 6: graph presence → evidence_strength zero"
        );
    }
}
