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

use crate::coords::{Axis, CoordinateSystem};
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
#[derive(Debug, Clone, Copy, Default)]
pub struct CouplingAxis;

impl CouplingAxis {
    pub fn new() -> Self {
        Self
    }
}

impl Axis for CouplingAxis {
    fn name(&self) -> &'static str {
        "coupling"
    }
    fn compute(&self, node: &Node, space: &Space) -> f64 {
        // Value-only out-degree: type-only import'lar (`import type`) runtime dependency
        // değildir, coupling'i artırmamalı. Mimari açıdan doğru: bir .d.ts dosyası
        // `import type {Foo}` yaparsa bu runtime coupling üretmez.
        let deg = space.out_degree_value(node.id, EdgeKind::Imports) as f64;
        deg / (1.0 + deg)
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
#[derive(Debug, Clone, Copy, Default)]
pub struct InstabilityAxis;

impl InstabilityAxis {
    pub fn new() -> Self {
        Self
    }
}

impl Axis for InstabilityAxis {
    fn name(&self) -> &'static str {
        "instability"
    }
    fn compute(&self, node: &Node, space: &Space) -> f64 {
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

// ═══════════════════════════════════════════════════════════════════════════════
// y — Cohesion (LCOM4 proxy, precomputed) — Faz 1.9
// ═══════════════════════════════════════════════════════════════════════════════

/// `y` ekseni — **Kohezyon** (LCOM4).
///
/// **Per-node cohesion:** `compute(node, space)` öncelikle `node.cohesion` okur
/// (analyzer tarafından SCIP LCOM4'ten set edilir, Faz 3.6+). Node'da değer yoksa
/// (`None`), `fallback` kullanılır (test/repo-level sabit değer için backward-compat).
///
/// `cohesion ∈ [0, 1]`: `1.0` = tam kohezif (LCOM4=1), `0.0` = kohezyon yok.
#[derive(Debug, Clone, Copy)]
pub struct CohesionAxis {
    /// `node.cohesion == None` durumunda kullanılacak değer (backward-compat).
    /// `None` = 0.5 nötr default.
    fallback: Option<f64>,
}

impl CohesionAxis {
    /// Per-node cohesion okuyan axis — analyzer `node.cohesion` set ettiğinde gerçek değer kullanılır.
    /// Set edilmediyse 0.5 nötr default.
    pub fn new() -> Self {
        Self { fallback: None }
    }

    /// Sabit fallback cohesion `[0,1]` — `node.cohesion` None ise bu kullanılır.
    /// Backward-compat için (eski test'ler `from_normalized`/`from_lcom4` kullanır).
    pub fn from_normalized(cohesion: f64) -> Self {
        Self {
            fallback: Some(cohesion.clamp(0.0, 1.0)),
        }
    }

    /// LCOM4 raw değerinden fallback mapping: `cohesion = 1 / lcom4`.
    /// `lcom4=1` → `1.0` (kohezif), `lcom4=2` → `0.5`, `lcom4≥4` → `≤0.25`.
    pub fn from_lcom4(lcom4: usize) -> Self {
        let cohesion = if lcom4 == 0 {
            1.0 // convention: 0 = undefined → conservative full cohesion
        } else {
            (1.0 / lcom4 as f64).clamp(0.0, 1.0)
        };
        Self {
            fallback: Some(cohesion),
        }
    }

    pub fn value(&self) -> f64 {
        self.fallback.unwrap_or(0.5)
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
    fn compute(&self, node: &Node, _space: &Space) -> f64 {
        // Per-node cohesion (analyzer SCIP LCOM4) → fallback → 0.5 neutral
        node.cohesion.or(self.fallback).unwrap_or(0.5)
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
    pub fn default_raw_three(entropy: EntropyAxis, witness: WitnessDepthAxis) -> Self {
        Self::empty()
            .with_axis(CouplingAxis::new())
            .with_axis(entropy)
            .with_axis(witness)
    }

    /// Varsayılan **raw** preset — 5 eksen (`x` coupling, `y` cohesion, `z` instability,
    /// `w` entropy, `v` witness-depth). Faz 1.9.
    ///
    /// `u` (vision alignment) **derived**'dır, preset'te YOK (inv #4 — `compute_derived`
    /// ile `DerivedPosition`'da, `vision.rs` Faz 1.7). `D` (Martin main-sequence) de
    /// derived — `compute_derived` ayrı parametre olarak alır (inv #10).
    pub fn default_raw_five(
        cohesion: CohesionAxis,
        entropy: EntropyAxis,
        witness: WitnessDepthAxis,
    ) -> Self {
        Self::empty()
            .with_axis(CouplingAxis::new())    // x — per-node
            .with_axis(cohesion)               // y — precomputed
            .with_axis(InstabilityAxis::new()) // z — per-node
            .with_axis(entropy)                // w — precomputed
            .with_axis(witness)                // v — precomputed
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
        assert!((v - 0.5).abs() < 1e-9, "type-only import should not count, coupling = {}", v);

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
        assert!((v_with_only_value - 1.0).abs() < 1e-9, "I with only value = 1.0");

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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        assert!(
            !cs.axis_names().contains(&"vision_alignment"),
            "vision_alignment raw preset'te olmamalı (derived, inv #4)"
        );
    }

    #[test]
    fn default_raw_three_raw_position_maps_correctly() {
        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.5),    // w = 0.5 (6.5/13.0, Faz 1.11 cap tune)
            WitnessDepthAxis::from_witness(0.3, 5),   // v
        );
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2));         // node 1: 1 import → x = 0.5

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
    fn cohesion_from_normalized_clamps() {
        assert!((CohesionAxis::from_normalized(1.5).value() - 1.0).abs() < 1e-9);
        assert!((CohesionAxis::from_normalized(-0.3).value() - 0.0).abs() < 1e-9);
        assert!((CohesionAxis::from_normalized(0.7).value() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn cohesion_axis_name_is_cohesion() {
        let a = CohesionAxis::from_normalized(0.5);
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
        assert!((y - 0.8).abs() < 1e-9, "per-node cohesion should be 0.8, got {}", y);
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
        assert!((y - 0.5).abs() < 1e-9, "no cohesion → default 0.5, got {}", y);
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
        assert!((y - 1.0).abs() < 1e-9, "fallback should be 1.0 when node has None, got {}", y);
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
        assert!((y - 0.6).abs() < 1e-9, "node cohesion should override fallback, got {}", y);
    }

    // --- default_raw_five preset — Faz 1.9 ---

    #[test]
    fn default_raw_five_has_five_axes_in_canonical_order() {
        let cs = CoordinateSystem::default_raw_five(
            CohesionAxis::from_lcom4(1),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        assert_eq!(cs.dim(), 5);
        assert_eq!(
            cs.axis_names(),
            vec!["coupling", "cohesion", "instability", "entropy", "witness_depth"]
        );
    }

    #[test]
    fn default_raw_five_excludes_vision_alignment() {
        // inv #4 — u derived, raw preset'te YOK
        let cs = CoordinateSystem::default_raw_five(
            CohesionAxis::from_lcom4(1),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        assert!(
            !cs.axis_names().contains(&"vision_alignment"),
            "vision_alignment derived — raw preset'te olmamalı"
        );
    }

    #[test]
    fn default_raw_five_raw_position_all_fields_populated() {
        let cs = CoordinateSystem::default_raw_five(
            CohesionAxis::from_normalized(0.8),       // y
            EntropyAxis::from_commit_entropy(6.5),    // w = 0.5 (6.5/13.0, Faz 1.11 cap tune)
            WitnessDepthAxis::from_witness(0.3, 5),   // v
        );
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2));         // node 1: Ce=1, Ca=0

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
            CohesionAxis::from_lcom4(1),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let mut space = Space::new();
        space.insert_node(node(1));
        space.insert_node(node(2));
        space.insert_edge(import_edge(1, 2)); // 1→2: node 1 unstable (I=1), node 2 stable (I=0)

        let raw1 = cs.raw_position_of(&node(1), &space);
        let raw2 = cs.raw_position_of(&node(2), &space);
        assert!((raw1.z - 1.0).abs() < 1e-9, "node 1 z (unstable) = {}", raw1.z);
        assert!(raw2.z.abs() < 1e-9, "node 2 z (stable) = {}", raw2.z);
    }
}
