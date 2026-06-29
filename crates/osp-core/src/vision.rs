//! Vizyon vektörü + sapma metrikleri + derived hesap (§5).
//!
//! **Faz 1.7:**
//! - `VisionVector` — elle-deklare vizyon (§5.1, RawPosition)
//! - `DeviationMetric` trait — `theta(&RawPosition, ...)` (inv #4: full Position DEĞİL)
//! - `CosineDeviation` — naif 5-boyutlu kosinüs, commit path'inde (inv #5)
//! - `DiffusionDeviation { t }` — spektral stub (Faz 2 doldurur; analyze-only, inv #5)
//! - `compute_derived()` — Raw + Vizyon → `DerivedPosition` (u, θ, D ayrı — inv #10)

use crate::coords::{DerivedPosition, RawPosition};
use crate::space::Space;

// ═══════════════════════════════════════════════════════════════════════════════
// VisionSource — vision vektörünün provenance'ı (epistemolojik dürüstlük)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir `VisionVector`'ün nereden geldiğini belirten provenance etiketi.
///
/// MetricValue'daki (source, confidence, coverage) provenance modelinin vision
/// için karşılığıdır (§3.2). UI "Vision: not loaded" ile "θ = 0.04 pass" çelişkisini
/// çözer: bir node'un θ değerinin hangi vision kaynağına dayandığı açık olmalıdır.
///
/// Güven hiyerarşisi (yüksek → düşük):
/// - `UserLoaded` — kullanıcı TOML `[raw]` ile elle deklare etti (en yüksek otorite)
/// - `RoleProfile` — kullanıcı TOML `[role_overrides.<Role>]` ile role-specific
/// - `GlobalDefault` — engine hardcoded default (analiz için, gate için değil)
/// - `BuiltinRole` — `VisionConfig::builtin_role_override` hardcoded sensible default
/// - `None` — vision yüklenmemiş; θ HESAPLANMAMALI (topology-only mod)
///
/// `confidence` alanı risk_score (§3.2, Faz 2 stub) için girdidir:
/// UserLoaded=1.0, RoleProfile=0.9, GlobalDefault=0.5, BuiltinRole=0.6, None=0.0.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum VisionSource {
    /// Vision yüklenmemiş. θ hesaplanmamalı (topology-only mod). "not loaded" UI etiketi.
    #[default]
    None,
    /// Engine hardcoded global default — analiz/görselleştirme içindir, gate için
    /// kullanıcı onayı gerekir. "global default" UI etiketi.
    GlobalDefault,
    /// `VisionConfig::builtin_role_override` — role-specific hardcoded sensible default.
    /// "role-default (builtin)" UI etiketi.
    BuiltinRole,
    /// Kullanıcı TOML `[role_overrides.<Role>]` — role-specific, user-declared.
    /// "role profile" UI etiketi.
    RoleProfile,
    /// Kullanıcı TOML `[raw]` — global, elle deklare (en yüksek otorite).
    /// "user loaded" UI etiketi.
    UserLoaded,
}

impl VisionSource {
    /// Provenance'a bağlı güven skoru ∈ [0, 1] (risk_score girdisi, Faz 2).
    pub fn confidence(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::GlobalDefault => 0.5,
            Self::BuiltinRole => 0.6,
            Self::RoleProfile => 0.9,
            Self::UserLoaded => 1.0,
        }
    }

    /// θ hesaplaması anlamlı mı? `None` → hayır (topology-only); diğerleri → evet.
    pub fn is_evaluable(self) -> bool {
        !matches!(self, Self::None)
    }

    /// UI etiketi (frontend için human-readable).
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "not loaded",
            Self::GlobalDefault => "global default",
            Self::BuiltinRole => "role-default (builtin)",
            Self::RoleProfile => "role profile",
            Self::UserLoaded => "user loaded",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VisionVector — elle-deklare (§5.1) + provenance
// ═══════════════════════════════════════════════════════════════════════════════

