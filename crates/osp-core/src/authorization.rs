//! INV-T9 — Authorization basis + digest + pending suspension types.
//!
//! Bu modül witness authorization bekleme durumunun (INV-T9) veri modelini taşır:
//! - [`AuthorizationBasis`]: witness'ın yetkilendirdiği claim'in tam kanonik temsili.
//! - [`AuthorizationBasisDigest`]: BLAKE3 tabanlı, domain-separated, canonical encoding digest.
//! - [`EvaluationContextDigest`]: vision config + rule-set + semantics versions digest.
//! - [`SpaceViewRevision`]: store-scoped, lane-qualified revision identity.
//! - [`Clock`] trait: deterministic time abstraction (core SystemTime::now() çağırmaz).
//!
//! **Prensip:** Digest, authorization basis'i *yeniden oluşturamaz*; yalnızca eldeki
//! basis'in aynı olup olmadığını doğrular. Bu yüzden [`PendingAuthorizationEnvelope`]
//! (Commit 4) hem digest hem full [`AuthorizationBasis`] taşır — load sırasında
//! digest tekrar hesaplanıp doğrulanır.

use crate::coords::{AxisDescriptor, CoordinateSystem};
use crate::space::NodeId;
use crate::trajectory::{
    ApplyTarget, AttemptOutcome, GateDecision, MutationDecision, PredicateCompletion,
};
use crate::witness::{AgentId, ClaimId, WitnessHoldReason, WitnessQuorumSnapshot};

// ═══════════════════════════════════════════════════════════════════════════════
// Claim identity + structural delta (canonical encoding için)
// ═══════════════════════════════════════════════════════════════════════════════

/// Claim'in kalıcı kimliği — digest'e dahil edilir.
///
/// `claim_id` + `task_id` + `author` kombinasyonu claim'i benzersiz tanımlar.
/// Structural delta'nın kendisi ayrıca [`CanonicalStructuralDelta`] içinde gelir.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClaimIdentity {
    pub claim_id: ClaimId,
    pub task_id: crate::trajectory::TaskId,
}

/// Claim author — INV-T9 digest'ine dahil (author-witness ayrımı için kritik).
pub type ClaimAuthor = AgentId;

/// Structural delta'nın tam kanonik temsili — witness'ın yetkilendirdiği structural
/// delta'nın tamamını bağlar (reviewer P0-4 inclusion table).
///
/// **Prensip:** Yalnız ölçümü etkileyen alanlar değil, witness'ın yetkilendirdiği
/// bütün author-controlled structural içeriği bağlanır. Engine-derived alanlar
/// (position) ve transient cache dahil DEĞİL.
///
/// Node kind/edge kind stable numeric tag olarak (format!("{:?}") DEĞİL).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalStructuralDelta {
    /// Eklenen node'lar (sorted by id). Full canonical content.
    pub new_nodes: Vec<CanonicalNode>,
    /// Eklenen edge'ler — sorted.
    pub new_edges: Vec<CanonicalEdge>,
    /// Kaldırılan edge'ler — sorted. G2c-2 subtractive delta.
    pub removed_edges: Vec<CanonicalEdge>,
}

/// Canonical node — witness'ın yetkilendirdiği structural içeriğin tam temsili.
///
/// Inclusion table (reviewer P0-4):
/// - id: identity
/// - kind: structural semantics
/// - mass: measurement input
/// - cohesion: measurement input
/// - classification: author-controlled structural (context-aware metric interpretation)
/// - role: author-controlled structural (role-aware vision)
/// - position: HAYIR (engine-derived, agent-declared değil)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalNode {
    pub id: NodeId,
    pub kind: CanonicalNodeKind,
    pub mass: CanonicalF64,
    pub cohesion: Option<CanonicalF64>,
    pub classification: CanonicalNodeClassification,
    pub role: CanonicalNodeRole,
}

/// Canonical edge — structural relationship.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct CanonicalEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: CanonicalEdgeKind,
    pub is_type_only: bool,
}

/// Stable numeric tags (format!("{:?}") DEĞİL).
///
/// **reviewer P1:** Artık validated newtype'lar (`canonical_tags` modülü). Ham `u8`
/// alias DEĞİL — imkânsız tag üretilemez, domain enum'a yeni varyant eklenince
/// compiler mapping'i güncellemeye zorlar. Tüm tipler `authorization` API yüzeyinden
/// re-export edilir (downstream kod kırılmaz).
pub use crate::canonical_tags::{
    CanonicalEdgeKind, CanonicalMetricSourceTag, CanonicalNodeClassification, CanonicalNodeKind,
    CanonicalNodeRole, ComparisonOpTag, PredicateAxisTag, PredicateFailurePolicyTag,
    PredicateModeTag, WitnessIndependencePolicyTag,
};

/// Canonical f64 — NaN reject, -0.0 normalize, to_bits encoding.
pub type CanonicalF64 = f64;

impl CanonicalStructuralDelta {
    /// **reviewer P1:** Validating smart constructor — vec'leri canonical (sorted)
    /// sıraya koyar VE structural identity çelişkilerini reddeder.
    ///
    /// Digest katmanı savunmacıdır: syntax gate normal akışta duplicate'leri yakalasa da
    /// canonical artifact deserialize edilerek doğrudan oluşturulabilir.
    ///
    /// Reddedilen durumlar:
    /// - duplicate node id (new_nodes içinde)
    /// - duplicate edge (aynı listede — new_edges veya removed_edges)
    /// - **cross-list çelişki** (plan-review): aynı edge ∈ new_edges ve ∈ removed_edges
    ///   → self-cancelling/ambiguous delta
    /// - non-finite node field (mass veya cohesion)
    pub fn try_new(
        mut new_nodes: Vec<CanonicalNode>,
        mut new_edges: Vec<CanonicalEdge>,
        mut removed_edges: Vec<CanonicalEdge>,
    ) -> Result<Self, CanonicalizationError> {
        // Duplicate node id kontrolü.
        new_nodes.sort_unstable_by_key(|n| n.id);
        for window in new_nodes.windows(2) {
            if window[0].id == window[1].id {
                return Err(CanonicalizationError::DuplicateNodeId(window[0].id));
            }
        }
        // Non-finite node field kontrolü.
        for node in &new_nodes {
            if !node.mass.is_finite() {
                return Err(CanonicalizationError::NonFiniteNodeField(node.id));
            }
            if let Some(c) = node.cohesion {
                if !c.is_finite() {
                    return Err(CanonicalizationError::NonFiniteNodeField(node.id));
                }
            }
        }
        // Duplicate edge kontrolü (aynı liste).
        new_edges.sort_unstable();
        if new_edges.windows(2).any(|w| w[0] == w[1]) {
            return Err(CanonicalizationError::DuplicateEdge);
        }
        removed_edges.sort_unstable();
        if removed_edges.windows(2).any(|w| w[0] == w[1]) {
            return Err(CanonicalizationError::DuplicateEdge);
        }
        // Cross-list çelişki: aynı edge hem new hem removed — self-cancelling delta.
        for ne in &new_edges {
            if removed_edges.iter().any(|re| re == ne) {
                return Err(CanonicalizationError::CrossListEdgeConflict);
            }
        }
        Ok(Self {
            new_nodes,
            new_edges,
            removed_edges,
        })
    }

    /// Convenience constructor — empty delta (engine context üretiminde sıklıkla kullanılır).
    pub fn empty() -> Self {
        Self {
            new_nodes: vec![],
            new_edges: vec![],
            removed_edges: vec![],
        }
    }
}

/// Predicate içeriği — her zaman bağlı (identifier yetersiz, içerik mutable olabilir).
///
/// **EffectiveMetricPredicate (reviewer P0-4):** Runtime evaluator üretir.
/// Canonical encoder kendi başına semantic varsayım YAPMAZ — effective modeli encode eder.
/// `None ↔ Some(default)` yalnız evaluator gerçekten aynı yorumluyorsa.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalPredicateContent {
    /// EffectiveMetricPredicate'lerin canonical serialization'ı.
    pub mode: PredicateModeTag,
    pub predicates: Vec<EffectiveMetricPredicate>,
}

/// Effective metric predicate — runtime evaluator'dan türetilmiş.
///
/// Canonical encoder bu modeli encode eder. Semantic normalization (None ↔ default)
/// yalnız evaluator aynı yorumluyorsa geçerli — encoder varsayım yapmaz.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EffectiveMetricPredicate {
    pub axis: PredicateAxisTag,
    pub operator: ComparisonOpTag,
    pub threshold: CanonicalF64,
    pub scope: CanonicalPredicateScope,
    pub required_source: EffectiveSourceRequirement,
    pub effective_weight: CanonicalF64,
    pub effective_tolerance: CanonicalF64,
}

/// **reviewer P1-1 (subgraph invariant):** Validated canonical subgraph scope.
///
/// **Type-level invariant:** sorted + deduplicated node ids. Bu newtype constructor
/// (`try_new`) ve custom Deserialize üzerinden üretilir; geçersiz yapı (duplicate id)
/// runtime'da DEĞİL, giriş noktasında reddedilir. Böylece iki ayrı canonical representation
/// (`[1,1,2]` vs `[1,2]`) oluşamaz.
///
/// **Empty subgraph semantiği:** `CanonicalSubgraphScope(vec![])` geçerli bir canonical
/// scope'tur — explicitly empty target set. Evaluation semantiği runtime
/// `PredicateScope::Subgraph([])` ile aynıdır. Boş subgraph runtime'da üretiliyor
/// (trajectory.rs decomposition fallback), bu yüzden reddedilmez.
///
/// **Artifact schema (reviewer P1):** The v1 artifact schema has not yet been
/// published. PR #69 henüz merge edilmedi; önceki revizyonların ürettiği pending
/// artifact'lar desteklenmez. Bu commit (external-tagged enum + validated newtype)
/// ilk v1 representation'ı finalizes. Eski `{ scope_tag, identity_bytes }` struct
/// wire formatı ile uyumlu DEĞİL — surrounding `CanonicalPredicateScope` enum
/// externally tagged olarak serileştiği için enclosing JSON değişti.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CanonicalSubgraphScope(Vec<u64>);

impl CanonicalSubgraphScope {
    /// **reviewer P1-1:** Validated constructor — sort + duplicate kontrolü.
    /// `[1,1,2]` → `Err(DuplicateScopeNode(1))`.
    pub fn try_new(mut ids: Vec<u64>) -> Result<Self, CanonicalizationError> {
        ids.sort_unstable();
        for pair in ids.windows(2) {
            if pair[0] == pair[1] {
                return Err(CanonicalizationError::DuplicateScopeNode(pair[0]));
            }
        }
        Ok(Self(ids))
    }

    /// Sorted, unique node ids (invariant korunduğu için canonical sıra).
    pub fn as_sorted_ids(&self) -> &[u64] {
        &self.0
    }
}

/// **reviewer P1-1:** Custom Deserialize — `try_new` üzerinden. Diskten `[1,1,2]`
/// gibi duplicate içeren artifact yüklenemez; invariant deserialize sırasında zorlanır.
impl<'de> serde::Deserialize<'de> for CanonicalSubgraphScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let ids = Vec::<u64>::deserialize(deserializer)?;
        Self::try_new(ids).map_err(serde::de::Error::custom)
    }
}

/// **reviewer P1 (raw u8 scope_tag fix):** Predicate scope — typed enum, çıplak u8 DEĞİL.
///
/// Önceki `{ scope_tag: u8, identity_bytes: Vec<u8> }` tasarımı diskten `scope_tag = 255`
/// gibi geçersiz varyantların deserialize edilmesine izin veriyordu. Bu enum geçersiz
/// varyantları compile-time'da reddeder; custom Deserialize enum dışı değerleri reddeder.
///
/// **reviewer P1-1:** `Subgraph` artık validated newtype (`CanonicalSubgraphScope`)
/// taşıyor — duplicate id ve canonical sıra type seviyesinde korunur.
///
/// Canonical encoding stable numeric tag kullanır: `Node → 0`, `Module → 1`, `Subgraph → 2`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CanonicalPredicateScope {
    /// Tek node scope — identity = node id (u64 LE bytes).
    Node(u64),
    /// Module scope — identity = module name string bytes.
    Module(String),
    /// Subgraph scope — validated newtype (sorted + deduplicated node ids).
    Subgraph(CanonicalSubgraphScope),
}

impl CanonicalPredicateScope {
    /// Stable numeric scope tag (canonical encoding için).
    pub fn scope_tag(&self) -> u8 {
        match self {
            Self::Node(_) => 0,
            Self::Module(_) => 1,
            Self::Subgraph(_) => 2,
        }
    }

    /// Identity bytes (canonical encoding için — tag'e ek olarak).
    ///
    /// **reviewer P1-1:** `Subgraph` armı tekrar sort ETMEZ — `CanonicalSubgraphScope`
    /// invariant'ı (sorted + unique) zaten type seviyesinde korunduğu için. Encoder'ın
    /// invalid yapıyı sessizce normalize etmesi invariant ihmalini gizler; bunun yerine
    /// mevcut canonical sıra encode edilir, `debug_assert!` defensive koruma sağlar.
    pub fn identity_bytes(&self) -> Vec<u8> {
        match self {
            Self::Node(id) => id.to_le_bytes().to_vec(),
            Self::Module(name) => name.as_bytes().to_vec(),
            Self::Subgraph(s) => {
                let ids = s.as_sorted_ids();
                debug_assert!(
                    ids.windows(2).all(|w| w[0] < w[1]),
                    "CanonicalSubgraphScope invariant violated: not sorted/unique"
                );
                let mut bytes = Vec::with_capacity(ids.len() * 8);
                for id in ids {
                    bytes.extend_from_slice(&id.to_le_bytes());
                }
                bytes
            }
        }
    }
}

/// **reviewer P1-1b (P0):** Effective source requirement — None/TreeSitter collision fix.
///
/// Önceki `{ source_tag: u8 }` tasarımında `None → 0` ve `Some(TreeSitter) → 0`
/// (TreeSitter=0) aynı byte dizisini üretiyordu. Bu enum ayrımı çakışmayı ortadan
/// kaldırır: `Any` ve `Exact(src)` farklı canonical encoding'e sahiptir.
///
/// Encoding: `Any → [0]`, `Exact(src) → [1, src_tag]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EffectiveSourceRequirement {
    /// Herhangi bir source kabul edilir (required_source = None).
    Any,
    /// Belirli bir source zorunlu (INV-T4 — placeholder ölçümle task kapatma engeli).
    Exact(crate::canonical_tags::CanonicalMetricSourceTag),
}

// ═══════════════════════════════════════════════════════════════════════════════
// CanonicalWitnessPolicy (reviewer P0-1 — witness policy basis'e bağlı)
// ═══════════════════════════════════════════════════════════════════════════════

/// Witness'ın yetkilendirdiği claim'in hangi authorization politikası altında
/// değerlendirildiğini bağlar (reviewer P0-1).
///
/// Aynı proposal `min_approvers=2, quorum=1.5` ve `min_approvers=0, quorum=0.0`
/// politikalarıyla farklı authorization basis üretmelidir.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalWitnessPolicy {
    pub schema_version: u32,
    pub min_approvers: u32,
    pub quorum_threshold: CanonicalF64,
    pub independence_policy: WitnessIndependencePolicyTag,
}

impl CanonicalWitnessPolicy {
    /// Effective requirement — record.witness_requirement ile cross-field doğrulanır.
    pub fn effective_requirement(&self) -> WitnessRequirement {
        WitnessRequirement {
            min_approvers: self.min_approvers as usize,
            quorum_threshold: self.quorum_threshold,
        }
    }
}

/// **reviewer plan-review #1 (P0):** CanonicalWitnessPolicy gerçek `omega`'dan türetilir.
///
/// Engine config YEDEK DEĞİL. Bu impl olmadan, placeholder basis üretirken engine config
/// değerleri artifact'e kaydedilebilir; gerçek witness değerlendirmesi `input.omega` ile
/// yapılırken basis farklı değerler taşır — high-risk witness safety sınırında P0.
///
/// ```text
/// Gerçek değerlendirme: 1 approver / quorum 1.0
/// Artifact basis:       2 approver / quorum 1.5   ← BU İMKANSIZ OLMALI
/// ```
///
/// `independence_policy`: omega independence taşımıyor (henüz) → Strict varsayılan.
/// Gelecekte omega genişletilirse buradan türetilir.
impl TryFrom<&crate::witness::WitnessSet> for CanonicalWitnessPolicy {
    type Error = AuthorizationBasisDigestError;

