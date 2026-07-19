//! Somut eksen gerçeklemeleri (OSP-formalism.md §2).
//!
//! **Faz 1.2 (bu modül):** 4 eksen — `x` (Coupling), `w` (Entropy), `v` (WitnessDepth),
//! `u` (VisionAlignment). Faz 0 spike metriklerinden (`osp-spike`) taşındı.
//!
//! **Faz 1.3 (sıradaki):** `y` (CohesionAxis, LCOM4 tree-sitter pseudo-type) +
//! `z` (InstabilityAxis, Martin `I×(1−D)`) → `CoordinateSystem::default_six()`.
//!
//! ## Per-node vs Repo-level
//! - **Per-node** (CouplingAxis): `compute` her düğüm için farklı değer döner (space'ten).
//! - **Repo-level** (Entropy/WitnessDepth/VisionAlignment): construction'da değer alır,
//!   tüm düğümlere aynı değeri döner. Faz 0'daki repo-aggregate metriklerin izdüşümü.

use crate::coords::{
    validate_direct_source, Axis, AxisDescriptor, AxisDescriptorError, AxisMeasurement,
    AxisMeasurementError, AxisParameterEncoder, AxisRegistrationError, AxisSourceError,
    CoordinateSystem, MetricSource,
};
use crate::space::{EdgeKind, Node, Space};

// ═══════════════════════════════════════════════════════════════════════════════
// x — Kuplaj (per-node)
// ═══════════════════════════════════════════════════════════════════════════════

/// `x` ekseni — **Kuplaj**.
///
/// Per-node: düğümün `Imports` out-degree'inin soft-normalize'u.
///
/// `x = out_degree(Imports) / (1 + out_degree(Imports))` ∈ [0, 1)
///
/// Not: Faz 0 spike'taki repo-level `κ = edges/nodes` ayrı bir aggregate'tir
/// (repo pozisyonu hesabında kullanılır, Faz 1.8). Bu eksen her modülün KENDİ
/// kuplajını ölçer.
///
/// **INV-T9 #70:** Construction-time source — graph topology kökenini bağlar.
/// `new()` güvenli default `Placeholder`; production preset `try_with_source(TreeSitter)`.
#[derive(Debug, Clone, Copy)]
pub struct CouplingAxis {
    source: MetricSource,
}

impl CouplingAxis {
    /// Güvenli default — provenance bilinmiyor (Placeholder).
    pub fn new() -> Self {
        Self {
            source: MetricSource::Placeholder,
        }
    }

    /// Fallible constructor — Mixed reddedilir (yalnız aggregation çıktısı).
    /// Production: `try_with_source(MetricSource::TreeSitter)`.
    pub fn try_with_source(source: MetricSource) -> Result<Self, AxisSourceError> {
        Ok(Self {
            source: validate_direct_source(source)?,
        })
    }

    pub fn source(&self) -> MetricSource {
        self.source
    }

    /// Value-only projection (legacy `compute()` için helper).
    fn compute_value(&self, node: &Node, space: &Space) -> f64 {
        let deg = space.out_degree_value(node.id, EdgeKind::Imports) as f64;
        deg / (1.0 + deg)
    }
}

impl Default for CouplingAxis {
    fn default() -> Self {
        Self::new()
    }
}