/// Projenin hedeflediği ideal konum. **Elle deklare edilir** (mimari kurallar +
/// DDD + NFR'lerden; LLM yalnızca önerir, asla otomatik uygulamaz).
///
/// `RawPosition` sarmalar — derived değil (inv #4). Faz 2 Space engine parse eder;
/// Faz 5 LLM önerileri yine insan-onayıyla eklenir.
///
/// `source` alanı provenance taşır (VisionSource) — "Vision: not loaded" iken
/// θ hesaplanması çelişkisini tip-seviyesinde çözer. Eski kod `VisionVector::new(raw)`
/// ile `VisionSource::GlobalDefault` varsayalım (backward-compat; gerçek kullanıcı
/// vision'ı `with_source` ile işaretlenmeli).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VisionVector {
    pub raw: RawPosition,
    #[serde(default = "default_global_source")]
    pub source: VisionSource,
}

fn default_global_source() -> VisionSource {
    VisionSource::GlobalDefault
}

impl VisionVector {
    /// Legacy constructor — source bilinmiyorsa `GlobalDefault` varsay.
    /// Yeni kod `with_source()` kullanmalı; vision yüklenmemişse `none()`.
    pub fn new(raw: RawPosition) -> Self {
        Self {
            raw,
            source: VisionSource::GlobalDefault,
        }
    }

    /// Belirli bir provenance ile kur.
    pub fn with_source(raw: RawPosition, source: VisionSource) -> Self {
        Self { raw, source }
    }

    /// Vision yok (topology-only mod). raw = zero; θ hesaplanmamalı.
    pub fn none() -> Self {
        Self {
            raw: RawPosition::default(),
            source: VisionSource::None,
        }
    }

    /// Inner RawPosition'a erişim.
    pub fn raw(&self) -> &RawPosition {
        &self.raw
    }

    /// Provenance etiketi.
    pub fn source(&self) -> VisionSource {
        self.source
    }

    /// θ bu vision'a karşı hesaplanabilir mi? (None → false)
    pub fn is_evaluable(&self) -> bool {
        self.source.is_evaluable()
    }
}

impl Default for VisionVector {
    fn default() -> Self {
        Self::none()
    }
}