    fn try_from(omega: &crate::witness::WitnessSet) -> Result<Self, Self::Error> {
        // Non-finite quorum reddet (canonical encoding ile tutarlı).
        if !omega.quorum_threshold.is_finite() {
            return Err(AuthorizationBasisDigestError::NonFiniteRejected);
        }
        Ok(Self {
            schema_version: 1,
            min_approvers: omega.min_approvers as u32,
            quorum_threshold: omega.quorum_threshold,
            independence_policy: WitnessIndependencePolicyTag::default(),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MeasurementInputContext + MeasurementInputDigest (INV-T9 Adım 3 — axis descriptors)
// ═══════════════════════════════════════════════════════════════════════════════

/// Measurement-visible coordinate-system state — axis implementation identity +
/// semantics version + canonical parameters (effective normalized runtime state).
///
/// **reviewer (ontolojik ayrım):** Context axis *tanımlarını* (formül/config/normalizasyon
/// sabitleri) taşır — "bu ölçüm hangi eksen tanımları ve semantiklerle üretildi?"
/// Ölçümün *ürettiği değer + source* `ProvenancedMeasuredResult`'da (basis'te).
///
/// **reviewer (daraltma):** Placeholder `config_tag`, sahte source policy (`metric_source_config`),
/// tekrar eden ölçüm değerleri (`repo_level_*`), evaluation girdileri (`theta_bound`/
/// `abstractness` — `EvaluationContextDigest`'te) kaldırıldı. Yalnız core raw axis
/// descriptor'ları (seçenek B): coupling/cohesion/instability/entropy/witness_depth.
///
/// **v1 schema:** Henüz yayınlanmadı; bu commit ilk v1 representation'ı finalizes.
/// Basis digest taşır (bound), full context taşımaz (readable) — self-description ileride.
pub const MEASUREMENT_INPUT_SCHEMA_VERSION: u32 = 1;
pub const MEASUREMENT_SEMANTICS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MeasurementInputContext {
    schema_version: u32,
    axis_descriptors: Vec<AxisDescriptor>,
    measurement_semantics_version: u32,
}

impl MeasurementInputContext {
    /// Runtime üretimi — güncel version sabitleri ile.
    pub fn try_new(descriptors: Vec<AxisDescriptor>) -> Result<Self, CanonicalizationError> {
        Self::try_from_parts(
            MEASUREMENT_INPUT_SCHEMA_VERSION,
            descriptors,
            MEASUREMENT_SEMANTICS_VERSION,
        )
    }

    /// Deserialize/migration sınırı — version'ları doğrular, normalize ETMEZ.
    /// Diskten `schema_version: 999` gelirse sessizce `1`'e normalize edilmez;
    /// `UnsupportedMeasurementInputSchema` ile reddedilir.
    fn try_from_parts(
        schema_version: u32,
        mut descriptors: Vec<AxisDescriptor>,
        measurement_semantics_version: u32,
    ) -> Result<Self, CanonicalizationError> {
        if schema_version != MEASUREMENT_INPUT_SCHEMA_VERSION {
            return Err(CanonicalizationError::UnsupportedMeasurementInputSchema(
                schema_version,
            ));
        }
        if measurement_semantics_version != MEASUREMENT_SEMANTICS_VERSION {
            return Err(CanonicalizationError::UnsupportedMeasurementSemantics(
                measurement_semantics_version,
            ));
        }
        // **reviewer P1 (core-only invariant):** context yalnız core raw axis descriptor'ları
        // taşır (dokümante invariant). Custom axis descriptor reddedilir.
        for d in &descriptors {
            if !crate::coords::is_core_raw_axis_id(d.axis_id()) {
                return Err(CanonicalizationError::UnsupportedMeasurementAxis(
                    d.axis_id().to_owned(),
                ));
            }
        }
        // Canonical sıralama (axis_id'ye göre) + duplicate reddi.
        descriptors.sort_unstable_by(|a, b| a.axis_id().cmp(b.axis_id()));
        for pair in descriptors.windows(2) {
            if pair[0].axis_id() == pair[1].axis_id() {
                return Err(CanonicalizationError::DuplicateIdentifier(
                    pair[0].axis_id().to_owned(),
                ));
            }
        }
        Ok(Self {
            schema_version,
            axis_descriptors: descriptors,
            measurement_semantics_version,
        })
    }

    /// Defensive validation — version + duplicate + core-only. `MeasurementInputDigest::compute`
    /// başında çağrılır (invariant drift tespiti).
    pub fn validate(&self) -> Result<(), CanonicalizationError> {
        if self.schema_version != MEASUREMENT_INPUT_SCHEMA_VERSION {
            return Err(CanonicalizationError::UnsupportedMeasurementInputSchema(
                self.schema_version,
            ));
        }
        if self.measurement_semantics_version != MEASUREMENT_SEMANTICS_VERSION {
            return Err(CanonicalizationError::UnsupportedMeasurementSemantics(
                self.measurement_semantics_version,
            ));
        }
        // **reviewer P1 (core-only invariant):** her descriptor core raw axis olmalı.
        for d in &self.axis_descriptors {
            if !crate::coords::is_core_raw_axis_id(d.axis_id()) {
                return Err(CanonicalizationError::UnsupportedMeasurementAxis(
                    d.axis_id().to_owned(),
                ));
            }
        }
        for pair in self.axis_descriptors.windows(2) {
            if pair[0].axis_id() >= pair[1].axis_id() {
                return Err(CanonicalizationError::DuplicateIdentifier(
                    pair[1].axis_id().to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn axis_descriptors(&self) -> &[AxisDescriptor] {
        &self.axis_descriptors
    }
    pub fn measurement_semantics_version(&self) -> u32 {
        self.measurement_semantics_version
    }
}

/// Custom `Deserialize` — version-preserving. `MeasurementInputContextWire` derived
/// deserialize ile wire format okunur, sonra `try_from_parts` version'ları doğrular.
impl<'de> serde::Deserialize<'de> for MeasurementInputContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct MeasurementInputContextWire {
            schema_version: u32,
            axis_descriptors: Vec<AxisDescriptor>,
            measurement_semantics_version: u32,
        }
        let wire = MeasurementInputContextWire::deserialize(deserializer)?;
        MeasurementInputContext::try_from_parts(
            wire.schema_version,
            wire.axis_descriptors,
            wire.measurement_semantics_version,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// `CoordinateSystem → MeasurementInputContext` köprüsü. `coords → authorization`
/// döngüsü yok — axis descriptor'lar neutral coords layer'da üretilir, context
/// authorization layer'da inşa edilir.
impl TryFrom<&CoordinateSystem> for MeasurementInputContext {
    type Error = CanonicalizationError;

    fn try_from(coords: &CoordinateSystem) -> Result<Self, Self::Error> {
        let descriptors = coords
            .canonical_raw_axis_descriptors()
            .map_err(|e| CanonicalizationError::AxisContextFailed(e.to_string()))?;
        Self::try_new(descriptors)
    }
}

/// Measurement input digest (BLAKE3, domain-separated).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MeasurementInputDigest([u8; 32]);

impl MeasurementInputDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.measurement-input.v1\0";

    /// **INV-T9 Adım 3:** Full axis descriptor listesi encode edilir (RuleDescriptor
    /// pattern'ı). `validate()` defensive çağrılır, sonra defensive sort + encode.
    pub fn compute(ctx: &MeasurementInputContext) -> Result<Self, AuthorizationBasisDigestError> {
        ctx.validate()
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        encode_u32(&mut hasher, ctx.schema_version(), "mi_schema");
        encode_u32(
            &mut hasher,
            ctx.measurement_semantics_version(),
            "mi_semver",
        );
        // Defensive sort (validate'de canonical sıra zaten garanti, ama encoder
        // kendi sıralamasına güvenmez).
        let mut sorted = ctx.axis_descriptors().to_vec();
        sorted.sort_unstable_by(|a, b| a.axis_id().cmp(b.axis_id()));
        let count = u64::try_from(sorted.len()).map_err(|_| {
            AuthorizationBasisDigestError::LengthOverflow {
                field: "mi_axis_count",
            }
        })?;
        encode_u64(&mut hasher, count, "mi_axis_count");
        for d in &sorted {
            encode_bytes(&mut hasher, d.axis_id().as_bytes())?;
            encode_u32(&mut hasher, d.semantics_version(), "mi_axis_semver");
            encode_bytes(&mut hasher, d.canonical_parameters())?;
        }
        let hash = hasher.finalize();
        Ok(Self(hash.into()))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateEvaluationBasis (reviewer P0-1 — mutation decision girdileri)
// ═══════════════════════════════════════════════════════════════════════════════

/// **reviewer P1-2 (P0):** Mutation decision'ı üreten gerçek PredicateGate girdileri.
///
/// Teyif edilen uyumsuzluklar düzeltildi:
/// - `target_vector`: doğrudan `input.target` (preferred_vector DEĞİL — evaluator input.target kullanır)
/// - `min_improvement_delta`: gerçek `is_improved_loss` girdisi (önceki basis taşımıyordu)
/// - `tolerance` (max_axis_regression yanlış adla) KALDIRILDI — evaluator kullanmıyor
/// - `improvement_policy`: mevcut sabit 0.85/0.15 threshold'ları explicit taşınır
///
/// Bu basis olmadan aynı claim + aynı predicate farklı task policy altında farklı mutation
/// decision üretebilir ama authorization basis bunu açıklayamaz.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PredicateEvaluationBasis {
    /// Gerçek evaluator target'ı — `input.target` (preferred_vector DEĞİL).
    pub target_vector: CanonicalRawPosition,
    pub loss_before: CanonicalF64,
    pub loss_after: CanonicalF64,
    pub failure_policy: PredicateFailurePolicyTag,
    /// Gerçek `is_improved_loss` girdisi: `loss_after < loss_before - min_improvement_delta`.
    pub min_improvement_delta: CanonicalF64,
    pub allow_progress_checkpoint: bool,
    /// Explicit improvement thresholds (mevcut sabit 0.85/0.15 semantiği).
    pub improvement_policy: EffectiveImprovementPolicy,
}

/// **reviewer P0-1:** Effective improvement policy — `trajectory` layer'ında tek source
/// of truth. `PredicateGate::evaluate` onu üretir, `PredicateGateOutput` ile döndürür,
/// engine authorization basis'e taşır (basis builder yeniden üretmez).
///
/// Detaylı dokümantasyon ve `current_semantics()` impl'i: [`crate::trajectory::EffectiveImprovementPolicy`].
pub use crate::trajectory::{EffectiveImprovementPolicy, IMPROVEMENT_SEMANTICS_VERSION};

/// Canonical raw position — 5-axis, NaN reject, -0.0 normalize.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalRawPosition {
    pub x: CanonicalF64,
    pub y: CanonicalF64,
    pub z: CanonicalF64,
    pub w: CanonicalF64,
    pub v: CanonicalF64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceViewId + SpaceViewRevision (reviewer P0-2 — lifecycle tam)
// ═══════════════════════════════════════════════════════════════════════════════

/// Space view revision — measurement-visible space content identity.
///
/// **reviewer P0-2:** Engine ayrı lane state'leri tutmuyorsa sahte lane-qualified
/// revision ÜRETİLMEZ. `intended_apply_target` basis'te zaten var. Base view tek
/// engine space ise revision da yalnız o view'ı tanımlar.
///
/// P1 resume'da staleness kontrolü: `current == base` → devam; `!=` → remeasure.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpaceViewRevision {
    pub view_id: SpaceViewId,
    pub sequence: u64,
    pub content_digest: SpaceDigest,
}

/// Space view identity — Persisted (cross-process) veya Ephemeral (process-local).
///
/// **Durability enforcement (reviewer P0-2):** Ephemeral + FilesystemStore + durable
/// suspension = fail-closed. Production CLI yalnız Persisted + Filesystem kabul eder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SpaceViewId {
    /// Cross-process — `<repo>/.osp/space-identity`'den yüklenir (repo path'inden DEĞİL).
    /// Repo taşınması kimliği değiştirmez; clone/fork bilinçli olarak aynı identity taşıyabilir.
    Persisted(PersistedSpaceViewId),
    /// Process-local — in-memory test. Cross-process resumable olarak sunulmaz.
    Ephemeral(u64),
}

/// Cryptographically random, fixed-size persisted identity (16 byte).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PersistedSpaceViewId([u8; 16]);

impl PersistedSpaceViewId {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
    /// **reviewer P0-3 + plan-review:** Cryptographically random identity üret — OS CSPRNG.
    ///
    /// `getrandom::fill` işletim sisteminin tercih ettiği rastgele kaynağını kullanır.
    /// **Fallback YOK** — timestamp/PID/address tabanlı yedekleme yapılmaz. Entropy
    /// edinilemezse typed error döner (fail-closed). Önceki BLAKE3+timestamp+pid yaklaşımı
    /// öngörülebilirdi ve aynı process içinde aynı timestamp çözünürlüğünde collision
    /// üretebiliyordu.
    ///
    /// Deterministic test için `generate_with(&dyn EntropySource)` kullanılır.
    pub fn generate() -> Result<Self, SpaceIdentityError> {
        Self::generate_with(&OsEntropy)
    }

    /// Injectable entropy source ile identity üret — deterministic test için.
    pub(crate) fn generate_with(src: &dyn EntropySource) -> Result<Self, SpaceIdentityError> {
        let mut bytes = [0u8; 16];
        src.fill(&mut bytes)?;
        Ok(Self(bytes))
    }
}

/// Operating-system entropy source — production. `getrandom::fill` wrapper.
pub(crate) struct OsEntropy;

/// Injectable entropy abstraction — deterministic test için (`FailingEntropySource`).
pub(crate) trait EntropySource {
    fn fill(&self, dest: &mut [u8]) -> Result<(), SpaceIdentityError>;
}

impl EntropySource for OsEntropy {
    fn fill(&self, dest: &mut [u8]) -> Result<(), SpaceIdentityError> {
        getrandom::fill(dest).map_err(|e| SpaceIdentityError::EntropyUnavailable {
            message: e.to_string(),
        })
    }
}

/// Space identity üretim/yükleme hataları.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SpaceIdentityError {
    /// OS entropy kaynağı kullanılamıyor. Fail-closed — fallback YOK.
    #[error("operating-system entropy unavailable: {message}")]
    EntropyUnavailable { message: String },
    /// Identity dosyası bozuk/geçersiz. Otomatik yeniden üretim YOK (fail-closed).
    #[error("space identity file is invalid: {0}")]
    InvalidFile(String),
    /// Identity dosyası I/O hatası.
    #[error("space identity file I/O failed: {0}")]
    IoFailed(String),
}

/// Space content digest (BLAKE3, 32 byte) — canonical binary encoding over nodes + edges.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpaceDigest([u8; 32]);

impl SpaceDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.space-content.v1\0";

    /// **reviewer P0-3:** Space içeriğinin gerçek canonical digest'ı.
    ///
    /// Node'lar id'ye göre sıralanır ve canonical encode edilir (id, kind, mass, cohesion,
    /// classification, role). **Position DAHİL DEĞİL** — engine-derived, author-controlled
    /// değil (authorization.rs:55-73 inclusion table). Edge'ler canonical sıralanır ve
    /// encode edilir (from, to, kind, is_type_only).
    ///
    /// Önceki placeholder yalnız `t_c` üzerinden hash üretiyordu — iki farklı space
    /// aynı `t_c`'de aynı digest üretiyordu.
    pub fn compute(space: &crate::space::Space) -> Result<Self, AuthorizationBasisDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // Node'ları id'ye göre sırala → canonical encode.
        let mut nodes: Vec<&crate::space::Node> = space.nodes.values().collect();
        nodes.sort_unstable_by_key(|n| n.id);
        encode_u64(&mut hasher, nodes.len() as u64, "space_node_count");
        for node in &nodes {
            let canonical = canonicalize_node(node)?;
            encode_canonical_node(&mut hasher, &canonical)?;
        }

        // Edge'leri canonical sırala → encode.
        let canonical_edges: Vec<CanonicalEdge> = space
            .edges
            .iter()
            .map(|e| {
                Ok(CanonicalEdge {
                    from: e.from,
                    to: e.to,
                    kind: CanonicalEdgeKind::try_from(&e.kind).map_err(
                        |err: CanonicalizationError| {
                            AuthorizationBasisDigestError::EncodingFailed(err.to_string())
                        },
                    )?,
                    is_type_only: e.is_type_only,
                })
            })
            .collect::<Result<Vec<_>, AuthorizationBasisDigestError>>()?;
        encode_canonical_edge_vec(&mut hasher, &canonical_edges)?;

        let hash = hasher.finalize();
        Ok(Self(hash.into()))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Domain `Node` → `CanonicalNode` dönüşümü (NodeKind → CanonicalNodeKind via TryFrom).
/// Position hariç — engine-derived. `pub(crate)` — engine context üretimi kullanır.
pub(crate) fn canonicalize_node(
    node: &crate::space::Node,
) -> Result<CanonicalNode, AuthorizationBasisDigestError> {
    Ok(CanonicalNode {
        id: node.id,
        kind: CanonicalNodeKind::try_from(&node.kind)
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?,
        mass: node.mass,
        cohesion: node.cohesion,
        classification: CanonicalNodeClassification::try_from(&node.classification)
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?,
        role: CanonicalNodeRole::try_from(&node.role)
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// EvaluationContextDigest — gate policy context
// ═══════════════════════════════════════════════════════════════════════════════

/// Gate policy context digest — vision config + rule-set + semantics versions.
///
/// Vision veya rule-set değişirse eski `PassedAll` sonucu artık geçerli olmayabilir.
/// Bu digest authorization basis'e bağlı olarak stale measurement tespitini sağlar.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EvaluationContextDigest([u8; 32]);

/// **plan-review düzeltme #4:** Rule descriptor — yalnız `rule_id` DEĞİL.
///
/// Aynı rule ID altında uygulama semantiği, parametreler veya threshold değişebilir.
/// Salt `rule_id` bağlamak `NoSelfImport v1` ile `v2`'yi aynı evaluation context
/// olarak gösterir — staleness kontrolünü bozar.
///
/// `semantics_version`: rule implementasyonu değiştiğinde artırılır. Mevcut 3 rule
/// parametresiz → default impl `semantics_version: 1, canonical_parameters: vec![]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuleDescriptor {
    pub rule_id: String,
    pub semantics_version: u32,
    pub canonical_parameters: Vec<u8>,
}

impl EvaluationContextDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.evaluation-context.v1\0";

    /// **reviewer P0-3:** Gerçek evaluation context digest.
    ///
    /// EngineConfig'in ölçüm-atan tüm alanları + rule descriptor'ları (id + semantics
    /// version + parameters) + vision target vector encode edilir. Önceki placeholder
    /// yalnız `theta_bound + rule count` kullanıyordu.
    ///
    /// **Rule versioning:** Rule implementasyonu değişip `semantics_version` artırılırsa
    /// context digest değişir → stale measurement tespiti çalışır.
    pub fn compute(
        config: &crate::engine::EngineConfig,
        rules: &[RuleDescriptor],
        vision_vector: &crate::coords::RawPosition,
    ) -> Result<Self, AuthorizationBasisDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // EngineConfig — ölçüm-atan tüm alanlar.
        encode_u32(&mut hasher, config.min_approvers as u32, "ec_min_approvers");
        encode_f64(&mut hasher, config.quorum_threshold, "ec_quorum")?;
        encode_f64(&mut hasher, config.theta_bound, "ec_theta_bound")?;
        encode_u64(
            &mut hasher,
            config.milestone_interval,
            "ec_milestone_interval",
        );
        encode_f64(&mut hasher, config.abstractness, "ec_abstractness")?;
        encode_f64(&mut hasher, config.merge_ratio_observable, "ec_merge_ratio")?;

        // Role overrides — sorted by key (tutarlı encoding için).
        let mut role_keys: Vec<&String> = config.role_overrides.keys().collect();
        role_keys.sort_unstable();
        encode_u64(
            &mut hasher,
            role_keys.len() as u64,
            "ec_role_override_count",
        );
        for key in role_keys {
            let ov = &config.role_overrides[key];
            encode_bytes(&mut hasher, key.as_bytes())?;
            encode_u8(&mut hasher, ov.x.is_some() as u8, "ec_role_x_present");
            if let Some(x) = ov.x {
                encode_f64(&mut hasher, x, "ec_role_x")?;
            }
            encode_u8(&mut hasher, ov.y.is_some() as u8, "ec_role_y_present");
            if let Some(y) = ov.y {
                encode_f64(&mut hasher, y, "ec_role_y")?;
            }
            encode_u8(&mut hasher, ov.z.is_some() as u8, "ec_role_z_present");
            if let Some(z) = ov.z {
                encode_f64(&mut hasher, z, "ec_role_z")?;
            }
        }

        // Rules — id + semantics_version + canonical_parameters.
        let mut sorted_rules = rules.to_vec();
        sorted_rules.sort_unstable_by(|a, b| a.rule_id.cmp(&b.rule_id));
        encode_u64(&mut hasher, sorted_rules.len() as u64, "ec_rule_count");
        for rule in &sorted_rules {
            encode_bytes(&mut hasher, rule.rule_id.as_bytes())?;
            encode_u32(&mut hasher, rule.semantics_version, "ec_rule_semver");
            encode_bytes(&mut hasher, &rule.canonical_parameters)?;
        }

        // Vision target vector (5-axis).
        encode_f64(&mut hasher, vision_vector.x, "ec_vision_x")?;
        encode_f64(&mut hasher, vision_vector.y, "ec_vision_y")?;
        encode_f64(&mut hasher, vision_vector.z, "ec_vision_z")?;
        encode_f64(&mut hasher, vision_vector.w, "ec_vision_w")?;
        encode_f64(&mut hasher, vision_vector.v, "ec_vision_v")?;

        let hash = hasher.finalize();
        Ok(Self(hash.into()))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AuthorizationBasis + Digest (BLAKE3, domain-separated, canonical)
// ═══════════════════════════════════════════════════════════════════════════════

/// Witness'ın yetkilendirdiği claim'in tam kanonik temsili.
///
/// Digest hesaplanırken TÜM alanlar dahil edilir — structural delta full canonical
/// (digest değil), predicate içerik her zaman bağlı (id yetersiz), witness policy
/// bağlı (P0-1), measurement input bağlı (P0-3), predicate evaluation girdileri bağlı
/// (P0-1). `created_at` dahil DEĞİL — aynı basis farklı zamanda aynı digest vermeli.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthorizationBasis {
    pub schema_version: u32,
    pub task_id: crate::trajectory::TaskId,
    pub claim_identity: ClaimIdentity,
    pub claim_author: ClaimAuthor,
    pub structural_delta: CanonicalStructuralDelta,
    pub predicate_content: CanonicalPredicateContent,
    pub predicate_evaluation: PredicateEvaluationBasis,
    pub measured_result: ProvenancedMeasuredResult,
    pub deterministic_gate_result: GateDecision,
    pub predicate_completion: PredicateCompletion,
    pub mutation_decision: MutationDecision,
    pub intended_apply_target: ApplyTarget,
    pub witness_policy: CanonicalWitnessPolicy,
    pub measurement_input_digest: MeasurementInputDigest,
    pub evaluation_context_digest: EvaluationContextDigest,
    pub base_space_view_revision: SpaceViewRevision,
}

/// **reviewer P0-1 (bloklayıcı):** Tek eksen ölçümü — value + source.
///
/// INV-T4 kararının evidence basis'i için her eksenin provenance'ı ayrı bağlanır.
/// Önceki tasarım yalnız coupling source'unu kaydediyordu; iki ölçüm aynı coupling
/// source ama farklı entropy source ile aynı basis'i üretebiliyordu.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalAxisMeasurement {
    pub value: CanonicalF64,
    pub source: crate::canonical_tags::CanonicalMetricSourceTag,
}

/// Measured result — 5 eksenin her biri value + source (INV-T4 per-axis provenance).
///
/// INV-T4 source-requirement kararının evidence basis'i tamamlanır: bir predicate
/// entropy eksenini hedefliyorsa ve required_source = Scip ise, measured.entropy.source
/// basis'e bağlıdır — placeholder source ile task kapatma engeli reconstructible.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProvenancedMeasuredResult {
    pub coupling: CanonicalAxisMeasurement,
    pub cohesion: CanonicalAxisMeasurement,
    pub instability: CanonicalAxisMeasurement,
    pub entropy: CanonicalAxisMeasurement,
    pub witness_depth: CanonicalAxisMeasurement,
}

/// BLAKE3 tabanlı authorization basis digest.
///
/// Domain separation: `"osp.authorization-basis.v1\0" || canonical_encoding`.
/// Float canonicalization: NaN reject, -0.0 → 0.0, little-endian, sorted collections,
/// `f64::to_bits()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AuthorizationBasisDigest([u8; 32]);

impl AuthorizationBasisDigest {
    /// Domain separation prefix.
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.authorization-basis.v1\0";

    /// Authorization basis'ten BLAKE3 digest hesapla.
    ///
    /// **Canonical binary encoding:** her alan deterministic byte sequence'e encode
    /// edilir. JSON kullanılmaz. Float canonicalization: NaN reject, -0.0 → 0.0
    /// normalize, `f64::to_bits()` little-endian. Collections sorted. Stable numeric
    /// tags (format!("{:?}") DEĞİL). Domain separation prefix.
    ///
    /// **reviewer P0-1/P0-3:** witness_policy, measurement_input_digest,
    /// predicate_evaluation basis'e bağlı. claim_identity.task_id encode edilir.
    pub fn compute(basis: &AuthorizationBasis) -> Result<Self, AuthorizationBasisDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // Identity.
        encode_u32(&mut hasher, basis.schema_version, "schema_version");
        encode_u64(&mut hasher, basis.task_id, "task_id");
        encode_u64(&mut hasher, basis.claim_identity.claim_id, "claim_id");
        encode_u64(&mut hasher, basis.claim_identity.task_id, "claim_task_id"); // P0-2 claim_identity.task_id
        encode_u64(&mut hasher, basis.claim_author, "claim_author");

        // Structural delta — CanonicalNode (full content) + CanonicalEdge (sorted).
        let mut sorted_nodes = basis.structural_delta.new_nodes.clone();
        sorted_nodes.sort_unstable_by_key(|n| n.id);
        encode_u64(&mut hasher, sorted_nodes.len() as u64, "new_node_count");
        for node in &sorted_nodes {
            encode_canonical_node(&mut hasher, node)?;
        }
        encode_canonical_edge_vec(&mut hasher, &basis.structural_delta.new_edges)?;
        encode_canonical_edge_vec(&mut hasher, &basis.structural_delta.removed_edges)?;

        // Predicate content — EffectiveMetricPredicate (evaluator-derived, sorted).
        // **reviewer P0-2b:** predicate'ler canonical byte dizisi olarak sıralanır ve
        // hash'e yazılır. Sorting ve hashing aynı encoder'ı kullanır — `-0.0` normalize.
        encode_tag(&mut hasher, basis.predicate_content.mode, "predicate_mode");
        encode_effective_predicate_set(&mut hasher, &basis.predicate_content.predicates)?;

        // Predicate evaluation basis (P0-1 — mutation decision girdileri).
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.target_vector.x,
            "target_x",
        )?;
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.target_vector.y,
            "target_y",
        )?;
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.target_vector.z,
            "target_z",
        )?;
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.target_vector.w,
            "target_w",
        )?;
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.target_vector.v,
            "target_v",
        )?;
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.loss_before,
            "loss_before",
        )?;
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.loss_after,
            "loss_after",
        )?;
        encode_tag(
            &mut hasher,
            basis.predicate_evaluation.failure_policy,
            "failure_policy",
        );
        encode_f64(
            &mut hasher,
            basis.predicate_evaluation.min_improvement_delta,
            "min_improvement_delta",
        )?;
        encode_u8(
            &mut hasher,
            basis.predicate_evaluation.allow_progress_checkpoint as u8,
            "allow_progress",
        );
        // Effective improvement policy — explicit thresholds (mevcut sabit 0.85/0.15).
        let ip = &basis.predicate_evaluation.improvement_policy;
        encode_f64(&mut hasher, ip.max_coupling, "max_coupling")?;
        encode_f64(&mut hasher, ip.max_instability, "max_instability")?;
        encode_f64(&mut hasher, ip.min_cohesion, "min_cohesion")?;
        encode_u32(&mut hasher, ip.semantics_version, "improvement_semver");

        // Measured result — 5 eksen value + source (INV-T4 per-axis provenance).
        encode_axis_measurement(&mut hasher, &basis.measured_result.coupling, "coupling")?;
        encode_axis_measurement(&mut hasher, &basis.measured_result.cohesion, "cohesion")?;
        encode_axis_measurement(
            &mut hasher,
            &basis.measured_result.instability,
            "instability",
        )?;
        encode_axis_measurement(&mut hasher, &basis.measured_result.entropy, "entropy")?;
        encode_axis_measurement(
            &mut hasher,
            &basis.measured_result.witness_depth,
            "witness_depth",
        )?;

        // Outcome tags.
        encode_u8(
            &mut hasher,
            gate_decision_tag(basis.deterministic_gate_result),
            "gate",
        );
        encode_u8(
            &mut hasher,
            predicate_completion_tag(basis.predicate_completion),
            "predicate_completion",
        );
        encode_u8(
            &mut hasher,
            mutation_decision_tag(basis.mutation_decision),
            "mutation_decision",
        );
        encode_u8(
            &mut hasher,
            apply_target_tag(&basis.intended_apply_target),
            "apply_target",
        );

        // Witness policy (P0-1).
        encode_u32(
            &mut hasher,
            basis.witness_policy.schema_version,
            "wp_schema",
        );
        encode_u32(
            &mut hasher,
            basis.witness_policy.min_approvers,
            "wp_min_approvers",
        );
        encode_f64(
            &mut hasher,
            basis.witness_policy.quorum_threshold,
            "wp_quorum",
        )?;
        encode_tag(
            &mut hasher,
            basis.witness_policy.independence_policy,
            "wp_independence",
        );

        // Digests — raw bytes.
        hasher.update(basis.measurement_input_digest.as_bytes());
        hasher.update(basis.evaluation_context_digest.as_bytes());
        hasher.update(basis.base_space_view_revision.content_digest.as_bytes());
        encode_space_view_id(&mut hasher, &basis.base_space_view_revision.view_id)?;
        encode_u64(
            &mut hasher,
            basis.base_space_view_revision.sequence,
            "space_revision_sequence",
        );

        let hash = hasher.finalize();
        Ok(Self(hash.into()))
    }

    /// Raw 32-byte digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hex string (CLI/JSON çıktısı için).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Hex string'den parse.
    pub fn from_hex(hex_str: &str) -> Result<Self, AuthorizationBasisDigestError> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| AuthorizationBasisDigestError::HexDecodeFailed(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(AuthorizationBasisDigestError::InvalidLength(bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Canonical binary encoding helpers (review P1-3)
// ═══════════════════════════════════════════════════════════════════════════════

fn encode_u64(hasher: &mut blake3::Hasher, val: u64, _field: &str) {
    hasher.update(&val.to_le_bytes());
}

fn encode_u32(hasher: &mut blake3::Hasher, val: u32, _field: &str) {
    hasher.update(&val.to_le_bytes());
}

fn encode_u8(hasher: &mut blake3::Hasher, val: u8, _field: &str) {
    hasher.update(&[val]);
}

/// Canonical numeric tag trait — newtype'ların `as_u8()` üzerinden encode edilmesi.
///
/// `encode_u8` ve `push_u8` generic hale gelir; newtype'lar otomatik desteklenir.
/// Ham `u8` de desteklenir (scope_tag gibi plain numeric alanlar için).
pub(crate) trait CanonicalTag {
    fn tag_u8(&self) -> u8;
}

impl CanonicalTag for u8 {
    fn tag_u8(&self) -> u8 {
        *self
    }
}

/// Tüm `canonical_tags` newtype'ları `CanonicalTag` uygular — macro yardımcı.
macro_rules! impl_canonical_tag_for_newtype {
    ($($name:ident),* $(,)?) => {
        $(
            impl $crate::authorization::CanonicalTag for $crate::canonical_tags::$name {
                fn tag_u8(&self) -> u8 {
                    self.as_u8()
                }
            }
        )*
    };
}

impl_canonical_tag_for_newtype!(
    CanonicalNodeKind,
    CanonicalEdgeKind,
    CanonicalNodeClassification,
    CanonicalNodeRole,
    PredicateAxisTag,
    ComparisonOpTag,
    CanonicalMetricSourceTag,
    PredicateModeTag,
    PredicateFailurePolicyTag,
);

impl CanonicalTag for crate::canonical_tags::WitnessIndependencePolicyTag {
    fn tag_u8(&self) -> u8 {
        self.as_u8()
    }
}

fn encode_tag<T: CanonicalTag>(hasher: &mut blake3::Hasher, val: T, field: &str) {
    encode_u8(hasher, val.tag_u8(), field);
}

fn encode_bytes(
    hasher: &mut blake3::Hasher,
    bytes: &[u8],
) -> Result<(), AuthorizationBasisDigestError> {
    encode_u64(hasher, bytes.len() as u64, "len");
    hasher.update(bytes);
    Ok(())
}

/// f64 canonical encoding — non-finite reject (NaN + ±Infinity), -0.0 → 0.0, little-endian to_bits.
///
/// **reviewer P0-2a:** yalnız NaN değil, ±Infinity de reddedilir. Plan NaN+infinity
/// rejection öngörüyordu; `is_nan()` kontrolü infinity'yi geçiriyordu.
fn encode_f64(
    hasher: &mut blake3::Hasher,
    val: f64,
    _field: &str,
) -> Result<(), AuthorizationBasisDigestError> {
    if !val.is_finite() {
        return Err(AuthorizationBasisDigestError::NonFiniteRejected);
    }
    // -0.0 → 0.0 normalize (to_bits farklı: -0.0 = 0x8000000000000000, 0.0 = 0x0).
    let normalized = if val == 0.0 { 0.0f64 } else { val };
    hasher.update(&normalized.to_bits().to_le_bytes());
    Ok(())
}

/// Option\<f64\> canonical encoding — **reviewer P0-1 (encoding collision fix).**
///
/// Önceki yaklaşım `None → encode_u8(0)` ve `Some(v) → encode_f64(v)` kullanıyordu;
/// bu `None` (1 byte) ile `Some(0.0)` (8 byte) dizilerini farklı uzunluklarda üretiyordu
/// ama `None` + `Some(0.0)` kombinasyonları dokuz sıfır byte'a çakışabiliyordu.
///
/// Presence tag: `None → [0]`, `Some(v) → [1] || canonical_f64(v)`. Tag olmadan aynı
/// byte dizisini üreten context çiftleri artık imkânsız.
fn encode_optional_f64(
    hasher: &mut blake3::Hasher,
    value: Option<f64>,
    field: &str,
) -> Result<(), AuthorizationBasisDigestError> {
    match value {
        None => {
            encode_u8(hasher, 0, field);
            Ok(())
        }
        Some(v) => {
            encode_u8(hasher, 1, field);
            encode_f64(hasher, v, field)?;
            Ok(())
        }
    }
}

/// **reviewer P0-1:** Per-axis measurement encoder — value + source tag.
fn encode_axis_measurement(
    hasher: &mut blake3::Hasher,
    m: &CanonicalAxisMeasurement,
    field: &str,
) -> Result<(), AuthorizationBasisDigestError> {
    encode_f64(hasher, m.value, field)?;
    encode_tag(hasher, m.source, field);
    Ok(())
}

fn encode_canonical_node(
    hasher: &mut blake3::Hasher,
    node: &CanonicalNode,
) -> Result<(), AuthorizationBasisDigestError> {
    encode_u64(hasher, node.id, "node_id");
    encode_tag(hasher, node.kind, "node_kind");
    encode_f64(hasher, node.mass, "node_mass")?;
    encode_optional_f64(hasher, node.cohesion, "node_cohesion")?;
    encode_tag(hasher, node.classification, "node_classification");
    encode_tag(hasher, node.role, "node_role");
    Ok(())
}

fn encode_canonical_edge_vec(
    hasher: &mut blake3::Hasher,
    edges: &[CanonicalEdge],
) -> Result<(), AuthorizationBasisDigestError> {
    let mut sorted = edges.to_vec();
    sorted.sort_unstable();
    encode_u64(hasher, sorted.len() as u64, "edge_count");
    for edge in &sorted {
        encode_u64(hasher, edge.from, "edge_from");
        encode_u64(hasher, edge.to, "edge_to");
        encode_tag(hasher, edge.kind, "edge_kind");
        encode_u8(hasher, edge.is_type_only as u8, "edge_is_type_only");
    }
    Ok(())
}

/// EffectiveMetricPredicate → canonical byte dizisi. **reviewer P0-2b:** sort ve hash
/// aynı canonical encoder'ı kullanır. Önceki comparator ham `to_bits()` kullanıyordu;
/// bu `-0.0` normalize etmediği için encoder ile çelişiyordu — semantik aynı predicate
/// seti farklı sıraya ve farklı digest'e gidebiliyordu.
fn encode_effective_predicate_to_vec(
    pred: &EffectiveMetricPredicate,
) -> Result<Vec<u8>, AuthorizationBasisDigestError> {
    let mut buf: Vec<u8> = Vec::with_capacity(48);
    push_tag(&mut buf, pred.axis);
    push_tag(&mut buf, pred.operator);
    push_f64(&mut buf, pred.threshold)?;
    push_u8(&mut buf, pred.scope.scope_tag());
    push_bytes(&mut buf, &pred.scope.identity_bytes());
    push_effective_source(&mut buf, &pred.required_source);
    push_f64(&mut buf, pred.effective_weight)?;
    push_f64(&mut buf, pred.effective_tolerance)?;
    Ok(buf)
}

/// **reviewer P1-1b (P0):** EffectiveSourceRequirement canonical encoding.
/// `Any → [0]`, `Exact(src) → [1, src_tag]` — None/TreeSitter collision fix.
fn push_effective_source(buf: &mut Vec<u8>, req: &EffectiveSourceRequirement) {
    match req {
        EffectiveSourceRequirement::Any => push_u8(buf, 0),
        EffectiveSourceRequirement::Exact(src) => {
            push_u8(buf, 1);
            push_tag(buf, *src);
        }
    }
}

/// Predicate set'i canonical byte dizilerine çevirip sıralayıp hash'e length-prefix
/// ile yazar. Salt konkatenasyon YOK — her predicate `encode_bytes` ile ayrılmış.
fn encode_effective_predicate_set(
    hasher: &mut blake3::Hasher,
    predicates: &[EffectiveMetricPredicate],
) -> Result<(), AuthorizationBasisDigestError> {
    let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(predicates.len());
    for pred in predicates {
        encoded.push(encode_effective_predicate_to_vec(pred)?);
    }
    encoded.sort_unstable();
    encode_u64(hasher, encoded.len() as u64, "predicate_count");
    for buf in &encoded {
        encode_bytes(hasher, buf)?;
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Vec\<u8\> canonical encoding helpers (predicate sort için)
// ──────────────────────────────────────────────────────────────────────────────

fn push_u8(buf: &mut Vec<u8>, val: u8) {
    buf.push(val);
}

fn push_tag<T: CanonicalTag>(buf: &mut Vec<u8>, val: T) {
    push_u8(buf, val.tag_u8());
}

fn push_u64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_le_bytes());
}

fn push_f64(buf: &mut Vec<u8>, val: f64) -> Result<(), AuthorizationBasisDigestError> {
    if !val.is_finite() {
        return Err(AuthorizationBasisDigestError::NonFiniteRejected);
    }
    let normalized = if val == 0.0 { 0.0f64 } else { val };
    buf.extend_from_slice(&normalized.to_bits().to_le_bytes());
    Ok(())
}

fn push_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    push_u64(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn encode_space_view_id(
    hasher: &mut blake3::Hasher,
    view_id: &SpaceViewId,
) -> Result<(), AuthorizationBasisDigestError> {
    match view_id {
        SpaceViewId::Persisted(id) => {
            encode_u8(hasher, 1, "view_id_persisted");
            hasher.update(id.as_bytes());
        }
        SpaceViewId::Ephemeral(id) => {
            encode_u8(hasher, 2, "view_id_ephemeral");
            encode_u64(hasher, *id, "ephemeral_id");
        }
    }
    Ok(())
}

fn gate_decision_tag(gd: crate::trajectory::GateDecision) -> u8 {
    use crate::trajectory::GateDecision::*;
    match gd {
        Unknown => 0,
        PassedAll => 1,
        RejectedBySyntax => 2,
        RejectedByVision => 3,
        RejectedByRule => 4,
        RejectedByTaskBinding => 5,
        BlockedByManeuverLimit => 6,
    }
}

fn predicate_completion_tag(pc: crate::trajectory::PredicateCompletion) -> u8 {
    use crate::trajectory::PredicateCompletion::*;
    match pc {
        NotCompleted => 0,
        Completed => 1,
    }
}

fn mutation_decision_tag(md: crate::trajectory::MutationDecision) -> u8 {
    use crate::trajectory::MutationDecision::*;
    match md {
        Reject => 0,
        AcceptAsProgress => 1,
        AcceptAsCompleted => 2,
        RequireOperatorApproval => 3,
    }
}

fn apply_target_tag(at: &crate::trajectory::ApplyTarget) -> u8 {
    use crate::trajectory::ApplyTarget::*;
    match at {
        NotApplied => 0,
        Lane(lane) => match lane {
            crate::trajectory::CommitLane::Mainline => 1,
            crate::trajectory::CommitLane::TrajectoryCheckpoint => 2,
            crate::trajectory::CommitLane::Sandbox => 3,
        },
    }
}

/// Authorization basis digest hesaplama hataları.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthorizationBasisDigestError {
    #[error("canonical encoding failed: {0}")]
    EncodingFailed(String),
    #[error("non-finite float (NaN or ±Infinity) detected in authorization basis — not allowed (canonical encoding)")]
    NonFiniteRejected,
    #[error("hex decode failed: {0}")]
    HexDecodeFailed(String),
    #[error("invalid digest length: expected 32 bytes, got {0}")]
    InvalidLength(usize),
    /// **INV-T9 Adım 3 (P1-a):** canonical length overflow (checked u64 conversion).
    #[error("canonical length overflow in {field}")]
    LengthOverflow { field: &'static str },
}

/// Canonical structural delta doğrulama hatası (A5 — duplicate/non-finite field).
///
/// `CanonicalStructuralDelta::try_new` bu hatayı döndürür. Digest katmanı savunmacıdır:
/// syntax gate normal akışta duplicate'leri yakalasa da canonical artifact deserialize
/// edilerek doğrudan oluşturulabilir.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CanonicalizationError {
    #[error("duplicate node id {0} in new_nodes")]
    DuplicateNodeId(u64),
    #[error("duplicate edge in structural delta (same list)")]
    DuplicateEdge,
    #[error("ambiguous delta: edge present in both new_edges and removed_edges")]
    CrossListEdgeConflict,
    #[error("non-finite node field (mass or cohesion) for node id {0}")]
    NonFiniteNodeField(u64),
    /// **reviewer P1-1:** deserialize sırasında geçersiz canonical tag (örn 255).
    /// Diskten yüklenen artifact valide edilmeden kullanılamaz.
    #[error("invalid canonical tag for {type_name}: {tag}")]
    InvalidCanonicalTag { type_name: &'static str, tag: u8 },
    /// **reviewer P0-2:** duplicate axis/rule identifier.
    #[error("duplicate identifier {0}")]
    DuplicateIdentifier(String),
    /// **reviewer P1-1:** duplicate node id in subgraph scope (canonical invariant).
    /// `[1,1,2]` iki ayrı canonical representation doğurur; reddedilir.
    #[error("duplicate scope node {0} in subgraph predicate scope")]
    DuplicateScopeNode(u64),
    /// **INV-T9 Adım 3:** unsupported measurement input schema version (deserialize/migration).
    #[error("unsupported measurement input schema version {0}")]
    UnsupportedMeasurementInputSchema(u32),
    /// **INV-T9 Adım 3:** unsupported measurement semantics version (deserialize/migration).
    #[error("unsupported measurement semantics version {0}")]
    UnsupportedMeasurementSemantics(u32),
    /// **INV-T9 Adım 3 (P1-a):** axis context / canonical length overflow.
    #[error("axis context failed: {0}")]
    AxisContextFailed(String),
    /// **INV-T9 Adım 3 (reviewer P1):** context yalnız core raw axis descriptor'ları
    /// taşır (dokümante invariant). Dışarıdan custom axis descriptor reddedilir.
    #[error("unsupported measurement axis (not core raw): {0}")]
    UnsupportedMeasurementAxis(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// hex encoding (inline — dependency eklemeden)
// ═══════════════════════════════════════════════════════════════════════════════

mod hex {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";

    pub fn encode(bytes: [u8; 32]) -> String {
        let mut s = String::with_capacity(64);
        for b in &bytes {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0xf) as usize] as char);
        }
        s
    }

    pub fn decode(hex: &str) -> Result<Vec<u8>, String> {
        if hex.len() % 2 != 0 {
            return Err("odd length hex string".to_string());
        }
        let mut out = Vec::with_capacity(hex.len() / 2);
        let bytes = hex.as_bytes();
        for chunk in bytes.chunks(2) {
            let hi = hex_nibble(chunk[0])?;
            let lo = hex_nibble(chunk[1])?;
            out.push((hi << 4) | lo);
        }
        Ok(out)
    }

    fn hex_nibble(c: u8) -> Result<u8, String> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => Err(format!("invalid hex char: {}", c as char)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Clock — deterministic time abstraction
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic clock abstraction.
///
/// Core doğrudan `SystemTime::now()` çağırmaz — `Clock` trait üzerinden. Production
/// `SystemClock` kullanır, testler `FixedClock`. Bu way'le authorization basis digest
/// testlerde deterministik olur (`created_at` digest'e dahil DEĞİL olsa bile).
pub trait Clock {
    fn unix_seconds(&self) -> u64;
}

/// Production clock — gerçek wall-clock time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_seconds(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Test clock — sabit timestamp.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn unix_seconds(&self) -> u64 {
        self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PendingAuthorization (Model B — Commit 4 genişletir: Envelope + Store)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T9 suspended authorization record (Model B).
///
/// Tüm authorization-gated mutation decision'larını kapsar (AcceptAsCompleted +
/// AcceptAsProgress). Navigator bunu `AwaitingWitnesses` varyantında döndürür.
/// Commit 4 `PendingAuthorizationEnvelope` (embedded AuthorizationBasis) +
/// `PendingAuthorizationStore` ekler.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PendingAuthorization {
    pub task_id: crate::trajectory::TaskId,
    pub claim_id: ClaimId,
    pub predicate_completion: PredicateCompletion,
    pub mutation_decision: MutationDecision,
    pub intended_apply_target: ApplyTarget,
    pub authorization_basis_digest: AuthorizationBasisDigest,
    pub base_space_view_revision: SpaceViewRevision,
    pub evaluation_context_digest: EvaluationContextDigest,
    pub witness_requirement: WitnessRequirement,
    /// INV-T9 Sabitleme 1 — hold nedeni artifact'te korunur.
    pub witness_hold_reason: WitnessHoldReason,
    pub witness_snapshot: WitnessQuorumSnapshot,
    pub attempt_evidence_id: AttemptEvidenceId,
    /// Clock trait'inden — digest'e DAHİL DEĞİL.
    pub created_at: u64,
}

/// Witness quorum gereksinimi (production: 2 approvers, 1.5 support).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WitnessRequirement {
    pub min_approvers: usize,
    pub quorum_threshold: f64,
}

impl WitnessRequirement {
    /// **reviewer plan-review #1 (P0):** WitnessRequirement gerçek `omega`'dan türetilir.
    ///
    /// Engine config YEDEK DEĞİL. Bu, `CanonicalWitnessPolicy::try_from(omega)` ile
    /// tutarlıdır — artifact'in witness policy ile record'un witness_requirement'i
    /// aynı omega kaynağından gelir. Cross-field doğrulama bozulmaz.
    pub fn from(omega: &crate::witness::WitnessSet) -> Self {
        Self {
            min_approvers: omega.min_approvers,
            quorum_threshold: omega.quorum_threshold,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AuthorizationContext — engine-owned single source (reviewer P0-4 + plan-review #1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Engine'in witness'tan ÖNCE ürettiği tek authorization context.
///
/// **reviewer P0-4:** Context bütün deterministik gate'ler geçtikten sonra (Q6),
/// `time.advance` (witness) çağrısından hemen önce üretilir. Satisfied/Held/Rejected
/// aynı context nesnesini kullanır — navigator veya başka bir katman basis'i yeniden
/// üretmez.
///
/// **plan-review #1:** `witness_requirement` gerçek `omega`'dan türetilir (engine config
/// DEĞİL). `basis.witness_policy` ile cross-field tutarlıdır.
///
/// **Commit 2 (Authorization lifecycle completion):** Bu struct Commit 1'de tanımlı ve
/// Held/Rejected'a thread edilir. Evaluated/Satisfied audit propagation ve tüm call
/// path'lerinde tekilleştirme Commit 2'de tamamlanır.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthorizationContext {
    /// PredicateGate sonucu (engine.rs:474) — gerçek outcome, hardcoded DEĞİL.
    pub outcome: AttemptOutcome,
    /// MutationDecision → ApplyTarget (engine.rs:476) — Reject→NotApplied buradan gelir.
    pub apply_target: ApplyTarget,
    /// Gerçek canonical basis — engine'in elindeki tüm verilerden inşa edilir.
    pub basis: AuthorizationBasis,
    /// `WitnessRequirement::from(omega)` — gerçek witness değerlendirmesiyle aynı kaynak.
    pub witness_requirement: WitnessRequirement,
}

/// Attempt evidence identifier (P1 resume'da evidence store lookup için).
pub type AttemptEvidenceId = u64;

/// Explicit witness rejection sonucu — agent proposal revises. Evidence-preserving.
///
/// `NavigatorResult::RequiresRevision` bu struct'ı taşır. Budget tüketmez, LLM
/// reinvocation YOK. Agent yeni structural proposal üretmeli.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RevisionRequired {
    pub task_id: crate::trajectory::TaskId,
    pub claim_id: ClaimId,
    pub authorization_basis_digest: AuthorizationBasisDigest,
    pub reasons: crate::witness::NonEmptyWitnessRejections,
    pub witness_snapshot: crate::witness::WitnessQuorumSnapshot,
    pub attempt_evidence_id: AttemptEvidenceId,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PendingAuthorizationEnvelope — self-contained artifact (Sabitleme 3)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T9 Sabitleme 3 — pending authorization artifact, embedded basis ile self-contained.
///
/// Digest tek başına authorization basis'i yeniden oluşturamaz; yalnızca eldeki basis'in
/// aynı olup olmadığını doğrular. Bu yüzden envelope hem `record.authorization_basis_digest`
/// hem full `authorization_basis` taşır. Load sırasında digest tekrar hesaplanıp doğrulanır.
///
/// Tek canonical schema: `"osp.pending-authorization.v1"` string. Record içinde ayrıca
/// schema_version alanı YOK (tekillik — smart constructor dışında oluşturulamaz).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PendingAuthorizationEnvelope {
    /// Tek canonical schema identifier.
    pub schema: String,
    pub record: PendingAuthorization,
    /// Self-contained — P1 claim/evidence store kurulmadan basis doğrulanabilir.
    pub authorization_basis: AuthorizationBasis,
}

/// Envelope schema sabitleri.
pub const PENDING_AUTHORIZATION_SCHEMA: &str = "osp.pending-authorization.v1";

impl PendingAuthorizationEnvelope {
    /// Smart constructor — digest'i basis'ten hesaplar, record'a yerleştirir.
    pub fn new(
        mut record: PendingAuthorization,
        basis: AuthorizationBasis,
    ) -> Result<Self, AuthorizationBasisDigestError> {
        let digest = AuthorizationBasisDigest::compute(&basis)?;
        record.authorization_basis_digest = digest;
        Ok(Self {
            schema: PENDING_AUTHORIZATION_SCHEMA.to_string(),
            record,
            authorization_basis: basis,
        })
    }

    /// Load + verify — envelope'ı deserialize eder, basis digest'ini tekrar hesaplayıp
    /// `record.authorization_basis_digest` ile karşılaştırır. Mismatch → integrity error.
    pub fn verify(&self) -> Result<(), PendingAuthorizationLoadError> {
        if self.schema != PENDING_AUTHORIZATION_SCHEMA {
            return Err(PendingAuthorizationLoadError::UnknownSchema {
                found: self.schema.clone(),
                expected: PENDING_AUTHORIZATION_SCHEMA,
            });
        }
        let computed = AuthorizationBasisDigest::compute(&self.authorization_basis)
            .map_err(|e| PendingAuthorizationLoadError::DigestComputationFailed(e.to_string()))?;
        if computed != self.record.authorization_basis_digest {
            return Err(PendingAuthorizationLoadError::BasisDigestMismatch);
        }
        Ok(())
    }
}

/// Pending authorization load hataları.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PendingAuthorizationLoadError {
    #[error("unknown schema: found {found}, expected {expected}")]
    UnknownSchema {
        found: String,
        expected: &'static str,
    },
    #[error("authorization basis digest mismatch — artifact may be tampered or corrupted")]
    BasisDigestMismatch,
    #[error("digest computation failed: {0}")]
    DigestComputationFailed(String),
    #[error("deserialization failed: {0}")]
    DeserializationFailed(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// PendingAuthorizationStore — navigator owns persistence (P0-1 çözümü)
// ═══════════════════════════════════════════════════════════════════════════════

/// **plan-review düzeltme #2:** Suspension durability capability.
///
/// Navigator, trait object üzerinden store'un ProcessLocal mi CrossProcess mu olduğunu
/// güvenilir biçimde anlamalıdır. Bu capability olmadan Ephemeral + Filesystem
/// kombinasyonu ya testleri kırar ya da production güven sınırını gevşetir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspensionDurability {
    /// Process-local — in-memory test store. Process restart'ta kaybolur.
    ProcessLocal,
    /// Cross-process — filesystem store. Process restart'ta korunur; persisted space
    /// identity gerektirir (Ephemeral identity ile fail-closed).
    CrossProcess,
}

/// Navigator'ın `AwaitingWitnesses` döndürmeden ÖNCE çağırdığı persistence abstraction.
///
/// Çökme penceresi YOK: `AwaitingWitnesses` yalnızca artifact başarılı publish edildikten
/// sonra return edilir. P0-1 çözümü — CLI yazmaz, navigator injected store'a persist eder.
///
/// **plan-review #2:** `durability()` capability — navigator Ephemeral identity +
/// CrossProcess store kombinasyonunu fail-closed olarak reddeder.
pub trait PendingAuthorizationStore {
    /// Store'un durability capability'si — ProcessLocal (test) veya CrossProcess (production).
    fn durability(&self) -> SuspensionDurability;

    fn persist(
        &mut self,
        envelope: &PendingAuthorizationEnvelope,
    ) -> Result<PendingAuthorizationReceipt, PendingAuthorizationStoreError>;
}

/// Başarılı persist'in kanıtı — artifact path + kimlik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAuthorizationReceipt {
    pub artifact_path: std::path::PathBuf,
    pub claim_id: ClaimId,
    pub authorization_basis_digest: AuthorizationBasisDigest,
}

/// Persist/load hataları.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PendingAuthorizationStoreError {
    #[error(
        "artifact already exists with different basis — integrity error (no silent overwrite)"
    )]
    BasisConflict { existing_path: std::path::PathBuf },
    #[error("artifact write failed: {0}")]
    WriteFailed(String),
    #[error("parent directory creation failed: {0}")]
    DirCreationFailed(String),
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
}

/// Dosya tabanlı default implementation.
///
/// Path: `<root>/.osp/pending-authorizations/<claim-id>--<basis-digest-hex>.json`
///
/// **No-clobber:** `create_new` — sessiz overwrite YOK.
/// **Idempotent:** aynı claim+digest+içerik → success; aynı claim+digest+farklı içerik →
/// integrity error; aynı claim+farklı digest → ayrı artifact.
///
/// **Crash-consistent publish:** same-dir temp → write_all → sync_all → atomic no-clobber
/// publish/rename → parent-dir sync where supported.
///
/// **Platform contract:** Windows rename mevcut hedef üzerinde atomik DEĞİL; biz
/// `create_new(true)` ile temp dosyayı oluşturup rename ediyoruz. Hedef zaten varsa
/// rename fail eder → idempotent success path'i (içerik aynı ise) veya conflict.
pub struct FilesystemPendingAuthorizationStore {
    root: std::path::PathBuf,
}

impl FilesystemPendingAuthorizationStore {
    /// Yeni store — `root` altında `.osp/pending-authorizations/` dizini kullanılır.
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Artifact path'i claim_id + digest'ten türet.
    fn artifact_path(
        &self,
        claim_id: ClaimId,
        digest: &AuthorizationBasisDigest,
    ) -> std::path::PathBuf {
        let hex = digest.to_hex();
        let filename = format!("claim-{claim_id}--{hex}.json");
        self.root
            .join(".osp")
            .join("pending-authorizations")
            .join(filename)
    }
}

impl PendingAuthorizationStore for FilesystemPendingAuthorizationStore {
    fn durability(&self) -> SuspensionDurability {
        SuspensionDurability::CrossProcess
    }

    fn persist(
        &mut self,
        envelope: &PendingAuthorizationEnvelope,
    ) -> Result<PendingAuthorizationReceipt, PendingAuthorizationStoreError> {
        use std::io::Write;

        let artifact_path = self.artifact_path(
            envelope.record.claim_id,
            &envelope.record.authorization_basis_digest,
        );

        // Idempotency: aynı path zaten varsa — içeriği karşılaştır.
        if artifact_path.exists() {
            let existing = std::fs::read(&artifact_path)
                .map_err(|e| PendingAuthorizationStoreError::WriteFailed(e.to_string()))?;
            let current = serde_json::to_vec_pretty(envelope)
                .map_err(|e| PendingAuthorizationStoreError::SerializationFailed(e.to_string()))?;
            if existing == current {
                // Idempotent success — aynı claim+digest+içerik.
                return Ok(PendingAuthorizationReceipt {
                    artifact_path,
                    claim_id: envelope.record.claim_id,
                    authorization_basis_digest: envelope.record.authorization_basis_digest.clone(),
                });
            } else {
                // Conflict — aynı path, farklı içerik (digest çakışması veya corruption).
                return Err(PendingAuthorizationStoreError::BasisConflict {
                    existing_path: artifact_path,
                });
            }
        }

        // Parent directory oluştur.
        if let Some(parent) = artifact_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PendingAuthorizationStoreError::DirCreationFailed(e.to_string()))?;
        }

        // **P1-4:** Unique temp dosya adı (concurrent writer çakışması yok).
        // Process id + thread id + atomic counter → benzersiz.
        use std::sync::atomic::{AtomicU64, Ordering};
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let temp_suffix = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let temp_path = artifact_path.with_file_name(format!(
            ".{}.tmp.{pid}.{temp_suffix}",
            artifact_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("pending")
        ));

        // Cleanup guard — hata yollarında temp dosyayı sil.
        let result = (|| -> Result<(), PendingAuthorizationStoreError> {
            let mut temp_file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|e| PendingAuthorizationStoreError::WriteFailed(e.to_string()))?;

            let json = serde_json::to_vec_pretty(envelope)
                .map_err(|e| PendingAuthorizationStoreError::SerializationFailed(e.to_string()))?;
            temp_file
                .write_all(&json)
                .map_err(|e| PendingAuthorizationStoreError::WriteFailed(e.to_string()))?;

            // sync_all — veriyi diske flush et (crash consistency).
            temp_file
                .sync_all()
                .map_err(|e| PendingAuthorizationStoreError::WriteFailed(e.to_string()))?;
            drop(temp_file);
            Ok(())
        })();

        if let Err(e) = result {
            // Cleanup guard — temp dosya kaldıysa sil.
            let _ = std::fs::remove_file(&temp_path);
            return Err(e);
        }

        // Atomic no-clobber publish (rename).
        // **Platform contract (review P1-4):** Unix'te rename mevcut hedefi overwrite eder.
        // Yukarıda exists() kontrolü yaptık ama TOCTOU window var. Windows'ta rename
        // mevcut hedefte fail eder (no-clobber semantics). Cross-platform gerçek no-clobber
        // için exists()+rename yeterli değil — race window minimal ama kabul edilir.
        // Production'da concurrent writer'lar farklı digest'ler (farklı path) kullanır.
        std::fs::rename(&temp_path, &artifact_path).map_err(|e| {
            // Cleanup: rename failse temp'i sil.
            let _ = std::fs::remove_file(&temp_path);
            PendingAuthorizationStoreError::WriteFailed(e.to_string())
        })?;

        // Parent directory sync (crash consistency) — Unix'te desteklenir.
        #[cfg(unix)]
        {
            if let Some(parent) = artifact_path.parent() {
                if let Ok(dir) = std::fs::File::open(parent) {
                    use std::os::unix::io::AsRawFd;
                    unsafe {
                        libc::fsync(dir.as_raw_fd());
                    }
                }
            }
        }

        Ok(PendingAuthorizationReceipt {
            artifact_path,
            claim_id: envelope.record.claim_id,
            authorization_basis_digest: envelope.record.authorization_basis_digest.clone(),
        })
    }
}