impl Axis for CouplingAxis {
    fn name(&self) -> &'static str {
        "coupling"
    }
    /// **INV-T9 #70 (semantics v2, P1-1 source encoding):** Descriptor formula marker 0
    /// (value-level Imports out-degree `deg/(1+deg)`) + stable source ID bytes bağlar.
    /// Algoritma değişirse (örn semantic-weighted) semantics_version artırılmalı.
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(0); // formula marker: parametresiz, value-level out-degree
        params.push_bytes(self.source.descriptor_id())?; // P1-1 source encoding
        AxisDescriptor::try_new(self.name(), 2, params)
    }
    fn measure(&self, node: &Node, space: &Space) -> Result<AxisMeasurement, AxisMeasurementError> {
        AxisMeasurement::try_new(self.compute_value(node, space), self.source)
    }
    fn compute(&self, node: &Node, space: &Space) -> f64 {
        self.compute_value(node, space)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// w — Entropi (repo-level)
// ═══════════════════════════════════════════════════════════════════════════════

/// `w` ekseni — **İstikrar/Entropi**.
///
/// Repo-level: tüm düğümler aynı değeri alır. Construction'da Faz 0'ın
/// `commit_entropy` değeri (Shannon H, commit→dosya dağılımı) verilir.
///
/// **Normalizasyon:** `H / 13.0` cap. 13.0 = Faz 1.11 kalibrasyon korpusu upper-bound
/// (date-fns 12.39, django 12.03). Faz 1.11'de 12.0 → 13.0 tune edildi (saturasyon 4→1).
#[derive(Debug, Clone, Copy)]
pub struct EntropyAxis {
    value: f64,
}

impl EntropyAxis {
    pub fn from_commit_entropy(h: f64) -> Self {
        Self {
            value: (h / 13.0).clamp(0.0, 1.0),
        }
    }

    /// Test/diagnostic için ham değeri oku.
    pub fn value(&self) -> f64 {
        self.value
    }
}

impl Axis for EntropyAxis {
    fn name(&self) -> &'static str {
        "entropy"
    }
    /// **INV-T9 #70 (semantics v2, P1-1 source encoding):** Descriptor `compute()`'un
    /// okuduğu tek effective state `self.value`'yu + Heuristic source ID bytes bağlar.
    /// Ham constructor `h` DEĞİL — `from_commit_entropy(13)` ve `(100)` clamp sonrası
    /// aynı value üretirse → aynı descriptor.
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(2); // formula marker: h/13.0 clamp
        params.push_f64(self.value)?;
        params.push_bytes(MetricSource::Heuristic.descriptor_id())?; // P1-1 source encoding
        AxisDescriptor::try_new(self.name(), 2, params)
    }
    fn measure(
        &self,
        _node: &Node,
        _space: &Space,
    ) -> Result<AxisMeasurement, AxisMeasurementError> {
        AxisMeasurement::try_new(self.value, MetricSource::Heuristic)
    }
    fn compute(&self, _node: &Node, _space: &Space) -> f64 {
        self.value
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// v — Şahitlik derinliği (repo-level)
// ═══════════════════════════════════════════════════════════════════════════════

/// `v` ekseni — **Şahitlik derinliği**.
///
/// Repo-level: `witnessed_ratio × ln(1 + distinct_witnesses)`, soft-normalize
/// `raw / (1 + raw)` ∈ [0, 1).
///
/// Faz 0 spike `distinct_authors`'ı proxy olarak kullandı; OSP-core'da parametre
/// `distinct_witnesses` (semantic doğru). Wiring sırasında Faz 0 verisi proxy
/// olarak geçirilebilir (Faz 1.8 re-spike'ta not edilir).
#[derive(Debug, Clone, Copy)]
pub struct WitnessDepthAxis {
    value: f64,
}

impl WitnessDepthAxis {
    pub fn from_witness(witnessed_ratio: f64, distinct_witnesses: usize) -> Self {
        let raw = witnessed_ratio.max(0.0) * (1.0 + distinct_witnesses as f64).ln();
        Self {
            value: raw / (1.0 + raw),
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }
}

impl Axis for WitnessDepthAxis {
    fn name(&self) -> &'static str {
        "witness_depth"
    }
    /// **INV-T9 #70 (semantics v2, P1-1 source encoding):** Descriptor `self.value`'yu
    /// + Heuristic source ID bytes bağlar. Ham `(ratio, distinct)` DEĞİL.
    ///
    /// Formula marker `3` = `raw=ratio*ln(1+distinct)`, `raw/(1+raw)` soft-normalize.
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(3); // formula marker: raw=ratio*ln(1+distinct), raw/(1+raw)
        params.push_f64(self.value)?;
        params.push_bytes(MetricSource::Heuristic.descriptor_id())?; // P1-1 source encoding
        AxisDescriptor::try_new(self.name(), 2, params)
    }
    fn measure(
        &self,
        _node: &Node,
        _space: &Space,
    ) -> Result<AxisMeasurement, AxisMeasurementError> {
        AxisMeasurement::try_new(self.value, MetricSource::Heuristic)
    }
    fn compute(&self, _node: &Node, _space: &Space) -> f64 {
        self.value
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// z — Instability (Martin I, per-node) — Faz 1.9
// ═══════════════════════════════════════════════════════════════════════════════

/// `z` ekseni — **Martin Instability** `I = Ce / (Ca + Ce)` (saf, inv #10).
///
/// Per-node: `Ce` = efferent coupling (fan-out = out-degree Imports), `Ca` = afferent
/// (fan-in = in-degree Imports). `I ∈ [0, 1]`:
/// - `I → 0`: **stable** (foundation — çok kişi bağlı, kendisi bağlanmaz)
/// - `I → 1`: **unstable** (çok kişiye bağlı, kimse ona bağlı değil)
///
/// İzole node (`Ce = Ca = 0`): `I = 0.5` (nötr convention — tanımsız matematiksel).
///
/// **`D` (Martin main-sequence distance) ayrı derived metric'tir** —
/// `DerivedPosition.main_sequence_distance`, `compute_derived` ile (inv #10). z'ye
/// gömülü DEĞİL.
///
/// **INV-T9 #70:** Construction-time source — CouplingAxis ile aynı graph topology
/// kökenini bağlar. `new()` güvenli default `Placeholder`; production preset
/// `try_with_source(TreeSitter)` (preset tek `topology_source` parametresi ile Coupling
/// ve Instability'a aynı source'u geçirir).
#[derive(Debug, Clone, Copy)]
pub struct InstabilityAxis {
    source: MetricSource,
}

impl InstabilityAxis {
    /// Güvenli default — provenance bilinmiyor (Placeholder).
    pub fn new() -> Self {
        Self {
            source: MetricSource::Placeholder,
        }
    }

    /// Fallible constructor — Mixed reddedilir.
    pub fn try_with_source(source: MetricSource) -> Result<Self, AxisSourceError> {
        Ok(Self {
            source: validate_direct_source(source)?,
        })
    }

    pub fn source(&self) -> MetricSource {
        self.source
    }

    /// Value-only projection (legacy `compute()` için helper).
    fn compute_value(&self, node: &Node, space: &Space) -> f64 {
        // Value-only degrees: type-only import'lar Ce/Ca'dan hariç (CouplingAxis ile aynı
        // rationale — runtime dependency değil).
        let ce = space.out_degree_value(node.id, EdgeKind::Imports) as f64; // fan-out
        let ca = space.in_degree_value(node.id, EdgeKind::Imports) as f64; // fan-in
        let denom = ce + ca;
        if denom > 0.0 {
            ce / denom
        } else {
            0.5 // izole → nötr convention
        }
    }
}

impl Default for InstabilityAxis {
    fn default() -> Self {
        Self::new()
    }
}

impl Axis for InstabilityAxis {
    fn name(&self) -> &'static str {
        "instability"
    }
    /// **INV-T9 #70 (semantics v2, P1-1 source encoding):** Formula marker 0 (Martin
    /// `I = Ce/(Ce+Ca)`, isolated→0.5) + source ID bytes.
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(0); // formula marker: value-level Martin I, isolated→0.5
        params.push_bytes(self.source.descriptor_id())?; // P1-1 source encoding
        AxisDescriptor::try_new(self.name(), 2, params)
    }
    fn measure(&self, node: &Node, space: &Space) -> Result<AxisMeasurement, AxisMeasurementError> {
        AxisMeasurement::try_new(self.compute_value(node, space), self.source)
    }
    fn compute(&self, node: &Node, space: &Space) -> f64 {
        self.compute_value(node, space)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// y — Cohesion (LCOM4 proxy, precomputed) — Faz 1.9
// ═══════════════════════════════════════════════════════════════════════════════

/// `y` ekseni — **Kohezyon** (LCOM4).
///
/// **Per-node cohesion:** `compute(node, space)` öncelikle `node.cohesion` okur
/// (analyzer tarafından SCIP LCOM4'ten set edilir — INV-T9 #70: yalnız gerçek SCIP
/// sonucunda `Some`). Node'da değer yoksa (`None`), `fallback` kullanılır.
///
/// `cohesion ∈ [0, 1]`: `1.0` = tam kohezif (LCOM4=1), `0.0` = kohezyon yok.
///
/// **INV-T9 #70 (per-node source policy):**
/// - `node.cohesion = Some(c)` → `(c, observed_source)` (production analyzer `Scip`)
/// - `node.cohesion = None` + `fallback = Some(fb)` → `(fb.value, fb.source)`
/// - `node.cohesion = None` + `fallback = None` → `(0.5, Placeholder)`
///
/// **Observational equivalence:** `new()` (None→0.5) ile `try_from_normalized(0.5)`
/// aynı `effective_fallback (0.5, Placeholder)` → aynı descriptor (P1-1).
#[derive(Debug, Clone, Copy)]
pub struct CohesionAxis {
    /// Per-node Some değerinin kaynak kökeni (production: `Scip`).
    observed_source: MetricSource,
    /// Effective fallback — `None` node'lar için. `None` → `(0.5, Placeholder)` normalize.
    fallback: Option<AxisMeasurement>,
}

impl CohesionAxis {
    /// Effective fallback — her zaman normalize. `None` → `(0.5, Placeholder)`.
    /// Constructor provenance DEĞİL effective davranışı bağlar (P1-1).
    fn effective_fallback(&self) -> AxisMeasurement {
        self.fallback.unwrap_or(AxisMeasurement {
            value: 0.5,
            source: MetricSource::Placeholder,
        })
    }

    /// Per-node cohesion okuyan axis — analyzer `node.cohesion` set ettiğinde gerçek değer kullanılır.
    /// Set edilmediyse `(0.5, Placeholder)` nötr default.
    pub fn new() -> Self {
        Self {
            observed_source: MetricSource::Placeholder,
            fallback: None,
        }
    }

    /// Per-node observed source — production analyzer `try_with_observed_source(MetricSource::Scip)`.
    /// Mixed reddedilir.
    pub fn try_with_observed_source(
        observed_source: MetricSource,
    ) -> Result<Self, AxisSourceError> {
        Ok(Self {
            observed_source: validate_direct_source(observed_source)?,
            fallback: None,
        })
    }

    /// Fallible fallback constructor — non-finite reject, clamp sonrası `[0,1]`.
    /// Backward-compat için (eski test'ler `from_normalized`/`from_lcom4` kullanır).
    pub fn try_from_normalized(cohesion: f64) -> Result<Self, AxisMeasurementError> {
        if !cohesion.is_finite() {
            return Err(AxisMeasurementError::NonFiniteValue);
        }
        let fallback =
            AxisMeasurement::try_new(cohesion.clamp(0.0, 1.0), MetricSource::Placeholder)?;
        Ok(Self {
            observed_source: MetricSource::Placeholder,
            fallback: Some(fallback),
        })
    }

    /// LCOM4 raw değerinden fallback mapping: `cohesion = 1 / lcom4`.
    /// `lcom4=1` → `1.0` (kohezif), `lcom4=2` → `0.5`, `lcom4≥4` → `≤0.25`.
    /// İnfallible — LCOM4→cohesion mapping finite garantili.
    pub fn from_lcom4(lcom4: usize) -> Self {
        let value = if lcom4 == 0 {
            1.0 // convention: 0 = undefined → conservative full cohesion
        } else {
            (1.0 / lcom4 as f64).clamp(0.0, 1.0)
        };
        Self {
            observed_source: MetricSource::Placeholder,
            fallback: Some(
                AxisMeasurement::try_new(value, MetricSource::Placeholder)
                    .expect("LCOM4 mapping is finite and normalized"),
            ),
        }
    }

    pub fn value(&self) -> f64 {
        self.effective_fallback().value
    }
}

impl Default for CohesionAxis {
    fn default() -> Self {
        Self::new()
    }
}

impl Axis for CohesionAxis {
    fn name(&self) -> &'static str {
        "cohesion"
    }
    /// **INV-T9 #70 (semantics v2, P1-1 source encoding, observational equivalence):**
    /// Descriptor effective fallback değerini + kaynaklarını bağlar; constructor
    /// provenance marker'ı (None vs Some) DEĞİL. `new()` (None→0.5) ile
    /// `try_from_normalized(0.5)` aynı effective fallback → aynı descriptor.
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
        let fb = self.effective_fallback();
        let mut params = AxisParameterEncoder::new();
        params.push_u8(1); // formula marker: LCOM4→cohesion normalize
        params.push_bytes(self.observed_source.descriptor_id())?; // observed source
        params.push_bytes(fb.source.descriptor_id())?; // effective fallback source
        params.push_f64(fb.value)?; // effective fallback value
        AxisDescriptor::try_new(self.name(), 2, params)
    }
    fn measure(
        &self,
        node: &Node,
        _space: &Space,
    ) -> Result<AxisMeasurement, AxisMeasurementError> {
        // Per-node policy: node.cohesion Some → observed_source; fallback Some →
        // fallback source; None → Placeholder (effective_fallback normalize).
        let fb = self.effective_fallback();
        let (value, source) = match node.cohesion {
            Some(c) => (c, self.observed_source),
            None => (fb.value, fb.source),
        };
        AxisMeasurement::try_new(value, source)
    }
    fn compute(&self, node: &Node, _space: &Space) -> f64 {
        // Legacy value-only projection — effective fallback kullanır.
        node.cohesion
            .or(Some(self.effective_fallback().value))
            .unwrap_or(0.5)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// (Faz 1.4) VisionAlignmentAxis KALDIRILDI — `u` artık DERIVED metric.
// `compute_derived()` ile hesaplanır (vision.rs Faz 1.7). Raw preset'te YOK (inv #4).
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Preset — CoordinateSystem::default_raw_three (Faz 1.4); default_raw_five Faz 1.9'da
// ═══════════════════════════════════════════════════════════════════════════════

impl CoordinateSystem {
    /// Varsayılan **raw** preset — 3 eksen (`x` coupling, `w` entropy, `v` witness-depth).
    ///
    /// Faz 1.9'da `y` (CohesionAxis LCOM4) + `z` (InstabilityAxis Martin I) eklenir
    /// → `default_raw_five`. `u` (vision alignment) **derived**'dır, preset'te YOK
    /// (inv #4 — `DerivedPosition` içinde `compute_derived` ile, Faz 1.7 `vision.rs`).
    ///
    /// **INV-T9 #70:** `topology_source` CouplingAxis'in graph topology kökenini bağlar
    /// (production: `TreeSitter`, test/synthetic: `Placeholder`). Mixed reddedilir.
    pub fn default_raw_three(
        topology_source: MetricSource,
        entropy: EntropyAxis,
        witness: WitnessDepthAxis,
    ) -> Result<Self, AxisRegistrationError> {
        let coupling = CouplingAxis::try_with_source(topology_source)?;
        Self::empty()
            .try_with_axis(coupling)?
            .try_with_axis(entropy)?
            .try_with_axis(witness)
    }

    /// Varsayılan **raw** preset — 5 eksen (`x` coupling, `y` cohesion, `z` instability,
    /// `w` entropy, `v` witness-depth). Faz 1.9.
    ///
    /// `u` (vision alignment) **derived**'dır, preset'te YOK (inv #4 — `compute_derived`
    /// ile `DerivedPosition`'da, `vision.rs` Faz 1.7). `D` (Martin main-sequence) de
    /// derived — `compute_derived` ayrı parametre olarak alır (inv #10).
    ///
    /// **INV-T9 #70:** `topology_source` tek bir graph topology kökenini Coupling ve
    /// Instability axis'lerine geçirir (aynı `Space` topology'sünden türetiliyor —
    /// ayrı parametreler yapay serbestlik yaratır). Production: `TreeSitter`, test/
    /// synthetic: `Placeholder`. Mixed reddedilir. CohesionAxis caller'a bırakılır
    /// (production `try_with_observed_source(Scip)`, test/synthetic `new()`).
    pub fn default_raw_five(
        topology_source: MetricSource,
        cohesion: CohesionAxis,
        entropy: EntropyAxis,
        witness: WitnessDepthAxis,
    ) -> Result<Self, AxisRegistrationError> {
        let coupling = CouplingAxis::try_with_source(topology_source)?;
        let instability = InstabilityAxis::try_with_source(topology_source)?;
        Self::empty()
            .try_with_axis(coupling)? // x — per-node
            .try_with_axis(cohesion)? // y — precomputed/per-node
            .try_with_axis(instability)? // z — per-node
            .try_with_axis(entropy)? // w — precomputed
            .try_with_axis(witness) // v — precomputed
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::{Edge, Node, NodeKind};

    fn node(id: u64) -> Node {
        Node {
            id,
            ..Default::default()
        }
    }

    fn import_edge(from: u64, to: u64) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Imports,
            ..Default::default()
        }
    }

    // --- CouplingAxis ---

    #[test]
    fn coupling_zero_for_isolated_node() {
        let mut space = Space::new();
        space.insert_node(node(1));
        let axis = CouplingAxis::new();
        assert!(axis.compute(&node(1), &space).abs() < 1e-9);
    }

    #[test]
    fn coupling_reflects_import_out_degree() {
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_node(node(3));
        space.insert_edge(import_edge(1, 2));
        space.insert_edge(import_edge(1, 3));
        // Calls kenarı coupling'e dahil olmamalı
        space.insert_edge(Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Calls,
            ..Default::default()
        });

        let axis = CouplingAxis::new();
        let v = axis.compute(&node(1), &space);
        // out_degree(Imports) = 2 → 2/3
        assert!((v - 2.0 / 3.0).abs() < 1e-9, "coupling = {}", v);
    }

    #[test]
    fn coupling_excludes_type_only_imports() {
        // Type-only import'lar (`import type`) runtime dependency değildir — coupling'i
        // artırmamalı. value-only degree kullanır: is_type_only=true edge'ler hariç.
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_node(node(3));
        // 1 value import → 2 (coupling'e girer)
        space.insert_edge(import_edge(1, 2));
        // 1 type-only import → 3 (coupling'e GİRMEZ)
        space.insert_edge(Edge {
            from: 1,
            to: 3,
            kind: EdgeKind::Imports,
            is_type_only: true,
        });

        let axis = CouplingAxis::new();
        let v = axis.compute(&node(1), &space);
        // value-only out_degree = 1 → 1/2 = 0.5 (type-only hariç)
        assert!(
            (v - 0.5).abs() < 1e-9,
            "type-only import should not count, coupling = {}",
            v
        );

        // Karşılaştırma: iki value import olsaydı 2/3 ≈ 0.667 olurdu.
        // Type-only'nin coupling'i düşürmesi mimari açıdan doğru sinyal.
    }

    #[test]
    fn instability_excludes_type_only_imports() {
        // InstabilityAxis de value-only degree kullanır (Ce/Ca type-only hariç).
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        // 1 value import (1→2): Ce=1, Ca=0 → I=1.0
        space.insert_edge(import_edge(1, 2));

        let axis = InstabilityAxis::new();
        let v_with_only_value = axis.compute(&node(1), &space);
        assert!(
            (v_with_only_value - 1.0).abs() < 1e-9,
            "I with only value = 1.0"
        );

        // Şimdi 1 type-only import ekle (1→2 zaten var, farklı node ekleyelim)
        space.insert_node(node(3));
        space.insert_edge(Edge {
            from: 1,
            to: 3,
            kind: EdgeKind::Imports,
            is_type_only: true,
        });
        // value-only Ce hâlâ 1 (type-only hariç) → I hâlâ 1.0
        let v_with_type_only = axis.compute(&node(1), &space);
        assert!(
            (v_with_type_only - 1.0).abs() < 1e-9,
            "type-only import should not change I, got {}",
            v_with_type_only
        );
    }

    #[test]
    fn coupling_saturates_below_one() {
        let mut space = Space::new();
        space.insert_node(node(1));
        for i in 2..100 {
            space.insert_node(node(i));
            space.insert_edge(import_edge(1, i));
        }
        let axis = CouplingAxis::new();
        let v = axis.compute(&node(1), &space);
        // 98 imports → 98/99 ≈ 0.99
        assert!(v > 0.98 && v < 1.0, "v = {}", v);
    }

    #[test]
    fn coupling_per_node_differs() {
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_node(node(3));
        space.insert_edge(import_edge(1, 2));
        space.insert_edge(import_edge(1, 3));
        // node 2 ve 3 hiç import etmiyor

        let axis = CouplingAxis::new();
        let v1 = axis.compute(&node(1), &space);
        let v2 = axis.compute(&node(2), &space);
        assert!(v1 > v2, "hub node daha yüksek coupling");
        assert!(v2.abs() < 1e-9);
    }

    // --- EntropyAxis ---

    #[test]
    fn entropy_normalizes_with_cap() {
        // Faz 1.11: cap 12.0 → 13.0 tune
        assert!((EntropyAxis::from_commit_entropy(6.5).value() - 0.5).abs() < 1e-9);
        assert!((EntropyAxis::from_commit_entropy(13.0).value() - 1.0).abs() < 1e-9);
        assert!((EntropyAxis::from_commit_entropy(15.0).value() - 1.0).abs() < 1e-9); // clamp
        assert!(EntropyAxis::from_commit_entropy(0.0).value().abs() < 1e-9);
    }

    #[test]
    fn entropy_constant_across_nodes() {
        let axis = EntropyAxis::from_commit_entropy(6.0);
        let space = Space::new();
        let v1 = axis.compute(&node(1), &space);
        let v2 = axis.compute(&node(2), &space);
        assert!((v1 - v2).abs() < 1e-9);
    }

    #[test]
    fn entropy_preserves_faz0_ordering() {
        // worms 6.60 < click 6.78 < fastapi 10.56 < django 12.03 < date-fns 12.39
        // Faz 1.11: cap 13.0 ile django (12.03) artık saturasyonda DEĞİL
        let worms = EntropyAxis::from_commit_entropy(6.60).value();
        let click = EntropyAxis::from_commit_entropy(6.78).value();
        let fastapi = EntropyAxis::from_commit_entropy(10.56).value();
        let django = EntropyAxis::from_commit_entropy(12.03).value();
        let datefns = EntropyAxis::from_commit_entropy(12.39).value();
        assert!(worms < click);
        assert!(click < fastapi);
        assert!(fastapi < django);
        assert!(django < datefns); // Faz 1.11: cap 13 → django < date-fns (önce = idi)
    }

    // --- WitnessDepthAxis ---

    #[test]
    fn witness_depth_zero_when_no_witnesses() {
        let a = WitnessDepthAxis::from_witness(0.0, 0);
        assert!(a.value().abs() < 1e-9);
    }

    #[test]
    fn witness_depth_zero_when_ratio_zero() {
        // ratio=0 → raw=0 → value=0, witnesses sayısı ne olursa olsun
        let a = WitnessDepthAxis::from_witness(0.0, 100);
        assert!(a.value().abs() < 1e-9);
    }

    #[test]
    fn witness_depth_increases_with_ratio_and_witnesses() {
        let low = WitnessDepthAxis::from_witness(0.1, 2).value();
        let mid = WitnessDepthAxis::from_witness(0.3, 5).value();
        let high = WitnessDepthAxis::from_witness(0.5, 20).value();
        assert!(low < mid);
        assert!(mid < high);
    }

    #[test]
    fn witness_depth_bounded_below_one() {
        // Soft-normalize raw/(1+raw) asimptotik olarak 1'e yaklaşır ama yavaş.
        // 1000 witness + ratio=1.0: raw = ln(1001) ≈ 6.9, value ≈ 0.87.
        let extreme = WitnessDepthAxis::from_witness(1.0, 1000);
        assert!(extreme.value() < 1.0);
        assert!(extreme.value() > 0.85, "value = {}", extreme.value());
    }

    // --- (Faz 1.4) VisionAlignmentAxis testleri KALDIRILDI — u artık derived ---

    // --- Preset (Faz 1.4: default_raw_three) ---

    #[test]
    fn default_raw_three_has_expected_axes() {
        let cs = CoordinateSystem::default_raw_three(
            MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        assert_eq!(cs.dim(), 3);
        assert_eq!(
            cs.axis_names(),
            vec!["coupling", "entropy", "witness_depth"]
        );
    }

    #[test]
    fn default_raw_three_excludes_vision_alignment() {
        // inv #4 — u derived'dır, raw preset'te YOK
        let cs = CoordinateSystem::default_raw_three(
            MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        assert!(
            !cs.axis_names().contains(&"vision_alignment"),
            "vision_alignment raw preset'te olmamalı (derived, inv #4)"
        );
    }

    #[test]
    fn default_raw_three_raw_position_maps_correctly() {
        let cs = CoordinateSystem::default_raw_three(
            MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.5), // w = 0.5 (6.5/13.0, Faz 1.11 cap tune)
            WitnessDepthAxis::from_witness(0.3, 5), // v
        )
        .unwrap();
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2)); // node 1: 1 import → x = 0.5

        let raw = cs.raw_position_of(&node(1), &space);
        // x: 1 import → 1/(1+1) = 0.5
        assert!((raw.x - 0.5).abs() < 1e-9, "x = {}", raw.x);
        // y, z: preset'te yok → 0.0
        assert!(raw.y.abs() < 1e-9, "y boş kalmalı (Faz 1.9)");
        assert!(raw.z.abs() < 1e-9, "z boş kalmalı (Faz 1.9)");
        // w: 6.0/12 = 0.5
        assert!((raw.w - 0.5).abs() < 1e-9, "w = {}", raw.w);
        // v: from_witness(0.3, 5) > 0
        assert!(raw.v > 0.0, "v = {}", raw.v);
    }

    // --- InstabilityAxis (z) — Faz 1.9, inv #10 ---

    #[test]
    fn instability_zero_for_pure_stable_foundation() {
        // Ca>0, Ce=0 → I=0 (foundation: herkes bağlı, kendisi bağlanmaz)
        let mut space = Space::new();
        space.insert_node(node(1)); // foundation
        space.insert_node(node(2));
        space.insert_node(node(3));
        space.insert_edge(import_edge(2, 1)); // 2 → 1
        space.insert_edge(import_edge(3, 1)); // 3 → 1
                                              // node 1: in-degree=2, out-degree=0 → I = 0/2 = 0
        let axis = InstabilityAxis::new();
        let i = axis.compute(&node(1), &space);
        assert!(i.abs() < 1e-9, "pure stable I = {}", i);
    }

    #[test]
    fn instability_one_for_pure_unstable_leaf() {
        // Ce>0, Ca=0 → I=1 (leaf: bağlıdır, kimse ona bağlı değil)
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2)); // 1 → 2
                                              // node 1: out=1, in=0 → I = 1/1 = 1
        let axis = InstabilityAxis::new();
        let i = axis.compute(&node(1), &space);
        assert!((i - 1.0).abs() < 1e-9, "pure unstable I = {}", i);
    }

    #[test]
    fn instability_half_for_balanced_coupling() {
        // Ce=Ca → I=0.5
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_node(node(3));
        space.insert_edge(import_edge(1, 2)); // 1 → 2 (out for 1)
        space.insert_edge(import_edge(3, 1)); // 3 → 1 (in for 1)
                                              // node 1: out=1, in=1 → I = 1/2 = 0.5
        let axis = InstabilityAxis::new();
        let i = axis.compute(&node(1), &space);
        assert!((i - 0.5).abs() < 1e-9, "balanced I = {}", i);
    }

    #[test]
    fn instability_neutral_half_for_isolated_node() {
        // Ce=Ca=0 → convention I=0.5 (nötr, tanımsız)
        let mut space = Space::new();
        space.insert_node(node(1));
        let axis = InstabilityAxis::new();
        let i = axis.compute(&node(1), &space);
        assert!((i - 0.5).abs() < 1e-9, "isolated I = {}", i);
    }

    #[test]
    fn instability_axis_name_is_instability() {
        assert_eq!(InstabilityAxis::new().name(), "instability");
    }

    // --- CohesionAxis (y) — Faz 1.9 ---

    #[test]
    fn cohesion_from_lcom4_one_is_fully_cohesive() {
        let a = CohesionAxis::from_lcom4(1);
        assert!((a.value() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cohesion_from_lcom4_two_is_half() {
        let a = CohesionAxis::from_lcom4(2);
        assert!((a.value() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cohesion_try_from_normalized_clamps() {
        assert!((CohesionAxis::try_from_normalized(1.5).unwrap().value() - 1.0).abs() < 1e-9);
        assert!((CohesionAxis::try_from_normalized(-0.3).unwrap().value() - 0.0).abs() < 1e-9);
        assert!((CohesionAxis::try_from_normalized(0.7).unwrap().value() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn cohesion_try_from_normalized_rejects_nan() {
        assert_eq!(
            CohesionAxis::try_from_normalized(f64::NAN).unwrap_err(),
            AxisMeasurementError::NonFiniteValue
        );
    }

    #[test]
    fn cohesion_axis_name_is_cohesion() {
        let a = CohesionAxis::try_from_normalized(0.5).unwrap();
        assert_eq!(a.name(), "cohesion");
    }

    // --- CohesionAxis per-node reading (Faz 3.6) ---

    #[test]
    fn cohesion_axis_reads_per_node_value() {
        // Node with cohesion=Some(0.8) → axis returns 0.8
        let space = Space::new();
        let node = Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            cohesion: Some(0.8),
            ..Default::default()
        };
        let axis = CohesionAxis::new();
        let y = axis.compute(&node, &space);
        assert!(
            (y - 0.8).abs() < 1e-9,
            "per-node cohesion should be 0.8, got {}",
            y
        );
    }

    #[test]
    fn cohesion_axis_falls_back_to_0_5_when_node_has_no_cohesion() {
        // Node with cohesion=None → axis returns default 0.5
        let space = Space::new();
        let node = Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            cohesion: None,
            ..Default::default()
        };
        let axis = CohesionAxis::new();
        let y = axis.compute(&node, &space);
        assert!(
            (y - 0.5).abs() < 1e-9,
            "no cohesion → default 0.5, got {}",
            y
        );
    }

    #[test]
    fn cohesion_axis_fallback_used_when_node_none() {
        // Node cohesion=None + CohesionAxis::from_lcom4(1) → fallback 1.0 wins
        let space = Space::new();
        let node = Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            cohesion: None,
            ..Default::default()
        };
        let axis = CohesionAxis::from_lcom4(1); // fallback = Some(1.0)
        let y = axis.compute(&node, &space);
        assert!(
            (y - 1.0).abs() < 1e-9,
            "fallback should be 1.0 when node has None, got {}",
            y
        );
    }

    #[test]
    fn cohesion_axis_node_value_overrides_fallback() {
        // Node cohesion=Some(0.6) + CohesionAxis::from_lcom4(1) → node wins (0.6)
        let space = Space::new();
        let node = Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            cohesion: Some(0.6),
            ..Default::default()
        };
        let axis = CohesionAxis::from_lcom4(1); // fallback = Some(1.0)
        let y = axis.compute(&node, &space);
        assert!(
            (y - 0.6).abs() < 1e-9,
            "node cohesion should override fallback, got {}",
            y
        );
    }

    // --- default_raw_five preset — Faz 1.9 ---

    #[test]
    fn default_raw_five_has_five_axes_in_canonical_order() {
        let cs = CoordinateSystem::default_raw_five(
            MetricSource::Placeholder,
            CohesionAxis::from_lcom4(1),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        assert_eq!(cs.dim(), 5);
        assert_eq!(
            cs.axis_names(),
            vec![
                "coupling",
                "cohesion",
                "instability",
                "entropy",
                "witness_depth"
            ]
        );
    }

    #[test]
    fn default_raw_five_excludes_vision_alignment() {
        // inv #4 — u derived, raw preset'te YOK
        let cs = CoordinateSystem::default_raw_five(
            MetricSource::Placeholder,
            CohesionAxis::from_lcom4(1),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        assert!(
            !cs.axis_names().contains(&"vision_alignment"),
            "vision_alignment derived — raw preset'te olmamalı"
        );
    }

    #[test]
    fn default_raw_five_raw_position_all_fields_populated() {
        let cs = CoordinateSystem::default_raw_five(
            MetricSource::Placeholder,
            CohesionAxis::try_from_normalized(0.8).unwrap(), // y
            EntropyAxis::from_commit_entropy(6.5), // w = 0.5 (6.5/13.0, Faz 1.11 cap tune)
            WitnessDepthAxis::from_witness(0.3, 5), // v
        )
        .unwrap();
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2)); // node 1: Ce=1, Ca=0

        let raw = cs.raw_position_of(&node(1), &space);
        // x: 1 import → 0.5
        assert!((raw.x - 0.5).abs() < 1e-9, "x = {}", raw.x);
        // y: precomputed 0.8
        assert!((raw.y - 0.8).abs() < 1e-9, "y = {}", raw.y);
        // z: I = Ce/(Ca+Ce) = 1/(0+1) = 1.0 (pure unstable leaf)
        assert!((raw.z - 1.0).abs() < 1e-9, "z (instability) = {}", raw.z);
        // w: 6.0/12 = 0.5
        assert!((raw.w - 0.5).abs() < 1e-9, "w = {}", raw.w);
        // v: from_witness > 0
        assert!(raw.v > 0.0, "v = {}", raw.v);
    }

    #[test]
    fn default_raw_five_instability_reflects_per_node_topology() {
        // İki node farklı I değerleri — per-node doğrulama
        let cs = CoordinateSystem::default_raw_five(
            MetricSource::Placeholder,
            CohesionAxis::from_lcom4(1),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2)); // 1→2: node 1 unstable (I=1), node 2 stable (I=0)

        let raw1 = cs.raw_position_of(&node(1), &space);
        let raw2 = cs.raw_position_of(&node(2), &space);
        assert!(
            (raw1.z - 1.0).abs() < 1e-9,
            "node 1 z (unstable) = {}",
            raw1.z
        );
        assert!(raw2.z.abs() < 1e-9, "node 2 z (stable) = {}", raw2.z);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Adım 3 — axis descriptor (effective normalized) testleri
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn axis_descriptor_coupling_explicit_implementation() {
        // INV-T9 #70 (semantics v2): Parametresiz axis ama explicit descriptor —
        // formula marker 0 + source ID bytes (default Placeholder).
        let d = CouplingAxis::new().descriptor().unwrap();
        assert_eq!(d.axis_id(), "coupling");
        assert_eq!(d.semantics_version(), 2);
        // marker 0 + source ID length-prefix(0x0B=11 "placeholder") + "placeholder"
        let mut expected = AxisParameterEncoder::new();
        expected.push_u8(0);
        expected
            .push_bytes(MetricSource::Placeholder.descriptor_id())
            .unwrap();
        assert_eq!(d.canonical_parameters(), &expected.finish()[..]);
    }

    #[test]
    fn axis_descriptor_instability_explicit_implementation() {
        let d = InstabilityAxis::new().descriptor().unwrap();
        assert_eq!(d.axis_id(), "instability");
        assert_eq!(d.semantics_version(), 2);
        let mut expected = AxisParameterEncoder::new();
        expected.push_u8(0);
        expected
            .push_bytes(MetricSource::Placeholder.descriptor_id())
            .unwrap();
        assert_eq!(d.canonical_parameters(), &expected.finish()[..]);
    }

    #[test]
    fn axis_descriptor_entropy_binds_effective_normalized_value() {
        // from_commit_entropy(6.0) → value = (6.0/13.0).clamp(0,1) ≈ 0.4615
        let axis = EntropyAxis::from_commit_entropy(6.0);
        let d = axis.descriptor().unwrap();
        assert_eq!(d.axis_id(), "entropy");
        assert_eq!(d.semantics_version(), 2);
        // marker 2 + effective value LE bytes + Heuristic source ID
        let expected_value = (6.0_f64 / 13.0).clamp(0.0, 1.0);
        let mut expected = AxisParameterEncoder::new();
        expected.push_u8(2);
        expected.push_f64(expected_value).unwrap();
        expected
            .push_bytes(MetricSource::Heuristic.descriptor_id())
            .unwrap();
        assert_eq!(d.canonical_parameters(), &expected.finish()[..]);
    }

    #[test]
    fn axis_descriptor_witness_depth_binds_effective_normalized_value() {
        let axis = WitnessDepthAxis::from_witness(0.3, 5);
        let d = axis.descriptor().unwrap();
        assert_eq!(d.axis_id(), "witness_depth");
        assert_eq!(d.semantics_version(), 2);
        let raw = 0.3_f64.max(0.0) * (1.0 + 5.0_f64).ln();
        let expected_value = raw / (1.0 + raw);
        let mut expected = AxisParameterEncoder::new();
        expected.push_u8(3);
        expected.push_f64(expected_value).unwrap();
        expected
            .push_bytes(MetricSource::Heuristic.descriptor_id())
            .unwrap();
        assert_eq!(d.canonical_parameters(), &expected.finish()[..]);
    }

    #[test]
    fn axis_descriptor_cohesion_binds_effective_fallback() {
        // try_from_normalized(0.5) → effective fallback (0.5, Placeholder), marker 1.
        let axis = CohesionAxis::try_from_normalized(0.5).unwrap();
        let d = axis.descriptor().unwrap();
        assert_eq!(d.axis_id(), "cohesion");
        assert_eq!(d.semantics_version(), 2);
        // marker 1 + observed_source(Placeholder) + fallback_source(Placeholder) + fallback_value(0.5)
        let mut expected = AxisParameterEncoder::new();
        expected.push_u8(1);
        expected
            .push_bytes(MetricSource::Placeholder.descriptor_id())
            .unwrap();
        expected
            .push_bytes(MetricSource::Placeholder.descriptor_id())
            .unwrap();
        expected.push_f64(0.5).unwrap();
        assert_eq!(d.canonical_parameters(), &expected.finish()[..]);
    }

    #[test]
    fn observationally_equivalent_cohesion_configs_share_descriptor() {
        // **P0-1 (effective model):** new() (None→0.5) ile try_from_normalized(0.5) aynı
        // effective fallback → aynı descriptor. Constructor provenance marker'ı YOK.
        let d_new = CohesionAxis::new().descriptor().unwrap();
        let d_norm = CohesionAxis::try_from_normalized(0.5)
            .unwrap()
            .descriptor()
            .unwrap();
        assert_eq!(
            d_new, d_norm,
            "new() and try_from_normalized(0.5) must share descriptor (observational equivalence)"
        );
    }

    #[test]
    fn observationally_equivalent_entropy_configs_share_descriptor() {
        // from_commit_entropy(13) → value = 1.0; from_commit_entropy(100) → clamp(100/13,0,1)=1.0
        // Aynı effective value → aynı descriptor.
        let d_13 = EntropyAxis::from_commit_entropy(13.0).descriptor().unwrap();
        let d_100 = EntropyAxis::from_commit_entropy(100.0)
            .descriptor()
            .unwrap();
        assert_eq!(
            d_13, d_100,
            "same effective value (clamp) must share descriptor"
        );
    }

    #[test]
    fn same_axis_id_with_different_descriptor_changes_digest() {
        // Aynı axis_id "coupling" ama farklı canonical_parameters → farklı descriptor.
        let d1 = CouplingAxis::new().descriptor().unwrap();
        let mut params = AxisParameterEncoder::new();
        params.push_u8(9); // farklı marker
        let d2 = AxisDescriptor::try_new("coupling", 2, params).unwrap();
        assert_ne!(d1, d2);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 — measure() + source provenance testleri
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn axis_measure_returns_source_for_coupling() {
        let axis = CouplingAxis::try_with_source(MetricSource::TreeSitter).unwrap();
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2));
        let m = axis.measure(&node(1), &space).unwrap();
        assert_eq!(m.source, MetricSource::TreeSitter);
        assert!((m.value - 0.5).abs() < 1e-9);
    }

    #[test]
    fn axis_measure_returns_heuristic_for_entropy() {
        let axis = EntropyAxis::from_commit_entropy(6.5);
        let space = Space::new();
        let m = axis.measure(&node(1), &space).unwrap();
        assert_eq!(m.source, MetricSource::Heuristic);
        assert!((m.value - 0.5).abs() < 1e-9); // 6.5/13.0
    }

    #[test]
    fn coupling_axis_rejects_mixed_source() {
        let err = CouplingAxis::try_with_source(MetricSource::Mixed).unwrap_err();
        assert_eq!(err, AxisSourceError::MixedCannotBeDeclaredDirectly);
    }

    #[test]
    fn instability_axis_rejects_mixed_source() {
        let err = InstabilityAxis::try_with_source(MetricSource::Mixed).unwrap_err();
        assert_eq!(err, AxisSourceError::MixedCannotBeDeclaredDirectly);
    }

    #[test]
    fn cohesion_axis_rejects_mixed_observed_source() {
        let err = CohesionAxis::try_with_observed_source(MetricSource::Mixed).unwrap_err();
        assert_eq!(err, AxisSourceError::MixedCannotBeDeclaredDirectly);
    }

    #[test]
    fn cohesion_measure_per_node_source_policy() {
        // Per-node source policy: Some → observed_source; None → fallback source.
        let axis = CohesionAxis::try_with_observed_source(MetricSource::Scip).unwrap();
        let space = Space::new();

        // Some(c) → Scip
        let node_with = Node {
            id: 1,
            cohesion: Some(0.8),
            ..Default::default()
        };
        let m = axis.measure(&node_with, &space).unwrap();
        assert_eq!(m.source, MetricSource::Scip);
        assert!((m.value - 0.8).abs() < 1e-9);

        // None → effective fallback Placeholder
        let node_without = Node {
            id: 2,
            cohesion: None,
            ..Default::default()
        };
        let m = axis.measure(&node_without, &space).unwrap();
        assert_eq!(m.source, MetricSource::Placeholder);
        assert!((m.value - 0.5).abs() < 1e-9);
    }

    #[test]
    fn missing_observed_cohesion_remains_placeholder_even_for_scip_axis() {
        // P2-1 exact regression: observed_source = Scip olsa bile node.cohesion = None
        // ise fallback source Placeholder olmalı. Analyzer P0 düzeltmesi ile birleşim
        // noktası — placeholder cohesion Scip olarak yükseltilmez.
        let axis = CohesionAxis::try_with_observed_source(MetricSource::Scip)
            .expect("Scip is a valid direct source");
        let node = Node {
            cohesion: None,
            ..Default::default()
        };
        let measured = axis
            .measure(&node, &Space::default())
            .expect("valid placeholder fallback");
        assert_eq!(measured.value, 0.5);
        assert_eq!(measured.source, MetricSource::Placeholder);
    }

    #[test]
    fn default_raw_five_rejects_mixed_topology_source() {
        let err = CoordinateSystem::default_raw_five(
            MetricSource::Mixed,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap_err();
        assert_eq!(
            err,
            AxisRegistrationError::InvalidAxisSource(
                AxisSourceError::MixedCannotBeDeclaredDirectly
            )
        );
    }

    #[test]
    fn default_raw_three_rejects_mixed_topology_source() {
        let err = CoordinateSystem::default_raw_three(
            MetricSource::Mixed,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap_err();
        assert_eq!(
            err,
            AxisRegistrationError::InvalidAxisSource(
                AxisSourceError::MixedCannotBeDeclaredDirectly
            )
        );
    }

    #[test]
    fn topology_source_propagates_to_both_coupling_and_instability() {
        // Coupling ve Instability aynı topology_source alır.
        let cs = CoordinateSystem::default_raw_five(
            MetricSource::TreeSitter,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2));

        // Coupling ve Instability descriptor'ları TreeSitter source ID taşır.
        let descriptors: std::collections::HashMap<_, _> = cs
            .canonical_raw_axis_descriptors()
            .unwrap()
            .into_iter()
            .map(|d| (d.axis_id().to_string(), d))
            .collect();
        let coupling = descriptors.get("coupling").unwrap();
        assert!(coupling
            .canonical_parameters()
            .windows(b"tree-sitter".len())
            .any(|w| w == b"tree-sitter"));
        let instability = descriptors.get("instability").unwrap();
        assert!(instability
            .canonical_parameters()
            .windows(b"tree-sitter".len())
            .any(|w| w == b"tree-sitter"));
    }
}