impl From<RawPosition> for VisionVector {
    fn from(raw: RawPosition) -> Self {
        Self::new(raw)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DeviationMetric trait (inv #4 — compile-time dairesellik koruması)
// ═══════════════════════════════════════════════════════════════════════════════

/// Sapma ölçümü. `theta` **yalnızca `&RawPosition`** alır — `&Position` (full) DEĞİL.
///
/// Bu imza **inv #4**'ü yapısal garanti eder: `u = 1 − θ` ve `θ = deviation(P, V_vision)`
/// daireselliğini tip-seviyesinde önler. `compute_theta(full_P)` yazmak derleme hatası.
///
/// Dönüş: `θ ∈ [0, 1]` normalize sapma. 0 = mükemmel hizalı, 1 = zıt (negatif uzay).
/// Negatif-uzay eşiği (formalism §5.2: `θ ≥ π/2`) → normalize'de `θ ≥ 0.5`.
pub trait DeviationMetric: Send + Sync {
    fn theta(&self, raw: &RawPosition, vision: &VisionVector, space: &Space) -> f64;
}

// ═══════════════════════════════════════════════════════════════════════════════
// CosineDeviation — naif baseline (inv #5: commit path'inde kullanılır)
// ═══════════════════════════════════════════════════════════════════════════════

/// Naif 5-boyutlu kosinüs sapması. Hızlı (`O(5)`), commit akışında güvenli (inv #5).
///
/// `θ = (1 − cos_sim) / 2` mapping: `cos_sim ∈ [-1, 1]` → `θ ∈ [0, 1]`.
/// - `cos_sim = 1` (aynı yön) → `θ = 0` (mükemmel hizalı)
/// - `cos_sim = 0` (ortogonal) → `θ = 0.5` (negatif-uzay eşiği)
/// - `cos_sim = -1` (zıt) → `θ = 1` (maksimum sapma)
#[derive(Debug, Clone, Copy, Default)]
pub struct CosineDeviation;

impl DeviationMetric for CosineDeviation {
    fn theta(&self, raw: &RawPosition, vision: &VisionVector, _space: &Space) -> f64 {
        // inv: source=None → θ hesaplanmamalı. Conservative: maksimum sapma döndür
        // (çift-katmanlı koruma — caller zaten is_evaluable() kontrol etmeli).
        if !vision.is_evaluable() {
            return 1.0;
        }
        cosine_normalized(raw, &vision.raw)
    }
}

/// 5-boyutlu kosinüs benzerliği → normalize sapma [0,1].
fn cosine_normalized(p: &RawPosition, v: &RawPosition) -> f64 {
    let dot = p.x * v.x + p.y * v.y + p.z * v.z + p.w * v.w + p.v * v.v;
    let norm_p = (p.x * p.x + p.y * p.y + p.z * p.z + p.w * p.w + p.v * p.v).sqrt();
    let norm_v = (v.x * v.x + v.y * v.y + v.z * v.z + v.w * v.w + v.v * v.v).sqrt();
    if norm_p < 1e-9 || norm_v < 1e-9 {
        return 1.0; // sıfır vektör → maksimum sapma (tanımsız, conservative)
    }
    let cos_sim = (dot / (norm_p * norm_v)).clamp(-1.0, 1.0);
    ((1.0 - cos_sim) / 2.0).clamp(0.0, 1.0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiffusionDeviation — spektral stub (Faz 2, inv #5: analyze-only)
// ═══════════════════════════════════════════════════════════════════════════════

/// Diffusion distance — spektral Laplacian tabanlı (§5.3).
///
/// **Faz 1.7: stub.** Gerçek hesap (`K_t = e^{−tL}`, `θ = ||f_P − f_V||₂`) Faz 2'de.
/// Şimdilik `todo!()` panik — kimse commit path'inde çağırmamalı (inv #5).
/// `commit()` her zaman `CosineDeviation` kullanır; `DiffusionDeviation` yalnızca
/// `osp analyze` / dashboard context (Faz 2).
#[derive(Debug, Clone, Copy)]
pub struct DiffusionDeviation {
    /// Diffusion zaman-parametresi (§5.3). `t* = 1/λ₂` aday (Faz 1.11 kalibrasyon).
    pub t: f64,
}

impl DeviationMetric for DiffusionDeviation {
    fn theta(&self, _raw: &RawPosition, _vision: &VisionVector, _space: &Space) -> f64 {
        // Faz 2: graph Laplacian L = D − A; K_t = e^{−tL}; θ = ||K_t·δ_P − K_t·δ_V||₂
        todo!("DiffusionDeviation::theta — Faz 2 implementasyonu (spektral Laplacian)")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// compute_derived — Raw + Vizyon → DerivedPosition (inv #10: D ayrı)
// ═══════════════════════════════════════════════════════════════════════════════

/// `RawPosition` + `VisionVector` → `DerivedPosition` hesapla.
///
/// - `theta` = metric.theta(raw, vision, space) ∈ [0,1]
/// - `u` = `(1 − theta).clamp(0, 1)` (vision alignment)
/// - `main_sequence_distance` = `|A + I − 1|` (Martin, inv #10 — z'ye gömülü DEĞİL)
/// - `risk_score` = 0.0 (Faz 2 composite risk)
///
/// `instability` (I) ve `abstractness` (A) ayrı parametreler: I `raw.z`'den okunabilir
/// (Faz 1.9 InstabilityAxis) AMA A bir raw eksen DEĞİL — D için ayrı gerekir. Explicit
/// parametreler compute_derived'i Faz 1.4-1.8 (raw.z=0) için de çalıştırır.
pub fn compute_derived(
    raw: &RawPosition,
    vision: &VisionVector,
    space: &Space,
    metric: &dyn DeviationMetric,
    instability: f64,
    abstractness: f64,
) -> DerivedPosition {
    let theta = metric.theta(raw, vision, space);
    let u = (1.0 - theta).clamp(0.0, 1.0);
    let d = (abstractness + instability - 1.0).abs().clamp(0.0, 1.0);
    DerivedPosition {
        u,
        theta,
        risk_score: 0.0, // Faz 2
        main_sequence_distance: d,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VisionVerdict — node analiz seviyesi karar semantiği (#5)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir node'un vision'a göre değerlendirme sonucu — analiz seviyesi (commit
/// pipeline'dan ayrı). Review'in #4 "advisory ayrı state" önerisinin formal hali.
///
/// **Önemli kapsam ayrımı:** Bu enum **commit pipeline Q5 gate'inin yerini almaz**.
/// Q5 (`check_claim_vision`) claim-based hard reject üretir (`EngineCommitError`).
/// `VisionVerdict` ise **repo-wide node analizi** içindir — her node'un mevcut
/// vision'a göre durumu (inspector, scatter coloring, breakdown). Aynı metrik
/// farklı rolde farklı anlama gelir:
///
/// - `Pass` — θ ≤ θ_bound, sapma kabul edilebilir (yeşil)
/// - `Warning` — θ_bound < θ ≤ θ_warn, boundary'ye yakın (sarı)
/// - `Advisory` — θ > θ_bound AMA node bir Support rolünde (test/fixture/migration).
///   Sapma "gerçek" ama bu rol bağlamında beklenen/normal. Hard fail DEĞİL.
/// - `Reject` — θ > θ_bound, Production/Core node (kırmızı)
/// - `Inconclusive` — θ hesaplanamadı (vision source=None, veya cohesion placeholder
///   iken θ sonucu düşük güven — "we don't know" epistemolojik dürüstlük)
///
/// Advisory, Warning'den farklıdır: Warning "dikkat, sorun olabilir" der; Advisory
/// ise "metrik sapıyor ama rol bağlamında normal olabilir" der. Bu ayrım OSP'nin
/// "aynı metrik farklı rolde farklı anlama gelir" iddiasının formal gövdesidir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VisionVerdict {
    /// θ ≤ θ_bound — kabul edilebilir sapma.
    Pass,
    /// θ_bound < θ ≤ θ_warn — boundary'ye yakın, dikkat.
    Warning,
    /// θ > θ_bound AMA Support rolünde (test/fixture/migration/generated/config/script).
    /// Sapma gerçek ama rol bağlamında beklenen. Hard fail değil, advisory.
    Advisory,
    /// θ > θ_warn — Production/Core/Adapter/Utility/Runtime node, sapma gerçek risk.
    Reject,
    /// θ hesaplanamadı (vision source=None) veya düşük güven (cohesion placeholder).
    /// "we don't know" — fail/pass kararı verilmez.
    Inconclusive,
}

impl VisionVerdict {
    /// UI etiketi (frontend için human-readable).
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warning => "warning",
            Self::Advisory => "advisory",
            Self::Reject => "reject",
            Self::Inconclusive => "inconclusive",
        }
    }

    /// UI rengi (frontend ile aynı değerler — tek kaynak).
    pub fn color_hex(self) -> &'static str {
        match self {
            Self::Pass => "#3fb950",         // yeşil
            Self::Warning => "#d29922",      // amber/sarı
            Self::Advisory => "#d29922",     // amber (warning ile aynı aile, ama farklı anlam)
            Self::Reject => "#f85149",       // kırmızı
            Self::Inconclusive => "#8b949e", // gri
        }
    }

    /// Bu verdict hard-reject mi? (Pass/Warning/Advisory/Inconclusive → false).
    /// Commit pipeline bu bilgiyi kullanmaz (kendi Q5 gate'i var); bu sadece
    /// analiz seviyesi "kaç ciddi sapma var?" sorusu için.
    pub fn is_hard_reject(self) -> bool {
        matches!(self, Self::Reject)
    }
}

/// Bir node'un vision sapma değerlendirmesi — analiz seviyesi `VisionVerdict` üretir.
///
/// **Parametreler:**
/// - `theta`: node'un vision'a sapması (CosineDeviation çıktısı)
/// - `theta_bound`: kabul eşiği (Q5 gate ile aynı, default 0.3)
/// - `theta_warn`: warning eşiği (theta_bound ile theta_bound arası warning).
///   Genelde `theta_bound`'a çok yakın, örn. bound − 0.05.
/// - `is_support_role`: node Support rolünde mi? (test/fixture/migration → true).
///   Advisory downgrade sadece Support için geçerli.
/// - `is_inconclusive`: cohesion placeholder veya vision source=None → true.
///
/// **Karar hiyerarşisi:**
/// 1. `is_inconclusive` → Inconclusive (en yüksek öncelik — "we don't know")
/// 2. theta ≤ theta_bound → Pass
/// 3. theta_bound < theta ≤ theta_warn → Warning
/// 4. theta > theta_warn AND is_support_role → Advisory (downgrade)
/// 5. theta > theta_warn AND !is_support_role → Reject
///
/// Bu, frontend'in soft-coded `advisoryTypes` array + `isAdvisory` mantığının
/// formal tip-seviyesi karşılığıdır. Frontend artık bu fonksiyonu çağırır (veya
/// aynı mantığı mirror'lar).
pub fn evaluate_node_vision(
    theta: f64,
    theta_bound: f64,
    theta_warn: f64,
    is_support_role: bool,
    is_inconclusive: bool,
) -> VisionVerdict {
    use VisionVerdict as V;
    // 1. En yüksek öncelik: ölçülmemişse karar verme.
    if is_inconclusive {
        return V::Inconclusive;
    }
    // 2-3. Pass / Warning boundary'leri.
    if theta <= theta_bound {
        return V::Pass;
    }
    if theta <= theta_warn {
        return V::Warning;
    }
    // 4-5. Üst sınır aşımı: Support → advisory downgrade, diğerleri → reject.
    if is_support_role {
        V::Advisory
    } else {
        V::Reject
    }
}

#[cfg(test)]
mod verdict_tests {
    use super::*;

    #[test]
    fn verdict_pass_when_theta_within_bound() {
        let v = evaluate_node_vision(0.10, 0.30, 0.40, false, false);
        assert_eq!(v, VisionVerdict::Pass);
    }

    #[test]
    fn verdict_warning_between_bound_and_warn() {
        // theta=0.35, bound=0.30, warn=0.40 → warning bandı içinde
        let v = evaluate_node_vision(0.35, 0.30, 0.40, false, false);
        assert_eq!(v, VisionVerdict::Warning);
    }

    #[test]
    fn verdict_reject_for_production_above_warn() {
        // Production node, θ > warn → reject (hard)
        let v = evaluate_node_vision(0.50, 0.30, 0.40, false, false);
        assert_eq!(v, VisionVerdict::Reject);
        assert!(v.is_hard_reject());
    }

    #[test]
    fn verdict_advisory_for_support_above_warn() {
        // Support (test) node, θ > warn → advisory (downgrade, hard değil)
        let v = evaluate_node_vision(0.50, 0.30, 0.40, true, false);
        assert_eq!(v, VisionVerdict::Advisory);
        assert!(!v.is_hard_reject(), "advisory is NOT a hard reject");
    }

    #[test]
    fn verdict_inconclusive_overrides_everything() {
        // Cohesion placeholder iken θ ne olursa olsun → inconclusive.
        // "we don't know" en yüksek öncelik.
        let v_pass = evaluate_node_vision(0.10, 0.30, 0.40, false, true);
        let v_high = evaluate_node_vision(0.90, 0.30, 0.40, true, true);
        assert_eq!(v_pass, VisionVerdict::Inconclusive);
        assert_eq!(v_high, VisionVerdict::Inconclusive);
    }

    #[test]
    fn verdict_boundary_theta_equals_bound_is_pass() {
        // θ == θ_bound → pass (≤ bound).
        let v = evaluate_node_vision(0.30, 0.30, 0.40, false, false);
        assert_eq!(v, VisionVerdict::Pass);
    }

    #[test]
    fn verdict_boundary_theta_equals_warn_is_warning() {
        // θ == θ_warn → warning (≤ warn).
        let v = evaluate_node_vision(0.40, 0.30, 0.40, false, false);
        assert_eq!(v, VisionVerdict::Warning);
    }

    #[test]
    fn verdict_labels_and_colors_distinct() {
        use VisionVerdict as V;
        let cases = [V::Pass, V::Warning, V::Advisory, V::Reject, V::Inconclusive];
        // etiketler benzersiz
        let labels: Vec<&str> = cases.iter().map(|v| v.label()).collect();
        let mut sorted = labels.clone();
        sorted.sort();
        for w in sorted.windows(2) {
            assert_ne!(w[0], w[1]);
        }
        // reject hard, diğerleri değil
        assert!(V::Reject.is_hard_reject());
        assert!(!V::Pass.is_hard_reject());
        assert!(!V::Advisory.is_hard_reject());
        assert!(!V::Inconclusive.is_hard_reject());
    }

    #[test]
    fn verdict_advisory_vs_warning_semantic_distinction() {
        // Aynı θ=0.50: warning değil (≤ warn değil), advisory (support) veya reject.
        // Bu üç durum aynı θ için rol'e göre farklı verdict → OSP'nin "aynı metrik
        // farklı rolde farklı anlam" iddiasının formal kanıtı.
        let support = evaluate_node_vision(0.50, 0.30, 0.40, true, false);
        let production = evaluate_node_vision(0.50, 0.30, 0.40, false, false);
        assert_eq!(support, VisionVerdict::Advisory);
        assert_eq!(production, VisionVerdict::Reject);
        assert_ne!(
            support, production,
            "same θ, different role → different verdict"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(x: f64, y: f64, z: f64, w: f64, v: f64) -> RawPosition {
        RawPosition { x, y, z, w, v }
    }

    // --- VisionVector ---

    #[test]
    fn vision_vector_wraps_raw_position() {
        let vv = VisionVector::new(raw(0.5, 0.5, 0.5, 0.5, 0.5));
        assert_eq!(vv.raw(), &raw(0.5, 0.5, 0.5, 0.5, 0.5));
    }

    #[test]
    fn vision_vector_from_raw() {
        let vv: VisionVector = raw(0.1, 0.2, 0.3, 0.4, 0.5).into();
        assert!((vv.raw().x - 0.1).abs() < 1e-9);
    }

    // --- CosineDeviation ---

    #[test]
    fn cosine_aligned_is_zero() {
        // raw == vision → cos_sim = 1 → θ = 0
        let v = raw(0.5, 0.5, 0.5, 0.5, 0.5);
        let vision = VisionVector::new(v);
        let theta = CosineDeviation.theta(&v, &vision, &Space::new());
        assert!(theta.abs() < 1e-9, "aligned θ = {}", theta);
    }

    #[test]
    fn cosine_opposite_is_near_one() {
        // raw = -vision → cos_sim = -1 → θ = 1
        let v = raw(1.0, 1.0, 1.0, 1.0, 1.0);
        let opposite = raw(-1.0, -1.0, -1.0, -1.0, -1.0);
        let vision = VisionVector::new(v);
        let theta = CosineDeviation.theta(&opposite, &vision, &Space::new());
        assert!((theta - 1.0).abs() < 1e-9, "opposite θ = {}", theta);
    }

    #[test]
    fn cosine_orthogonal_is_half() {
        // cos_sim = 0 → θ = 0.5 (negatif-uzay eşiği)
        let v = raw(1.0, 0.0, 0.0, 0.0, 0.0);
        let ortho = raw(0.0, 1.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(v);
        let theta = CosineDeviation.theta(&ortho, &vision, &Space::new());
        assert!((theta - 0.5).abs() < 1e-9, "orthogonal θ = {}", theta);
    }

    #[test]
    fn cosine_zero_vector_returns_max_deviation() {
        // norm=0 → tanımsız → conservative maksimum (1.0)
        let zero = raw(0.0, 0.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(raw(0.5, 0.5, 0.5, 0.5, 0.5));
        let theta = CosineDeviation.theta(&zero, &vision, &Space::new());
        assert!((theta - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_symmetric_under_scale() {
        // kosinüs ölçek-bağımsız: 2x raw aynı θ
        let v = raw(0.5, 0.5, 0.5, 0.5, 0.5);
        let scaled = raw(1.0, 1.0, 1.0, 1.0, 1.0);
        let vision = VisionVector::new(v);
        let t1 = CosineDeviation.theta(&v, &vision, &Space::new());
        let t2 = CosineDeviation.theta(&scaled, &vision, &Space::new());
        assert!((t1 - t2).abs() < 1e-9);
    }

    // --- inv #4: DeviationMetric imzası RawPosition alır (compile-time garanti) ---

    #[test]
    fn deviation_metric_trait_compiles_with_raw_position() {
        // Bu test derlendiği sürece inv #4 geçerli: theta &RawPosition alır.
        // theta(&Position) denendiğinde derleme hatası (tip uyumsuzluğu).
        fn check<M: DeviationMetric>(m: &M, r: &RawPosition, v: &VisionVector, s: &Space) -> f64 {
            m.theta(r, v, s)
        }
        let m = CosineDeviation;
        let r = raw(0.5, 0.5, 0.5, 0.5, 0.5);
        let v = VisionVector::new(r);
        let theta = check(&m, &r, &v, &Space::new());
        assert!(theta.is_finite());
    }

    // --- compute_derived (inv #10: D ayrı) ---

    #[test]
    fn compute_derived_u_is_one_minus_theta() {
        let r = raw(1.0, 0.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(raw(1.0, 0.0, 0.0, 0.0, 0.0)); // aligned → θ=0 → u=1
        let d = compute_derived(&r, &vision, &Space::new(), &CosineDeviation, 0.5, 0.5);
        assert!((d.theta).abs() < 1e-9);
        assert!((d.u - 1.0).abs() < 1e-9);
    }

    #[test]
    fn compute_derived_d_zero_on_main_sequence() {
        // A + I = 1 → D = 0 (Martin ideal çizgi)
        let r = raw(0.0, 0.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(raw(1.0, 1.0, 1.0, 1.0, 1.0));
        // I=0.3, A=0.7 → A+I=1.0 → D=0
        let d = compute_derived(&r, &vision, &Space::new(), &CosineDeviation, 0.3, 0.7);
        assert!(
            d.main_sequence_distance.abs() < 1e-9,
            "D = {}",
            d.main_sequence_distance
        );
    }

    #[test]
    fn compute_derived_d_max_at_zone_of_pain() {
        // Zone of Pain: A=0, I=0 (concrete + rigid) → D = |0+0-1| = 1
        let r = raw(0.0, 0.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(raw(1.0, 1.0, 1.0, 1.0, 1.0));
        let d = compute_derived(&r, &vision, &Space::new(), &CosineDeviation, 0.0, 0.0);
        assert!((d.main_sequence_distance - 1.0).abs() < 1e-9);
    }

    #[test]
    fn compute_derived_d_max_at_zone_of_uselessness() {
        // Zone of Uselessness: A=1, I=1 (abstract + unstable) → D = |1+1-1| = 1
        let r = raw(0.0, 0.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(raw(1.0, 1.0, 1.0, 1.0, 1.0));
        let d = compute_derived(&r, &vision, &Space::new(), &CosineDeviation, 1.0, 1.0);
        assert!((d.main_sequence_distance - 1.0).abs() < 1e-9);
    }

    #[test]
    fn compute_derived_d_separate_from_z() {
        // inv #10 — D ayrı field; z (instability) raw'da. compute_derived ikisini ayrı tutar.
        let r = raw(0.0, 0.0, 0.4, 0.0, 0.0); // z = I = 0.4 (raw'da)
        let vision = VisionVector::new(raw(1.0, 1.0, 1.0, 1.0, 1.0));
        // instability param = 0.4 (raw.z ile aynı), abstractness = 0.3
        let d = compute_derived(&r, &vision, &Space::new(), &CosineDeviation, 0.4, 0.3);
        // D = |0.3 + 0.4 - 1| = 0.3
        assert!(
            (d.main_sequence_distance - 0.3).abs() < 1e-9,
            "D = {}",
            d.main_sequence_distance
        );
        // z (raw) değişmedi — compute_derived raw'yı mutate etmez
        assert!((r.z - 0.4).abs() < 1e-9);
    }

    #[test]
    fn compute_derived_orthogonal_negative_space() {
        // θ = 0.5 (orthogonal) → negatif-uzay eşiği; u = 0.5
        let r = raw(1.0, 0.0, 0.0, 0.0, 0.0);
        let vision = VisionVector::new(raw(0.0, 1.0, 0.0, 0.0, 0.0));
        let d = compute_derived(&r, &vision, &Space::new(), &CosineDeviation, 0.5, 0.5);
        assert!((d.theta - 0.5).abs() < 1e-9);
        assert!((d.u - 0.5).abs() < 1e-9);
    }

    // --- DiffusionDeviation stub (Faz 2) ---

    #[test]
    #[should_panic(expected = "DiffusionDeviation::theta")]
    fn diffusion_deviation_is_faz2_stub() {
        // inv #5 — DiffusionDeviation commit path'inde değil; Faz 2 doldurana kadar todo!.
        let d = DiffusionDeviation { t: 1.0 };
        let _ = d.theta(
            &raw(0.5, 0.5, 0.5, 0.5, 0.5),
            &VisionVector::new(raw(0.5, 0.5, 0.5, 0.5, 0.5)),
            &Space::new(),
        );
    }

    // ── VisionSource provenance ──────────────────────────────────────────────
    // #2 görevi: "Vision: not loaded" iken θ hesaplanması çelişkisini çöz.
    // None → θ hesaplanmamalı (topology-only); diğerleri → evaluable.

    #[test]
    fn vision_source_confidence_hierarchy() {
        // Güven sıralaması: None < GlobalDefault < BuiltinRole < RoleProfile < UserLoaded
        use super::VisionSource as S;
        assert!((S::None.confidence() - 0.0).abs() < 1e-9);
        assert!((S::GlobalDefault.confidence() - 0.5).abs() < 1e-9);
        assert!((S::BuiltinRole.confidence() - 0.6).abs() < 1e-9);
        assert!((S::RoleProfile.confidence() - 0.9).abs() < 1e-9);
        assert!((S::UserLoaded.confidence() - 1.0).abs() < 1e-9);
        assert!(S::None.confidence() < S::GlobalDefault.confidence());
        assert!(S::GlobalDefault.confidence() < S::BuiltinRole.confidence());
        assert!(S::BuiltinRole.confidence() < S::RoleProfile.confidence());
        assert!(S::RoleProfile.confidence() < S::UserLoaded.confidence());
    }

    #[test]
    fn vision_source_evaluable_when_not_none() {
        use super::VisionSource as S;
        assert!(!S::None.is_evaluable(), "None must NOT be evaluable");
        assert!(S::GlobalDefault.is_evaluable());
        assert!(S::BuiltinRole.is_evaluable());
        assert!(S::RoleProfile.is_evaluable());
        assert!(S::UserLoaded.is_evaluable());
    }

    #[test]
    fn vision_vector_none_is_topology_only() {
        // VisionVector::none() → source=None → is_evaluable() false.
        let v = VisionVector::none();
        assert_eq!(v.source(), VisionSource::None);
        assert!(!v.is_evaluable());
        // none() raw'ı default (zero) — ama bu hesaba katılmamalı (topology-only).
        assert_eq!(v.raw(), &RawPosition::default());
    }

    #[test]
    fn cosine_returns_max_deviation_for_none_vision() {
        // Kritik: source=None vision ile θ hesaplanmamalı. Conservative davranış:
        // maksimum sapma (1.0) döndür — bu ya caller'ı is_evaluable() kontrol etmeye
        // zorlar ya da güvenli-taraf hatası üretir (topology-only modda hiçbir claim
        // "pass" olarak yanlış işaretlenmez).
        let none_vision = VisionVector::none();
        let aligned_raw = raw(0.5, 0.5, 0.5, 0.5, 0.5);
        let theta = CosineDeviation.theta(&aligned_raw, &none_vision, &Space::new());
        assert!(
            (theta - 1.0).abs() < 1e-9,
            "None vision must return max deviation, got θ = {}",
            theta
        );
    }

    #[test]
    fn vision_vector_with_source_preserves_provenance() {
        // with_source provenance'ı korur; new() GlobalDefault varsayar (legacy).
        let user_vision =
            VisionVector::with_source(raw(0.3, 0.7, 0.5, 0.5, 0.5), VisionSource::UserLoaded);
        assert_eq!(user_vision.source(), VisionSource::UserLoaded);
        assert!(user_vision.is_evaluable());

        let legacy = VisionVector::new(raw(0.3, 0.7, 0.5, 0.5, 0.5));
        assert_eq!(
            legacy.source(),
            VisionSource::GlobalDefault,
            "new() backwards-compat: GlobalDefault varsay"
        );
    }

    #[test]
    fn vision_source_labels_are_distinct() {
        // UI etiketleri benzersiz ve human-readable olmalı.
        use super::VisionSource as S;
        let labels = [
            S::None.label(),
            S::GlobalDefault.label(),
            S::BuiltinRole.label(),
            S::RoleProfile.label(),
            S::UserLoaded.label(),
        ];
        // tüm etiketler benzersiz
        let mut sorted = labels;
        sorted.sort();
        for w in sorted.windows(2) {
            assert_ne!(w[0], w[1], "duplicate label: {}", w[0]);
        }
    }

    #[test]
    fn vision_vector_default_is_none() {
        // Default → None (topology-only). Engine kurulurken vision verilmezse
        // güvenli taraf: değerlendirme yapılmaz, "not loaded" etiketi gösterilir.
        let v = VisionVector::default();
        assert_eq!(v.source(), VisionSource::None);
        assert!(!v.is_evaluable());
    }
}