/// Artifact'ı dosyadan yükle + verify (P1 resume için, ama P0'da da test edilebilir).
pub fn load_pending_authorization(
    path: &std::path::Path,
) -> Result<PendingAuthorizationEnvelope, PendingAuthorizationLoadError> {
    let bytes = std::fs::read(path)
        .map_err(|e| PendingAuthorizationLoadError::DeserializationFailed(e.to_string()))?;
    let envelope: PendingAuthorizationEnvelope = serde_json::from_slice(&bytes)
        .map_err(|e| PendingAuthorizationLoadError::DeserializationFailed(e.to_string()))?;
    envelope.verify()?;
    Ok(envelope)
}

/// Null store — persist çağrılarını kabul eder ama hiçbir şey yazmaz (in-memory testler için).
///
/// Production'da KULLANILMAZ — sadece navigator testleri için. `AwaitingWitnesses` yine
/// döner ama artifact_path boş olur. Real persist `FilesystemPendingAuthorizationStore` ile.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullPendingAuthorizationStore;

impl PendingAuthorizationStore for NullPendingAuthorizationStore {
    fn durability(&self) -> SuspensionDurability {
        SuspensionDurability::ProcessLocal
    }

    fn persist(
        &mut self,
        envelope: &PendingAuthorizationEnvelope,
    ) -> Result<PendingAuthorizationReceipt, PendingAuthorizationStoreError> {
        Ok(PendingAuthorizationReceipt {
            artifact_path: std::path::PathBuf::new(), // null — no artifact
            claim_id: envelope.record.claim_id,
            authorization_basis_digest: envelope.record.authorization_basis_digest.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::{CommitLane, TaskId};

    fn sample_basis() -> AuthorizationBasis {
        AuthorizationBasis {
            schema_version: 1,
            task_id: TaskId::from(1u64),
            claim_identity: ClaimIdentity {
                claim_id: ClaimId::from(42u64),
                task_id: TaskId::from(1u64),
            },
            claim_author: AgentId::from(100u64),
            structural_delta: CanonicalStructuralDelta::try_new(
                vec![CanonicalNode {
                    id: 10,
                    kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Module).unwrap(),
                    mass: 100.0,
                    cohesion: Some(0.5),
                    classification: CanonicalNodeClassification::try_from(
                        &crate::space::NodeClassification::Production,
                    )
                    .unwrap(),
                    role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap(),
                }],
                vec![],
                vec![CanonicalEdge {
                    from: 0,
                    to: 1,
                    kind: CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
                    is_type_only: false,
                }],
            )
            .unwrap(),
            predicate_content: CanonicalPredicateContent {
                mode: PredicateModeTag::try_from(&crate::trajectory::PredicateMode::All).unwrap(),
                predicates: vec![],
            },
            predicate_evaluation: PredicateEvaluationBasis {
                target_vector: CanonicalRawPosition {
                    x: 0.55,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                },
                loss_before: 1.0,
                loss_after: 0.5,
                failure_policy: PredicateFailurePolicyTag::try_from(
                    &crate::trajectory::PredicateFailurePolicy::StrictReject,
                )
                .unwrap(),
                allow_progress_checkpoint: false,
                min_improvement_delta: 0.1,
                improvement_policy: EffectiveImprovementPolicy::current_semantics(),
            },
            measured_result: {
                let scip = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
                    &crate::coords::MetricSource::Scip,
                )
                .unwrap();
                let mk = |v: f64| CanonicalAxisMeasurement {
                    value: v,
                    source: scip,
                };
                ProvenancedMeasuredResult {
                    coupling: mk(0.5),
                    cohesion: mk(0.6),
                    instability: mk(0.4),
                    entropy: mk(0.5),
                    witness_depth: mk(0.3),
                }
            },
            deterministic_gate_result: GateDecision::PassedAll,
            predicate_completion: PredicateCompletion::Completed,
            mutation_decision: MutationDecision::AcceptAsCompleted,
            intended_apply_target: ApplyTarget::Lane(CommitLane::Mainline),
            witness_policy: CanonicalWitnessPolicy {
                schema_version: 1,
                min_approvers: 2,
                quorum_threshold: 1.5,
                independence_policy: WitnessIndependencePolicyTag::STRICT,
            },
            measurement_input_digest: MeasurementInputDigest::from_bytes([0xcc; 32]),
            evaluation_context_digest: EvaluationContextDigest::from_bytes([0xaa; 32]),
            base_space_view_revision: SpaceViewRevision {
                view_id: SpaceViewId::Persisted(PersistedSpaceViewId::from_bytes([0xdd; 16])),
                sequence: 7,
                content_digest: SpaceDigest::from_bytes([0xbb; 32]),
            },
        }
    }

    #[test]
    fn authorization_basis_digest_is_stable_for_identical_basis() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis).unwrap();
        assert_eq!(d1, d2, "same basis → same digest");
    }

    #[test]
    fn authorization_basis_digest_changes_when_claim_changes() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let mut basis2 = basis.clone();
        basis2.claim_identity.claim_id = ClaimId::from(99u64);
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "different claim → different digest");
    }

    #[test]
    fn authorization_basis_digest_changes_when_space_view_id_changes() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let mut basis2 = basis.clone();
        basis2.base_space_view_revision.view_id =
            SpaceViewId::Persisted(PersistedSpaceViewId::from_bytes([0xee; 16]));
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "different space view id → different digest");
    }

    #[test]
    fn authorization_basis_digest_changes_when_predicate_content_changes() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let mut basis2 = basis.clone();
        basis2
            .predicate_content
            .predicates
            .push(EffectiveMetricPredicate {
                axis: PredicateAxisTag::try_from(&crate::trajectory::PredicateAxis::Cohesion)
                    .unwrap(),
                operator: ComparisonOpTag::try_from(&crate::trajectory::ComparisonOp::Lt).unwrap(),
                threshold: 0.6,
                scope: CanonicalPredicateScope::Node(0),
                required_source: EffectiveSourceRequirement::Any,
                effective_weight: 1.0,
                effective_tolerance: 0.0,
            });
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "different predicate content → different digest");
    }

    #[test]
    fn authorization_basis_digest_hex_roundtrip() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let hex = d1.to_hex();
        let d2 = AuthorizationBasisDigest::from_hex(&hex).unwrap();
        assert_eq!(d1, d2, "hex roundtrip");
    }

    #[test]
    fn authorization_basis_digest_uses_domain_separation() {
        // Domain separation: farklı prefix → farklı digest (same content).
        // Canonical binary encoding domain separator içerir; raw BLAKE3 (separator yok)
        // farklı digest üretir.
        let basis = sample_basis();
        let digest = AuthorizationBasisDigest::compute(&basis).unwrap();

        // Raw BLAKE3 without domain separation — struct'ın Debug çıktısını hash'le (control).
        // Bu yaklaşık ama domain separation'ın farklı bir digest ürettiğini gösterir.
        let debug_bytes = format!("{basis:?}");
        let raw_hash = blake3::hash(debug_bytes.as_bytes());
        let raw_bytes: [u8; 32] = raw_hash.into();

        assert_ne!(
            digest.as_bytes(),
            &raw_bytes,
            "domain separation must produce different digest"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Canonical encoding tests (review P1-3)
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn authorization_basis_digest_rejects_nan_in_measured_result() {
        let basis = sample_basis();
        let mut basis2 = basis.clone();
        basis2.measured_result.coupling.value = f64::NAN;
        let err = AuthorizationBasisDigest::compute(&basis2).unwrap_err();
        assert_eq!(err, AuthorizationBasisDigestError::NonFiniteRejected);
    }

    // **reviewer P0-1 (per-axis non-finite):** 5 eksenin HER BİRİ NaN/±Infinity
    // reddetmeli — bir eksen predicate tarafından kullanılmıyor olsa bile basis'in
    // parçasıysa non-finite geçmemeli. Fixed axis sırası: coupling, cohesion,
    // instability, entropy, witness_depth.
    fn set_axis(basis: &mut AuthorizationBasis, axis: &str, v: f64) {
        match axis {
            "coupling" => basis.measured_result.coupling.value = v,
            "cohesion" => basis.measured_result.cohesion.value = v,
            "instability" => basis.measured_result.instability.value = v,
            "entropy" => basis.measured_result.entropy.value = v,
            "witness_depth" => basis.measured_result.witness_depth.value = v,
            _ => unreachable!("unknown axis {axis}"),
        }
    }

    #[test]
    fn measured_result_rejects_non_finite_value_on_every_axis() {
        for axis in [
            "coupling",
            "cohesion",
            "instability",
            "entropy",
            "witness_depth",
        ] {
            for non_finite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut basis = sample_basis();
                set_axis(&mut basis, axis, non_finite);
                let err = AuthorizationBasisDigest::compute(&basis).unwrap_err();
                assert_eq!(
                    err,
                    AuthorizationBasisDigestError::NonFiniteRejected,
                    "axis {axis} with {non_finite} must be rejected"
                );
            }
        }
    }

    #[test]
    fn authorization_basis_normalizes_negative_zero_on_every_axis() {
        // **reviewer P0-1:** 5 eksenin HER BİRİ -0.0 ve +0.0'ı aynı digest'e normalize etmeli.
        for axis in [
            "coupling",
            "cohesion",
            "instability",
            "entropy",
            "witness_depth",
        ] {
            let mut basis_pos = sample_basis();
            set_axis(&mut basis_pos, axis, 0.0f64);
            let mut basis_neg = sample_basis();
            set_axis(&mut basis_neg, axis, -0.0f64);
            let d_pos = AuthorizationBasisDigest::compute(&basis_pos).unwrap();
            let d_neg = AuthorizationBasisDigest::compute(&basis_neg).unwrap();
            assert_eq!(
                d_pos, d_neg,
                "axis {axis}: -0.0 and +0.0 must normalize to same digest"
            );
        }
    }

    #[test]
    fn authorization_basis_changes_when_only_entropy_source_changes() {
        // **reviewer P0-1 (per-axis provenance):** yalnızca entropy ekseninin source'u
        // değişince basis digest değişmeli — INV-T4 source-requirement evidence basis.
        let scip = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::Scip,
        )
        .unwrap();
        let treesitter = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::TreeSitter,
        )
        .unwrap();
        let basis1 = sample_basis();
        let mut basis2 = basis1.clone();
        // measured.entropy.source Scip → TreeSitter (value sabit).
        basis2.measured_result.entropy.source = treesitter;
        // sample_basis tüm eksenleri Scip ile kuruyor; basis1 ile karşılaştır.
        assert_ne!(scip, treesitter, "test fixture: sources must differ");
        let d1 = AuthorizationBasisDigest::compute(&basis1).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "entropy source change must change digest");
    }

    #[test]
    fn authorization_basis_changes_when_only_witness_depth_source_changes() {
        // **reviewer P0-1 (per-axis provenance):** yalnızca witness_depth ekseninin
        // source'u değişince basis digest değişmeli.
        let treesitter = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::TreeSitter,
        )
        .unwrap();
        let heuristic = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::Heuristic,
        )
        .unwrap();
        let mut basis1 = sample_basis();
        let mut basis2 = sample_basis();
        basis1.measured_result.witness_depth.source = treesitter;
        basis2.measured_result.witness_depth.source = heuristic;
        let d1 = AuthorizationBasisDigest::compute(&basis1).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "witness_depth source change must change digest");
    }

    #[test]
    fn canonical_subgraph_scope_rejects_duplicate_node() {
        // **reviewer P1-1:** [1,1,2] → Err(DuplicateScopeNode(1)).
        let err = CanonicalSubgraphScope::try_new(vec![1, 1, 2]).unwrap_err();
        assert_eq!(err, CanonicalizationError::DuplicateScopeNode(1));
    }

    #[test]
    fn canonical_subgraph_scope_normalizes_order() {
        // **reviewer P1-1:** constructor sort eder — [3,1,2] → [1,2,3].
        let s = CanonicalSubgraphScope::try_new(vec![3, 1, 2]).unwrap();
        assert_eq!(s.as_sorted_ids(), &[1, 2, 3]);
    }

    #[test]
    fn canonical_scope_deserialization_rejects_duplicate_subgraph_node() {
        // **reviewer P1-1:** diskten [1,1,2] yüklenemez — custom Deserialize try_new üzerinden.
        let json = serde_json::to_string(&vec![1u64, 1, 2]).unwrap();
        let err = serde_json::from_str::<CanonicalSubgraphScope>(&json).unwrap_err();
        assert!(
            err.to_string().contains("duplicate scope node"),
            "deserialize must reject duplicate: {err}"
        );
    }

    #[test]
    fn empty_subgraph_has_one_canonical_representation() {
        // **reviewer P1-2:** empty subgraph geçerli, tek canonical rep.
        let empty_a = CanonicalSubgraphScope::try_new(vec![]).unwrap();
        let empty_b = CanonicalSubgraphScope::try_new(vec![]).unwrap();
        assert_eq!(empty_a, empty_b, "two empty subgraphs must be equal");
        assert!(empty_a.as_sorted_ids().is_empty());

        // Boş ile dolu farklı scope'lar.
        let non_empty = CanonicalSubgraphScope::try_new(vec![5]).unwrap();
        assert_ne!(
            CanonicalPredicateScope::Subgraph(empty_a),
            CanonicalPredicateScope::Subgraph(non_empty),
            "empty vs non-empty subgraph must differ"
        );
    }

    #[test]
    fn subgraph_identity_bytes_sorted_and_unique() {
        // **reviewer P1-1:** identity_bytes canonical (sorted) sıra encode eder —
        // tekrar sort ETMEZ (invariant type seviyesinde korundu).
        let s = CanonicalSubgraphScope::try_new(vec![3, 1, 2]).unwrap();
        let scope = CanonicalPredicateScope::Subgraph(s);
        let bytes = scope.identity_bytes();
        // [1,2,3] sorted → LE bytes concat.
        let mut expected = Vec::new();
        for id in [1u64, 2, 3] {
            expected.extend_from_slice(&id.to_le_bytes());
        }
        assert_eq!(bytes, expected);
    }

    #[test]
    fn authorization_basis_digest_normalizes_negative_zero() {
        // -0.0 ve +0.0 aynı digest vermeli (canonical normalization).
        let basis_pos = sample_basis();
        let mut basis_neg = basis_pos.clone();
        basis_neg.measured_result.coupling.value = -0.0f64;
        // basis_pos.x = 0.5, basis_neg.x = -0.0 → farklı. İkisini de 0.0 yap.
        let mut basis_zero = basis_pos.clone();
        basis_zero.measured_result.coupling.value = 0.0f64;

        let mut basis_neg_zero = basis_pos.clone();
        basis_neg_zero.measured_result.coupling.value = -0.0f64;

        let d1 = AuthorizationBasisDigest::compute(&basis_zero).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis_neg_zero).unwrap();
        assert_eq!(d1, d2, "-0.0 and +0.0 must normalize to same digest");
    }

    #[test]
    fn authorization_basis_digest_is_order_independent_for_node_ids() {
        // Same nodes in different order → same digest (sorted encoding).
        let basis1 = sample_basis();
        let mut basis2 = basis1.clone();
        // new_nodes sırasını ters çevir.
        basis2.structural_delta.new_nodes.reverse();

        let d1 = AuthorizationBasisDigest::compute(&basis1).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_eq!(d1, d2, "same nodes different order → same digest (sorted)");
    }

    #[test]
    fn authorization_basis_digest_is_order_independent_for_edges() {
        let basis1 = sample_basis();
        let mut basis2 = basis1.clone();
        basis2.structural_delta.removed_edges.reverse();

        let d1 = AuthorizationBasisDigest::compute(&basis1).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_eq!(d1, d2, "same edges different order → same digest (sorted)");
    }

    #[test]
    fn authorization_basis_digest_changes_when_rule_set_context_changes() {
        // Evaluation context digest değişince basis digest değişir.
        let basis1 = sample_basis();
        let mut basis2 = basis1.clone();
        basis2.evaluation_context_digest = EvaluationContextDigest::from_bytes([0xff; 32]);

        let d1 = AuthorizationBasisDigest::compute(&basis1).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "different evaluation context → different digest");
    }

    #[test]
    fn authorization_basis_digest_changes_when_mutation_decision_changes() {
        let basis1 = sample_basis();
        let mut basis2 = basis1.clone();
        basis2.mutation_decision = crate::trajectory::MutationDecision::AcceptAsProgress;

        let d1 = AuthorizationBasisDigest::compute(&basis1).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "different mutation decision → different digest");
    }

    #[test]
    fn canonical_structural_delta_constructor_sorts_collections() {
        let mk_node = |id| CanonicalNode {
            id,
            kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Module).unwrap(),
            mass: 1.0,
            cohesion: None,
            classification: CanonicalNodeClassification::try_from(
                &crate::space::NodeClassification::Production,
            )
            .unwrap(),
            role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap(),
        };
        let mk_edge = |from, to, kind_num| CanonicalEdge {
            from,
            to,
            kind: CanonicalEdgeKind::try_from(
                &(match kind_num {
                    0 => crate::space::EdgeKind::Imports,
                    _ => crate::space::EdgeKind::Calls,
                }),
            )
            .unwrap(),
            is_type_only: false,
        };
        let delta = CanonicalStructuralDelta::try_new(
            vec![mk_node(3), mk_node(1), mk_node(2)],
            vec![mk_edge(2, 1, 1), mk_edge(1, 2, 0)],
            vec![],
        )
        .unwrap();
        assert_eq!(
            delta.new_nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "nodes sorted by id"
        );
        assert_eq!(delta.new_edges[0].from, 1, "edges sorted");
    }

    #[test]
    fn fixed_clock_is_deterministic() {
        let clock = FixedClock(1_700_000_000);
        assert_eq!(clock.unix_seconds(), 1_700_000_000);
        assert_eq!(clock.unix_seconds(), 1_700_000_000, "deterministic");
    }

    #[test]
    fn space_view_revision_serializes_roundtrip() {
        let rev = SpaceViewRevision {
            view_id: SpaceViewId::Persisted(PersistedSpaceViewId::from_bytes([0xab; 16])),
            sequence: 42,
            content_digest: SpaceDigest::from_bytes([0xcd; 32]),
        };
        let json = serde_json::to_string(&rev).unwrap();
        let rev2: SpaceViewRevision = serde_json::from_str(&json).unwrap();
        assert_eq!(rev, rev2);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Envelope + Store tests (Commit 4)
    // ═══════════════════════════════════════════════════════════════════════════════

    fn sample_pending_record() -> PendingAuthorization {
        PendingAuthorization {
            task_id: TaskId::from(1u64),
            claim_id: ClaimId::from(42u64),
            predicate_completion: PredicateCompletion::Completed,
            mutation_decision: MutationDecision::AcceptAsCompleted,
            intended_apply_target: ApplyTarget::Lane(CommitLane::Mainline),
            authorization_basis_digest: AuthorizationBasisDigest::from_hex(
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(), // placeholder — Envelope::new overwrite eder
            base_space_view_revision: SpaceViewRevision {
                view_id: SpaceViewId::Persisted(PersistedSpaceViewId::from_bytes([0xdd; 16])),
                sequence: 7,
                content_digest: SpaceDigest::from_bytes([0xbb; 32]),
            },
            evaluation_context_digest: EvaluationContextDigest::from_bytes([0xaa; 32]),
            witness_requirement: WitnessRequirement {
                min_approvers: 2,
                quorum_threshold: 1.5,
            },
            witness_hold_reason: WitnessHoldReason::MinApproversNotMet {
                distinct: 0,
                required: 2,
            },
            witness_snapshot: WitnessQuorumSnapshot {
                approvers: 0,
                required_approvers: 2,
                support: 0.0,
                required_support: 1.5,
            },
            attempt_evidence_id: 1,
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn pending_authorization_preserves_witness_hold_reason() {
        // Sabitleme 1 — hold nedeni artifact'te korunur.
        let record = sample_pending_record();
        assert!(matches!(
            record.witness_hold_reason,
            WitnessHoldReason::MinApproversNotMet {
                distinct: 0,
                required: 2
            }
        ));
    }

    #[test]
    fn envelope_new_computes_and_sets_digest() {
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis.clone()).unwrap();

        let expected = AuthorizationBasisDigest::compute(&basis).unwrap();
        assert_eq!(envelope.record.authorization_basis_digest, expected);
        assert_eq!(envelope.schema, PENDING_AUTHORIZATION_SCHEMA);
    }

    #[test]
    fn envelope_verify_succeeds_for_valid_envelope() {
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();
        envelope.verify().expect("valid envelope should verify");
    }

    #[test]
    fn envelope_verify_rejects_basis_digest_mismatch() {
        let basis = sample_basis();
        let record = sample_pending_record();
        let mut envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        // Tamper — farklı digest set et.
        envelope.record.authorization_basis_digest = AuthorizationBasisDigest::from_hex(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();

        let err = envelope.verify().unwrap_err();
        assert_eq!(err, PendingAuthorizationLoadError::BasisDigestMismatch);
    }

    #[test]
    fn envelope_verify_rejects_unknown_schema() {
        let basis = sample_basis();
        let record = sample_pending_record();
        let mut envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();
        envelope.schema = "osp.pending-authorization.v999".to_string();

        let err = envelope.verify().unwrap_err();
        assert!(matches!(
            err,
            PendingAuthorizationLoadError::UnknownSchema { .. }
        ));
    }

    #[test]
    fn pending_authorization_round_trips_through_serde() {
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let json = serde_json::to_string(&envelope).unwrap();
        let envelope2: PendingAuthorizationEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, envelope2);
    }

    #[test]
    fn pending_authorization_rejects_unknown_schema_version() {
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let mut json = serde_json::to_string(&envelope).unwrap();
        // Schema'yı boz.
        json = json.replace(PENDING_AUTHORIZATION_SCHEMA, "osp.bogus.v1");
        let envelope2: PendingAuthorizationEnvelope = serde_json::from_str(&json).unwrap();
        let err = envelope2.verify().unwrap_err();
        assert!(matches!(
            err,
            PendingAuthorizationLoadError::UnknownSchema { .. }
        ));
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // FilesystemPendingAuthorizationStore tests
    // ═══════════════════════════════════════════════════════════════════════════════

    fn temp_dir() -> std::path::PathBuf {
        tempfile::tempdir().expect("temp dir").keep()
    }

    #[test]
    fn filesystem_store_persists_artifact() {
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let receipt = store.persist(&envelope).expect("persist");
        assert!(receipt.artifact_path.exists(), "artifact should exist");
        assert!(receipt
            .artifact_path
            .to_string_lossy()
            .contains("claim-42--"));
        assert!(receipt.artifact_path.to_string_lossy().contains(".json"));
    }

    #[test]
    fn filesystem_store_is_idempotent_for_identical_basis() {
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let receipt1 = store.persist(&envelope).expect("first persist");
        let receipt2 = store
            .persist(&envelope)
            .expect("second persist (idempotent)");

        assert_eq!(receipt1.artifact_path, receipt2.artifact_path);
    }

    #[test]
    fn filesystem_store_never_silently_overwrites_different_basis() {
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);

        // İlk envelope persist et.
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();
        let receipt = store.persist(&envelope).expect("first persist");

        // Aynı path'e FARKLI içerik yaz (manuel corruption / digest collision simülasyonu).
        // Store bunu idempotent success DEĞİL, BasisConflict olarak algılamalı.
        std::fs::write(&receipt.artifact_path, b"{\"completely\":\"different\"}").unwrap();

        let err = store.persist(&envelope).unwrap_err();
        assert!(
            matches!(err, PendingAuthorizationStoreError::BasisConflict { .. }),
            "same path + different content must be BasisConflict, got: {err:?}"
        );
    }

    #[test]
    fn filesystem_store_filename_uses_validated_ids_only() {
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let receipt = store.persist(&envelope).expect("persist");
        let filename = receipt
            .artifact_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            filename.starts_with("claim-42--"),
            "filename must use claim_id + digest: {filename}"
        );
        assert!(filename.ends_with(".json"));
    }

    #[test]
    fn filesystem_store_load_roundtrips_and_verifies() {
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let receipt = store.persist(&envelope).expect("persist");
        let loaded = load_pending_authorization(&receipt.artifact_path).expect("load + verify");
        assert_eq!(loaded, envelope);
    }

    #[test]
    fn pending_record_contains_everything_required_for_future_resume() {
        // Bu test P1 resume için gerekli tüm alanların mevcudiyetini garanti eder.
        let record = sample_pending_record();
        // Resume için kritik alanlar:
        let _task_id = record.task_id;
        let _claim_id = record.claim_id;
        let _predicate_completion = record.predicate_completion;
        let _mutation_decision = record.mutation_decision;
        let _intended_apply_target = record.intended_apply_target;
        let _authorization_basis_digest = &record.authorization_basis_digest;
        let _base_space_view_revision = &record.base_space_view_revision;
        let _evaluation_context_digest = &record.evaluation_context_digest;
        let _witness_requirement = &record.witness_requirement;
        let _witness_hold_reason = &record.witness_hold_reason;
        let _witness_snapshot = &record.witness_snapshot;
        let _attempt_evidence_id = record.attempt_evidence_id;
        let _created_at = record.created_at;
        // Hepsi erişilebilir — record complete.
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Commit 1 amend — Canonical primitives regression testleri
    // (reviewer P0-1..P0-3, P1 + plan-review düzeltmeleri)
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **INV-T9 Adım 3:** Yeni dar model — context axis descriptor listesinden üretilir.
    /// `entropy`/`witness_depth` artık context'te DEĞİL; axis descriptor'lara gömülü
    /// (effective normalized value). Bu helper test için 5 core axis descriptor'ı üretir.
    fn sample_measurement_context() -> MeasurementInputContext {
        use crate::coords::{AxisDescriptor, AxisParameterEncoder};
        // 5 core axis descriptor — effective normalized values ile.
        let mk = |id: &str, marker: u8, value: f64| -> AxisDescriptor {
            let mut params = AxisParameterEncoder::new();
            params.push_u8(marker);
            params.push_f64(value).unwrap();
            AxisDescriptor::try_new(id, 1, params).unwrap()
        };
        let descriptors = vec![
            mk("coupling", 0, 0.0), // parametresiz marker, value placeholder
            mk("cohesion", 1, 0.5),
            mk("instability", 0, 0.0),
            mk("entropy", 2, 0.5),
            mk("witness_depth", 3, 0.3),
        ];
        MeasurementInputContext::try_new(descriptors).unwrap()
    }

    #[test]
    fn measurement_digest_distinguishes_none_some_zero_positions() {
        // **INV-T9 Adım 3 (yeni model):** context artık axis descriptor listesi taşıyor;
        // Option<f64> presence collision eski modelde kaldı. Yeni test: farklı axis
        // descriptor canonical_parameters → farklı digest. Aynı descriptor listesi →
        // aynı digest (stability).
        let ctx_a = sample_measurement_context();
        let ctx_b = sample_measurement_context();
        let d_a = MeasurementInputDigest::compute(&ctx_a).unwrap();
        let d_b = MeasurementInputDigest::compute(&ctx_b).unwrap();
        assert_eq!(
            d_a, d_b,
            "identical descriptor list → same digest (stability)"
        );

        // Farklı entropy axis descriptor (farklı canonical_parameters) → farklı digest.
        use crate::coords::{AxisDescriptor, AxisParameterEncoder};
        let mk = |id: &str, marker: u8, value: f64| -> AxisDescriptor {
            let mut params = AxisParameterEncoder::new();
            params.push_u8(marker);
            params.push_f64(value).unwrap();
            AxisDescriptor::try_new(id, 1, params).unwrap()
        };
        let descriptors_changed = vec![
            mk("coupling", 0, 0.0),
            mk("cohesion", 1, 0.5),
            mk("instability", 0, 0.0),
            mk("entropy", 2, 0.9), // farklı effective value
            mk("witness_depth", 3, 0.3),
        ];
        let ctx_c = MeasurementInputContext::try_new(descriptors_changed).unwrap();
        let d_c = MeasurementInputDigest::compute(&ctx_c).unwrap();
        assert_ne!(
            d_a, d_c,
            "axis descriptor change (entropy effective value) must produce different digest"
        );
    }

    #[test]
    fn authorization_basis_rejects_positive_infinity() {
        // **reviewer P0-2a:** ±Infinity reddedilmeli (yalnız NaN değil).
        let basis = sample_basis();
        let mut basis2 = basis.clone();
        basis2.measured_result.coupling.value = f64::INFINITY;
        let err = AuthorizationBasisDigest::compute(&basis2).unwrap_err();
        assert_eq!(err, AuthorizationBasisDigestError::NonFiniteRejected);
    }

    #[test]
    fn authorization_basis_rejects_negative_infinity() {
        let basis = sample_basis();
        let mut basis2 = basis.clone();
        basis2.measured_result.cohesion.value = f64::NEG_INFINITY;
        let err = AuthorizationBasisDigest::compute(&basis2).unwrap_err();
        assert_eq!(err, AuthorizationBasisDigestError::NonFiniteRejected);
    }

    #[test]
    fn predicate_sort_uses_normalized_canonical_float_encoding() {
        // **reviewer P0-2b:** Sorting canonical byte dizisi ile yapılır.
        // -0.0 ve 0.0 aynı canonical byte dizisini üretmeli → aynı digest.
        let mut basis_pos = sample_basis();
        basis_pos
            .predicate_content
            .predicates
            .push(EffectiveMetricPredicate {
                axis: PredicateAxisTag::try_from(&crate::trajectory::PredicateAxis::Coupling)
                    .unwrap(),
                operator: ComparisonOpTag::try_from(&crate::trajectory::ComparisonOp::Lt).unwrap(),
                threshold: -0.0f64, // negative zero
                scope: CanonicalPredicateScope::Node(0),
                required_source: EffectiveSourceRequirement::Any,
                effective_weight: 1.0,
                effective_tolerance: 0.0,
            });
        let mut basis_zero = sample_basis();
        basis_zero
            .predicate_content
            .predicates
            .push(EffectiveMetricPredicate {
                axis: PredicateAxisTag::try_from(&crate::trajectory::PredicateAxis::Coupling)
                    .unwrap(),
                operator: ComparisonOpTag::try_from(&crate::trajectory::ComparisonOp::Lt).unwrap(),
                threshold: 0.0f64, // positive zero
                scope: CanonicalPredicateScope::Node(0),
                required_source: EffectiveSourceRequirement::Any,
                effective_weight: 1.0,
                effective_tolerance: 0.0,
            });
        let d1 = AuthorizationBasisDigest::compute(&basis_pos).unwrap();
        let d2 = AuthorizationBasisDigest::compute(&basis_zero).unwrap();
        assert_eq!(
            d1, d2,
            "-0.0 and 0.0 predicate thresholds must normalize to same digest"
        );
    }

    #[test]
    fn canonical_structural_delta_rejects_duplicate_node_id() {
        // **reviewer P1:** duplicate node ID reddedilmeli.
        let node = || CanonicalNode {
            id: 5,
            kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Module).unwrap(),
            mass: 1.0,
            cohesion: None,
            classification: CanonicalNodeClassification::try_from(
                &crate::space::NodeClassification::Production,
            )
            .unwrap(),
            role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap(),
        };
        let err = CanonicalStructuralDelta::try_new(vec![node(), node()], vec![], vec![]);
        assert!(matches!(
            err,
            Err(CanonicalizationError::DuplicateNodeId(5))
        ));
    }

    #[test]
    fn canonical_structural_delta_rejects_non_finite_node_field() {
        let node = CanonicalNode {
            id: 7,
            kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Module).unwrap(),
            mass: f64::NAN,
            cohesion: None,
            classification: CanonicalNodeClassification::try_from(
                &crate::space::NodeClassification::Production,
            )
            .unwrap(),
            role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap(),
        };
        let err = CanonicalStructuralDelta::try_new(vec![node], vec![], vec![]);
        assert!(matches!(
            err,
            Err(CanonicalizationError::NonFiniteNodeField(7))
        ));
    }

    #[test]
    fn canonical_structural_delta_rejects_cross_list_edge_conflict() {
        // **plan-review ikincil:** aynı edge new_edges ve removed_edges'te → ambiguous delta.
        let edge = CanonicalEdge {
            from: 1,
            to: 2,
            kind: CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
            is_type_only: false,
        };
        let err = CanonicalStructuralDelta::try_new(vec![], vec![edge.clone()], vec![edge]);
        assert_eq!(
            err.unwrap_err(),
            CanonicalizationError::CrossListEdgeConflict
        );
    }

    #[test]
    fn persisted_space_view_id_has_expected_length() {
        // **reviewer P0-3:** CSPRNG identity — 16 byte.
        let id = PersistedSpaceViewId::generate().unwrap();
        assert_eq!(id.as_bytes().len(), 16);
    }

    #[test]
    fn persisted_space_view_id_generation_propagates_entropy_failure() {
        // **plan-review:** Injectable EntropySource ile deterministic failure test.
        struct FailingEntropy;
        impl super::EntropySource for FailingEntropy {
            fn fill(&self, _dest: &mut [u8]) -> Result<(), SpaceIdentityError> {
                Err(SpaceIdentityError::EntropyUnavailable {
                    message: "simulated failure".to_string(),
                })
            }
        }
        let err = PersistedSpaceViewId::generate_with(&FailingEntropy).unwrap_err();
        assert!(matches!(err, SpaceIdentityError::EntropyUnavailable { .. }));
    }

    #[test]
    fn persisted_space_view_id_generation_uses_os_entropy() {
        // İki generate çağrısı farklı byte dizileri üretmeli (CSPRNG).
        let id1 = PersistedSpaceViewId::generate().unwrap();
        let id2 = PersistedSpaceViewId::generate().unwrap();
        assert_ne!(
            id1.as_bytes(),
            id2.as_bytes(),
            "CSPRNG must produce unique ids"
        );
    }

    #[test]
    fn persisted_space_view_id_serialization_roundtrip() {
        let id = PersistedSpaceViewId::generate().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: PersistedSpaceViewId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn space_digest_is_stable_for_identical_space() {
        // **reviewer P0-3 (A7):** SpaceDigest gerçek canonical içerik.
        use crate::coords::{DerivedPosition, Position, RawPosition};
        use crate::space::{Edge, EdgeKind, Node, NodeClassification, NodeKind, NodeRole, Space};
        let mk_space = || {
            let mut space = Space::default();
            space.nodes.insert(
                1,
                Node {
                    id: 1,
                    kind: NodeKind::Module,
                    mass: 10.0,
                    position: Position {
                        raw: RawPosition::default(),
                        derived: DerivedPosition::default(),
                    },
                    cohesion: Some(0.5),
                    classification: NodeClassification::Production,
                    role: NodeRole::Runtime,
                },
            );
            space.edges.push(Edge {
                from: 1,
                to: 2,
                kind: EdgeKind::Imports,
                is_type_only: false,
            });
            space
        };
        let d1 = SpaceDigest::compute(&mk_space()).unwrap();
        let d2 = SpaceDigest::compute(&mk_space()).unwrap();
        assert_eq!(d1, d2, "identical spaces → same digest");
    }

    #[test]
    fn space_digest_excludes_position_field() {
        // **reviewer P0-4 inclusion table:** position engine-derived, dahil DEĞİL.
        // Sadece position farklı, diğer tüm alanlar aynı → aynı digest.
        use crate::coords::{DerivedPosition, Position, RawPosition};
        use crate::space::{Node, NodeClassification, NodeKind, NodeRole, Space};
        let mk_space = |x: f64| {
            let mut space = Space::default();
            space.nodes.insert(
                1,
                Node {
                    id: 1,
                    kind: NodeKind::Module,
                    mass: 10.0,
                    position: Position {
                        raw: RawPosition {
                            x,
                            y: 0.0,
                            z: 0.0,
                            w: 0.0,
                            v: 0.0,
                        },
                        derived: DerivedPosition::default(),
                    },
                    cohesion: Some(0.5),
                    classification: NodeClassification::Production,
                    role: NodeRole::Runtime,
                },
            );
            space
        };
        let d1 = SpaceDigest::compute(&mk_space(0.3)).unwrap();
        let d2 = SpaceDigest::compute(&mk_space(0.9)).unwrap();
        assert_eq!(
            d1, d2,
            "position is engine-derived and must NOT affect space digest"
        );
    }

    #[test]
    fn space_digest_changes_when_node_kind_changes() {
        use crate::coords::{DerivedPosition, Position, RawPosition};
        use crate::space::{Node, NodeClassification, NodeKind, NodeRole, Space};
        let mk_space = |kind: NodeKind| {
            let mut space = Space::default();
            space.nodes.insert(
                1,
                Node {
                    id: 1,
                    kind,
                    mass: 10.0,
                    position: Position {
                        raw: RawPosition::default(),
                        derived: DerivedPosition::default(),
                    },
                    cohesion: Some(0.5),
                    classification: NodeClassification::Production,
                    role: NodeRole::Runtime,
                },
            );
            space
        };
        let d1 = SpaceDigest::compute(&mk_space(NodeKind::Module)).unwrap();
        let d2 = SpaceDigest::compute(&mk_space(NodeKind::Concept)).unwrap();
        assert_ne!(d1, d2, "different node kind → different digest");
    }

    #[test]
    fn evaluation_context_digest_is_stable_for_identical_context() {
        // **reviewer P0-3 (A8):** EvaluationContextDigest gerçek içerik.
        use crate::coords::RawPosition;
        let config = crate::engine::EngineConfig {
            min_approvers: 2,
            quorum_threshold: 1.5,
            theta_bound: 0.3,
            milestone_interval: 1000,
            abstractness: 0.5,
            merge_ratio_observable: 0.1,
            role_overrides: std::collections::HashMap::new(),
        };
        let rules = vec![RuleDescriptor {
            rule_id: "structural.no_self_import".to_string(),
            semantics_version: 1,
            canonical_parameters: vec![],
        }];
        let vision = RawPosition {
            x: 0.5,
            y: 0.6,
            z: 0.4,
            w: 0.5,
            v: 0.3,
        };
        let d1 = EvaluationContextDigest::compute(&config, &rules, &vision).unwrap();
        let d2 = EvaluationContextDigest::compute(&config, &rules, &vision).unwrap();
        assert_eq!(d1, d2);
    }

    #[test]
    fn evaluation_context_digest_changes_when_theta_bound_changes() {
        use crate::coords::RawPosition;
        let mk = |theta: f64| {
            let config = crate::engine::EngineConfig {
                min_approvers: 2,
                quorum_threshold: 1.5,
                theta_bound: theta,
                milestone_interval: 1000,
                abstractness: 0.5,
                merge_ratio_observable: 0.1,
                role_overrides: std::collections::HashMap::new(),
            };
            EvaluationContextDigest::compute(&config, &[], &RawPosition::default()).unwrap()
        };
        assert_ne!(mk(0.3), mk(0.5));
    }

    #[test]
    fn evaluation_context_digest_changes_when_rule_added() {
        use crate::coords::RawPosition;
        let config = crate::engine::EngineConfig {
            min_approvers: 2,
            quorum_threshold: 1.5,
            theta_bound: 0.3,
            milestone_interval: 1000,
            abstractness: 0.5,
            merge_ratio_observable: 0.1,
            role_overrides: std::collections::HashMap::new(),
        };
        let d_no_rules =
            EvaluationContextDigest::compute(&config, &[], &RawPosition::default()).unwrap();
        let d_one_rule = EvaluationContextDigest::compute(
            &config,
            &[RuleDescriptor {
                rule_id: "test.rule".to_string(),
                semantics_version: 1,
                canonical_parameters: vec![],
            }],
            &RawPosition::default(),
        )
        .unwrap();
        assert_ne!(d_no_rules, d_one_rule);
    }

    #[test]
    fn evaluation_context_digest_changes_when_rule_semantics_version_changes() {
        // **plan-review #4:** semantics_version artarsa digest değişmeli.
        use crate::coords::RawPosition;
        let config = crate::engine::EngineConfig {
            min_approvers: 2,
            quorum_threshold: 1.5,
            theta_bound: 0.3,
            milestone_interval: 1000,
            abstractness: 0.5,
            merge_ratio_observable: 0.1,
            role_overrides: std::collections::HashMap::new(),
        };
        let mk = |semver: u32| {
            EvaluationContextDigest::compute(
                &config,
                &[RuleDescriptor {
                    rule_id: "test.rule".to_string(),
                    semantics_version: semver,
                    canonical_parameters: vec![],
                }],
                &RawPosition::default(),
            )
            .unwrap()
        };
        assert_ne!(mk(1), mk(2));
    }

    #[test]
    fn witness_requirement_derives_from_omega_not_config() {
        // **plan-review #1 (P0):** WitnessRequirement gerçek omega'dan.
        let omega = crate::witness::WitnessSet::new(vec![]).with_quorum(3, 2.0);
        let req = WitnessRequirement::from(&omega);
        assert_eq!(req.min_approvers, 3);
        assert_eq!(req.quorum_threshold, 2.0);
    }

    #[test]
    fn canonical_witness_policy_derives_from_omega_not_config() {
        // **plan-review #1 (P0):** CanonicalWitnessPolicy gerçek omega'dan.
        let omega = crate::witness::WitnessSet::new(vec![]).with_quorum(0, 0.0);
        let policy = CanonicalWitnessPolicy::try_from(&omega).unwrap();
        assert_eq!(policy.min_approvers, 0);
        assert_eq!(policy.quorum_threshold, 0.0);
        // Farklı omega → farklı policy.
        let omega2 = crate::witness::WitnessSet::new(vec![]).with_quorum(5, 3.0);
        let policy2 = CanonicalWitnessPolicy::try_from(&omega2).unwrap();
        assert_ne!(policy.min_approvers, policy2.min_approvers);
    }

    #[test]
    fn ephemeral_identity_with_cross_process_store_fails_closed() {
        // **plan-review #2 (D3):** Ephemeral + CrossProcess → fail-closed.
        // NullPendingAuthorizationStore ProcessLocal döndürür — Ephemeral ile OK.
        let null_store = NullPendingAuthorizationStore;
        assert_eq!(null_store.durability(), SuspensionDurability::ProcessLocal);

        // FilesystemStore CrossProcess döndürür.
        let dir = temp_dir();
        let fs_store = FilesystemPendingAuthorizationStore::new(&dir);
        assert_eq!(fs_store.durability(), SuspensionDurability::CrossProcess);
    }

    #[test]
    fn filesystem_store_durability_is_cross_process() {
        let dir = temp_dir();
        let store = FilesystemPendingAuthorizationStore::new(&dir);
        assert_eq!(store.durability(), SuspensionDurability::CrossProcess);
    }

    #[test]
    fn null_store_durability_is_process_local() {
        let store = NullPendingAuthorizationStore;
        assert_eq!(store.durability(), SuspensionDurability::ProcessLocal);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Adım 3 — MeasurementInputContext version preservation + validation
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn measurement_context_runtime_constructor_uses_current_versions() {
        let ctx = sample_measurement_context();
        assert_eq!(ctx.schema_version(), MEASUREMENT_INPUT_SCHEMA_VERSION);
        assert_eq!(
            ctx.measurement_semantics_version(),
            MEASUREMENT_SEMANTICS_VERSION
        );
        // 5 core axis descriptor.
        assert_eq!(ctx.axis_descriptors().len(), 5);
    }

    #[test]
    fn measurement_context_deserialization_rejects_unknown_schema_version() {
        // Wire format: schema_version=999 → UnsupportedMeasurementInputSchema.
        let ctx = sample_measurement_context();
        let mut json = serde_json::to_value(&ctx).unwrap();
        json["schema_version"] = serde_json::json!(999);
        let json_str = serde_json::to_string(&json).unwrap();
        let err = serde_json::from_str::<MeasurementInputContext>(&json_str).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported measurement input schema"),
            "deserialize must reject unknown schema: {err}"
        );
    }

    #[test]
    fn measurement_context_deserialization_rejects_unknown_semantics_version() {
        let ctx = sample_measurement_context();
        let mut json = serde_json::to_value(&ctx).unwrap();
        json["measurement_semantics_version"] = serde_json::json!(999);
        let json_str = serde_json::to_string(&json).unwrap();
        let err = serde_json::from_str::<MeasurementInputContext>(&json_str).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported measurement semantics"),
            "deserialize must reject unknown semantics: {err}"
        );
    }

    #[test]
    fn measurement_context_defensively_rejects_duplicate_axis_descriptors() {
        // try_new duplicate axis_id reddetmeli (canonical sıralama sonrası windows check).
        use crate::coords::{AxisDescriptor, AxisParameterEncoder};
        let mk = |id: &str| -> AxisDescriptor {
            let mut p = AxisParameterEncoder::new();
            p.push_u8(0);
            AxisDescriptor::try_new(id, 1, p).unwrap()
        };
        let err =
            MeasurementInputContext::try_new(vec![mk("coupling"), mk("coupling")]).unwrap_err();
        assert_eq!(
            err,
            CanonicalizationError::DuplicateIdentifier("coupling".into())
        );
    }

    #[test]
    fn measurement_context_rejects_non_core_axis_descriptor() {
        // **reviewer P1 (core-only invariant):** context yalnız core raw axis descriptor'ları
        // taşır (dokümante invariant). Custom axis "security" reddedilir.
        use crate::coords::{AxisDescriptor, AxisParameterEncoder};
        let mut p = AxisParameterEncoder::new();
        p.push_u8(0);
        let security = AxisDescriptor::try_new("security", 1, p).unwrap();
        let err = MeasurementInputContext::try_new(vec![security]).unwrap_err();
        assert_eq!(
            err,
            CanonicalizationError::UnsupportedMeasurementAxis("security".into())
        );
    }

    #[test]
    fn measurement_context_deserialization_rejects_non_core_axis() {
        // **reviewer P1:** diskten custom axis descriptor yüklenemez — try_from_parts
        // core-only kontrolü custom axis'i reddeder.
        let ctx = sample_measurement_context();
        let mut json = serde_json::to_value(&ctx).unwrap();
        // İlk descriptor'ı custom axis ile değiştir.
        json["axis_descriptors"][0]["axis_id"] = serde_json::json!("security");
        let json_str = serde_json::to_string(&json).unwrap();
        let err = serde_json::from_str::<MeasurementInputContext>(&json_str).unwrap_err();
        assert!(
            err.to_string().contains("unsupported measurement axis"),
            "deserialize must reject non-core axis: {err}"
        );
    }

    #[test]
    fn measurement_context_excludes_repo_level_values() {
        // **Ontolojik ayrım:** context axis tanımlarını taşır, ölçüm değerleri basis'te.
        // Context'te repo_level_entropy/witness_depth field YOK — serialization'da görünmemeli.
        let ctx = sample_measurement_context();
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(
            !json.contains("repo_level_entropy"),
            "context must not carry repo_level values (in basis)"
        );
        assert!(
            !json.contains("repo_level_witness_depth"),
            "context must not carry repo_level values (in basis)"
        );
        assert!(
            !json.contains("metric_source_config"),
            "context must not carry placeholder metric source policy"
        );
    }

    #[test]
    fn measurement_input_digest_reflects_real_coordinate_system() {
        // Gerçek CoordinateSystem'den üretilen context → digest placeholder 0 DEĞİL,
        // gerçek axis descriptor içerikleri. İki farklı coord_system → farklı digest.
        use crate::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
        let cs1 = crate::coords::CoordinateSystem::default_raw_five(
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let cs2 = crate::coords::CoordinateSystem::default_raw_five(
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(9.0), // farklı effective entropy
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let ctx1 = MeasurementInputContext::try_from(&cs1).unwrap();
        let ctx2 = MeasurementInputContext::try_from(&cs2).unwrap();
        let d1 = MeasurementInputDigest::compute(&ctx1).unwrap();
        let d2 = MeasurementInputDigest::compute(&ctx2).unwrap();
        assert_ne!(
            d1, d2,
            "different axis effective state (entropy) must change digest"
        );
    }

    #[test]
    fn measurement_digest_is_independent_of_axis_registration_order_for_raw_mapping() {
        // Aynı axis'ler farklı registration sırasında → aynı descriptor seti (sorted) →
        // aynı digest. Seçenek B: axis order normatif DEĞİL (name-mapped).
        use crate::axes::{CohesionAxis, EntropyAxis, InstabilityAxis, WitnessDepthAxis};
        use crate::coords::CoordinateSystem;
        // Sıra 1: coupling, cohesion, instability, entropy, witness
        let cs1 = CoordinateSystem::empty()
            .try_with_axis(crate::axes::CouplingAxis::new())
            .unwrap()
            .try_with_axis(CohesionAxis::new())
            .unwrap()
            .try_with_axis(InstabilityAxis::new())
            .unwrap()
            .try_with_axis(EntropyAxis::from_commit_entropy(6.0))
            .unwrap()
            .try_with_axis(WitnessDepthAxis::from_witness(0.3, 5))
            .unwrap();
        // Sıra 2: ters
        let cs2 = CoordinateSystem::empty()
            .try_with_axis(WitnessDepthAxis::from_witness(0.3, 5))
            .unwrap()
            .try_with_axis(EntropyAxis::from_commit_entropy(6.0))
            .unwrap()
            .try_with_axis(InstabilityAxis::new())
            .unwrap()
            .try_with_axis(CohesionAxis::new())
            .unwrap()
            .try_with_axis(crate::axes::CouplingAxis::new())
            .unwrap();
        let d1 = MeasurementInputDigest::compute(&MeasurementInputContext::try_from(&cs1).unwrap())
            .unwrap();
        let d2 = MeasurementInputDigest::compute(&MeasurementInputContext::try_from(&cs2).unwrap())
            .unwrap();
        assert_eq!(
            d1, d2,
            "registration order must not affect digest (name-mapped, sorted descriptors)"
        );
    }
}
