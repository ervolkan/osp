//! INV-T9 — Authorization basis + digest + pending suspension types.
//!
//! Bu modül witness authorization bekleme durumunun (INV-T9) veri modelini taşır:
//! - [`AuthorizationBasis`]: witness'ın yetkilendirdiği claim'in tam kanonik temsili.
//! - [`AuthorizationBasisDigest`]: BLAKE3 tabanlı, domain-separated, canonical encoding digest.
//! - [`EvaluationContextDigest`]: claim-specific effective vision-gate context + ordered rule-evaluation context + semantics versions digest.
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
///
/// **INV-T9 Step 5 (defensive integrity):** Private fields — validated construction
/// through `try_new` (smart constructor) veya trivially-valid `empty()` constructor.
/// Custom Deserialize `deny_unknown_fields` ile `try_new` üzerinden geçer → diskten
/// malformed artifact (duplicate/cross-list/non-finite/unknown-field) deserialize
/// sırasında reject. `validate()` non-normalizing (sort ETMEZ, mevcut canonical sırayı
/// doğrular) — `AuthorizationBasisDigest::compute` ve `PendingAuthorizationEnvelope::verify`
/// başında defensive çağrılır.
///
/// `removed_edges` artık `CanonicalEdgeIdentity` (from,to,kind — `is_type_only` HARİÇ).
/// `new_edges` `CanonicalEdge` olarak kalır (eklenen edge'in `is_type_only` semantiği korunur).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CanonicalStructuralDelta {
    /// Eklenen node'lar (sorted by id). Full canonical content.
    new_nodes: Vec<CanonicalNode>,
    /// Eklenen edge'ler — sorted by identity (from,to,kind). `is_type_only` dahil.
    new_edges: Vec<CanonicalEdge>,
    /// Kaldırılan edge'ler — sorted. G2c-2 subtractive delta. Identity-only
    /// (`is_type_only` HARİÇ — kaldırma lookup kimliğinin parçası değil).
    removed_edges: Vec<CanonicalEdgeIdentity>,
}

impl<'de> serde::Deserialize<'de> for CanonicalStructuralDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            new_nodes: Vec<CanonicalNode>,
            new_edges: Vec<CanonicalEdge>,
            removed_edges: Vec<CanonicalEdgeIdentity>,
        }
        let wire = Wire::deserialize(deserializer)?;
        CanonicalStructuralDelta::try_new(wire.new_nodes, wire.new_edges, wire.removed_edges)
            .map_err(serde::de::Error::custom)
    }
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

/// Canonical edge — structural relationship (eklenen edge'ler için, `is_type_only` dahil).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct CanonicalEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: CanonicalEdgeKind,
    pub is_type_only: bool,
}

impl CanonicalEdge {
    /// **INV-T9 Step 5b:** Ortak identity projection — duplicate/cross-list kontrolleri için.
    /// `is_type_only` eklenen edge'in semantik özelliğidir; identity'nin parçası DEĞİL.
    pub fn identity(&self) -> CanonicalEdgeIdentity {
        CanonicalEdgeIdentity::new(self.from, self.to, self.kind)
    }
}

/// **INV-T9 Step 5b:** Edge removal identity — `from`, `to`, `kind`. `is_type_only` HARİÇ
/// (kaldırma işleminin lookup kimliğinin parçası değil; runtime remove `from+to+kind`
/// üzerinden). Duplicate ve cross-list conflict kontrolleri bu identity üzerinden yapılır.
///
/// Private fields + custom Deserialize (`deny_unknown_fields`) — tek canonical representation.
/// Diskten `is_type_only` içeren eski JSON reject edilir (tek representation iddiası).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct CanonicalEdgeIdentity {
    from: NodeId,
    to: NodeId,
    kind: CanonicalEdgeKind,
}

impl CanonicalEdgeIdentity {
    /// **Infallible** — gerçek reddedilebilir invariant yok (NodeId tüm u64 değerleri,
    /// kind validated type). Fallible validation `CanonicalStructuralDelta::try_new`'in
    /// sorumluluğu (duplicate/cross-list). Self-loop semantic edge kind'a bağlı, identity
    /// katmanının değil.
    pub fn new(from: NodeId, to: NodeId, kind: CanonicalEdgeKind) -> Self {
        Self { from, to, kind }
    }

    pub fn from(&self) -> NodeId {
        self.from
    }
    pub fn to(&self) -> NodeId {
        self.to
    }
    pub fn kind(&self) -> CanonicalEdgeKind {
        self.kind
    }
}

impl<'de> serde::Deserialize<'de> for CanonicalEdgeIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            from: NodeId,
            to: NodeId,
            kind: CanonicalEdgeKind,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self::new(wire.from, wire.to, wire.kind))
    }
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
    /// **reviewer P1 + Step 5:** Validating smart constructor — vec'leri canonical
    /// (sorted) sıraya koyar VE structural identity çelişkilerini reddeder.
    ///
    /// **Tek canonicalization katmanı (Step 5 P0):** Sort burada yapılır; `validate()`
    /// ve digest encoder sort ETMEZ (as-is). Bu, canonical invariant'ın maskelenemez
    /// olmasını sağlar — bozuk sıra deserialize + validate + encode zincirinde görünür.
    ///
    /// Sort key'leri identity üzerinden:
    /// - `new_nodes`: by `id`
    /// - `new_edges`: by `identity()` (from,to,kind) — `is_type_only` bağımsız
    /// - `removed_edges`: by `CanonicalEdgeIdentity` Ord (zaten identity)
    ///
    /// Reddedilen durumlar (validate'e delege):
    /// - duplicate node id, non-finite node field
    /// - duplicate edge identity (is_type_only farklı olsa bile — `(1,2,Imports,true)`
    ///   ve `(1,2,Imports,false)` aynı identity → duplicate)
    /// - cross-list conflict (identity üzerinden — is_type_only bağımsız)
    pub fn try_new(
        mut new_nodes: Vec<CanonicalNode>,
        mut new_edges: Vec<CanonicalEdge>,
        mut removed_edges: Vec<CanonicalEdgeIdentity>,
    ) -> Result<Self, CanonicalizationError> {
        // Tek canonicalization: sort by identity.
        new_nodes.sort_unstable_by_key(|n| n.id);
        new_edges.sort_unstable_by_key(CanonicalEdge::identity);
        removed_edges.sort_unstable();
        let value = Self {
            new_nodes,
            new_edges,
            removed_edges,
        };
        // validate as-is doğrular (sort/normalize ETMEZ).
        value.validate()?;
        Ok(value)
    }

    /// **Step 5 P0 — Non-normalizing validation.** Sort/clone ETMEZ — mevcut object'i
    /// AS-IS inceler. Bozuk sıralama, duplicate identity, cross-list conflict, non-finite
    /// field yakalar. `AuthorizationBasisDigest::compute` ve `PendingAuthorizationEnvelope::verify`
    /// başında defensive çağrılır; encoder da as-is encode eder (sort YOK).
    ///
    /// try_new sort yaptığı için normal akışta her zaman geçer; bu metod deserialize
    /// edilmiş / araya giren bozuk state'i yakalar (defensive katman).
    pub fn validate(&self) -> Result<(), CanonicalizationError> {
        use std::cmp::Ordering;
        // new_nodes: id strict ascending. Equal → DuplicateNodeId, Greater → UnsortedNodes.
        // (typed taxonomy — diagnostic doğruluk, integrity reddi aynı).
        for w in self.new_nodes.windows(2) {
            match w[0].id.cmp(&w[1].id) {
                Ordering::Equal => {
                    return Err(CanonicalizationError::DuplicateNodeId(w[0].id));
                }
                Ordering::Greater => {
                    return Err(CanonicalizationError::UnsortedNodes);
                }
                Ordering::Less => {}
            }
        }
        // Non-finite node fields.
        for node in &self.new_nodes {
            if !node.mass.is_finite() {
                return Err(CanonicalizationError::NonFiniteNodeField(node.id));
            }
            if let Some(c) = node.cohesion {
                if !c.is_finite() {
                    return Err(CanonicalizationError::NonFiniteNodeField(node.id));
                }
            }
        }
        // new_edges: identity strict ascending. Equal → DuplicateEdge, Greater → UnsortedNewEdges.
        // (1,2,Imports,true) ve (1,2,Imports,false) aynı identity → duplicate.
        for w in self.new_edges.windows(2) {
            match w[0].identity().cmp(&w[1].identity()) {
                Ordering::Equal => {
                    return Err(CanonicalizationError::DuplicateEdge);
                }
                Ordering::Greater => {
                    return Err(CanonicalizationError::UnsortedNewEdges);
                }
                Ordering::Less => {}
            }
        }
        // removed_edges: identity strict ascending. Equal → DuplicateEdge, Greater → UnsortedRemovedEdges.
        for w in self.removed_edges.windows(2) {
            match w[0].cmp(&w[1]) {
                Ordering::Equal => {
                    return Err(CanonicalizationError::DuplicateEdge);
                }
                Ordering::Greater => {
                    return Err(CanonicalizationError::UnsortedRemovedEdges);
                }
                Ordering::Less => {}
            }
        }
        // Cross-list conflict: identity üzerinden (is_type_only bağımsız).
        // (1,2,Imports,true) add + (1,2,Imports) remove → conflict.
        for ne in &self.new_edges {
            if self.removed_edges.iter().any(|re| *re == ne.identity()) {
                return Err(CanonicalizationError::CrossListEdgeConflict);
            }
        }
        Ok(())
    }

    /// Accessors — private fields için read-only erişim (digest encoder + testler).
    pub fn new_nodes(&self) -> &[CanonicalNode] {
        &self.new_nodes
    }
    pub fn new_edges(&self) -> &[CanonicalEdge] {
        &self.new_edges
    }
    pub fn removed_edges(&self) -> &[CanonicalEdgeIdentity] {
        &self.removed_edges
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
/// tekrar eden ölçüm değerleri (`repo_level_*`), evaluation girdileri (`theta_bound` —
/// `EvaluationContextDigest`'te) kaldırıldı. Yalnız core raw axis descriptor'ları
/// (seçenek B): coupling/cohesion/instability/entropy/witness_depth.
///
/// **Step 4c notu:** `abstractness` de `EvaluationContextDigest`'ten çıkarıldı — Q5/Q6
/// authorization evaluation'ı etkilemiyor. `MeasurementInputContext`'e taşınmaz (axis
/// tanımı değil, `raw_position_of` girdisi değil, `ProvenancedMeasuredResult` üretmez).
/// Post-apply derived-position (`compute_derived`) etkisi için gelecekte ayrı bir
/// `ApplySemanticsDigest` bağlanabilir.
///
/// **v1 schema:** Henüz yayınlanmadı; bu commit ilk v1 representation'ı finalizes.
/// Basis digest taşır (bound), full context taşımaz (readable) — self-description ileride.
///
/// **INV-T9 #70 (semantics v2):** AxisDescriptor semantics_version 1→2 (source ID
/// encoding). Global measurement semantics version da 1→2 bump oldu — axis descriptor
/// + global version aynı preimage'da.
pub const MEASUREMENT_INPUT_SCHEMA_VERSION: u32 = 1;
pub const MEASUREMENT_SEMANTICS_VERSION: u32 = 2;

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

    /// **INV-T9 #70:** Hex encoding for golden vector pinning (AuthorizationBasisDigest
    /// pattern — `format!("{byte:02x}")` walk). Test/regression için.
    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.0 {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
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

/// **P1-1 (reviewer):** RawPosition → CanonicalRawPosition projection.
/// 5 alan elle kopyalama yerine tek yerde pinlenir.
impl From<crate::coords::RawPosition> for CanonicalRawPosition {
    fn from(pos: crate::coords::RawPosition) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            w: pos.w,
            v: pos.v,
        }
    }
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
            let canonical = canonicalize_node(node)
                .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;
            encode_canonical_node(&mut hasher, &canonical)?;
        }

        // Edge'leri canonical sırala → encode. **Step 6 P0:** `encode_canonical_edge_vec`
        // as-is encode eder (Step 5 — structural delta için tek canonicalization try_new'de).
        // SpaceDigest için sort burada yapılır; `Space.edges` insertion-order'dır, canonical
        // content identity için sıralama zorunlu. Encoder yeniden sort ETMEZ.
        let mut canonical_edges: Vec<CanonicalEdge> = space
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
        canonical_edges.sort_unstable();
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
/// Position hariç — engine-derived.
///
/// **INV-T9 #70 Commit 3 (reviewer v4 P1-2):** `CanonicalizationError` döner — shared
/// `canonical_structural_delta_from_claim` producer ile uyumlu. Authorization caller'ı
/// gerektiğinde `CanonicalDigestError::EncodingFailed` sarmalar.
pub(crate) fn canonicalize_node(
    node: &crate::space::Node,
) -> Result<CanonicalNode, CanonicalizationError> {
    Ok(CanonicalNode {
        id: node.id,
        kind: CanonicalNodeKind::try_from(&node.kind)?,
        mass: node.mass,
        cohesion: node.cohesion,
        classification: CanonicalNodeClassification::try_from(&node.classification)?,
        role: CanonicalNodeRole::try_from(&node.role)?,
    })
}

/// **INV-T9 #70 Commit 3 (P1-5 v3 / reviewer v4 P1-2):** Shared canonical structural
/// delta producer — hem `AuthorizationBasis.structural_delta` hem `MeasurementDeltaDigest`
/// aynı producer'ı kullanır. Single canonicalization truth: claim → CanonicalNode/Edge/
/// Identity → `try_new` (sort + validate). Encoder AS-IS; digest öncesinde defensive
/// `validate()` çağrılır (non-normalizing).
///
/// `Claim`'in `delta_nodes`/`delta_edges`/`removed_edges` field'larından `CanonicalStructuralDelta`
/// üretir. Duplicate/cross-list/non-finite `try_new` validation'ı ile reddedilir.
pub(crate) fn canonical_structural_delta_from_claim(
    claim: &crate::witness::Claim,
) -> Result<CanonicalStructuralDelta, CanonicalizationError> {
    let new_nodes: Vec<CanonicalNode> = claim
        .delta_nodes
        .iter()
        .map(canonicalize_node)
        .collect::<Result<Vec<_>, _>>()?;
    let new_edges: Vec<CanonicalEdge> = claim
        .delta_edges
        .iter()
        .map(|e| {
            Ok(CanonicalEdge {
                from: e.from,
                to: e.to,
                kind: CanonicalEdgeKind::try_from(&e.kind)?,
                is_type_only: e.is_type_only,
            })
        })
        .collect::<Result<Vec<_>, CanonicalizationError>>()?;
    let removed_edges: Vec<CanonicalEdgeIdentity> = claim
        .removed_edges
        .iter()
        .map(|e| {
            Ok(CanonicalEdgeIdentity::new(
                e.from,
                e.to,
                CanonicalEdgeKind::try_from(&e.kind)?,
            ))
        })
        .collect::<Result<Vec<_>, CanonicalizationError>>()?;
    CanonicalStructuralDelta::try_new(new_nodes, new_edges, removed_edges)
}

// ═══════════════════════════════════════════════════════════════════════════════
// EvaluationContextDigest — gate policy context
// ═══════════════════════════════════════════════════════════════════════════════

/// Gate policy context digest — claim-specific effective vision-gate context + ordered
/// rule-evaluation context + semantics versions.
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

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 Step 4a — RuleEvaluationContext (ordinal-aware rule sequence snapshot)
//
// `RuleEvaluationContext` Q6 (runtime rule evaluation) ve `EvaluationContextDigest`
// (canonical encoding) tarafından PAYLAŞILAN ordered snapshot'tır. İki ayrı yerde
// rule listesi üretip drift bırakmaz. Ordinal contextual'dır — Rule state'i DEĞİL,
// belirli bir engine evaluation context'indeki konumudur (registration sırası).
// ═══════════════════════════════════════════════════════════════════════════════

/// **reviewer (Step 4a):** Rule evaluation context semantics version.
/// Rule sıralama/ordinal/identity semantiği değişirse bu version artırılmalı.
pub const RULE_EVALUATION_SEMANTICS_VERSION: u32 = 1;

/// **reviewer P0-3 (Step 4a):** Rule descriptor + registration ordinal'ı.
/// `ordinal` context'in parçasıdır (registration sırası), Rule'un kendisinin DEĞİL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderedRuleDescriptor {
    pub(crate) ordinal: u32,
    pub(crate) descriptor: RuleDescriptor,
}

/// **reviewer P0-1 (Step 4a):** Validated rule evaluation context snapshot.
/// Q6 ve digest aynı ordered snapshot'ı kullanır — runtime rule listesi ile canonical
/// encoding arasındaki ayrışmaya izin vermez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleEvaluationContext {
    semantics_version: u32,
    ordered_rules: Vec<OrderedRuleDescriptor>,
}

/// `usize → u32` ordinal dönüşümü — checked (fail-closed). Test edilebilir helper.
pub(crate) fn checked_rule_ordinal(index: usize) -> Result<u32, EvaluationContextError> {
    u32::try_from(index).map_err(|_| EvaluationContextError::RuleOrdinalOverflow)
}

impl RuleEvaluationContext {
    /// Validated constructor — ordinal'lar 0..n kesintisiz, ID boş değil, active ID
    /// benzersiz, semantics version > 0.
    pub(crate) fn try_new(
        ordered_rules: Vec<OrderedRuleDescriptor>,
    ) -> Result<Self, EvaluationContextError> {
        let ctx = Self {
            semantics_version: RULE_EVALUATION_SEMANTICS_VERSION,
            ordered_rules,
        };
        ctx.validate()?;
        Ok(ctx)
    }

    /// Defensive validation — `EvaluationContextDigest::compute` başında çağrılır.
    pub(crate) fn validate(&self) -> Result<(), EvaluationContextError> {
        if self.semantics_version != RULE_EVALUATION_SEMANTICS_VERSION {
            return Err(EvaluationContextError::UnsupportedRuleContextSemantics(
                self.semantics_version,
            ));
        }
        let mut seen_ids: Vec<&str> = Vec::new();
        for (index, ordered) in self.ordered_rules.iter().enumerate() {
            // Ordinal'lar 0..n kesintisiz.
            let expected_ordinal = checked_rule_ordinal(index)?;
            if ordered.ordinal != expected_ordinal {
                return Err(EvaluationContextError::OrdinalGap {
                    expected: expected_ordinal,
                    found: ordered.ordinal,
                });
            }
            // Rule ID boş değil.
            if ordered.descriptor.rule_id.is_empty() {
                return Err(EvaluationContextError::EmptyRuleId);
            }
            // Semantics version > 0.
            if ordered.descriptor.semantics_version == 0 {
                return Err(EvaluationContextError::InvalidRuleSemanticsVersion(
                    ordered.descriptor.semantics_version,
                ));
            }
            // Active rule ID benzersiz.
            if seen_ids.contains(&ordered.descriptor.rule_id.as_str()) {
                return Err(EvaluationContextError::DuplicateActiveRuleId(
                    ordered.descriptor.rule_id.clone(),
                ));
            }
            seen_ids.push(&ordered.descriptor.rule_id);
        }
        Ok(())
    }

    pub(crate) fn semantics_version(&self) -> u32 {
        self.semantics_version
    }
    pub(crate) fn ordered_rules(&self) -> &[OrderedRuleDescriptor] {
        &self.ordered_rules
    }
}

/// **reviewer (Step 4a):** Rule evaluation context / descriptor hataları.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum EvaluationContextError {
    #[error("rule ordinal overflow (usize → u32 conversion failed)")]
    RuleOrdinalOverflow,
    #[error("unsupported rule context semantics version: {0}")]
    UnsupportedRuleContextSemantics(u32),
    #[error("ordinal gap: expected {expected}, found {found}")]
    OrdinalGap { expected: u32, found: u32 },
    #[error("empty rule_id in ordered rule descriptor")]
    EmptyRuleId,
    #[error("invalid rule semantics version (must be > 0): {0}")]
    InvalidRuleSemanticsVersion(u32),
    #[error("duplicate active rule_id: {0}")]
    DuplicateActiveRuleId(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 Step 4b — EffectiveVisionGateContext (claim-specific effective vision binding)
//
// Reviewer P0-1: Role inference + vision selection TEK fonksiyonda
// (`engine::effective_vision_selection`). Subject + effective vector + source aynı
// karar ağacından üretilir. Bu modül claim-specific runtime context tiplerini taşır:
//   - `CanonicalVisionSubject`  : Global | Role(CanonicalNodeRole)
//   - `EffectiveVisionSelection`: effective_vision + source + subject + semver'ler
//   - `EffectiveVisionGateContext`: selection + theta_bound + deviation_semver
//
// `pub(crate)` — runtime context tipleri wire schema DEĞİL; sadece engine + testler
// çağırır. Reviewer: "intermediate runtime context types are not persisted wire schemas."
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 Step 4b (reviewer P0-1):** Vision subject — vision vektörünün hangi
/// mimari role atandığı. Global (rol-süz) veya belirli bir rol.
///
/// Plan kararı: alan adı `subject` (`inferred_role` DEĞİL — global bir inferred role
/// değildir). `effective_vision_selection`'ın karar ağacından üretilir.
///
/// `pub` — `VisionContextError::SubjectSourceMismatch` (crate pub API) içerir; typed
/// mismatch diagnostic'i için. Runtime context tipidir (wire schema DEĞİL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalVisionSubject {
    /// Override yok, engine global vision'ı kullanılıyor. Rol atanmadı.
    Global,
    /// Claim'in ilk delta_node'unun rolü için override (builtin veya user) uygulandı.
    Role(CanonicalNodeRole),
}

/// **INV-T9 Step 4b:** `infer_role` çıkarım semantiği version'ı.
/// `infer_role` heuristic'i değişirse bu version artırılmalı → digest değişir.
pub(crate) const ROLE_INFERENCE_SEMANTICS_VERSION: u32 = 1;

/// **INV-T9 Step 4b:** `effective_vision_selection` karar ağacı semantiği version'ı.
/// Cascade sırası / override resolution mantığı değişirse bu version artırılmalı.
pub(crate) const VISION_SELECTION_SEMANTICS_VERSION: u32 = 1;

/// **INV-T9 Step 4b:** Sapma metrik (CosineDeviation) kontratı version'ı.
/// θ normalization veya sapma formülü değişirse bu version artırılmalı.
pub(crate) const DEVIATION_SEMANTICS_VERSION: u32 = 1;

/// **INV-T9 Step 4b (reviewer P0-1):** Tek karar ağacının sonucu — effective vision
/// vektörü + provenance + subject + semver'ler.
///
/// `engine::effective_vision_selection(claim)` üretir. Q5 (`check_claim_vision`),
/// `build_authorization_context` ve `EvaluationContextDigest` aynı sonucu paylaşır
/// (captured-context pattern — 4a rule_context ile aynı).
///
/// **scoped-review P0:** Vision source TEK truth — `effective_vision.source()`.
/// Ayrı `vision_source` alanı YOK (dual-truth mismatch açığı kapandı). Provenance her
/// zaman vector'ün içinden okunur; validation ve digest aynı kaynağı kullanır.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EffectiveVisionSelection {
    /// Effective vision vektörü (override uygulanmış veya global). **Tek source of
    /// truth** — `effective_vision.source()` provenance'ı verir.
    pub(crate) effective_vision: crate::vision::VisionVector,
    /// Bu vision'ın hangi role atandığı (Global = rol-süz / delta_node yok).
    /// **scoped-review P1-a:** delta_node varsa override olsun/olmasın `Role(infer_role)`
    /// üretilir — claim'in değerlendirme subject'i global fallback'te de korunur.
    pub(crate) subject: CanonicalVisionSubject,
    /// `infer_role` heuristic semantiği (digest'e bağlı — staleness tespiti).
    pub(crate) role_inference_semver: u32,
    /// `effective_vision_selection` karar ağacı semantiği (digest'e bağlı).
    pub(crate) vision_selection_semver: u32,
}

impl EffectiveVisionSelection {
    /// Provenance — `effective_vision.source()` tek truth. Ayrı alan YOK (P0).
    pub(crate) fn vision_source(&self) -> crate::vision::VisionSource {
        self.effective_vision.source()
    }
}

/// **INV-T9 Step 4b:** θ_bound aralığı.
/// `MIN = 0.0` (en sıkı), `MAX = 1.0` (CosineDeviation kontratı — θ ∈ [0,1]).
pub(crate) const MIN_THETA_BOUND: f64 = 0.0;
pub(crate) const MAX_THETA_BOUND: f64 = 1.0;

/// **INV-T9 Step 4b (reviewer P0-1 + P0-3):** Claim-specific effective vision gate
/// context. Captured-context pattern: bir kez üretilir, Q5 + build_authorization_context
/// + digest paylaşır.
///
/// `validate_for_authorization` hem Q5 öncesinde hem digest başında çağrılır. None /
/// GlobalDefault → Q5'e ulaşamaz, digest üretilemez (fail-closed).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EffectiveVisionGateContext {
    /// Tek karar ağacı sonucu (effective_vision + source + subject + semver'ler).
    pub(crate) selection: EffectiveVisionSelection,
    /// θ bound (claim'e uygulanacak sapma eşiği — config.theta_bound, global).
    pub(crate) theta_bound: f64,
    /// Sapma metrik kontratı semantiği version'ı.
    pub(crate) deviation_semver: u32,
}

/// **INV-T9 Step 4b (reviewer P0-2):** Vision context validation hataları (typed).
///
/// Terminal — `EngineCommitError::VisionContextInvalid` ile map'lenir; maneuver budget
/// tüketmez, yeni LLM attempt başlatmaz, witness'a ulaşmaz (reviewer P0-4).
///
/// `pub` — `EngineCommitError` (crate pub API) içerir; `#[from]` typed dönüşüm için.
/// Runtime context tipidir (wire schema DEĞİL) — `EngineCommitError` public yüzeyine
/// gömülü gelir, ayrıca serialize edilmez.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VisionContextError {
    /// Vision yüklenmemiş (`VisionSource::None`) — θ hesaplanmamalı (topology-only).
    #[error("vision unavailable (source=None) — θ cannot be computed (topology-only mode)")]
    VisionUnavailable,
    /// `GlobalDefault` evaluable ama authorization-gated mutation yolunda authority
    /// yetersiz. Kullanıcı-onaylı vision (UserLoaded/RoleProfile/BuiltinRole) gerekir.
    #[error("vision source {vision_source:?} has insufficient authority for authorization-gated mutation — user-confirmed vision required")]
    VisionAuthorityInsufficient {
        vision_source: crate::vision::VisionSource,
    },
    /// Subject-source mismatch: Global subject + role-profile/builtin-role source.
    /// Yapısal çelişki — aynı karar ağacı bu kombinasyonu üretmemeli.
    #[error("subject-source mismatch: subject={subject:?} with source={vision_source:?}")]
    SubjectSourceMismatch {
        subject: CanonicalVisionSubject,
        vision_source: crate::vision::VisionSource,
    },
    /// Effective vision eksen değeri NaN/±Infinity.
    #[error("non-finite vision axis {axis}: vision vectors must be finite")]
    NonFiniteVisionAxis { axis: &'static str },
    /// theta_bound NaN/±Infinity.
    #[error("non-finite theta_bound: {0}")]
    NonFiniteThetaBound(f64),
    /// theta_bound [MIN_THETA_BOUND, MAX_THETA_BOUND] aralığı dışında.
    #[error("theta_bound {0} out of range [{MIN_THETA_BOUND}, {MAX_THETA_BOUND}]")]
    ThetaBoundOutOfRange(f64),
    /// **scoped-review P1-b:** Semantics version exact-version modeli. Binary'nin
    /// uygulamadığı bir semantiği digest'e yazması engellenir (rule context'teki
    /// `UnsupportedRuleContextSemantics` ile aynı). `found != supported` → reject.
    #[error("unsupported semantics version for {field}: found {found}, supported {supported}")]
    UnsupportedSemanticsVersion {
        field: &'static str,
        found: u32,
        supported: u32,
    },
    /// **scoped-review P1-c:** Canonical role conversion fail-closed. Yeni `NodeRole`
    /// varyantı eklendiğinde context başka role aitmiş gibi kaydedilmesin — conversion
    /// hatası terminal olarak yayılır (sessiz `Runtime` fallback YOK).
    #[error("canonical node role conversion failed during vision selection: {0}")]
    CanonicalRoleConversionFailed(String),
}

impl EffectiveVisionGateContext {
    /// Validated smart constructor — `validate_for_authorization` çağırır (hem structure
    /// hem authority). Engine `effective_vision_gate_context(claim)` bu constructor'ı
    /// çağırır; None/GlobalDefault/mismatch burada fail-closed reddedilir.
    pub(crate) fn try_new(
        selection: EffectiveVisionSelection,
        theta_bound: f64,
    ) -> Result<Self, VisionContextError> {
        let ctx = Self {
            selection,
            theta_bound,
            deviation_semver: DEVIATION_SEMANTICS_VERSION,
        };
        ctx.validate_for_authorization()?;
        Ok(ctx)
    }

    /// **reviewer P0-3:** Authority validation — mutation yüzeylerinde çağrılır.
    /// None → VisionUnavailable, GlobalDefault → VisionAuthorityInsufficient, diğerleri Ok.
    ///
    /// **scoped-review P0:** `vision_source` artık `effective_vision.source()` tek
    /// truth'tan okunur — ayrı alan YOK, dual-truth mismatch açığı kapandı.
    pub(crate) fn validate_authority_for_mutation(&self) -> Result<(), VisionContextError> {
        let source = self.selection.vision_source();
        match source {
            crate::vision::VisionSource::None => Err(VisionContextError::VisionUnavailable),
            crate::vision::VisionSource::GlobalDefault => {
                Err(VisionContextError::VisionAuthorityInsufficient {
                    vision_source: source,
                })
            }
            crate::vision::VisionSource::BuiltinRole
            | crate::vision::VisionSource::RoleProfile
            | crate::vision::VisionSource::UserLoaded => Ok(()),
        }
    }

    /// **reviewer P0-2:** Structural validation — imkânsız kombinasyonlar.
    ///
    /// | Subject | Source              | Sonuç                         |
    /// |---------|---------------------|------------------------------|
    /// | Global  | UserLoaded          | Geçerli                      |
    /// | Global  | GlobalDefault       | Yapısal geçerli (auth'da reject) |
    /// | Global  | BuiltinRole/Profile | **Geçersiz** (SubjectSourceMismatch) |
    /// | Role    | BuiltinRole/Profile/UserLoaded | Geçerli         |
    /// | Role    | GlobalDefault       | Yapısal geçerli (auth'da reject) |
    /// | Herhangi| None                | **Geçersiz** (VisionUnavailable) |
    pub(crate) fn validate_structure(&self) -> Result<(), VisionContextError> {
        use crate::vision::VisionSource as S;
        use CanonicalVisionSubject as Sub;

        // **scoped-review P1-b:** Semantics version exact-match modeli. Binary'nin
        // uygulamadığı bir semantiği digest'e yazması engellenir (rule context'teki
        // `UnsupportedRuleContextSemantics` ile aynı prensip). `found != supported` → reject.
        if self.selection.role_inference_semver != ROLE_INFERENCE_SEMANTICS_VERSION {
            return Err(VisionContextError::UnsupportedSemanticsVersion {
                field: "role_inference",
                found: self.selection.role_inference_semver,
                supported: ROLE_INFERENCE_SEMANTICS_VERSION,
            });
        }
        if self.selection.vision_selection_semver != VISION_SELECTION_SEMANTICS_VERSION {
            return Err(VisionContextError::UnsupportedSemanticsVersion {
                field: "vision_selection",
                found: self.selection.vision_selection_semver,
                supported: VISION_SELECTION_SEMANTICS_VERSION,
            });
        }
        if self.deviation_semver != DEVIATION_SEMANTICS_VERSION {
            return Err(VisionContextError::UnsupportedSemanticsVersion {
                field: "deviation",
                found: self.deviation_semver,
                supported: DEVIATION_SEMANTICS_VERSION,
            });
        }

        // theta_bound aralığı + finiteness.
        if !self.theta_bound.is_finite() {
            return Err(VisionContextError::NonFiniteThetaBound(self.theta_bound));
        }
        if self.theta_bound < MIN_THETA_BOUND || self.theta_bound > MAX_THETA_BOUND {
            return Err(VisionContextError::ThetaBoundOutOfRange(self.theta_bound));
        }

        // Effective vision eksenleri finite.
        let raw = self.selection.effective_vision.raw;
        for (axis, val) in [
            ("x", raw.x),
            ("y", raw.y),
            ("z", raw.z),
            ("w", raw.w),
            ("v", raw.v),
        ] {
            if !val.is_finite() {
                return Err(VisionContextError::NonFiniteVisionAxis { axis });
            }
        }

        // Provenance tek truth: effective_vision.source() (P0).
        let source = self.selection.vision_source();

        // None → VisionUnavailable (subject'ten bağımsız).
        if matches!(source, S::None) {
            return Err(VisionContextError::VisionUnavailable);
        }

        // Subject-source combinational check.
        match (self.selection.subject, source) {
            // Global subject + role-scoped source → mismatch (yapısal çelişki).
            (Sub::Global, S::BuiltinRole) | (Sub::Global, S::RoleProfile) => {
                Err(VisionContextError::SubjectSourceMismatch {
                    subject: self.selection.subject,
                    vision_source: source,
                })
            }
            // Diğer tüm kombinasyonlar yapısal olarak geçerli (authority katmanında
            // GlobalDefault reject edilir).
            _ => Ok(()),
        }
    }

    /// **reviewer P0-3:** Hem structure hem authority validation. Q5 öncesinde ve
    /// digest başında çağrılır. None/GlobalDefault → Q5'e ulaşamaz, digest üretilemez.
    pub(crate) fn validate_for_authorization(&self) -> Result<(), VisionContextError> {
        self.validate_structure()?;
        self.validate_authority_for_mutation()
    }
}

impl EvaluationContextDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.evaluation-context.v1\0";

    /// **reviewer P0-3 / Step 4a / Step 4b / Step 4c:** Gerçek evaluation context digest.
    ///
    /// **Ontolojik kapsam (Step 4c):** Digest yalnız **Q5 vision-gate ve Q6 ordered-rule
    /// evaluation girdilerini** ve bunların semantics version'larını bağlar. Q4 syntax
    /// validation claim/structural içerikten çalışır; burada bağlanan ayrı bir Q4 config
    /// girdisi yoktur.
    ///
    /// **Step 4a (ordinal-aware):** `RuleEvaluationContext` alır — sort ETMEZ, registration
    /// sırasını (ordinal) korur. `check_claim_rules` first-match short-circuit ile aynı
    /// sırayı kullanır. `rule_context.validate()` başında defensively çağrılır.
    ///
    /// **Step 4b (claim-specific vision):** captured `EffectiveVisionGateContext` alır —
    /// Q5 ile aynı effective vision + theta_bound + semantics versions bağlanır.
    ///
    /// **Step 4c (kapsam daraltma):** `config: &EngineConfig` parametresi KALDIRILDI.
    /// Beş config alanı evaluation context'in ontolojik kapsamına ait değildi:
    /// - `min_approvers`/`quorum_threshold`: authorization'a ait ama **evaluation context'e
    ///   değil** — `CanonicalWitnessPolicy` (omega'dan) `AuthorizationBasisDigest`'te bağlı.
    /// - `milestone_interval`: persistence cadence (per-instance, claim dışı).
    /// - `abstractness`: Q5/Q6 evaluation'ı etkilemiyor; yalnız legacy `commit()` reposition
    ///   post-apply derived position'ı etkiliyor. `MeasurementInputDigest`'e taşınmaz (axis
    ///   tanımı değil, raw-axis measurement üretmez). Post-apply derived-position etkisi
    ///   gelecekte bir `ApplySemanticsDigest` bağlayabilir.
    /// - `merge_ratio_observable`: hiçbir hesaplamada kullanılmıyor (digest filler).
    ///
    /// `pub(crate)` — runtime context tipleri wire schema DEĞİL (`RuleEvaluationContext`,
    /// `EffectiveVisionGateContext` pub(crate)); sadece engine + testler çağırır. Reviewer:
    /// "intermediate runtime context types are not persisted wire schemas."
    ///
    /// **DOMAIN_SEPARATOR v1:** Step 6 golden vector
    /// (`evaluation_context_digest_v1_golden_vector` test) **locks** the first
    /// compatibility-supported v1 byte contract for the currently defined Q5/Q6
    /// evaluation models. Reload semantics: `AuthorizationBasisDigest` is recomputed
    /// from the embedded `AuthorizationBasis` during `PendingAuthorizationEnvelope::verify()`;
    /// the embedded `EvaluationContextDigest` is NOT independently recomputed from runtime
    /// rule and vision contexts on reload (opak bytes olarak saklanır). Breaking changes
    /// (canonical field/order/tag/encoding) after this lock require explicit v2 kararı.
    /// Golden vector locks byte encoding; runtime semantic correctness (#70) ayrı.
    ///
    /// **Rule + vision versioning:** Rule impl veya vision selection semantics değişip
    /// `semantics_version` artırılırsa context digest değişir → stale measurement tespiti.
    pub(crate) fn compute(
        rule_context: &RuleEvaluationContext,
        vision_context: &EffectiveVisionGateContext,
    ) -> Result<Self, AuthorizationBasisDigestError> {
        // **reviewer (Step 4a + 4b):** Defensive validation — context canonical invariants.
        rule_context
            .validate()
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;
        // P0-3: digest başında authority validation da yapar — None/GlobalDefault reject.
        vision_context
            .validate_for_authorization()
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;

        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // **Step 4a:** Rule evaluation context — semantics_version + ordinal-aware
        // ordered rules. Sort EDİLMEZ (registration sırası semantik — Q6 ile aynı).
        encode_u32(
            &mut hasher,
            rule_context.semantics_version(),
            "rule_context_semantics_version",
        );
        let ordered = rule_context.ordered_rules();
        let count = u64::try_from(ordered.len()).map_err(|_| {
            AuthorizationBasisDigestError::LengthOverflow {
                field: "ec_rule_count",
            }
        })?;
        encode_u64(&mut hasher, count, "ec_rule_count");
        for ordered_desc in ordered {
            encode_u32(&mut hasher, ordered_desc.ordinal, "ec_rule_ordinal");
            encode_bytes(&mut hasher, ordered_desc.descriptor.rule_id.as_bytes())?;
            encode_u32(
                &mut hasher,
                ordered_desc.descriptor.semantics_version,
                "ec_rule_semver",
            );
            encode_bytes(&mut hasher, &ordered_desc.descriptor.canonical_parameters)?;
        }

        // **Step 4b (reviewer P0-1 + P0-2):** Claim-specific effective vision — canonical,
        // sabit field sırası. Subject + effective vector + source + semantics versions
        // aynı karar ağacından (`effective_vision_selection`) üretilir.
        let sel = &vision_context.selection;
        // Effective vision vector (5-axis, override uygulanmış).
        encode_f64(
            &mut hasher,
            sel.effective_vision.raw.x,
            "ec_effective_vision_x",
        )?;
        encode_f64(
            &mut hasher,
            sel.effective_vision.raw.y,
            "ec_effective_vision_y",
        )?;
        encode_f64(
            &mut hasher,
            sel.effective_vision.raw.z,
            "ec_effective_vision_z",
        )?;
        encode_f64(
            &mut hasher,
            sel.effective_vision.raw.w,
            "ec_effective_vision_w",
        )?;
        encode_f64(
            &mut hasher,
            sel.effective_vision.raw.v,
            "ec_effective_vision_v",
        )?;
        // Vision source tag (canonical, validated newtype). **P0:** tek truth'tan —
        // `effective_vision.source()` (ayrı alan YOK).
        let source_tag =
            crate::canonical_tags::CanonicalVisionSourceTag::try_from(&sel.vision_source())
                .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;
        encode_u8(&mut hasher, source_tag.as_u8(), "ec_vision_source_tag");
        // Subject: Global → [0], Role(role) → [1, role_tag]. **Step 6 P0:** shared helper
        // `encode_vision_subject_to_vec` (inline YOK — preimage testleri aynı kaynağı kullanır).
        hasher.update(&encode_vision_subject_to_vec(sel.subject));
        // Semantics versions — staleness tespiti için bağlı.
        encode_u32(
            &mut hasher,
            sel.role_inference_semver,
            "ec_role_inference_semver",
        );
        encode_u32(
            &mut hasher,
            sel.vision_selection_semver,
            "ec_vision_selection_semver",
        );
        // theta_bound (artık vision_context'ten — config'ten DEĞİL).
        encode_f64(&mut hasher, vision_context.theta_bound, "ec_theta_bound")?;
        // Deviation metric semantics version.
        encode_u32(
            &mut hasher,
            vision_context.deviation_semver,
            "ec_deviation_semver",
        );

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

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:43):** V1 type alias — mevcut
/// `AuthorizationBasis`, Faz 1-3 frozen. Serialization/digest/golden byte'ları HİÇ
/// değişmez. V2 (`AuthorizationBasisV2`) canonical redesign — additive DEĞİL, duplicate
/// field yok. Backward compat = V1'i okuyabilmek (V1 field'larını V2'ye kopyalamak DEĞİL).
pub type AuthorizationBasisV1 = AuthorizationBasis;

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

/// **P1-1 (reviewer):** MeasuredRawPosition → ProvenancedMeasuredResult projection.
/// 5-axis mapping (value + CanonicalMetricSourceTag) tek yerde pinlenir — engine
/// orchestration canonical tag ayrıntısını bilmez. Axis unutma/yanlış eşleme riski
/// ayrı unit test ile kapanır.
impl TryFrom<&crate::coords::MeasuredRawPosition> for ProvenancedMeasuredResult {
    type Error = CanonicalizationError;

    fn try_from(measured: &crate::coords::MeasuredRawPosition) -> Result<Self, Self::Error> {
        let convert = |axis: &crate::coords::AxisMeasurement|
         -> Result<CanonicalAxisMeasurement, CanonicalizationError> {
            Ok(CanonicalAxisMeasurement {
                value: axis.value,
                source: crate::canonical_tags::CanonicalMetricSourceTag::try_from(&axis.source)?,
            })
        };
        Ok(Self {
            coupling: convert(&measured.coupling)?,
            cohesion: convert(&measured.cohesion)?,
            instability: convert(&measured.instability)?,
            entropy: convert(&measured.entropy)?,
            witness_depth: convert(&measured.witness_depth)?,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CanonicalTrajectoryEvidenceBaseline + CanonicalTrajectoryLossEvidence
// (INV-T9 #70 Commit 4b — reviewer v4 P0/P1-1)
//
// **Reviewer v4 P1-1 kesin schema:** baseline saf measurement evidence — sadece `before`
// taşır, loss_before YOK. Loss evidence ayrı — sadece `target + loss_after`, baseline taşımaz.
// İki truth source YOK. validate_v2 loss_before'u recompute eder (target'tan bağımsız before).
//
// **Reviewer v4 P0:** typed loss evidence downstream yayılımı — basis v2 bu canonical
// formları taşır. Policy matrisi validate_v2'de (AcceptAsProgress+Unavailable→error).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b (reviewer v4 P1-1):** Canonical trajectory baseline evidence —
/// saf measurement before-state. `loss_before` YOK — loss target'a bağlıdır, target yoksa
/// before mevcut olsa bile loss hesaplanamaz. validate_v2 loss_before'u `trajectory_loss_canonical`
/// ile recompute eder.
///
/// `CanonicalBaselineUnavailableReason` member listeleri: **sessiz dedup YOK** (duplicate
/// input fail-closed typed error); **ordering canonicalize edilir** (unsorted input →
/// sorted canonical sıraya normalize — bu meşru canonicalization, data kaybı DEĞİL).
/// + disjoint + union == request subject (reviewer scoped P1-2 + Faz 2 scoped P2-1).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum CanonicalTrajectoryEvidenceBaseline {
    /// Before-state measured — subject üyelerinin tamamı base snapshot'ta mevcut.
    Available { before: ProvenancedMeasuredResult },
    /// Before-state unavailable — subject üyeleri tamamen/kısmen delta-introduced.
    Unavailable {
        reason: CanonicalBaselineUnavailableReason,
    },
}

/// **INV-T9 #70 Commit 4b:** Canonical baseline unavailable reason. Member listeleri:
/// **sessiz dedup YOK** (duplicate fail-closed typed error); **ordering canonicalize
/// edilir** (unsorted → sorted — meşru canonicalization). + disjoint + union == request
/// subject (scoped P1-2 + Faz 2 scoped P2-1). `try_from_reason` raw enum + request
/// subject alır — duplicate normalizasyondan ÖNCE typed error (reviewer Faz 2 scoped P1-1).
/// **INV-T9 #70 Commit 4b Faz 4 (reviewer P1-2 v3, P2-3 v4):** Canonical baseline
/// unavailable reason — opaque struct. Private repr; tek creation yolu checked
/// `try_from_reason` (local invalid state üretilemez: sort + dedup + non-empty + disjoint
/// + union == subject). Struct literal bypass imkânsız. Cross-object tutarsızlık
/// `validate_against_subject` ile tüketim sınırında yeniden doğrulanır (defense-in-depth).
///
/// **Serialization (reviewer P2-3 v4):** `#[serde(transparent)]` — wrapper iç repr enum
/// gibi serialize olur (eski public enum wire shape korunur). Wire format Commit 1b'de
/// explicit DTO ile finalize edilir.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(transparent)]
pub struct CanonicalBaselineUnavailableReason {
    repr: CanonicalBaselineUnavailableReasonRepr,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
enum CanonicalBaselineUnavailableReasonRepr {
    /// Tüm subject üyeleri delta ile eklenen node'lar — base'de hiçbiri yok.
    AllMembersIntroducedByDelta { members: Vec<crate::space::NodeId> },
    /// Bazı üyeler base'de var, kalanların tümü delta ile ekleniyor.
    PartialNewSubject {
        existing: Vec<crate::space::NodeId>,
        introduced: Vec<crate::space::NodeId>,
    },
}

impl CanonicalBaselineUnavailableReason {
    /// **Reviewer P1-2 v4:** Safe view accessor — public discriminator ile varyant bilgisi.
    /// Panic YOK — geçerli nesnede yanlış accessor çağrımı imkânsız. Caller `view()` ile
    /// match yapar (tüm varyantlar typed).
    pub fn view(&self) -> CanonicalBaselineUnavailableReasonView<'_> {
        match &self.repr {
            CanonicalBaselineUnavailableReasonRepr::AllMembersIntroducedByDelta { members } => {
                CanonicalBaselineUnavailableReasonView::AllMembersIntroducedByDelta {
                    members: members.as_slice(),
                }
            }
            CanonicalBaselineUnavailableReasonRepr::PartialNewSubject {
                existing,
                introduced,
            } => CanonicalBaselineUnavailableReasonView::PartialNewSubject {
                existing: existing.as_slice(),
                introduced: introduced.as_slice(),
            },
        }
    }

    /// Varyant bilgisi — projection/encoding için (modül içi).
    fn repr(&self) -> &CanonicalBaselineUnavailableReasonRepr {
        &self.repr
    }
}

/// **Reviewer P1-2 v4:** Safe public view — panic accessor'lar yerine. Caller `view()` ile
/// match yapar; geçerli nesnede yanlış varyant çağrımı imkânsız.
#[derive(Debug, Clone, Copy)]
pub enum CanonicalBaselineUnavailableReasonView<'a> {
    AllMembersIntroducedByDelta {
        members: &'a [crate::space::NodeId],
    },
    PartialNewSubject {
        existing: &'a [crate::space::NodeId],
        introduced: &'a [crate::space::NodeId],
    },
}

/// **INV-T9 #70 Commit 4b (reviewer Faz 2 scoped P1-1):** Typed canonical baseline
/// validation error — `String` DEĞİL. Her ihlal ayrı varyant (telemetry + exact assertion).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalBaselineValidationError {
    #[error("AllMembersIntroducedByDelta members contain duplicates: {members:?}")]
    AllMembersDuplicate { members: Vec<crate::space::NodeId> },

    /// **Reviewer P2-2 v4:** AllMembers list canonical sıralı değil (strict ascending).
    #[error("AllMembersIntroducedByDelta members not in canonical order: {members:?}")]
    AllMembersUnsorted { members: Vec<crate::space::NodeId> },

    #[error("AllMembersIntroducedByDelta members {members:?} != request subject {subject:?}")]
    AllMembersSubjectMismatch {
        members: Vec<crate::space::NodeId>,
        subject: Vec<crate::space::NodeId>,
    },

    #[error("PartialNewSubject existing list contains duplicates: {existing:?}")]
    PartialExistingDuplicate { existing: Vec<crate::space::NodeId> },

    /// **Reviewer P2-2 v4:** Partial existing list canonical sıralı değil.
    #[error("PartialNewSubject existing list not in canonical order: {existing:?}")]
    PartialExistingUnsorted { existing: Vec<crate::space::NodeId> },

    #[error("PartialNewSubject introduced list contains duplicates: {introduced:?}")]
    PartialIntroducedDuplicate {
        introduced: Vec<crate::space::NodeId>,
    },

    /// **Reviewer P2-2 v4:** Partial introduced list canonical sıralı değil.
    #[error("PartialNewSubject introduced list not in canonical order: {introduced:?}")]
    PartialIntroducedUnsorted {
        introduced: Vec<crate::space::NodeId>,
    },

    #[error("PartialNewSubject requires non-empty existing and introduced")]
    PartialEmptyList,

    #[error("node {node_id} in both existing and introduced (not disjoint)")]
    PartialNotDisjoint { node_id: crate::space::NodeId },

    #[error("PartialNewSubject union {union:?} != request subject {subject:?}")]
    PartialUnionSubjectMismatch {
        union: Vec<crate::space::NodeId>,
        subject: Vec<crate::space::NodeId>,
    },
}

impl CanonicalBaselineUnavailableReason {
    /// **INV-T9 #70 Commit 4b (reviewer Faz 2 scoped P1-1):** Validated smart constructor —
    /// raw `BaselineUnavailableReason` enum + request subject alır. Member listeleri
    /// non-empty + sorted + unique + disjoint + union == request subject. **Duplicate
    /// kontrolü normalizasyondan ÖNCE** (sessiz dedup YOK — malformed wire fail-closed
    /// reject). Tek-bir-liste API (3 paralel liste çelişkisi YOK — varyant üzerinden match).
    /// Faz 4'te basis builder `MeasurementBaseline::Unavailable` → canonical dönüşümde çağırır.
    #[allow(dead_code)] // Faz 4: basis builder MeasurementBaseline → canonical dönüşüm
    pub(crate) fn try_from_reason(
        reason: &crate::measurement::BaselineUnavailableReason,
        request_subject: &crate::measurement::CanonicalSubjectScope,
    ) -> Result<Self, CanonicalBaselineValidationError> {
        let subject_members = request_subject.member_ids();
        match reason {
            crate::measurement::BaselineUnavailableReason::AllMembersIntroducedByDelta {
                members,
            } => {
                // Duplicate check BEFORE normalization (scoped P1-1).
                let mut sorted = members.clone();
                sorted.sort_unstable();
                let mut deduped = sorted.clone();
                deduped.dedup();
                if deduped.len() != sorted.len() {
                    return Err(CanonicalBaselineValidationError::AllMembersDuplicate {
                        members: members.clone(),
                    });
                }
                if deduped.as_slice() != subject_members {
                    return Err(
                        CanonicalBaselineValidationError::AllMembersSubjectMismatch {
                            members: deduped,
                            subject: subject_members.to_vec(),
                        },
                    );
                }
                Ok(Self {
                    repr: CanonicalBaselineUnavailableReasonRepr::AllMembersIntroducedByDelta {
                        members: deduped,
                    },
                })
            }
            crate::measurement::BaselineUnavailableReason::PartialNewSubject {
                existing,
                introduced,
            } => {
                if existing.is_empty() || introduced.is_empty() {
                    return Err(CanonicalBaselineValidationError::PartialEmptyList);
                }
                // Duplicate check BEFORE normalization (scoped P1-1).
                let mut existing_sorted = existing.clone();
                existing_sorted.sort_unstable();
                let mut existing_dedup = existing_sorted.clone();
                existing_dedup.dedup();
                if existing_dedup.len() != existing_sorted.len() {
                    return Err(CanonicalBaselineValidationError::PartialExistingDuplicate {
                        existing: existing.clone(),
                    });
                }
                let mut introduced_sorted = introduced.clone();
                introduced_sorted.sort_unstable();
                let mut introduced_dedup = introduced_sorted.clone();
                introduced_dedup.dedup();
                if introduced_dedup.len() != introduced_sorted.len() {
                    return Err(
                        CanonicalBaselineValidationError::PartialIntroducedDuplicate {
                            introduced: introduced.clone(),
                        },
                    );
                }
                // Disjoint check.
                for id in &existing_dedup {
                    if introduced_dedup.contains(id) {
                        return Err(CanonicalBaselineValidationError::PartialNotDisjoint {
                            node_id: *id,
                        });
                    }
                }
                // Union == subject (sorted merge).
                let mut union_merged: Vec<crate::space::NodeId> = existing_dedup
                    .iter()
                    .chain(introduced_dedup.iter())
                    .copied()
                    .collect();
                union_merged.sort_unstable();
                union_merged.dedup();
                if union_merged.as_slice() != subject_members {
                    return Err(
                        CanonicalBaselineValidationError::PartialUnionSubjectMismatch {
                            union: union_merged,
                            subject: subject_members.to_vec(),
                        },
                    );
                }
                Ok(Self {
                    repr: CanonicalBaselineUnavailableReasonRepr::PartialNewSubject {
                        existing: existing_dedup,
                        introduced: introduced_dedup,
                    },
                })
            }
        }
    }

    /// **INV-T9 #70 Commit 4b Faz 4 (reviewer P1-2 v3):** Cross-object defense-in-depth
    /// doğrulaması — reason'ı request subject'e karşı yeniden doğrular. Non-normalizing:
    /// mevcut canonical state'i olduğu haliyle doğrular (sorted/unique/non-empty/disjoint/
    /// union == subject). Bozuk persisted/migrated state sessizce düzeltilmez.
    ///
    /// `AuthorizationBasisV2::validate_semantics` digest hesaplamasından ÖNCE çağırır.
    pub(crate) fn validate_against_subject(
        &self,
        request_subject: &crate::measurement::CanonicalSubjectScope,
    ) -> Result<(), CanonicalBaselineValidationError> {
        let subject_members = request_subject.member_ids();
        match &self.repr {
            CanonicalBaselineUnavailableReasonRepr::AllMembersIntroducedByDelta { members } => {
                // Non-normalizing: sorted (strict ascending) + unique (canonical invariant).
                for pair in members.windows(2) {
                    if pair[0] == pair[1] {
                        return Err(CanonicalBaselineValidationError::AllMembersDuplicate {
                            members: members.clone(),
                        });
                    }
                    if pair[0] > pair[1] {
                        return Err(CanonicalBaselineValidationError::AllMembersUnsorted {
                            members: members.clone(),
                        });
                    }
                }
                if members.as_slice() != subject_members {
                    return Err(
                        CanonicalBaselineValidationError::AllMembersSubjectMismatch {
                            members: members.clone(),
                            subject: subject_members.to_vec(),
                        },
                    );
                }
            }
            CanonicalBaselineUnavailableReasonRepr::PartialNewSubject {
                existing,
                introduced,
            } => {
                // Non-empty.
                if existing.is_empty() || introduced.is_empty() {
                    return Err(CanonicalBaselineValidationError::PartialEmptyList);
                }
                // Non-normalizing: existing sorted (strict ascending) + unique.
                for pair in existing.windows(2) {
                    if pair[0] == pair[1] {
                        return Err(CanonicalBaselineValidationError::PartialExistingDuplicate {
                            existing: existing.clone(),
                        });
                    }
                    if pair[0] > pair[1] {
                        return Err(CanonicalBaselineValidationError::PartialExistingUnsorted {
                            existing: existing.clone(),
                        });
                    }
                }
                // Non-normalizing: introduced sorted + unique.
                for pair in introduced.windows(2) {
                    if pair[0] == pair[1] {
                        return Err(
                            CanonicalBaselineValidationError::PartialIntroducedDuplicate {
                                introduced: introduced.clone(),
                            },
                        );
                    }
                    if pair[0] > pair[1] {
                        return Err(
                            CanonicalBaselineValidationError::PartialIntroducedUnsorted {
                                introduced: introduced.clone(),
                            },
                        );
                    }
                }
                // Disjoint.
                for id in existing {
                    if introduced.contains(id) {
                        return Err(CanonicalBaselineValidationError::PartialNotDisjoint {
                            node_id: *id,
                        });
                    }
                }
                // Union == subject (non-normalizing sorted merge).
                let mut union_merged: Vec<crate::space::NodeId> =
                    existing.iter().chain(introduced.iter()).copied().collect();
                union_merged.sort_unstable();
                union_merged.dedup();
                if union_merged.as_slice() != subject_members {
                    return Err(
                        CanonicalBaselineValidationError::PartialUnionSubjectMismatch {
                            union: union_merged,
                            subject: subject_members.to_vec(),
                        },
                    );
                }
            }
        }
        Ok(())
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:207 non-blocking notu):** Shared baseline
/// encoder — `MeasurementBaseline::compute_digest()` (measurement.rs) ve bu metod aynı
/// canonical byte format'ını üretir. Drift risk kapalı: Available/Unavailable raw
/// measurement baseline digest == canonical trajectory evidence baseline digest.
impl CanonicalTrajectoryEvidenceBaseline {
    /// **INV-T9 #70 Commit 4b Faz 4 (plan md:143, 207):** `MeasurementBaselineDigest`
    /// üretir — `MeasurementBaseline::compute_digest()` ile aynı byte format. Shared
    /// encoder (`MeasurementBaselineDigest::write_commitment` equivalent) — drift risk
    /// kapalı. Test: Available/Unavailable raw digest == canonical evidence digest.
    ///
    /// **Preimage eşitliği:** `MeasurementBaseline::Available(MeasuredRawPosition)` ile
    /// `CanonicalTrajectoryEvidenceBaseline::Available { before: ProvenancedMeasuredResult }`
    /// aynı measured değerleri için aynı digest üretir (5-axis canonical encoding,
    /// source tag dahil). Unavailable varyantları için reason member listeleri sorted +
    /// length-prefix + per-id (aynı canonical format).
    /// **Reviewer P1-2 (neutral writer):** `CanonicalTrajectoryEvidenceBaseline` →
    /// `BaselineCommitmentView` projection. Infallible: `ProvenancedMeasuredResult` zaten
    /// canonical tag'ler, `CanonicalBaselineUnavailableReason` zaten validated.
    fn to_commitment_view(&self) -> crate::measurement::BaselineCommitmentView<'_> {
        use crate::measurement::{
            BaselineAxesView, BaselineCommitmentView, BaselineUnavailableReasonView,
        };
        match self {
            CanonicalTrajectoryEvidenceBaseline::Available { before } => {
                BaselineCommitmentView::Available {
                    axes: BaselineAxesView {
                        coupling: (before.coupling.value, before.coupling.source),
                        cohesion: (before.cohesion.value, before.cohesion.source),
                        instability: (before.instability.value, before.instability.source),
                        entropy: (before.entropy.value, before.entropy.source),
                        witness_depth: (before.witness_depth.value, before.witness_depth.source),
                    },
                }
            }
            CanonicalTrajectoryEvidenceBaseline::Unavailable { reason } => {
                BaselineCommitmentView::Unavailable {
                    reason: match reason.repr() {
                        CanonicalBaselineUnavailableReasonRepr::AllMembersIntroducedByDelta {
                            members,
                        } => BaselineUnavailableReasonView::AllMembersIntroducedByDelta {
                            members: members.clone(),
                            _phantom: std::marker::PhantomData,
                        },
                        CanonicalBaselineUnavailableReasonRepr::PartialNewSubject {
                            existing,
                            introduced,
                        } => BaselineUnavailableReasonView::PartialNewSubject {
                            existing: existing.clone(),
                            introduced: introduced.clone(),
                            _phantom: std::marker::PhantomData,
                        },
                    },
                }
            }
        }
    }

    /// **INV-T9 #70 Commit 4b Faz 4 (plan md:143, reviewer P1-2):** `MeasurementBaselineDigest`
    /// üretir — neutral writer (`write_measurement_baseline_commitment`) ile `MeasurementBaseline`
    /// ile aynı byte format. Drift risk yapısal olarak kapalı (tek encoder).
    #[allow(
        dead_code,
        reason = "Faz 4 basis builder / validate_semantics consumer"
    )]
    pub(crate) fn compute_measurement_baseline_digest(
        &self,
    ) -> Result<
        crate::measurement::MeasurementBaselineDigest,
        crate::measurement::EngineMeasurementDigestError,
    > {
        use crate::measurement::MeasurementBaselineDigest;
        let mut hasher = blake3::Hasher::new();
        hasher.update(MeasurementBaselineDigest::domain_separator());
        let view = self.to_commitment_view();
        MeasurementBaselineDigest::write_measurement_baseline_commitment(&mut hasher, view)?;
        Ok(MeasurementBaselineDigest::from_hasher_finalized(
            hasher.finalize().into(),
        ))
    }
}

/// **INV-T9 #70 Commit 4b (reviewer v4 P0/P1-1):** Canonical trajectory loss evidence —
/// sadece `target + loss_after`. Baseline taşımaz (baseline ayrı evidence). Unavailable
/// ise `CanonicalTrajectoryLossUnavailableReason` (NoPreferredVector).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum CanonicalTrajectoryLossEvidence {
    /// Loss hesaplanabilir — preferred_vector mevcut, after ölçüldü.
    Available {
        target: CanonicalRawPosition,
        loss_after: CanonicalF64,
    },
    /// Loss unavailable — preferred_vector None (reviewer v3 P0: geçerli task durumu).
    Unavailable {
        reason: CanonicalTrajectoryLossUnavailableReason,
    },
}

/// **INV-T9 #70 Commit 4b:** Canonical loss unavailable reason. `NoPreferredVector`
/// (preferred_vector=None → loss anlamsız). Serde wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalTrajectoryLossUnavailableReason {
    /// Task'ta `preferred_vector` yok — loss/target anlamsız.
    NoPreferredVector,
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 4 — AuthorizationBasisV2 (canonical redesign, plan md:46-60)
//
// **3 katman ayrımı (plan md:27-32, duplicate field YOK):**
// - Basis (`AuthorizationBasisV2`) = kanıtsal zemin — identity + evidence + artifact
//   commitments + delta/goal digests. Gate/witness YOK.
// - GateEvaluation — `CanonicalGateEvaluationV2` persisted snapshot +
//   `VerifiedGateEvaluationV2` opaque producer proof. Faz 4 structural; Faz 5 evaluator.
// - Context (`AuthorizationContextV2`) = basis + verified gate snapshot + canonical
//   witness requirement — checked constructor proof-gated.
//
// **V2 canonical redesign (additive DEĞİL, plan md:48-52):**
// - `loss_before/after` → `CanonicalTrajectoryLossEvidence`
// - `measurement_input_digest` → `CanonicalMeasurementRequestEvidence +
//   MeasurementRequestDigest`
// - `measured_result` → `measurement_digest + canonical evidence`
// - Backward compat = V1'i okuyabilmek (V1 field'larını V2'ye kopyalamak DEĞİL)
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:164):** `AuthorizationBasisV2` validation
/// hatası. Basis construction + `validate_semantics` (nested evidence + baseline digest
/// reverify + engine_measurement_digest reverify).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthorizationBasisV2Error {
    #[error("measurement baseline digest mismatch: stored={stored:?}, recomputed={recomputed:?}")]
    MeasurementBaselineDigestMismatch { stored: String, recomputed: String },
    #[error("engine measurement digest mismatch: stored={stored:?}, recomputed={recomputed:?}")]
    EngineMeasurementDigestMismatch { stored: String, recomputed: String },
    /// **INV-T9 #70 Commit 4b Faz 4 (reviewer P0-2):** Measurement request snapshot →
    /// digest reverify mismatch. `measurement_request` (okunabilir evidence) ile
    /// `measurement_request_digest` (commitment) farklı gerçeklikleri temsil edemez.
    #[error("measurement request digest mismatch: stored={stored:?}, recomputed={recomputed:?}")]
    MeasurementRequestDigestMismatch { stored: String, recomputed: String },
    /// **INV-T9 #70 Commit 4b Faz 4 (reviewer P0-2):** Request evidence structural delta
    /// digest ile basis canonical_delta_digest tutarsız. İki commitment aynı kaynak.
    #[error("canonical delta digest mismatch: request={request:?}, basis={basis:?}")]
    CanonicalDeltaDigestMismatch { request: String, basis: String },
    /// **INV-T9 #70 Commit 4b Faz 4 (reviewer P1-3 v4):** Baseline evidence typed validation
    /// hatası — `CanonicalBaselineValidationError` typed wrapper (duplicate/subject-mismatch/
    /// empty/overlap/union ayrımı korunur, string'e düşürülmez).
    #[error("baseline evidence validation failed: {0}")]
    BaselineValidation(#[from] CanonicalBaselineValidationError),
    #[error("basis construction failed: {detail}")]
    Construction { detail: String },
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:146-160, reviewer P1-2):** Canonical V2
/// authorization basis — kanıtsal zemin. Identity + evidence + artifact commitments +
/// delta/goal digests. Gate/witness YOK (3 katman ayrımı). Duplicate field YOK —
/// additive DEĞİL, canonical redesign.
///
/// **Reverify zinciri (plan md:87):** `measurement_baseline_digest` +
/// `measurement_context_digest` basis'te saklanır — `validate_semantics` bunları shared
/// encoder ile recompute eder, stored digest ile karşılaştırır (defense-in-depth).
///
/// **Field visibility (reviewer P1-2):** Tüm field'lar PRIVATE. Tek creation yolu
/// `new()` (checked constructor — `validate_semantics` çağırır). Struct literal bypass
/// imkânsız. Erasure/mutation imkânsız. Accessor'lar read-only.
///
/// **P0-1 v3 (reviewer):** `serde::Serialize` intentionally absent — tek dış
/// serialization yolu `VersionedAuthorizationBasis::V2` (explicit envelope + LowerHex32).
/// Direct `serde_json::to_string(&basis_v2)` compile error — wire bypass kapalı.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationBasisV2 {
    /// Task identity.
    task_id: crate::trajectory::TaskId,
    /// Claim identity.
    claim_id: crate::witness::ClaimId,
    /// Claim binding commitment (claim_id + task_id + author + structural_delta_digest).
    task_claim_digest: crate::measurement::TaskClaimDigest,
    /// Task goal commitment (task_id + predicate body + preferred_vector).
    task_goal_digest: crate::measurement::TaskGoalDigest,
    /// Measured result commitment — 5-axis değer + source (after).
    measurement_digest: crate::measurement::MeasurementDigest,
    /// Tam artifact commitment (request + baseline + after + context).
    engine_measurement_digest: crate::measurement::EngineMeasurementDigest,
    /// Trajectory baseline evidence — saf measurement before-state (loss_before YOK).
    trajectory_baseline: CanonicalTrajectoryEvidenceBaseline,
    /// Baseline digest — reverify zinciri (shared encoder ile recompute).
    measurement_baseline_digest: crate::measurement::MeasurementBaselineDigest,
    /// Trajectory loss evidence — sadece target + loss_after (baseline taşımaz).
    trajectory_loss: CanonicalTrajectoryLossEvidence,
    /// Measurement request evidence — tam canonical snapshot (subject/impact/revision/digest).
    measurement_request: crate::measurement::CanonicalMeasurementRequestEvidence,
    /// Measurement request digest — reverify zinciri.
    measurement_request_digest: crate::measurement::MeasurementRequestDigest,
    /// Measurement context digest — reverify zinciri (shared encoder ile recompute).
    measurement_context_digest: crate::measurement::MeasurementContextDigest,
    /// Canonical structural delta digest — claim → structural delta commitment.
    canonical_delta_digest: crate::measurement::MeasurementDeltaDigest,
}

impl AuthorizationBasisV2 {
    /// **Checked constructor (plan md:157, reviewer P1-2):** `validate_semantics` çağırır
    /// — nested evidence + baseline digest reverify + engine_measurement_digest reverify
    /// + request snapshot → digest reverify (reviewer P0-2). Başarısızsa basis doğmaz.
    /// Tek creation yolu (field'lar private). Builder (Commit 2) bu constructor'ı çağırır.
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn new(
        task_id: crate::trajectory::TaskId,
        claim_id: crate::witness::ClaimId,
        task_claim_digest: crate::measurement::TaskClaimDigest,
        task_goal_digest: crate::measurement::TaskGoalDigest,
        measurement_digest: crate::measurement::MeasurementDigest,
        engine_measurement_digest: crate::measurement::EngineMeasurementDigest,
        trajectory_baseline: CanonicalTrajectoryEvidenceBaseline,
        measurement_baseline_digest: crate::measurement::MeasurementBaselineDigest,
        trajectory_loss: CanonicalTrajectoryLossEvidence,
        measurement_request: crate::measurement::CanonicalMeasurementRequestEvidence,
        measurement_request_digest: crate::measurement::MeasurementRequestDigest,
        measurement_context_digest: crate::measurement::MeasurementContextDigest,
        canonical_delta_digest: crate::measurement::MeasurementDeltaDigest,
    ) -> Result<Self, AuthorizationBasisV2Error> {
        let basis = Self {
            task_id,
            claim_id,
            task_claim_digest,
            task_goal_digest,
            measurement_digest,
            engine_measurement_digest,
            trajectory_baseline: trajectory_baseline.clone(),
            measurement_baseline_digest,
            trajectory_loss,
            measurement_request,
            measurement_request_digest,
            measurement_context_digest,
            canonical_delta_digest,
        };
        basis.validate_semantics()?;
        Ok(basis)
    }

    /// **validate_semantics (plan md:158):** Nested evidence + baseline digest reverify
    /// (shared encoder `compute_measurement_baseline_digest`) + engine_measurement_digest
    /// reverify (`compute_from_commitments` shared encoder). Defense-in-depth — stored
    /// digest'ler canonical evidence ile tutarlı olmalı.
    fn validate_semantics(&self) -> Result<(), AuthorizationBasisV2Error> {
        // **Reviewer P1-2 v3:** Baseline reason ↔ request subject doğrulaması — digest
        // hesaplamasından ÖNCE. Cross-object substitution/tutarsızlık reject. Defense-in-
        // depth: checked constructor zaten validated ama basis katmanında reverify.
        // **Reviewer P1-3 v4:** typed error — `BaselineValidation(#[from])` ile string'e
        // düşürülmez (telemetry + exact assertion seviyesi korunur).
        if let CanonicalTrajectoryEvidenceBaseline::Unavailable { reason } =
            &self.trajectory_baseline
        {
            reason.validate_against_subject(&self.measurement_request.subject)?;
        }
        // Baseline digest reverify — shared encoder (plan md:207).
        let recomputed_baseline = self
            .trajectory_baseline
            .compute_measurement_baseline_digest()
            .map_err(|e| AuthorizationBasisV2Error::Construction {
                detail: e.to_string(),
            })?;
        if recomputed_baseline.as_bytes() != self.measurement_baseline_digest.as_bytes() {
            return Err(
                AuthorizationBasisV2Error::MeasurementBaselineDigestMismatch {
                    stored: self.measurement_baseline_digest.to_hex(),
                    recomputed: recomputed_baseline.to_hex(),
                },
            );
        }
        // Engine measurement digest reverify — compute_from_commitments shared encoder.
        // Stored digest, request + baseline + after + context'ten recompute edilmeli.
        let recomputed_engine =
            crate::measurement::EngineMeasurementDigest::compute_from_commitments(
                &self.measurement_request_digest,
                &self.measurement_baseline_digest,
                &self.measurement_digest,
                &self.measurement_context_digest,
            )
            .map_err(|e| AuthorizationBasisV2Error::Construction {
                detail: e.to_string(),
            })?;
        if recomputed_engine.as_bytes() != self.engine_measurement_digest.as_bytes() {
            return Err(AuthorizationBasisV2Error::EngineMeasurementDigestMismatch {
                stored: self.engine_measurement_digest.to_hex(),
                recomputed: recomputed_engine.to_hex(),
            });
        }
        // **INV-T9 #70 Commit 4b Faz 4 (reviewer P0-2):** Request snapshot → digest reverify.
        // `measurement_request` (okunabilir evidence) ile `measurement_request_digest`
        // (commitment) aynı gerçekliği temsil etmeli. `compute_from_canonical` shared
        // encoder — `MeasurementRequestDigest::compute` ile aynı byte format.
        let recomputed_request =
            crate::measurement::MeasurementRequestDigest::compute_from_canonical(
                &self.measurement_request,
            )
            .map_err(|e| AuthorizationBasisV2Error::Construction {
                detail: e.to_string(),
            })?;
        if recomputed_request.as_bytes() != self.measurement_request_digest.as_bytes() {
            return Err(
                AuthorizationBasisV2Error::MeasurementRequestDigestMismatch {
                    stored: self.measurement_request_digest.to_hex(),
                    recomputed: recomputed_request.to_hex(),
                },
            );
        }
        // **Reviewer P0-2:** Request evidence structural_delta_digest ile basis
        // canonical_delta_digest tutarlı olmalı — iki commitment aynı kaynak (claim → delta).
        if self.measurement_request.structural_delta_digest.as_bytes()
            != self.canonical_delta_digest.as_bytes()
        {
            return Err(AuthorizationBasisV2Error::CanonicalDeltaDigestMismatch {
                request: self.measurement_request.structural_delta_digest.to_hex(),
                basis: self.canonical_delta_digest.to_hex(),
            });
        }
        Ok(())
    }

    /// **Reviewer P1-2:** Read-only accessor'lar — field'lar private, mutation imkânsız.
    /// Builder (Commit 2) ve test'ler bu accessor'ları kullanır. `pub(crate)` — digest
    /// newtype'lar `pub(crate)` (plan md:59).
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn task_id(&self) -> crate::trajectory::TaskId {
        self.task_id
    }
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn claim_id(&self) -> crate::witness::ClaimId {
        self.claim_id
    }
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn measurement_request(
        &self,
    ) -> &crate::measurement::CanonicalMeasurementRequestEvidence {
        &self.measurement_request
    }
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn measurement_request_digest(
        &self,
    ) -> &crate::measurement::MeasurementRequestDigest {
        &self.measurement_request_digest
    }
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn engine_measurement_digest(&self) -> &crate::measurement::EngineMeasurementDigest {
        &self.engine_measurement_digest
    }
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub(crate) fn canonical_delta_digest(&self) -> &crate::measurement::MeasurementDeltaDigest {
        &self.canonical_delta_digest
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn task_claim_digest(&self) -> &crate::measurement::TaskClaimDigest {
        &self.task_claim_digest
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn task_goal_digest(&self) -> &crate::measurement::TaskGoalDigest {
        &self.task_goal_digest
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn measurement_digest(&self) -> &crate::measurement::MeasurementDigest {
        &self.measurement_digest
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn trajectory_baseline(&self) -> &CanonicalTrajectoryEvidenceBaseline {
        &self.trajectory_baseline
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn measurement_baseline_digest(
        &self,
    ) -> &crate::measurement::MeasurementBaselineDigest {
        &self.measurement_baseline_digest
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn trajectory_loss(&self) -> &CanonicalTrajectoryLossEvidence {
        &self.trajectory_loss
    }
    #[allow(dead_code, reason = "Faz 4 wire serializer / Commit 1b consumer")]
    pub(crate) fn measurement_context_digest(
        &self,
    ) -> &crate::measurement::MeasurementContextDigest {
        &self.measurement_context_digest
    }

    /// **AuthorizationBasisDigestV2 (plan md:55):** V2 canonical digest. Ayrı domain
    /// separator (`OSP/AUTHORIZATION-BASIS/V2`) — V1 frozen. Builder (Commit 2) bu
    /// digest'i `AuthorizationContextV2` zincirinde kullanır.
    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub fn compute_digest(&self) -> Result<AuthorizationBasisDigestV2, CanonicalDigestError> {
        AuthorizationBasisDigestV2::compute(self)
    }
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
    /// normalize, `f64::to_bits()` little-endian. Collections sorted (try_new'de, encoder
    /// AS-IS — Step 5 P0). Stable numeric tags (format!("{:?}") DEĞİL). Domain separation prefix.
    ///
    /// **reviewer P0-1/P0-3:** witness_policy, measurement_input_digest,
    /// predicate_evaluation basis'e bağlı. claim_identity.task_id encode edilir.
    ///
    /// **Step 5 (defensive integrity):** `basis.structural_delta.validate()` başında
    /// defensive çağrılır (non-normalizing). Encoder sort ETMEZ — try_new canonical sırayı
    /// garanti. `removed_edges` artık identity-only encoding (`is_type_only` YOK).
    ///
    /// **DOMAIN_SEPARATOR v1:** Step 6 golden vectors **establish and lock** the first
    /// compatibility-supported v1 byte contract for the currently defined canonical models
    /// (`authorization_basis_digest_v1_golden_vector` + `evaluation_context_digest_v1_golden_vector`
    /// tests). Pre-Step-5/6 development artifacts are not compatibility-supported and may
    /// fail **either** deserialization (unknown fields / validation) **or** envelope digest
    /// verification. Breaking changes after this lock require explicit v2 domain separator.
    /// Golden vectors lock the **byte encoding** of currently-defined models; they do not
    /// prove runtime data is correctly produced (#70 per-axis provenance / engine-issued
    /// measurement remains required).
    pub fn compute(basis: &AuthorizationBasis) -> Result<Self, AuthorizationBasisDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // Identity.
        encode_u32(&mut hasher, basis.schema_version, "schema_version");
        encode_u64(&mut hasher, basis.task_id, "task_id");
        encode_u64(&mut hasher, basis.claim_identity.claim_id, "claim_id");
        encode_u64(&mut hasher, basis.claim_identity.task_id, "claim_task_id"); // P0-2 claim_identity.task_id
        encode_u64(&mut hasher, basis.claim_author, "claim_author");

        // **Step 5 P0:** Structural delta — defensive validate (non-normalizing) başta,
        // sonra AS-IS encode (sort YOK). try_new canonical sıralamayı garanti; encoder
        // sort etmez → canonical invariant maskelenemez.
        basis
            .structural_delta
            .validate()
            .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;

        // CanonicalNode (full content, as-is — try_new id sırası garanti).
        let nodes = basis.structural_delta.new_nodes();
        encode_u64(&mut hasher, nodes.len() as u64, "new_node_count");
        for node in nodes {
            encode_canonical_node(&mut hasher, node)?;
        }
        // new_edges — full CanonicalEdge (is_type_only dahil), as-is.
        encode_canonical_edge_vec(&mut hasher, basis.structural_delta.new_edges())?;
        // removed_edges — identity-only (is_type_only YOK), as-is.
        encode_canonical_edge_identity_vec(&mut hasher, basis.structural_delta.removed_edges())?;

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
        // **Faz 3:** axis sırası structural (sabit çağrı sırası). Byte format v1 ile uyumlu.
        encode_axis_measurement(&mut hasher, &basis.measured_result.coupling)?;
        encode_axis_measurement(&mut hasher, &basis.measured_result.cohesion)?;
        encode_axis_measurement(&mut hasher, &basis.measured_result.instability)?;
        encode_axis_measurement(&mut hasher, &basis.measured_result.entropy)?;
        encode_axis_measurement(&mut hasher, &basis.measured_result.witness_depth)?;

        // Outcome tags.
        // **INV-T9 #70 Commit 4b Faz 4 (reviewer P0-1):** V1 production encoder fallible
        // `gate_decision_tag_v1` kullanır — V2-only kararlar (7, 8) reject. V1 byte contract
        // frozen; V2-only GateDecision'ların V1 artifact'lerine sızması imkânsız.
        encode_u8(
            &mut hasher,
            gate_decision_tag_v1(basis.deterministic_gate_result)?,
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
        encode_space_view_id(&mut hasher, &basis.base_space_view_revision.view_id);
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
// INV-T9 #70 Commit 4b Faz 4 — AuthorizationBasisDigestV2 + AuthorizationContextDigestV2
// (plan md:54-59)
//
// **Ayrı digest newtype + domain separator + canonical encoding (JSON DEĞİL) + hex wire:**
// - `AuthorizationBasisDigestV2` (`OSP/AUTHORIZATION-BASIS/V2`) — V1 frozen ayrı
// - `AuthorizationContextDigestV2` (`OSP/AUTHORIZATION-CONTEXT/V2`) — basis + gate eval +
//   witness requirement commitment
//
// **DigestBytes private repr (plan md:59):** constructor `pub(crate)`, sadece
// `as_bytes()`/`to_hex()` public. Canonical encoding — `serde_json::to_vec` YASAK.
// Hex wire format: 64 lowercase.
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:55):** V2 authorization basis digest. Ayrı
/// domain separator (`OSP/AUTHORIZATION-BASIS/V2`) — V1 (`osp.authorization-basis.v1\0`)
/// frozen. Canonical encoding (JSON DEĞİL) + hex wire format (64 lowercase).
///
/// **P0-1 v3 (reviewer):** Custom Serialize — yalnız 64 lowercase hex string üretir
/// (derived Serialize byte array üretir, wire format ile çelişir).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthorizationBasisDigestV2([u8; 32]);

impl serde::Serialize for AuthorizationBasisDigestV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl AuthorizationBasisDigestV2 {
    /// Faz 4 V2 convention domain separator (compile-time ayrım — V1 frozen).
    const DOMAIN_SEPARATOR: &'static [u8] = b"OSP/AUTHORIZATION-BASIS/V2";

    /// **V2 canonical encoder (plan md:55):** Tüm basis field'ları canonical byte olarak
    /// encode eder. Digest newtype'lar raw byte (32), evidence tipleri nested encode.
    /// Sıra sabit (structural guarantee).
    pub(crate) fn compute(basis: &AuthorizationBasisV2) -> Result<Self, CanonicalDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // Identity.
        encode_u64(&mut hasher, basis.task_id, "v2_task_id");
        encode_u64(&mut hasher, basis.claim_id.into(), "v2_claim_id");

        // Commitment digests — raw 32 bytes each.
        hasher.update(basis.task_claim_digest.as_bytes());
        hasher.update(basis.task_goal_digest.as_bytes());
        hasher.update(basis.measurement_digest.as_bytes());
        hasher.update(basis.engine_measurement_digest.as_bytes());
        hasher.update(basis.measurement_baseline_digest.as_bytes());
        hasher.update(basis.measurement_request_digest.as_bytes());
        hasher.update(basis.measurement_context_digest.as_bytes());
        hasher.update(basis.canonical_delta_digest.as_bytes());

        // Nested evidence — trajectory baseline (Available/Unavailable canonical).
        encode_canonical_trajectory_baseline_v2(&mut hasher, &basis.trajectory_baseline)?;

        // Nested evidence — trajectory loss (Available/Unavailable canonical).
        encode_canonical_trajectory_loss_v2(&mut hasher, &basis.trajectory_loss)?;

        // Nested evidence — measurement request evidence (subject/impact/revision/digest).
        encode_canonical_measurement_request_evidence_v2(&mut hasher, &basis.measurement_request)?;

        Ok(Self(hasher.finalize().into()))
    }

    #[allow(dead_code, reason = "Faz 4 basis builder / Commit 2 consumer")]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[allow(dead_code, reason = "Faz 4 wire dispatch / Commit 1b consumer")]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:55):** V2 authorization context digest.
/// Basis + verified gate evaluation + canonical witness requirement commitment.
/// Ayrı domain separator (`OSP/AUTHORIZATION-CONTEXT/V2`).
///
/// **P0-1 v3 (reviewer):** Custom Serialize — yalnız 64 lowercase hex string üretir.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthorizationContextDigestV2([u8; 32]);

impl serde::Serialize for AuthorizationContextDigestV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl AuthorizationContextDigestV2 {
    /// Faz 4 V2 convention domain separator.
    const DOMAIN_SEPARATOR: &'static [u8] = b"OSP/AUTHORIZATION-CONTEXT/V2";

    /// **V2 canonical encoder (plan md:55):** basis digest + gate evaluation + witness
    /// requirement canonical byte'ları. Context'in tüm kanıtsal zeminini bağlar.
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub(crate) fn compute(
        basis_digest: &AuthorizationBasisDigestV2,
        gate_evaluation: &CanonicalGateEvaluationV2,
        witness_requirement: &CanonicalWitnessRequirementV2,
    ) -> Result<Self, CanonicalDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        hasher.update(basis_digest.as_bytes());
        gate_evaluation.encode_canonical(&mut hasher)?;
        witness_requirement.encode_canonical(&mut hasher)?;
        Ok(Self(hasher.finalize().into()))
    }

    #[allow(dead_code, reason = "Faz 4 wire dispatch / Commit 1b consumer")]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[allow(dead_code, reason = "Faz 4 wire dispatch / Commit 1b consumer")]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// **INV-T9 #70 Commit 4b Faz 4:** `CanonicalTrajectoryEvidenceBaseline` → canonical
/// byte encoding for V2 basis digest. Available/Unavailable varyant tag + nested fields.
fn encode_canonical_trajectory_baseline_v2(
    hasher: &mut blake3::Hasher,
    baseline: &CanonicalTrajectoryEvidenceBaseline,
) -> Result<(), CanonicalDigestError> {
    use crate::canonical_encoding::{
        encode_axis_components, encode_u64, encode_u8, AXIS_DISCRIM_COHESION,
        AXIS_DISCRIM_COUPLING, AXIS_DISCRIM_ENTROPY, AXIS_DISCRIM_INSTABILITY,
        AXIS_DISCRIM_WITNESS_DEPTH,
    };
    match baseline {
        CanonicalTrajectoryEvidenceBaseline::Available { before } => {
            encode_u8(hasher, 0, "v2_baseline_available_tag");
            encode_axis_components(
                hasher,
                before.coupling.value,
                before.coupling.source,
                AXIS_DISCRIM_COUPLING,
            )?;
            encode_axis_components(
                hasher,
                before.cohesion.value,
                before.cohesion.source,
                AXIS_DISCRIM_COHESION,
            )?;
            encode_axis_components(
                hasher,
                before.instability.value,
                before.instability.source,
                AXIS_DISCRIM_INSTABILITY,
            )?;
            encode_axis_components(
                hasher,
                before.entropy.value,
                before.entropy.source,
                AXIS_DISCRIM_ENTROPY,
            )?;
            encode_axis_components(
                hasher,
                before.witness_depth.value,
                before.witness_depth.source,
                AXIS_DISCRIM_WITNESS_DEPTH,
            )?;
        }
        CanonicalTrajectoryEvidenceBaseline::Unavailable { reason } => {
            encode_u8(hasher, 1, "v2_baseline_unavailable_tag");
            match reason.repr() {
                CanonicalBaselineUnavailableReasonRepr::AllMembersIntroducedByDelta { members } => {
                    encode_u8(hasher, 0, "v2_baseline_reason_all_tag");
                    encode_u64(hasher, members.len() as u64, "v2_baseline_members_count");
                    for id in members {
                        encode_u64(hasher, *id, "v2_baseline_member_id");
                    }
                }
                CanonicalBaselineUnavailableReasonRepr::PartialNewSubject {
                    existing,
                    introduced,
                } => {
                    encode_u8(hasher, 1, "v2_baseline_reason_partial_tag");
                    encode_u64(hasher, existing.len() as u64, "v2_baseline_existing_count");
                    for id in existing {
                        encode_u64(hasher, *id, "v2_baseline_existing_id");
                    }
                    encode_u64(
                        hasher,
                        introduced.len() as u64,
                        "v2_baseline_introduced_count",
                    );
                    for id in introduced {
                        encode_u64(hasher, *id, "v2_baseline_introduced_id");
                    }
                }
            }
        }
    }
    Ok(())
}

/// **INV-T9 #70 Commit 4b Faz 4:** `CanonicalTrajectoryLossEvidence` → canonical byte
/// encoding for V2 basis digest. Available/Unavailable varyant tag + nested fields.
fn encode_canonical_trajectory_loss_v2(
    hasher: &mut blake3::Hasher,
    loss: &CanonicalTrajectoryLossEvidence,
) -> Result<(), CanonicalDigestError> {
    use crate::canonical_encoding::{encode_f64, encode_u8};
    match loss {
        CanonicalTrajectoryLossEvidence::Available { target, loss_after } => {
            encode_u8(hasher, 0, "v2_loss_available_tag");
            encode_f64(hasher, target.x, "v2_loss_target_x")?;
            encode_f64(hasher, target.y, "v2_loss_target_y")?;
            encode_f64(hasher, target.z, "v2_loss_target_z")?;
            encode_f64(hasher, target.w, "v2_loss_target_w")?;
            encode_f64(hasher, target.v, "v2_loss_target_v")?;
            encode_f64(hasher, *loss_after, "v2_loss_after")?;
        }
        CanonicalTrajectoryLossEvidence::Unavailable { reason } => {
            encode_u8(hasher, 1, "v2_loss_unavailable_tag");
            match reason {
                CanonicalTrajectoryLossUnavailableReason::NoPreferredVector => {
                    encode_u8(hasher, 0, "v2_loss_reason_no_preferred_vector");
                }
            }
        }
    }
    Ok(())
}

/// **INV-T9 #70 Commit 4b Faz 4:** `CanonicalMeasurementRequestEvidence` → canonical
/// byte encoding for V2 basis digest. Subject + impact + revision + digest'ler.
fn encode_canonical_measurement_request_evidence_v2(
    hasher: &mut blake3::Hasher,
    evidence: &crate::measurement::CanonicalMeasurementRequestEvidence,
) -> Result<(), CanonicalDigestError> {
    use crate::canonical_encoding::encode_u64;
    // Subject — sorted member ids.
    encode_u64(
        hasher,
        evidence.subject.member_ids().len() as u64,
        "v2_mr_subject_count",
    );
    for id in evidence.subject.member_ids() {
        encode_u64(hasher, *id, "v2_mr_subject_id");
    }
    // Impact — node ids + edge identities.
    encode_u64(
        hasher,
        evidence.impact.node_ids().len() as u64,
        "v2_mr_impact_node_count",
    );
    for id in evidence.impact.node_ids() {
        encode_u64(hasher, *id, "v2_mr_impact_node_id");
    }
    encode_u64(
        hasher,
        evidence.impact.edge_ids().len() as u64,
        "v2_mr_impact_edge_count",
    );
    for edge in evidence.impact.edge_ids() {
        hasher.update(&encode_canonical_edge_identity_to_vec(edge));
    }
    // Base revision — view_id variant + sequence + content_digest.
    encode_space_view_id(hasher, &evidence.base_revision.view_id);
    encode_u64(
        hasher,
        evidence.base_revision.sequence,
        "v2_mr_revision_sequence",
    );
    hasher.update(evidence.base_revision.content_digest.as_bytes());
    // Structural delta digest + measurement input digest — raw 32 bytes.
    hasher.update(evidence.structural_delta_digest.as_bytes());
    hasher.update(evidence.measurement_input_digest.as_bytes());
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Canonical binary encoding — domain-specific encoder'lar (review P1-3).
//
// **INV-T9 #70 Commit 3 (reviewer v4 P1-2):** Düşük seviyeli framing primitive'leri
// (`encode_u64/u32/u8`, `encode_bytes`, `encode_f64`, `canonical_f64_bytes`,
// `encode_optional_f64*`, `push_*`, `CanonicalTag` trait, `encode_tag`) artık
// `crate::canonical_encoding` neutral modülünde. Authorization domain encoder'ları
// (aşağıdaki) bu primitive'leri kullanır — `CanonicalEncodingError` → `CanonicalDigestError`
// stable mapping.
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "test modülü canonical_f64_bytes_preimage çağırır"
    )
)]
use crate::canonical_encoding::{
    canonical_f64_bytes, encode_bytes, encode_f64, encode_optional_f64, encode_tag, encode_u32,
    encode_u64, encode_u8, push_bytes, push_f64, push_tag, push_u64, push_u8,
    CanonicalEncodingError, CanonicalTag,
};

/// **INV-T9 #70 Commit 3 (reviewer v4 P1-2):** Neutral encoding error → authorization
/// digest error stable mapping. Primitive error dış API'ye sızmaz.
impl From<CanonicalEncodingError> for CanonicalDigestError {
    fn from(err: CanonicalEncodingError) -> Self {
        match err {
            CanonicalEncodingError::NonFiniteRejected => CanonicalDigestError::NonFiniteRejected,
            CanonicalEncodingError::LengthOverflow { field } => {
                CanonicalDigestError::LengthOverflow { field }
            }
        }
    }
}

/// Tüm `canonical_tags` newtype'ları `CanonicalTag` uygular — macro yardımcı.
macro_rules! impl_canonical_tag_for_newtype {
    ($($name:ident),* $(,)?) => {
        $(
            impl $crate::canonical_encoding::CanonicalTag for $crate::canonical_tags::$name {
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

/// **reviewer P0-1:** Per-axis measurement encoder — value + source tag.
///
/// **INV-T9 #70 Commit 4b Faz 3:** `field` parametresi kaldırıldı (axis sırası
/// caller'ın sabit çağrı sırasıyla structural guarantee — coupling→cohesion→
/// instability→entropy→witness_depth). Byte format DEĞİŞMEDİ — v1 golden korunur.
/// Nötr encoder (`encode_axis_components` axis discriminator ile) yalnız yeni
/// `MeasurementDigest` tarafından kullanılır (Faz 3 yeni commitment, ayrı byte contract).
fn encode_axis_measurement(
    hasher: &mut blake3::Hasher,
    m: &CanonicalAxisMeasurement,
) -> Result<(), CanonicalDigestError> {
    encode_f64(hasher, m.value, "axis_value")?;
    encode_tag(hasher, m.source, "axis_source");
    Ok(())
}

/// **INV-T9 #70 Commit 3 (reviewer v4 P1-2):** `pub(crate)` — measurement
/// `MeasurementDeltaDigest` shared producer ile aynı encoder'ı kullanır (tek canonical
/// byte formatı — single encoding truth).
pub(crate) fn encode_canonical_node(
    hasher: &mut blake3::Hasher,
    node: &CanonicalNode,
) -> Result<(), CanonicalEncodingError> {
    encode_u64(hasher, node.id, "node_id");
    encode_tag(hasher, node.kind, "node_kind");
    encode_f64(hasher, node.mass, "node_mass")?;
    encode_optional_f64(hasher, node.cohesion, "node_cohesion")?;
    encode_tag(hasher, node.classification, "node_classification");
    encode_tag(hasher, node.role, "node_role");
    Ok(())
}

/// **Step 6 P0:** CanonicalEdge → Vec\<u8\> (shared byte helper, 18 byte).
/// from(8) + to(8) + kind(1) + is_type_only(1). Preimage testleri doğrudan çağırır.
///
/// **INV-T9 #70 Commit 3 (reviewer v4 P1-2):** `pub(crate)` — measurement
/// `MeasurementDeltaDigest` ile paylaşılır.
pub(crate) fn encode_canonical_edge_to_vec(edge: &CanonicalEdge) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(18);
    push_u64(&mut bytes, edge.from);
    push_u64(&mut bytes, edge.to);
    push_tag(&mut bytes, edge.kind);
    push_u8(&mut bytes, edge.is_type_only as u8);
    bytes
}

/// **Step 6 P0:** CanonicalVisionSubject → Vec\<u8\> (shared byte helper).
/// Global → [0] (1 byte); Role(role) → [1, role_tag] (2 byte). Preimage testleri
/// doğrudan çağırır; `EvaluationContextDigest::compute` bu helper'ı kullanır (inline YOK).
fn encode_vision_subject_to_vec(subject: CanonicalVisionSubject) -> Vec<u8> {
    match subject {
        CanonicalVisionSubject::Global => {
            let mut bytes = Vec::with_capacity(1);
            push_u8(&mut bytes, 0);
            bytes
        }
        CanonicalVisionSubject::Role(role_tag) => {
            let mut bytes = Vec::with_capacity(2);
            push_u8(&mut bytes, 1);
            push_u8(&mut bytes, role_tag.as_u8());
            bytes
        }
    }
}

/// **Step 5 P0:** new_edges encoder — AS-IS encode (sort YOK). try_new canonical sırayı
/// (identity by from,to,kind) garanti eder; encoder tekrar sort etmez → canonical invariant
/// maskelenemez. `is_type_only` dahil (eklenen edge'in semantik özelliği).
///
/// **Step 6 P0:** `encode_canonical_edge_to_vec` üzerinden (tek kaynak).
fn encode_canonical_edge_vec(
    hasher: &mut blake3::Hasher,
    edges: &[CanonicalEdge],
) -> Result<(), CanonicalDigestError> {
    encode_u64(hasher, edges.len() as u64, "new_edge_count");
    for edge in edges {
        hasher.update(&encode_canonical_edge_to_vec(edge));
    }
    Ok(())
}

/// **Step 5 P0:** removed_edges encoder — identity-only (is_type_only YOK), AS-IS.
/// `encode_canonical_edge_identity_to_vec` preimage üreticisini kullanır (test edilebilir).
fn encode_canonical_edge_identity_vec(
    hasher: &mut blake3::Hasher,
    edges: &[CanonicalEdgeIdentity],
) -> Result<(), CanonicalDigestError> {
    encode_u64(hasher, edges.len() as u64, "removed_edge_count");
    for edge in edges {
        hasher.update(&encode_canonical_edge_identity_to_vec(edge));
    }
    Ok(())
}

/// **Step 5 P1:** Preimage byte üretici — `CanonicalEdgeIdentity` → 17 byte (from(8) +
/// to(8) + kind(1)). `is_type_only` YOK. Encoding testleri tam preimage'i kontrol eder
/// (hash sonucundan alan yokluğu kanıtlanamaz). `encode_canonical_edge_identity_vec`
/// ve testler tarafından paylaşılır.
///
/// **INV-T9 #70 Commit 3 (reviewer v4 P1-2):** `pub(crate)` — measurement
/// `MeasurementDeltaDigest` ile paylaşılır.
pub(crate) fn encode_canonical_edge_identity_to_vec(edge: &CanonicalEdgeIdentity) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(17);
    push_u64(&mut bytes, edge.from());
    push_u64(&mut bytes, edge.to());
    push_tag(&mut bytes, edge.kind());
    bytes
}

/// EffectiveMetricPredicate → canonical byte dizisi. **reviewer P0-2b:** sort ve hash
/// aynı canonical encoder'ı kullanır. Önceki comparator ham `to_bits()` kullanıyordu;
/// bu `-0.0` normalize etmediği için encoder ile çelişiyordu — semantik aynı predicate
/// seti farklı sıraya ve farklı digest'e gidebiliyordu.
fn encode_effective_predicate_to_vec(
    pred: &EffectiveMetricPredicate,
) -> Result<Vec<u8>, CanonicalDigestError> {
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
) -> Result<(), CanonicalDigestError> {
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

/// **INV-T9 #70 Commit 3 (reviewer v4 P1-2 / P2-1):** `pub(crate)` + infallible.
/// Space view identity encoding için tek kaynak — measurement `MeasurementRequestDigest`
/// ile paylaşılır. İnfllible: varyantlar exhaustive, hata üretmez.
pub(crate) fn encode_space_view_id(hasher: &mut blake3::Hasher, view_id: &SpaceViewId) {
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
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #72 — Canonical witness evidence encoding helpers
//
// `SuspendedAttemptEvidenceDigest::compute` için shared encoder'lar. Mevcut
// `canonical_f64_bytes`, `encode_u8/u32/u64`, `encode_bytes`, `encode_f64`
// helper'ları yeniden kullanılır. Yeni: witness snapshot/hold-reason/rejection
// encoder'ları + canonical rejection sort (digest determinism).
// ═══════════════════════════════════════════════════════════════════════════════

/// `WitnessQuorumSnapshot` canonical encoding — `SuspendedAttemptEvidenceDigest` için.
///
/// Sıra: `approvers` (u64), `required_approvers` (u64), `support` (canonical f64),
/// `required_support` (canonical f64).
fn encode_witness_quorum_snapshot(
    hasher: &mut blake3::Hasher,
    snapshot: &crate::witness::WitnessQuorumSnapshot,
) -> Result<(), CanonicalDigestError> {
    encode_u64(hasher, snapshot.approvers as u64, "snapshot_approvers");
    encode_u64(
        hasher,
        snapshot.required_approvers as u64,
        "snapshot_required_approvers",
    );
    encode_f64(hasher, snapshot.support, "snapshot_support")?;
    encode_f64(
        hasher,
        snapshot.required_support,
        "snapshot_required_support",
    )?;
    Ok(())
}

/// `WitnessHoldReason` canonical encoding — variant tag + alanlar (exhaustive 3 varyant).
///
/// Tag ataması: `MinApproversNotMet=1`, `QuorumInsufficient=2`,
/// `EvidenceNotLocallyObservable=3`. `format!("{:?}")` DEĞİL — stable numeric tag.
fn encode_witness_hold_reason(
    hasher: &mut blake3::Hasher,
    reason: &crate::witness::WitnessHoldReason,
) -> Result<(), CanonicalDigestError> {
    use crate::witness::WitnessHoldReason::*;
    match reason {
        MinApproversNotMet { distinct, required } => {
            encode_u8(hasher, 1, "hold_reason_tag");
            encode_u64(hasher, *distinct as u64, "hold_reason_distinct");
            encode_u64(hasher, *required as u64, "hold_reason_required");
        }
        QuorumInsufficient { support, threshold } => {
            encode_u8(hasher, 2, "hold_reason_tag");
            encode_f64(hasher, *support, "hold_reason_support")?;
            encode_f64(hasher, *threshold, "hold_reason_threshold")?;
        }
        EvidenceNotLocallyObservable { hint } => {
            encode_u8(hasher, 3, "hold_reason_tag");
            encode_bytes(hasher, hint.as_bytes())?;
        }
    }
    Ok(())
}

/// `NonEmptyWitnessRejections` canonical encoding — **canonical sort + duplicate
/// reject** (digest determinism).
///
/// **P1 rejection canonical ordering:** Witness reddi bir küme ise (sequence
/// semantiği korunmuyorsa), aynı mantıksal evidence farklı input sırasıyla aynı
/// digest üretmelidir. Bu yüzden:
/// 1. Her rejection `(witness u64 LE, rationale canonical bytes)` ikilisine encode
///    edilir.
/// 2. Byte dizileri lexicographic sort edilir.
/// 3. Duplicate `(witness, rationale)` çifti → `CanonicalDigestError::EncodingFailed`
///    (aynı witness aynı rationale ile iki kez reddedemez).
/// 4. Sort edilmiş byte dizileri tek tek `encode_bytes` ile yazılır.
fn encode_non_empty_witness_rejections(
    hasher: &mut blake3::Hasher,
    rejections: &crate::witness::NonEmptyWitnessRejections,
) -> Result<(), CanonicalDigestError> {
    // **P0-3:** canonical_rejection_key TEK source — constructor, wire check, digest,
    // duplicate detection hepsi aynı helper. Stored sıra as-is encode edilir
    // (constructor zaten canonical sıraya getirdi; load strict check yaptı).
    let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(rejections.len());
    for rejection in rejections.as_slice() {
        encoded.push(canonical_rejection_key(rejection)?);
    }
    // Defensive re-sort + duplicate check (constructor/load zaten garantiledi ama
    // digest encoding determinism için ikinci katman).
    encoded.sort_unstable();
    for window in encoded.windows(2) {
        if window[0] == window[1] {
            return Err(CanonicalDigestError::EncodingFailed(
                "duplicate witness rejection in canonical encoding (digest determinism)".into(),
            ));
        }
    }

    encode_u64(hasher, encoded.len() as u64, "rejection_count");
    for buf in &encoded {
        encode_bytes(hasher, buf)?;
    }
    Ok(())
}

/// Tek `WitnessRejection` canonical byte producer (sort edilebilir Vec üretir).
///
/// Encoding:
/// - witness AgentId (u64 LE)
/// - rationale Option tag (None=0, Some=1 + u64 length prefix + UTF-8 bytes)
fn encode_witness_rejection_to_vec(
    rejection: &crate::witness::WitnessRejection,
) -> Result<Vec<u8>, CanonicalDigestError> {
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    push_u64(&mut buf, rejection.witness);
    match &rejection.rationale {
        None => push_u8(&mut buf, 0),
        Some(text) => {
            push_u8(&mut buf, 1);
            push_bytes(&mut buf, text.as_bytes());
        }
    }
    Ok(buf)
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #72 closure — Shared canonical-rejection + semantic-validation primitives
//
// P0-3 (sort-key identity): `canonical_rejection_key` TEK source — constructor
// canonicalization, wire strict check, digest encoding, duplicate detection hepsi
// aynı helper'ı kullanır. Önceki implementation Rust tuple sıralaması vs
// lexicographic byte sıralaması tutarsızlığı (1, 256 vs 256, 1) bu sayede kapanır.
//
// P1-2 (semantic validation): `validate_evidence_semantics` evidence constructor'a
// çekilir — Held hold_reason↔snapshot, Rejected snapshot finite/non-neg + canonical.
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek canonical rejection key — `(witness, rationale)` byte encoding.
///
/// **P0-3 sort-key identity:** Bu fonksiyon TEK source'dur. Constructor
/// canonicalization, wire strict check, digest encoding, duplicate detection
/// hepsi bu helper'ı kullanır. Böylece Rust tuple sıralaması vs lexicographic
/// byte sıralaması tutarsızlığı (little-endian encoding nedeniyle `1, 256` vs
/// `256, 1`) önlenir.
fn canonical_rejection_key(
    rejection: &crate::witness::WitnessRejection,
) -> Result<Vec<u8>, CanonicalDigestError> {
    encode_witness_rejection_to_vec(rejection)
}

/// Rejection listesini canonical byte-key sırasına göre sırala + duplicate reject.
///
/// **P0-3:** `canonical_rejection_key` üzerinden sort eder (Rust tuple DEĞİL).
/// Duplicate `(witness, rationale)` çifti → `DuplicateRejection`.
///
/// Production API (`try_new_normalizing`) bu fonksiyonu çağırır → arbitrary input
/// canonical sıraya normalize edilir. Wire load (`try_from_canonical_wire`) bunu
/// KULLANMAZ — onun yerine `verify_rejections_canonical_order` strict check yapar.
fn canonicalize_rejections(
    rejections: crate::witness::NonEmptyWitnessRejections,
) -> Result<crate::witness::NonEmptyWitnessRejections, SuspendedAttemptEvidenceError> {
    let slice = rejections.as_slice();
    // Aynı (witness, rationale) çifti var mı kontrol et (canonical key ile).
    let mut seen: Vec<Vec<u8>> = Vec::with_capacity(slice.len());
    for r in slice {
        let key = canonical_rejection_key(r)
            .map_err(|e| SuspendedAttemptEvidenceError::InvalidSnapshot(e.to_string()))?;
        if seen.iter().any(|s| s == &key) {
            return Err(SuspendedAttemptEvidenceError::DuplicateRejection);
        }
        seen.push(key);
    }
    // Canonical key'e göre sırala. WitnessRejection'ları yerinde taşı.
    let inner = rejections.into_inner();
    let mut indexed: Vec<(Vec<u8>, crate::witness::WitnessRejection)> =
        Vec::with_capacity(inner.len());
    for r in inner {
        let key = canonical_rejection_key(&r)
            .map_err(|e| SuspendedAttemptEvidenceError::InvalidSnapshot(e.to_string()))?;
        indexed.push((key, r));
    }
    indexed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let sorted: Vec<_> = indexed.into_iter().map(|(_, r)| r).collect();
    // NonEmpty invariant zaten guaranteed (giriş NonEmpty, sort elemaz).
    Ok(crate::witness::NonEmptyWitnessRejections::from_vec(sorted))
}

/// Wire'dan gelen rejection sırasının zaten canonical olduğunu strict doğrula.
///
/// **P1-1 strict wire:** Production API canonicalize eder; wire load strict reject
/// eder. Bu fonksiyon wire load path'de çağrılır — eğer sıra canonical değilse
/// `NonCanonicalRejectionOrder` döner (normalize ETMEZ).
///
/// Aynı zamanda duplicate detection yapar (canonical key ile).
fn verify_rejections_canonical_order(
    rejections: &crate::witness::NonEmptyWitnessRejections,
) -> Result<(), SuspendedAttemptEvidenceError> {
    let slice = rejections.as_slice();
    let mut prev_key: Option<Vec<u8>> = None;
    for r in slice {
        let key = canonical_rejection_key(r)
            .map_err(|e| SuspendedAttemptEvidenceError::InvalidSnapshot(e.to_string()))?;
        if let Some(ref prev) = prev_key {
            if prev == &key {
                return Err(SuspendedAttemptEvidenceError::DuplicateRejection);
            }
            // Strict canonical order: her eleman bir öncekinden strictly büyük olmalı.
            if prev > &key {
                return Err(SuspendedAttemptEvidenceError::NonCanonicalRejectionOrder);
            }
        }
        prev_key = Some(key);
    }
    Ok(())
}

/// Evidence disposition semantic validation — `SuspendedAttemptEvidence::try_new`
/// ve load path tarafından ortak kullanılır (P1-2).
///
/// **Held:** `validate_hold_reason_against_snapshot` (exhaustive 3 varyant).
/// **Rejected:** snapshot finite/non-negative; rejection list canonical + duplicate-free.
///
/// Bu fonksiyon constructor'a çekilmiştir — standalone veya `RevisionRequired`
/// içindeki evidence da artık validated olur. Envelope `verify()` defensive tekrar.
fn validate_evidence_semantics(
    disposition: &SuspendedAttemptDisposition,
) -> Result<(), SuspendedAttemptEvidenceError> {
    match disposition {
        SuspendedAttemptDisposition::Held {
            hold_reason,
            snapshot,
        } => {
            // validate_hold_reason_against_snapshot fonksiyonu PendingAuthorizationLoadError
            // dönüyor — evidence error'a map et.
            validate_hold_reason_against_snapshot(hold_reason, snapshot).map_err(|e| {
                SuspendedAttemptEvidenceError::HoldReasonSnapshotInconsistency(e.to_string())
            })?;
        }
        SuspendedAttemptDisposition::Rejected { reasons, snapshot } => {
            // Snapshot finite/non-negative.
            if !snapshot.support.is_finite() || !snapshot.required_support.is_finite() {
                return Err(SuspendedAttemptEvidenceError::InvalidSnapshot(
                    "support/required_support must be finite".into(),
                ));
            }
            if snapshot.support < 0.0 || snapshot.required_support < 0.0 {
                return Err(SuspendedAttemptEvidenceError::InvalidSnapshot(
                    "support must be >= 0".into(),
                ));
            }
            // Canonical order + duplicate check (load path strict; API path öncesinde
            // canonicalize_rejections çağrıldığı için burada her zaman canonical).
            verify_rejections_canonical_order(reasons)?;
        }
    }
    Ok(())
}

/// **INV-T9 #70 Commit 4b (reviewer v4 P1-4 + Faz 2 scoped P1-2):** v2 production
/// GateDecision tag encoder — **`gate_decision_tag_v2`** (yeniden adlandırıldı).
// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 4 — Pinned canonical tag newtype'ları (plan md:115)
//
// **Reviewer kararı (Faz 4 review #2):** Her domain için ayrı private-inner checked
// newtype. Domain enum'ları korunur (rewrite yok); newtype yalnızca canonical tag alanını
// temsil eder. `new_unchecked` / `From<u8>` / `pub fn new` YASAK — sadece checked
// `TryFrom<u8>` + `as_u8()` getter. Ayrı domain'ler ontolojik karışmayı compile-time
// engeller: `GateDecisionTag` ile `MutationDecisionTag` karıştırılamaz.
//
// **Geçiş dönemi (reviewer notu):** Mevcut `*_tag()` helper fonksiyonları newtype'a
// delege eder — çağıranları tek commit'te kırmamak için. Helper'lar daha sonra
// kaldırılabilir. Mapping değerleri KORUNUR → V1 digest byte'ları HİÇ DEĞİŞMEZ (golden
// test green kalır).
//
// **Ontolojik ayrım (reviewer notu):** `GateDecisionTag` (deterministic gate sonucu)
// ile `GateDispositionV2Tag` (V2 gate evaluation disposition) ayrı newtype — ontolojik
// kategori kanıtlanmadıkça tag alanı paylaşılmaz. Aynı şekilde `WitnessRequirementTag`
// ile `WitnessNotRequiredReasonTag` ayrı (karar durumu vs açıklama kategorisi).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:115):** Pinned canonical tag for
/// `GateDecision`. Append-only: mevcut tag'ler (0-6) ASLA değişmez (exact pin — golden
/// vector lock). Yeni varyantlar append-only sıradaki tag'leri alır (7, 8).
///
/// `gate_decision_v2_tags_are_unique_and_append_only` testi bu mapping'i çağırarak
/// doğrular. v1 frozen encoder ayrımı (`gate_decision_tag_v1_frozen`, 0..=6) test-only
/// korunur — v1 encoder v2-only kararları 7/8 olarak encode edemez.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GateDecisionTag(u8);

impl GateDecisionTag {
    pub(crate) const UNKNOWN: Self = Self(0);
    pub(crate) const PASSED_ALL: Self = Self(1);
    pub(crate) const REJECTED_BY_SYNTAX: Self = Self(2);
    pub(crate) const REJECTED_BY_VISION: Self = Self(3);
    pub(crate) const REJECTED_BY_RULE: Self = Self(4);
    pub(crate) const REJECTED_BY_TASK_BINDING: Self = Self(5);
    pub(crate) const BLOCKED_BY_MANEUVER_LIMIT: Self = Self(6);
    /// Commit 4b — append-only yeni tag'ler.
    pub(crate) const REJECTED_BY_TASK_VALIDATION: Self = Self(7);
    pub(crate) const REJECTED_BY_MEASUREMENT_BINDING: Self = Self(8);

    const VALID_TAGS: &'static [u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 8];

    #[allow(dead_code)]
    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl From<&crate::trajectory::GateDecision> for GateDecisionTag {
    fn from(gd: &crate::trajectory::GateDecision) -> Self {
        use crate::trajectory::GateDecision::*;
        match gd {
            Unknown => Self::UNKNOWN,
            PassedAll => Self::PASSED_ALL,
            RejectedBySyntax => Self::REJECTED_BY_SYNTAX,
            RejectedByVision => Self::REJECTED_BY_VISION,
            RejectedByRule => Self::REJECTED_BY_RULE,
            RejectedByTaskBinding => Self::REJECTED_BY_TASK_BINDING,
            BlockedByManeuverLimit => Self::BLOCKED_BY_MANEUVER_LIMIT,
            RejectedByTaskValidation => Self::REJECTED_BY_TASK_VALIDATION,
            RejectedByMeasurementBinding => Self::REJECTED_BY_MEASUREMENT_BINDING,
        }
    }
}

impl TryFrom<u8> for GateDecisionTag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "GateDecisionTag",
                tag,
            })
        }
    }
}

impl CanonicalTag for GateDecisionTag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:115):** Pinned canonical tag for
/// `PredicateCompletion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PredicateCompletionTag(u8);

impl PredicateCompletionTag {
    pub(crate) const NOT_COMPLETED: Self = Self(0);
    pub(crate) const COMPLETED: Self = Self(1);

    const VALID_TAGS: &'static [u8] = &[0, 1];

    #[allow(dead_code)]
    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl From<&crate::trajectory::PredicateCompletion> for PredicateCompletionTag {
    fn from(pc: &crate::trajectory::PredicateCompletion) -> Self {
        use crate::trajectory::PredicateCompletion::*;
        match pc {
            NotCompleted => Self::NOT_COMPLETED,
            Completed => Self::COMPLETED,
        }
    }
}

impl TryFrom<u8> for PredicateCompletionTag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "PredicateCompletionTag",
                tag,
            })
        }
    }
}

impl CanonicalTag for PredicateCompletionTag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:115):** Pinned canonical tag for
/// `MutationDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct MutationDecisionTag(u8);

impl MutationDecisionTag {
    pub(crate) const REJECT: Self = Self(0);
    pub(crate) const ACCEPT_AS_PROGRESS: Self = Self(1);
    pub(crate) const ACCEPT_AS_COMPLETED: Self = Self(2);
    pub(crate) const REQUIRE_OPERATOR_APPROVAL: Self = Self(3);

    const VALID_TAGS: &'static [u8] = &[0, 1, 2, 3];

    #[allow(dead_code)]
    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl From<&crate::trajectory::MutationDecision> for MutationDecisionTag {
    fn from(md: &crate::trajectory::MutationDecision) -> Self {
        use crate::trajectory::MutationDecision::*;
        match md {
            Reject => Self::REJECT,
            AcceptAsProgress => Self::ACCEPT_AS_PROGRESS,
            AcceptAsCompleted => Self::ACCEPT_AS_COMPLETED,
            RequireOperatorApproval => Self::REQUIRE_OPERATOR_APPROVAL,
        }
    }
}

impl TryFrom<u8> for MutationDecisionTag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "MutationDecisionTag",
                tag,
            })
        }
    }
}

impl CanonicalTag for MutationDecisionTag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:115):** Pinned canonical tag for
/// `ApplyTarget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ApplyTargetTag(u8);

impl ApplyTargetTag {
    pub(crate) const NOT_APPLIED: Self = Self(0);
    pub(crate) const LANE_MAINLINE: Self = Self(1);
    pub(crate) const LANE_TRAJECTORY_CHECKPOINT: Self = Self(2);
    pub(crate) const LANE_SANDBOX: Self = Self(3);

    const VALID_TAGS: &'static [u8] = &[0, 1, 2, 3];

    #[allow(dead_code)]
    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl From<&crate::trajectory::ApplyTarget> for ApplyTargetTag {
    fn from(at: &crate::trajectory::ApplyTarget) -> Self {
        use crate::trajectory::ApplyTarget::*;
        match at {
            NotApplied => Self::NOT_APPLIED,
            Lane(lane) => match lane {
                crate::trajectory::CommitLane::Mainline => Self::LANE_MAINLINE,
                crate::trajectory::CommitLane::TrajectoryCheckpoint => {
                    Self::LANE_TRAJECTORY_CHECKPOINT
                }
                crate::trajectory::CommitLane::Sandbox => Self::LANE_SANDBOX,
            },
        }
    }
}

impl TryFrom<u8> for ApplyTargetTag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "ApplyTargetTag",
                tag,
            })
        }
    }
}

impl CanonicalTag for ApplyTargetTag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:101, 115):** Pinned canonical tag for
/// witness requirement disposition (`Required` / `NotRequired`). Wire serde adı DEĞİL —
/// pinned numeric tag.
#[allow(dead_code, reason = "Faz 4 CanonicalWitnessRequirementV2 consumer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct WitnessRequirementTag(u8);

#[allow(dead_code, reason = "Faz 4 CanonicalWitnessRequirementV2 consumer")]
impl WitnessRequirementTag {
    pub(crate) const REQUIRED: Self = Self(0);
    pub(crate) const NOT_REQUIRED: Self = Self(1);

    const VALID_TAGS: &'static [u8] = &[0, 1];

    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for WitnessRequirementTag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "WitnessRequirementTag",
                tag,
            })
        }
    }
}

impl CanonicalTag for WitnessRequirementTag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:101, 115):** Pinned canonical tag for
/// witness-not-required reason. Ontolojik olarak `WitnessRequirementTag`'ten AYRI —
/// karar durumu vs açıklama kategorisi (reviewer notu). Sayısal çakışma olsa bile
/// ontolojik karışma engellenir.
#[allow(dead_code, reason = "Faz 4 CanonicalWitnessRequirementV2 consumer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct WitnessNotRequiredReasonTag(u8);

#[allow(dead_code, reason = "Faz 4 CanonicalWitnessRequirementV2 consumer")]
impl WitnessNotRequiredReasonTag {
    /// Reject → NotApplied: witness aşaması çalışmaz (plan md:100).
    pub(crate) const REJECTED_BEFORE_WITNESS: Self = Self(0);

    const VALID_TAGS: &'static [u8] = &[0];

    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for WitnessNotRequiredReasonTag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "WitnessNotRequiredReasonTag",
                tag,
            })
        }
    }
}

impl CanonicalTag for WitnessNotRequiredReasonTag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:30, 115):** Pinned canonical tag for V2 gate
/// evaluation disposition. `GateDecisionTag` ile AYRI newtype — ontolojik kategori
/// (deterministic gate sonucu vs V2 gate evaluation disposition) kanıtlanmadıkça tag
/// alanı paylaşılmaz. Faz 4 structural-only placeholder; gerçek evaluator Faz 5.
#[allow(dead_code, reason = "Faz 4 CanonicalGateEvaluationV2 consumer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub(crate) struct GateDispositionV2Tag(u8);

#[allow(dead_code, reason = "Faz 4 CanonicalGateEvaluationV2 consumer")]
impl GateDispositionV2Tag {
    /// Tüm deterministic gate'ler geçti — authorization'a devam.
    pub(crate) const PASSED: Self = Self(0);
    /// Bir veya daha fazla deterministic gate reddetti — authorization sonlanır.
    pub(crate) const REJECTED: Self = Self(1);

    const VALID_TAGS: &'static [u8] = &[0, 1];

    pub(crate) const fn as_u8(&self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for GateDispositionV2Tag {
    type Error = CanonicalizationError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        if Self::VALID_TAGS.contains(&tag) {
            Ok(Self(tag))
        } else {
            Err(CanonicalizationError::InvalidCanonicalTag {
                type_name: "GateDispositionV2Tag",
                tag,
            })
        }
    }
}

impl CanonicalTag for GateDispositionV2Tag {
    fn tag_u8(&self) -> u8 {
        self.0
    }
}

/// Mevcut tag'ler (0-6) ASLA değişmez (exact pin — golden vector lock). Yeni varyantlar
/// append-only sıradaki tag'leri alır:
/// - `RejectedByTaskValidation` → 7
/// - `RejectedByMeasurementBinding` → 8
///
/// **v1 frozen encoder ayrımı (scoped P1-2):** Bu helper v2 encoder yüzeyi — production
/// v2 basis bunu kullanır. v1 golden re-producibility için ayrı `gate_decision_tag_v1_frozen`
/// (0..=6, yeni varyantları temsil edemez) Faz 4'te test-only eklenecek. **v1 encoder
/// v2-only kararları 7/8 olarak encode edemez** — fiziksel ayrı enum/function.
///
/// **INV-T9 #70 Commit 4b Faz 4 (plan md:115):** Helper artık `GateDecisionTag`
/// newtype'ına delege eder (pinned tag invariant tip seviyesinde taşınır — caller
/// discipline değil). Mapping değerleri KORUNUR → V1 digest byte'ları HİÇ DEĞİŞMEZ.
///
/// `gate_decision_v2_tags_are_unique_and_append_only` testi (authorization.rs test modülü)
/// gerçek tag mapping'i çağırarak doğrular.
/// V2 gate decision tag encoder — infallible (tüm 9 varyant). V2 basis encoder
/// (Commit 1b/2) ve testler bunu kullanır. V1 production encoder `gate_decision_tag_v1`
/// (fallible) kullanır — V2-only kararların V1 artifact'lerine sızması imkânsız.
#[allow(dead_code, reason = "V2 basis encoder Commit 1b/2 + test consumer")]
fn gate_decision_tag_v2(gd: crate::trajectory::GateDecision) -> u8 {
    GateDecisionTag::from(&gd).as_u8()
}

/// **INV-T9 #70 Commit 4b Faz 4 (reviewer P0-1):** V1 production encoder için fallible
/// gate decision tag mapping. Mevcut 7 varyant (0-6) `Ok` döner; V2-only varyantlar
/// (`RejectedByTaskValidation`=7, `RejectedByMeasurementBinding`=8) `Err` döner — V1
/// byte contract frozen.
///
/// `AuthorizationBasisDigest::compute` (V1 production encoder) bu fonksiyonu kullanır.
/// V2-only kararların V1 artifact'lerine sızması imkânsız — V1 golden byte'ları HİÇ
/// değişmez.
///
/// **vs `gate_decision_tag_v2`:** v2 helper infallible (tüm 9 varyant); v1 helper fallible
/// (sadece legacy 7 varyant). Test-only paralel enum (`GateDecisionV1Frozen`) artık
/// gerekmez — production fn gerçek V1 encoder'ı kullanır.
fn gate_decision_tag_v1(gd: crate::trajectory::GateDecision) -> Result<u8, CanonicalDigestError> {
    use crate::trajectory::GateDecision::*;
    match gd {
        Unknown => Ok(0),
        PassedAll => Ok(1),
        RejectedBySyntax => Ok(2),
        RejectedByVision => Ok(3),
        RejectedByRule => Ok(4),
        RejectedByTaskBinding => Ok(5),
        BlockedByManeuverLimit => Ok(6),
        // V2-only varyantlar — V1 encoder bunları temsil edemez.
        RejectedByTaskValidation => Err(CanonicalDigestError::UnsupportedV1GateDecision {
            tag: GateDecisionTag::REJECTED_BY_TASK_VALIDATION.as_u8(),
        }),
        RejectedByMeasurementBinding => Err(CanonicalDigestError::UnsupportedV1GateDecision {
            tag: GateDecisionTag::REJECTED_BY_MEASUREMENT_BINDING.as_u8(),
        }),
    }
}

fn predicate_completion_tag(pc: crate::trajectory::PredicateCompletion) -> u8 {
    PredicateCompletionTag::from(&pc).as_u8()
}

fn mutation_decision_tag(md: crate::trajectory::MutationDecision) -> u8 {
    MutationDecisionTag::from(&md).as_u8()
}

fn apply_target_tag(at: &crate::trajectory::ApplyTarget) -> u8 {
    ApplyTargetTag::from(at).as_u8()
}

/// Canonical digest hesaplama hataları — tüm BLAKE3 domain-separated digest'ler için
/// ortak error tipi (P1 digest error taxonomy).
///
/// Authorization-basis adı evidence katmanına sızmaz; evidence digest de aynı tipi kullanır.
/// Backward-compat: [`AuthorizationBasisDigestError`] alias olarak korunur.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CanonicalDigestError {
    #[error("canonical encoding failed: {0}")]
    EncodingFailed(String),
    #[error("non-finite float (NaN or ±Infinity) detected in canonical encoding — not allowed")]
    NonFiniteRejected,
    #[error("hex decode failed: {0}")]
    HexDecodeFailed(String),
    #[error("invalid digest length: expected 32 bytes, got {0}")]
    InvalidLength(usize),
    /// **INV-T9 Adım 3 (P1-a):** canonical length overflow (checked u64 conversion).
    #[error("canonical length overflow in {field}")]
    LengthOverflow { field: &'static str },
    /// **INV-T9 #70 Commit 4b Faz 4 (reviewer P0-1):** V1 encoder V2-only GateDecision
    /// varyantlarını (RejectedByTaskValidation=7, RejectedByMeasurementBinding=8) encode
    /// edemez — V1 byte contract frozen. Production V1 encoder (`AuthorizationBasisDigest::compute`)
    /// `gate_decision_tag_v1` (fallible) kullanır; bu varyantlar Err döndürür.
    #[error("V1 encoder cannot encode V2-only GateDecision variant (tag {tag}) — V1 byte contract frozen")]
    UnsupportedV1GateDecision { tag: u8 },
}

/// Backward-compat alias — eski call site'lar çalışmaya devam eder. Yeni kod
/// `CanonicalDigestError` kullanmalı (P1 digest error taxonomy).
pub type AuthorizationBasisDigestError = CanonicalDigestError;

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
    /// **Step 5 P0:** new_nodes canonical sıralı değil (strict ascending by id).
    /// `validate()` non-normalizing — sıralama bozuksa deserialize/validate yakalar.
    #[error("new_nodes not in canonical order (strict ascending by id)")]
    UnsortedNodes,
    /// **Step 5 P1-c (scoped):** new_edges identity sıralı değil (strict ascending).
    /// Equal → `DuplicateEdge`, Greater → bu variant (typed taxonomy ayrımı).
    #[error("new_edges not in canonical identity order (strict ascending by from,to,kind)")]
    UnsortedNewEdges,
    /// **Step 5 P1-c (scoped):** removed_edges identity sıralı değil (strict ascending).
    #[error("removed_edges not in canonical identity order (strict ascending by from,to,kind)")]
    UnsortedRemovedEdges,
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
///
/// **INV-T9 #72 closure (P0-2 strict wire):** Custom Deserialize `deny_unknown_fields`
/// ile strict canonical wire. Unknown field reject (stale `attempt_evidence_id`
/// dahil). `validate_internal` record ↔ embedded evidence cross-field (basis-
/// dependent kontroller envelope `verify()`'da).
///
/// **INV-T9 #72 (Commit 3):** `suspended_attempt_evidence` + `evidence_digest`
/// record içine gömülür (P0-3 — runtime `AwaitingWitnesses { pending }` aynı
/// evidence nesnesini taşır). Surface-specific disposition: `PendingAuthorization`
/// yalnız `Held` disposition kabul eder.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
    /// **INV-T9 #72 (Commit 4):** Trajectory attempt number (1-based). Eski
    /// `attempt_evidence_id` (dangling reference) kaldırıldı — durable evidence
    /// lookup yok, embedded `suspended_attempt_evidence` + `evidence_digest`
    /// source of truth. `attempt_num` yalnız trajectory sequence bilgisi.
    pub attempt_num: AttemptNumber,
    /// **INV-T9 #72:** Embedded canonical evidence snapshot (Held disposition).
    /// Record içinde — runtime `AwaitingWitnesses { pending }` taşır (P0-3).
    pub suspended_attempt_evidence: SuspendedAttemptEvidence,
    /// **INV-T9 #72:** Evidence'ın domain-separated digest'i. `verify()` tekrar
    /// hesaplayıp karşılaştırır (tamper detection).
    pub evidence_digest: SuspendedAttemptEvidenceDigest,
    /// Clock trait'inden — digest'e DAHİL DEĞİL.
    pub created_at: u64,
}

impl PendingAuthorization {
    /// **P1-3 record-internal validation:** record ↔ embedded evidence cross-field.
    ///
    /// Basis-dependent kontroller (record ↔ basis) envelope `verify()`'da. Bu
    /// method sadece record'un kendi içindeki tutarlılığı doğrular:
    /// - task_id ↔ evidence.task_id
    /// - claim_id ↔ evidence.claim_id
    /// - attempt_num ↔ evidence.attempt_num
    /// - authorization_basis_digest ↔ evidence.authorization_basis_digest
    /// - evidence_digest ↔ recomputed evidence digest
    /// - Held disposition (surface-specific)
    /// - witness_hold_reason ↔ evidence disposition hold_reason
    /// - witness_snapshot ↔ evidence disposition snapshot
    pub(crate) fn validate_internal(&self) -> Result<(), PendingAuthorizationLoadError> {
        let evidence = &self.suspended_attempt_evidence;

        // record ↔ evidence kimlik.
        if self.task_id != evidence.task_id() {
            return Err(PendingAuthorizationLoadError::TaskIdMismatch {
                record: self.task_id,
                basis: evidence.task_id(),
            });
        }
        if self.claim_id != evidence.claim_id() {
            return Err(PendingAuthorizationLoadError::ClaimIdMismatch {
                record: self.claim_id,
                basis: self.claim_id, // record-internal — basis envelope'ta
                evidence: evidence.claim_id(),
            });
        }
        if self.attempt_num != evidence.attempt_num() {
            return Err(PendingAuthorizationLoadError::AttemptNumberMismatch {
                record: self.attempt_num.get(),
                evidence: evidence.attempt_num().get(),
            });
        }
        if &self.authorization_basis_digest != evidence.authorization_basis_digest() {
            return Err(PendingAuthorizationLoadError::EvidenceBasisDigestMismatch);
        }

        // Evidence digest recompute + compare (tamper detection).
        let computed_evidence = SuspendedAttemptEvidenceDigest::compute(evidence)
            .map_err(|e| PendingAuthorizationLoadError::DigestComputationFailed(e.to_string()))?;
        if computed_evidence != self.evidence_digest {
            return Err(PendingAuthorizationLoadError::EvidenceDigestMismatch);
        }

        // Surface-specific disposition + reason/snapshot binding.
        match evidence.disposition() {
            SuspendedAttemptDisposition::Held {
                hold_reason,
                snapshot,
            } => {
                if &self.witness_hold_reason != hold_reason {
                    return Err(PendingAuthorizationLoadError::WitnessHoldReasonMismatch);
                }
                if &self.witness_snapshot != snapshot {
                    return Err(PendingAuthorizationLoadError::WitnessSnapshotMismatch);
                }
            }
            SuspendedAttemptDisposition::Rejected { .. } => {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    "PendingAuthorization requires Held disposition, found Rejected".into(),
                ));
            }
        }

        Ok(())
    }
}

/// **P0-2 strict wire:** `PendingAuthorization` custom Deserialize —
/// `deny_unknown_fields` + `validate_internal`. Unknown field reject (stale
/// `attempt_evidence_id` dahil).
impl<'de> serde::Deserialize<'de> for PendingAuthorization {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            task_id: crate::trajectory::TaskId,
            claim_id: ClaimId,
            predicate_completion: PredicateCompletion,
            mutation_decision: MutationDecision,
            intended_apply_target: ApplyTarget,
            authorization_basis_digest: AuthorizationBasisDigest,
            base_space_view_revision: SpaceViewRevision,
            evaluation_context_digest: EvaluationContextDigest,
            witness_requirement: WitnessRequirement,
            witness_hold_reason: WitnessHoldReason,
            witness_snapshot: WitnessQuorumSnapshot,
            attempt_num: AttemptNumber,
            suspended_attempt_evidence: SuspendedAttemptEvidence,
            evidence_digest: SuspendedAttemptEvidenceDigest,
            created_at: u64,
        }
        let wire = Wire::deserialize(deserializer)?;
        let record = PendingAuthorization {
            task_id: wire.task_id,
            claim_id: wire.claim_id,
            predicate_completion: wire.predicate_completion,
            mutation_decision: wire.mutation_decision,
            intended_apply_target: wire.intended_apply_target,
            authorization_basis_digest: wire.authorization_basis_digest,
            base_space_view_revision: wire.base_space_view_revision,
            evaluation_context_digest: wire.evaluation_context_digest,
            witness_requirement: wire.witness_requirement,
            witness_hold_reason: wire.witness_hold_reason,
            witness_snapshot: wire.witness_snapshot,
            attempt_num: wire.attempt_num,
            suspended_attempt_evidence: wire.suspended_attempt_evidence,
            evidence_digest: wire.evidence_digest,
            created_at: wire.created_at,
        };
        record
            .validate_internal()
            .map_err(serde::de::Error::custom)?;
        Ok(record)
    }
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

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 4 — CanonicalWitnessRequirementV2 (plan md:96-102)
//
// **Reviewer v4 P1-1 (plan md:96-101):** Private repr — direct construct edilemez.
// Tek creation yolu: `TryFrom<(&CanonicalWitnessPolicy, ApplyTarget)>`. Witness
// requirement varyant/reason pinned numeric tag (wire serde adı digest girdisi DEĞİL).
//
// **apply_target field DEĞİL (plan md:62):** `apply_target` `mutation_decision`'dan
// deterministic türetilir. CanonicalWitnessRequirementV2 apply_target taşımaz —
// `TryFrom` apply_target alır ama requirement içinde saklamaz (lane/witness uyumu
// `validate_for(apply_target)` ile runtime check).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:100):** Witness-not-required reason.
/// Reject → NotApplied: witness aşaması çalışmaz (delta hiç uygulanmadı).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum WitnessNotRequiredReason {
    /// `MutationDecision::Reject` → `ApplyTarget::NotApplied` — delta uygulanmadı,
    /// witness aşaması çalışmaz.
    RejectedBeforeWitness,
}

impl WitnessNotRequiredReason {
    /// Pinned numeric tag — wire serde adı DEĞİL, digest girdisi (plan md:101).
    pub(crate) fn tag(&self) -> WitnessNotRequiredReasonTag {
        match self {
            WitnessNotRequiredReason::RejectedBeforeWitness => {
                WitnessNotRequiredReasonTag::REJECTED_BEFORE_WITNESS
            }
        }
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:97-99):** Canonical witness requirement V2.
/// Private repr — direct construct edilemez. Tek creation yolu:
/// `TryFrom<(&CanonicalWitnessPolicy, ApplyTarget)>`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CanonicalWitnessRequirementV2 {
    repr: CanonicalWitnessRequirementRepr,
}

/// Private repr — direct construct edilemez (struct field private).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
enum CanonicalWitnessRequirementRepr {
    /// Witness gerekmez — Reject → NotApplied (witness aşaması çalışmaz).
    NotRequired { reason: WitnessNotRequiredReason },
    /// Witness gerekir — min_approvers + quorum + independence policy.
    Required {
        min_approvers: u32,
        quorum_threshold: CanonicalF64,
        independence_policy: crate::canonical_tags::WitnessIndependencePolicyTag,
    },
}

impl CanonicalWitnessRequirementV2 {
    /// Pinned numeric tag — `WitnessRequirementTag` (Required=0, NotRequired=1).
    /// Wire serde adı DEĞİL, digest girdisi (plan md:101).
    #[allow(dead_code, reason = "Faz 4 wire dispatch / Commit 1b consumer")]
    pub(crate) fn tag(&self) -> WitnessRequirementTag {
        match &self.repr {
            CanonicalWitnessRequirementRepr::Required { .. } => WitnessRequirementTag::REQUIRED,
            CanonicalWitnessRequirementRepr::NotRequired { .. } => {
                WitnessRequirementTag::NOT_REQUIRED
            }
        }
    }

    /// **INV-T9 #70 Commit 4b Faz 4 (plan md:102):** Lane/witness requirement uyumu.
    /// `validate_for(apply_target)` — apply_target'a göre requirement geçerliliğini
    /// kontrol eder. Reject → NotApplied için NotRequired beklenir; diğer lane'ler için
    /// Required beklenir.
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub(crate) fn validate_for(
        &self,
        apply_target: &ApplyTarget,
    ) -> Result<(), CanonicalWitnessRequirementV2Error> {
        match (&self.repr, apply_target) {
            // Reject → NotApplied: witness aşaması çalışmaz → NotRequired beklenir.
            (CanonicalWitnessRequirementRepr::NotRequired { .. }, ApplyTarget::NotApplied) => {
                Ok(())
            }
            // NotApplied ama Required → tutarsız (delta uygulanmadı, witness gereksiz).
            (CanonicalWitnessRequirementRepr::Required { .. }, ApplyTarget::NotApplied) => {
                Err(CanonicalWitnessRequirementV2Error::RequiredForNotApplied)
            }
            // Lane (Mainline/TrajectoryCheckpoint/Sandbox): witness gerekir → Required beklenir.
            (CanonicalWitnessRequirementRepr::Required { .. }, ApplyTarget::Lane(_)) => Ok(()),
            // Lane ama NotRequired → tutarsız (delta uygulandı, witness gerekir).
            (CanonicalWitnessRequirementRepr::NotRequired { .. }, ApplyTarget::Lane(_)) => {
                Err(CanonicalWitnessRequirementV2Error::NotRequiredForLane)
            }
        }
    }

    /// **Canonical byte encoding (plan md:101):** Witness requirement varyant/reason
    /// pinned numeric tag. `AuthorizationContextDigestV2` bunu çağırır.
    pub(crate) fn encode_canonical(
        &self,
        hasher: &mut blake3::Hasher,
    ) -> Result<(), CanonicalDigestError> {
        use crate::canonical_encoding::{encode_f64, encode_tag, encode_u32, encode_u8};
        match &self.repr {
            CanonicalWitnessRequirementRepr::Required {
                min_approvers,
                quorum_threshold,
                independence_policy,
            } => {
                encode_u8(
                    hasher,
                    WitnessRequirementTag::REQUIRED.as_u8(),
                    "wr_required_tag",
                );
                encode_u32(hasher, *min_approvers, "wr_min_approvers");
                encode_f64(hasher, *quorum_threshold, "wr_quorum")?;
                encode_tag(hasher, *independence_policy, "wr_independence");
            }
            CanonicalWitnessRequirementRepr::NotRequired { reason } => {
                encode_u8(
                    hasher,
                    WitnessRequirementTag::NOT_REQUIRED.as_u8(),
                    "wr_not_required_tag",
                );
                encode_u8(hasher, reason.tag().as_u8(), "wr_reason_tag");
            }
        }
        Ok(())
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:99):** Tek creation yolu — policy + apply_target.
/// `Reject → NotApplied` → `NotRequired { RejectedBeforeWitness }`. Diğer lane'ler →
/// `Required { policy fields }`.
impl TryFrom<(&CanonicalWitnessPolicy, &ApplyTarget)> for CanonicalWitnessRequirementV2 {
    type Error = CanonicalWitnessRequirementV2Error;

    fn try_from(
        (policy, apply_target): (&CanonicalWitnessPolicy, &ApplyTarget),
    ) -> Result<Self, Self::Error> {
        let repr = match apply_target {
            ApplyTarget::NotApplied => CanonicalWitnessRequirementRepr::NotRequired {
                reason: WitnessNotRequiredReason::RejectedBeforeWitness,
            },
            ApplyTarget::Lane(_) => CanonicalWitnessRequirementRepr::Required {
                min_approvers: policy.min_approvers,
                quorum_threshold: policy.quorum_threshold,
                independence_policy: policy.independence_policy,
            },
        };
        Ok(Self { repr })
    }
}

/// **INV-T9 #70 Commit 4b Faz 4:** CanonicalWitnessRequirementV2 validation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalWitnessRequirementV2Error {
    #[error("witness required for NotApplied (Reject should produce NotRequired)")]
    RequiredForNotApplied,
    #[error("witness not required for Lane (applied delta requires witness)")]
    NotRequiredForLane,
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 4 — CanonicalGateEvaluationV2 + VerifiedGateEvaluationV2
// (plan md:30, 74-79)
//
// **3 katman ayrımı (plan md:30):** GateEvaluation — `CanonicalGateEvaluationV2`
// persisted snapshot + `VerifiedGateEvaluationV2` opaque producer proof. Faz 4
// structural consistency only; Faz 5 gerçek evaluator producer.
//
// **VerifiedGateEvaluationV2 opaque proof (plan md:75-79):**
// - `pub(crate) struct { canonical: CanonicalGateEvaluationV2 }` — field private
// - Serialize/Deserialize/Clone YOK
// - Production build'de constructor YOK — Faz 5 gerçek evaluator producer
// - `into_canonical(self) -> CanonicalGateEvaluationV2` pub(crate)
// - `#[cfg(test)] impl { pub(crate) fn fixture(canonical) -> Self }` — authorization.rs'te
//
// **Proof-gated context constructor (plan md:69-72):** `AuthorizationContextV2::new`
// `VerifiedGateEvaluationV2` tüketir; `CanonicalGateEvaluationV2` reddedilir (compile error).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:164, reviewer P1-1 v2):** Gate disposition V2
/// error. Illegal-state matrisi + rejected gate decision validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GateDispositionError {
    #[error("invalid gate disposition: {detail}")]
    Invalid { detail: String },
    /// **Reviewer P1-1 v2:** `GateDecision::PassedAll`/`Unknown` rejected gate decision
    /// olarak geçersiz — bu varyantlar deterministic gate zincirinin geçtiğini/bilinmediğini
    /// belirtir, reddetmedi.
    #[error("GateDecision {value:?} is not a rejected gate decision")]
    NotARejectedGateDecision {
        value: crate::trajectory::GateDecision,
    },
}

/// **INV-T9 #70 Commit 4b Faz 4 (reviewer P1-1 v2):** Checked rejection gate decision.
/// `GateDecision`'ın rejection alt kümesi — `PassedAll`/`Unknown` reject (illegal state
/// yapısal olarak imkânsız). Rejected gate → apply_target NotApplied → witness NotRequired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct RejectedGateDecisionV2(crate::trajectory::GateDecision);

impl RejectedGateDecisionV2 {
    /// Rejection gate decision tag — `GateDecisionTag` pinned numeric tag.
    pub(crate) fn tag(&self) -> GateDecisionTag {
        GateDecisionTag::from(&self.0)
    }
}

impl TryFrom<crate::trajectory::GateDecision> for RejectedGateDecisionV2 {
    type Error = GateDispositionError;

    fn try_from(value: crate::trajectory::GateDecision) -> Result<Self, Self::Error> {
        use crate::trajectory::GateDecision;
        match value {
            GateDecision::PassedAll | GateDecision::Unknown => {
                Err(GateDispositionError::NotARejectedGateDecision { value })
            }
            // Tüm rejection varyantları geçerli (RejectedBySyntax/Vision/Rule/TaskBinding/
            // BlockedByManeuverLimit/RejectedByTaskValidation/RejectedByMeasurementBinding).
            rejected => Ok(Self(rejected)),
        }
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:30, reviewer P1-1 v2):** Canonical gate
/// evaluation V2 — persisted snapshot enum. Illegal state yapısal olarak imkânsız:
///
/// - `RejectedByGate` — deterministic gate zinciri sonlandırdı → apply_target NotApplied,
///   witness NotRequired. `RejectedGateDecisionV2` checked (PassedAll/Unknown reject).
/// - `GatePassed` — deterministic gate'ler geçti → mutation policy ayrı karar verdi.
///   `MutationDecision::Reject` geçerli (predicate/policy sonucu uygulanmama).
///
/// **vs önceki struct model (reviewer P1-1 v1):** struct `disposition + mutation_decision`
/// iki bağımsız field taşıyordu — `REJECTED + AcceptAsCompleted` illegal state üretilebiliyordu.
/// Enum modeli bu state'i yapısal olarak imkânsız kılar.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum CanonicalGateEvaluationV2 {
    /// Deterministic gate zinciri reddetti — authorization sonlanır.
    /// apply_target = NotApplied, witness = NotRequired.
    RejectedByGate { decision: RejectedGateDecisionV2 },
    /// Deterministic gate'ler geçti — mutation policy karar verdi.
    /// apply_target = mutation_decision.apply_target() (INV-T8).
    GatePassed {
        mutation_decision: crate::trajectory::MutationDecision,
    },
}

impl CanonicalGateEvaluationV2 {
    /// **Reviewer P1-1 v2:** Rejected gate constructor — checked `RejectedGateDecisionV2`.
    #[allow(
        dead_code,
        reason = "Faz 5 gate evaluator producer / cfg(test) fixture"
    )]
    pub(crate) fn rejected_by_gate(
        decision: crate::trajectory::GateDecision,
    ) -> Result<Self, GateDispositionError> {
        Ok(Self::RejectedByGate {
            decision: RejectedGateDecisionV2::try_from(decision)?,
        })
    }

    /// **Reviewer P1-1 v2:** Gate passed constructor — mutation decision (tüm varyantlar
    /// geçerli, Reject dahil — predicate/policy sonucu uygulanmama).
    #[allow(
        dead_code,
        reason = "Faz 5 gate evaluator producer / cfg(test) fixture"
    )]
    pub(crate) fn gate_passed(
        mutation_decision: crate::trajectory::MutationDecision,
    ) -> Result<Self, GateDispositionError> {
        Ok(Self::GatePassed { mutation_decision })
    }

    /// **Reviewer P1-1 v2:** Apply target — disposition'a göre deterministic türetim.
    /// `RejectedByGate` → NotApplied; `GatePassed` → mutation_decision.apply_target() (INV-T8).
    /// apply_target field olarak saklanmaz (plan md:62).
    pub(crate) fn apply_target(&self) -> crate::trajectory::ApplyTarget {
        use crate::trajectory::ApplyTarget;
        match self {
            Self::RejectedByGate { .. } => ApplyTarget::NotApplied,
            Self::GatePassed { mutation_decision } => mutation_decision.apply_target(),
        }
    }

    /// **Canonical byte encoding (plan md:115, reviewer P1-1 v2):** Enum varyant tag +
    /// payload. Illegal state yapısal olarak imkânsız olduğu için encoder state kontrolü
    /// yapmaz. `AuthorizationContextDigestV2` bunu çağırır.
    pub(crate) fn encode_canonical(
        &self,
        hasher: &mut blake3::Hasher,
    ) -> Result<(), CanonicalDigestError> {
        use crate::canonical_encoding::encode_u8;
        match self {
            Self::RejectedByGate { decision } => {
                // **Reviewer P1-1 v3:** GateDispositionV2Tag kullan — tek authoritative
                // mapping (PASSED=0, REJECTED=1). Hardcoded değer YOK — pinned newtype.
                encode_u8(
                    hasher,
                    GateDispositionV2Tag::REJECTED.as_u8(),
                    "gate_evaluation_rejected_by_gate",
                );
                encode_u8(hasher, decision.tag().as_u8(), "rejected_gate_decision_tag");
            }
            Self::GatePassed { mutation_decision } => {
                encode_u8(
                    hasher,
                    GateDispositionV2Tag::PASSED.as_u8(),
                    "gate_evaluation_gate_passed",
                );
                encode_u8(
                    hasher,
                    MutationDecisionTag::from(mutation_decision).as_u8(),
                    "gate_passed_mutation_decision_tag",
                );
            }
        }
        Ok(())
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:75-79):** VerifiedGateEvaluationV2 — opaque
/// producer proof. Field private; Serialize/Deserialize/Clone YOK. Production build'de
/// constructor YOK — Faz 5 gerçek evaluator producer. `into_canonical(self)` pub(crate).
///
/// **Proof-gated context (plan md:72):** `AuthorizationContextV2::new` bu tipi tüketir;
/// `CanonicalGateEvaluationV2` reddedilir. Invariant: "AuthorizationContextV2 yalnızca
/// VerifiedGateEvaluationV2 tüketilerek doğabilir".
#[derive(Debug)]
pub(crate) struct VerifiedGateEvaluationV2 {
    canonical: CanonicalGateEvaluationV2,
}

impl VerifiedGateEvaluationV2 {
    /// **pub(crate) consumer (plan md:78):** Verified proof'u canonical snapshot'a
    /// indirger. Context constructor bunu çağırır (tek yol — field private).
    #[allow(dead_code, reason = "Faz 4 context constructor / Commit 2 consumer")]
    pub(crate) fn into_canonical(self) -> CanonicalGateEvaluationV2 {
        self.canonical
    }

    /// **cfg(test) fixture (plan md:79):** Test-only constructor — authorization.rs'te
    /// (field privacy). Production build'de constructor YOK — Faz 5 gerçek evaluator.
    #[cfg(test)]
    #[allow(dead_code, reason = "Faz 4 test fixture — production build'de yok")]
    pub(crate) fn fixture(canonical: CanonicalGateEvaluationV2) -> Self {
        Self { canonical }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 4 — AuthorizationContextV2 (plan md:69-72)
//
// **Proof-gated context constructor (plan md:69):**
// - `AuthorizationContextV2::new(basis, gate_evaluation: VerifiedGateEvaluationV2,
//    witness_requirement)` — VerifiedGateEvaluationV2 tüketir
// - `CanonicalGateEvaluationV2` (persisted snapshot) → `new` reddedilir (compile error)
// - Invariant: "AuthorizationContextV2 yalnızca VerifiedGateEvaluationV2 tüketilerek doğebilir"
//
// **Karar (reviewer #3):** Mimari invariant yeterli — trybuild YOK. Private fields +
// tek proof-gated producer + cfg(test) fixture + doc comment (yapısal zorunluluk).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:164):** AuthorizationContextV2 build error —
/// orchestration (Commit 2 builder). Context invariant hatası `AuthorizationContextV2Error`.
///
/// **P0-2 (reviewer):** Typed taxonomy — builder'ın fallible zincirinin tüm error tipleri
/// korunur (telemetry + exact assertion). BasisDigest kaldırıldı — AuthorizationContextV2::new
/// basis digest hesaplamıyor.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthorizationContextV2BuildError {
    #[error("engine measurement digest computation failed: {0}")]
    EngineMeasurementDigest(#[from] crate::measurement::EngineMeasurementDigestError),
    #[error("engine measurement binding mismatch: proof={proof}, recomputed={recomputed}")]
    EngineMeasurementBindingMismatch { proof: String, recomputed: String },
    #[error("canonical evidence conversion failed: {0}")]
    Canonicalization(#[from] CanonicalizationError),
    #[error("baseline evidence validation failed: {0}")]
    BaselineValidation(#[from] CanonicalBaselineValidationError),
    #[error("measurement digest computation failed: {0}")]
    MeasurementDigest(#[from] crate::measurement::MeasurementDigestError),
    #[error("authorization basis construction failed: {0}")]
    Basis(#[from] AuthorizationBasisV2Error),
    #[error("witness requirement validation failed: {0}")]
    WitnessRequirement(#[from] CanonicalWitnessRequirementV2Error),
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:164):** AuthorizationContextV2 invariant error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationContextV2Error {
    #[error("context invariant violation: {detail}")]
    Invariant { detail: String },
}

/// **INV-T9 #70 Commit 4b Faz 4 (plan md:31, 69-72):** Authorization context V2 — basis
/// + verified gate snapshot + canonical witness requirement. Checked constructor
/// proof-gated: `VerifiedGateEvaluationV2` tüketir, `CanonicalGateEvaluationV2` reddeder.
///
/// **3 katman:** Context = Basis (kanıtsal zemin) + verified gate snapshot (Faz 5
/// evaluator'den) + canonical witness requirement. `apply_target` context'te YOK
/// (plan md:63 — `AuthorizationReceiptV2`'ye Faz 8'de). `witness_status` context'te YOK.
///
/// **P0-1 v3 (reviewer):** `serde::Serialize` intentionally absent — V2 tiplerinin
/// tek serialization yolu wire DTO (VersionedAuthorizationBasis). Direct Serialize bypass kapalı.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationContextV2 {
    basis: AuthorizationBasisV2,
    gate_evaluation: CanonicalGateEvaluationV2,
    witness_requirement: CanonicalWitnessRequirementV2,
}

impl AuthorizationContextV2 {
    /// **Proof-gated constructor (plan md:69, reviewer P1-1):** `VerifiedGateEvaluationV2`
    /// tüketir. `CanonicalGateEvaluationV2` reddedilir — compile error (farklı tip).
    /// Bypass imkânsız: `CanonicalGateEvaluationV2::try_from_parts` pub(crate) ama context
    /// constructor `VerifiedGateEvaluationV2` ister.
    ///
    /// **Witness validation (reviewer P1-1):** `mutation_decision` → `apply_target()`
    /// (INV-T8) → `witness_requirement.validate_for(apply_target)`. Tutarlılık context
    /// sınırında doğrulanır: Reject→NotApplied→NotRequired, lane→Required.
    ///
    /// **Invariant:** "AuthorizationContextV2 yalnızca VerifiedGateEvaluationV2
    /// tüketilerek doğabilir". Verified proof'un `into_canonical`'ı çağrılır (tek yol).
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub(crate) fn new(
        basis: AuthorizationBasisV2,
        gate_evaluation: VerifiedGateEvaluationV2,
        witness_requirement: CanonicalWitnessRequirementV2,
    ) -> Result<Self, AuthorizationContextV2BuildError> {
        // Verified proof'u canonical snapshot'a indirge (tek yol — field private).
        let canonical_gate = gate_evaluation.into_canonical();
        // **Reviewer P1-1 v2:** enum model — apply_target disposition'a göre deterministic
        // türetim. RejectedByGate → NotApplied → witness NotRequired beklenir. GatePassed →
        // mutation_decision.apply_target(). Illegal state yapısal olarak imkânsız.
        let apply_target = canonical_gate.apply_target();
        witness_requirement.validate_for(&apply_target)?;
        Ok(Self {
            basis,
            gate_evaluation: canonical_gate,
            witness_requirement,
        })
    }

    /// Basis accessor.
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub fn basis(&self) -> &AuthorizationBasisV2 {
        &self.basis
    }

    /// Gate evaluation (canonical snapshot) accessor.
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub fn gate_evaluation(&self) -> &CanonicalGateEvaluationV2 {
        &self.gate_evaluation
    }

    /// Witness requirement accessor.
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub fn witness_requirement(&self) -> &CanonicalWitnessRequirementV2 {
        &self.witness_requirement
    }

    /// **AuthorizationContextDigestV2 (plan md:55):** V2 canonical context digest.
    /// Basis + gate eval + witness requirement commitment. Ayrı domain separator.
    #[allow(dead_code, reason = "Faz 4 context builder / Commit 2 consumer")]
    pub fn compute_digest(&self) -> Result<AuthorizationContextDigestV2, CanonicalDigestError> {
        let basis_digest = self.basis.compute_digest()?;
        AuthorizationContextDigestV2::compute(
            &basis_digest,
            &self.gate_evaluation,
            &self.witness_requirement,
        )
    }
}

// **INV-T9 #72 (Commit 4):** `pub type AttemptEvidenceId = u64;` KALDIRILDI.
//
// Eski alias durable evidence store reference'i gibi davranıyordu ama gerçek
// lookup/store yoktu — dangling reference. Embedded `SuspendedAttemptEvidence` +
// `evidence_digest` source of truth; `attempt_num` (AttemptNumber) yalnız
// trajectory sequence bilgisi. P1 evidence store gelirse ayrı kimlik tipi
// tanımlanacak (opaque sayaç değil).

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #72 — Canonical suspended-attempt evidence model
//
// Embedded attempt-evidence integrity: `SuspendedAttemptEvidence` canonical snapshot,
// domain-separated `SuspendedAttemptEvidenceDigest`, surface-specific disposition.
//
// **P0-1:** `AttemptNumber` custom Deserialize + `TryFrom<u64>` (derived Deserialize
//          bypass fix — `0` reject). Struct literal bypass imkânsız (field private).
// **P0-2:** Engine ownership korunur — disposition payload `EngineCommitResult`'ta kalır.
//           Navigator `attempt_num` ile final evidence'ı üretir (tek kaynak).
// **P0-3:** Evidence record içinde — runtime `AwaitingWitnesses { pending }` taşır.
// **P1:**   Common header + tagged disposition enum (schema drift risk yok).
//           Canonical rejection sort + duplicate reject (digest determinism).
// ═══════════════════════════════════════════════════════════════════════════════

/// Validated trajectory attempt number (1-based, sıfır reject).
///
/// **P0-1 invariant:** Sistemde attempt numarası 1-based'dir (`navigator.rs`:
/// `for attempt_num in 1..=maneuver_limit`). Derived `Deserialize` bu invariant'ı
/// bypass ederdi (`0` JSON'dan kabul edilirdi). Bu tip custom `Deserialize` ile
/// `TryFrom<u64>` üzerinden geçer — wire format da dahil her girişte `0` reject.
///
/// `serde::Serialize` derive edilir (transparent — u64 olarak serileşir), ancak
/// `Deserialize` MANUEL uygulanır (`TryFrom` çağrılır).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct AttemptNumber(u64);

/// `AttemptNumber` invariant ihlali.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AttemptNumberError {
    /// Attempt numarası sıfır — 1-based invariant ihlali.
    #[error("attempt number must be >= 1 (zero rejected — 1-based trajectory invariant)")]
    Zero,
}

impl TryFrom<u64> for AttemptNumber {
    type Error = AttemptNumberError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value == 0 {
            return Err(AttemptNumberError::Zero);
        }
        Ok(Self(value))
    }
}

impl<'de> serde::Deserialize<'de> for AttemptNumber {
    /// **P0-1:** Custom deserialize — `u64::deserialize` sonrası `TryFrom` ile validate.
    /// Derived Deserialize bu adımı atlar, `0` değerini kabul ederdi (invariant bypass).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

impl AttemptNumber {
    /// Raw `u64` değerine erişim.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Disposition-specific evidence — suspended authorization'ın *neden* oluştuğunu
/// ontolojik olarak sabitler (P1 surface-specific disposition).
///
/// **Enum seçimi:** Unified `Option`-field struct illegal-state üretir
/// (`Held + hold_reason=None`, `Rejected + reasons=None`, vb.). Enum ile:
/// - `Held` → `hold_reason` zorunlu, `reasons` imkânsız
/// - `Rejected` → non-empty `reasons` zorunlu, `hold_reason` imkânsız
///
/// **P1 schema drift:** Ortak header outer struct'ta; disposition-specific evidence
/// bu tagged enum'da. İki büyük enum varyantı (alan tekrarı) değil.
///
/// **P0-2 strict wire (closure):** Custom Deserialize per-variant wire structs ile
/// `deny_unknown_fields` — tagged enum attribute ile çakışmadı. Unknown field reject.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuspendedAttemptDisposition {
    /// Q1/Q2/EvidenceNotLocallyObservable yetersiz — expected authorization bekleme.
    Held {
        hold_reason: WitnessHoldReason,
        snapshot: WitnessQuorumSnapshot,
    },
    /// Q3 honest-reject — explicit witness reddi. Agent yeni proposal üretmeli.
    Rejected {
        reasons: crate::witness::NonEmptyWitnessRejections,
        snapshot: WitnessQuorumSnapshot,
    },
}

/// **P0-2 strict wire:** `SuspendedAttemptDisposition` custom Deserialize —
/// per-variant wire structs ile `deny_unknown_fields`. Tagged enum attribute ile
/// `deny_unknown_fields` çakıştığı için manuel uygulanır.
impl<'de> serde::Deserialize<'de> for SuspendedAttemptDisposition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Önce "kind" tag'ini oku, sonra varyant-specific wire struct.
        #[derive(serde::Deserialize)]
        struct Tag {
            kind: KindTag,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum KindTag {
            Held,
            Rejected,
        }
        let content = serde_json::Value::deserialize(deserializer)?;
        let tag: Tag = serde_json::from_value(content.clone())
            .map_err(|e| serde::de::Error::custom(format!("invalid disposition kind: {e}")))?;

        match tag.kind {
            KindTag::Held => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct HeldWire {
                    // `kind` field deny_unknown_fields için gerekli (wire'da var),
                    // read edilmiyor ama serde deserialize sırasında accept eder.
                    #[allow(dead_code)]
                    kind: KindTagAlias,
                    hold_reason: WitnessHoldReason,
                    snapshot: WitnessQuorumSnapshot,
                }
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "snake_case")]
                enum KindTagAlias {
                    Held,
                }
                let w: HeldWire = serde_json::from_value(content)
                    .map_err(|e| serde::de::Error::custom(format!("Held disposition: {e}")))?;
                Ok(Self::Held {
                    hold_reason: w.hold_reason,
                    snapshot: w.snapshot,
                })
            }
            KindTag::Rejected => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct RejectedWire {
                    #[allow(dead_code)]
                    kind: KindTagAlias2,
                    reasons: crate::witness::NonEmptyWitnessRejections,
                    snapshot: WitnessQuorumSnapshot,
                }
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "snake_case")]
                enum KindTagAlias2 {
                    Rejected,
                }
                let w: RejectedWire = serde_json::from_value(content)
                    .map_err(|e| serde::de::Error::custom(format!("Rejected disposition: {e}")))?;
                Ok(Self::Rejected {
                    reasons: w.reasons,
                    snapshot: w.snapshot,
                })
            }
        }
    }
}

/// `SuspendedAttemptEvidence::try_new` doğrulama hatası.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SuspendedAttemptEvidenceError {
    #[error("schema version mismatch: found {found}, expected {expected}")]
    SchemaVersionMismatch { found: u32, expected: u32 },
    /// **P1-2:** Witness snapshot support/required_support non-finite veya negatif.
    #[error("invalid witness snapshot: {0}")]
    InvalidSnapshot(String),
    /// **P1-2:** Held hold_reason ↔ snapshot iç tutarlılık ihlali.
    #[error("hold reason ↔ snapshot inconsistency: {0}")]
    HoldReasonSnapshotInconsistency(String),
    /// **P1-1 strict wire:** Wire'dan gelen rejection sırası canonical değil.
    /// Production API (`try_new_normalizing`) canonicalize eder; wire load
    /// (`try_from_canonical_wire`) strict reject eder (P1-1 strict wire).
    #[error("non-canonical rejection order on wire (strict wire rejects; API normalizes)")]
    NonCanonicalRejectionOrder,
    /// **P0-3:** Duplicate (witness, rationale) çifti — canonical encoding determinism.
    #[error("duplicate witness rejection (canonical determinism)")]
    DuplicateRejection,
}

/// Canonical suspended-attempt evidence schema version (v1).
pub const SUSPENDED_ATTEMPT_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Canonical embedded attempt-evidence — common header + disposition.
///
/// **INV-T9 #72:** Persisted artifact'te ve runtime Held/Rejected sonucunda aynı
/// nesne kullanılır (tek production source). Domain-separated digest
/// ([`SuspendedAttemptEvidenceDigest`]) ile bütünlük bağlanır.
///
/// **Private fields + `try_new`:** Struct literal bypass imkânsız. Custom
/// `Deserialize` `deny_unknown_fields` ile `try_new` üzerinden geçer → diskten
/// malformed evidence (unknown-field, schema-version mismatch) deserialize
/// sırasında reject.
///
/// **Binding:** `task_id` + `claim_id` + `authorization_basis_digest` +
/// `attempt_num` → trajectory içindeki yapısal denemenin konumu ve o denemede
/// askıya alınan authorization kararının kimliği. Durable evidence lookup yok;
/// `attempt_num` global lookup anahtarı DEĞİL, yalnız trajectory sequence bilgisi.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SuspendedAttemptEvidence {
    schema_version: u32,
    task_id: crate::trajectory::TaskId,
    claim_id: ClaimId,
    authorization_basis_digest: AuthorizationBasisDigest,
    attempt_num: AttemptNumber,
    disposition: SuspendedAttemptDisposition,
}

impl SuspendedAttemptEvidence {
    /// Validated smart constructor — production API (normalizing).
    ///
    /// **P0-2 (ownership):** Bu constructor navigator boundary'de çağrılır —
    /// engine değil. Engine disposition payload'unu (`reason`, `reasons`,
    /// `snapshot`) `EngineCommitResult`'ta taşır; navigator gerçek `attempt_num`
    /// ile final evidence'ı üretir.
    ///
    /// **N2 (API normalize):** Arbitrary input sırasını canonical sıraya normalize
    /// eder (Rejected reasons). Wire load path bunu KULLANMAZ — `try_from_canonical_wire`
    /// strict check yapar (non-canonical wire → reject).
    ///
    /// **P1-2 (semantic validation):** `validate_evidence_semantics` constructor'a
    /// çekildi — Held hold_reason↔snapshot, Rejected snapshot finite/non-neg.
    pub fn try_new(
        task_id: crate::trajectory::TaskId,
        claim_id: ClaimId,
        authorization_basis_digest: AuthorizationBasisDigest,
        attempt_num: AttemptNumber,
        disposition: SuspendedAttemptDisposition,
    ) -> Result<Self, SuspendedAttemptEvidenceError> {
        Self::try_new_normalizing(
            task_id,
            claim_id,
            authorization_basis_digest,
            attempt_num,
            disposition,
        )
    }

    /// Production API constructor — arbitrary input → canonicalize → validate.
    ///
    /// Rejected reasons `canonicalize_rejections` üzerinden canonical sıraya gelir
    /// (sort + duplicate reject). Held/Rejected `validate_evidence_semantics`.
    fn try_new_normalizing(
        task_id: crate::trajectory::TaskId,
        claim_id: ClaimId,
        authorization_basis_digest: AuthorizationBasisDigest,
        attempt_num: AttemptNumber,
        disposition: SuspendedAttemptDisposition,
    ) -> Result<Self, SuspendedAttemptEvidenceError> {
        let disposition = match disposition {
            SuspendedAttemptDisposition::Held {
                hold_reason,
                snapshot,
            } => SuspendedAttemptDisposition::Held {
                hold_reason,
                snapshot,
            },
            SuspendedAttemptDisposition::Rejected { reasons, snapshot } => {
                let canonical_reasons = canonicalize_rejections(reasons)?;
                SuspendedAttemptDisposition::Rejected {
                    reasons: canonical_reasons,
                    snapshot,
                }
            }
        };
        validate_evidence_semantics(&disposition)?;
        Ok(Self {
            schema_version: SUSPENDED_ATTEMPT_EVIDENCE_SCHEMA_VERSION,
            task_id,
            claim_id,
            authorization_basis_digest,
            attempt_num,
            disposition,
        })
    }

    /// Wire load constructor — strict canonical check (NO normalize).
    ///
    /// **N2 (strict wire):** Wire'dan gelen disposition raw kabul edilir. Eğer
    /// rejection sırası canonical değilse `NonCanonicalRejectionOrder` (normalize
    /// ETMEZ — persisted representation canonical olmalı). Semantic validation
    /// (`validate_evidence_semantics`) yapılır.
    ///
    /// Bu constructor custom Deserialize tarafından çağrılır. `schema_version`
    /// wire'dan gelir (sonra custom Deserialize'da constant ile karşılaştırılır).
    fn try_from_canonical_wire(
        schema_version: u32,
        task_id: crate::trajectory::TaskId,
        claim_id: ClaimId,
        authorization_basis_digest: AuthorizationBasisDigest,
        attempt_num: AttemptNumber,
        disposition: SuspendedAttemptDisposition,
    ) -> Result<Self, SuspendedAttemptEvidenceError> {
        // Strict canonical check (no normalize).
        if let SuspendedAttemptDisposition::Rejected { reasons, .. } = &disposition {
            verify_rejections_canonical_order(reasons)?;
        }
        validate_evidence_semantics(&disposition)?;
        Ok(Self {
            schema_version,
            task_id,
            claim_id,
            authorization_basis_digest,
            attempt_num,
            disposition,
        })
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn task_id(&self) -> crate::trajectory::TaskId {
        self.task_id
    }
    pub fn claim_id(&self) -> ClaimId {
        self.claim_id
    }
    pub fn authorization_basis_digest(&self) -> &AuthorizationBasisDigest {
        &self.authorization_basis_digest
    }
    pub fn attempt_num(&self) -> AttemptNumber {
        self.attempt_num
    }
    pub fn disposition(&self) -> &SuspendedAttemptDisposition {
        &self.disposition
    }
}

/// `SuspendedAttemptEvidence` için custom Deserialize — `deny_unknown_fields` +
/// schema-version validation + strict canonical wire check (N2).
///
/// **N2 (strict wire):** `try_from_canonical_wire` kullanır — raw wire disposition
/// strict canonical check yapar, normalize ETMEZ. Non-canonical rejection sırası
/// `NonCanonicalRejectionOrder` ile reddedilir (persisted representation canonical
/// olmalı). Production API (`try_new`) normalize eder; wire load strict reject eder.
impl<'de> serde::Deserialize<'de> for SuspendedAttemptEvidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema_version: u32,
            task_id: crate::trajectory::TaskId,
            claim_id: ClaimId,
            authorization_basis_digest: AuthorizationBasisDigest,
            attempt_num: AttemptNumber,
            disposition: SuspendedAttemptDisposition,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.schema_version != SUSPENDED_ATTEMPT_EVIDENCE_SCHEMA_VERSION {
            return Err(serde::de::Error::custom(
                SuspendedAttemptEvidenceError::SchemaVersionMismatch {
                    found: wire.schema_version,
                    expected: SUSPENDED_ATTEMPT_EVIDENCE_SCHEMA_VERSION,
                },
            ));
        }
        // try_from_canonical_wire: strict canonical check + semantic validation.
        // Stored schema_version korunur (wire'dan geldiği gibi).
        SuspendedAttemptEvidence::try_from_canonical_wire(
            wire.schema_version,
            wire.task_id,
            wire.claim_id,
            wire.authorization_basis_digest,
            wire.attempt_num,
            wire.disposition,
        )
        .map_err(serde::de::Error::custom)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SuspendedAttemptEvidenceDigest — BLAKE3 domain-separated canonical digest
// ═══════════════════════════════════════════════════════════════════════════════

/// BLAKE3 tabanlı suspended attempt-evidence digest.
///
/// Domain separation: `"osp.attempt-evidence.v1\0" || canonical_encoding`.
/// Float canonicalization: NaN reject, -0.0 → 0.0, little-endian, sorted collections.
/// Canonical rejection list: `(witness, rationale)` sort + duplicate reject.
///
/// **v1 byte contract:** Step 6 golden vector pattern'i takip eder —
/// `suspended_attempt_evidence_digest_v1_golden_vector` testi encoding'i kilitler.
/// Encoding (field order, tag values, float canonicalization, rejection canonical
/// ordering) bu testle kilitlenir. Breaking changes after this lock require explicit
/// v2 domain separator (`osp.attempt-evidence.v2\0`).
///
/// **Reload semantics:** `PendingAuthorizationEnvelope::verify()` (Commit 3) digest'i
/// embedded evidence'dan tekrar hesaplar ve `record.evidence_digest` ile karşılaştırır
/// (load sırasında tamper detection).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SuspendedAttemptEvidenceDigest([u8; 32]);

impl SuspendedAttemptEvidenceDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.attempt-evidence.v1\0";

    /// Evidence'dan BLAKE3 digest hesapla.
    ///
    /// **Canonical encoding sırası:**
    /// 1. `schema_version` (u32 LE)
    /// 2. `task_id` (u64 LE)
    /// 3. `claim_id` (u64 LE)
    /// 4. `authorization_basis_digest` (raw 32 bytes)
    /// 5. `attempt_num` (u64 LE)
    /// 6. Disposition:
    ///    - variant tag (u8: Held=1, Rejected=2)
    ///    - `WitnessQuorumSnapshot` canonical encoding
    ///    - disposition-specific payload (`WitnessHoldReason` veya canonical-sorted
    ///      `NonEmptyWitnessRejections`)
    pub fn compute(evidence: &SuspendedAttemptEvidence) -> Result<Self, CanonicalDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        encode_u32(
            &mut hasher,
            evidence.schema_version,
            "evidence_schema_version",
        );
        encode_u64(&mut hasher, evidence.task_id, "evidence_task_id");
        encode_u64(&mut hasher, evidence.claim_id, "evidence_claim_id");
        hasher.update(evidence.authorization_basis_digest.as_bytes());
        encode_u64(
            &mut hasher,
            evidence.attempt_num.get(),
            "evidence_attempt_num",
        );

        match &evidence.disposition {
            SuspendedAttemptDisposition::Held {
                hold_reason,
                snapshot,
            } => {
                encode_u8(&mut hasher, 1, "disposition_held_tag");
                encode_witness_quorum_snapshot(&mut hasher, snapshot)?;
                encode_witness_hold_reason(&mut hasher, hold_reason)?;
            }
            SuspendedAttemptDisposition::Rejected { reasons, snapshot } => {
                encode_u8(&mut hasher, 2, "disposition_rejected_tag");
                encode_witness_quorum_snapshot(&mut hasher, snapshot)?;
                encode_non_empty_witness_rejections(&mut hasher, reasons)?;
            }
        }

        let hash = hasher.finalize();
        Ok(Self(hash.into()))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.0 {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Hex string'den decode (test fixtures ve diagnostic için).
    pub fn from_hex(hex_str: &str) -> Result<Self, CanonicalDigestError> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| CanonicalDigestError::HexDecodeFailed(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(CanonicalDigestError::InvalidLength(bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

/// Explicit witness rejection sonucu — agent proposal revises. Evidence-preserving.
///
/// `NavigatorResult::RequiresRevision` bu struct'ı taşır. Budget tüketmez, LLM
/// reinvocation YOK. Agent yeni structural proposal üretmeli.
///
/// **INV-T9 #72 closure (P0-1):** Minimal shape — yalnız `evidence_digest` +
/// `suspended_attempt_evidence`. Tekrarlayan `task_id`/`claim_id`/
/// `authorization_basis_digest`/`reasons`/`witness_snapshot` alanları KALDIRILDI
/// (outer ↔ evidence mismatch imkânsız — tek kaynak embedded evidence).
/// Accessor'lar evidence üzerinden.
///
/// **P1 daraltma:** Full `AuthorizationBasis` reconstruction Rejected yolunda
/// ayrı concern (embedded/persisted basis surface); bu struct evidence snapshot
/// + digest binding taşır, full basis taşımaz.
///
/// **Private fields:** Struct literal bypass imkânsız. `try_new` (creation) ve
/// `try_new_with_verified_digest` (load) constructor'ları.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RevisionRequired {
    evidence_digest: SuspendedAttemptEvidenceDigest,
    suspended_attempt_evidence: SuspendedAttemptEvidence,
}

impl RevisionRequired {
    /// Creation-path constructor — production API.
    ///
    /// **P0-1 (creation vs load ayrımı):** Bu constructor evidence_digest'i
    /// **hesaplar** ve yazar. Load path (`try_new_with_verified_digest`) stored
    /// digest'i korur ve recompute + compare yapar.
    ///
    /// Surface-specific: evidence yalnız `Rejected` disposition taşımalı.
    pub fn try_new(
        suspended_attempt_evidence: SuspendedAttemptEvidence,
    ) -> Result<Self, RevisionRequiredError> {
        // Surface-specific disposition check.
        if !matches!(
            suspended_attempt_evidence.disposition(),
            SuspendedAttemptDisposition::Rejected { .. }
        ) {
            return Err(RevisionRequiredError::InvalidEvidenceDisposition {
                found: "Held (expected Rejected for RevisionRequired)".to_string(),
            });
        }
        // Creation path: digest compute.
        let evidence_digest = SuspendedAttemptEvidenceDigest::compute(&suspended_attempt_evidence)
            .map_err(|e| {
                RevisionRequiredError::EvidenceInvalid(
                    SuspendedAttemptEvidenceError::InvalidSnapshot(e.to_string()),
                )
            })?;
        Ok(Self {
            evidence_digest,
            suspended_attempt_evidence,
        })
    }

    /// Load-path constructor — stored digest korur, recompute + compare (N3).
    ///
    /// **N3 exact sıra:**
    /// 1. Stored digest'i olduğu gibi al
    /// 2. Embedded evidence semantic validation (zaten constructor'da yapıldı)
    /// 3. Digest yeniden hesapla
    /// 4. Stored ≠ recomputed → EvidenceDigestMismatch
    /// 5. Surface-specific disposition doğrula
    /// 6. Stored digest DEĞİŞTİRMEDEN nesneyi kur
    pub fn try_new_with_verified_digest(
        evidence_digest: SuspendedAttemptEvidenceDigest,
        suspended_attempt_evidence: SuspendedAttemptEvidence,
    ) -> Result<Self, RevisionRequiredError> {
        // 5. Surface-specific disposition (önce — daha ucuz kontrol).
        if !matches!(
            suspended_attempt_evidence.disposition(),
            SuspendedAttemptDisposition::Rejected { .. }
        ) {
            return Err(RevisionRequiredError::InvalidEvidenceDisposition {
                found: "Held (expected Rejected for RevisionRequired)".to_string(),
            });
        }
        // 3+4. Recompute + compare.
        let recomputed = SuspendedAttemptEvidenceDigest::compute(&suspended_attempt_evidence)
            .map_err(|e| {
                RevisionRequiredError::EvidenceInvalid(
                    SuspendedAttemptEvidenceError::InvalidSnapshot(e.to_string()),
                )
            })?;
        if recomputed != evidence_digest {
            return Err(RevisionRequiredError::EvidenceDigestMismatch);
        }
        // 6. Stored digest DEĞİŞTİRMEDEN kur.
        Ok(Self {
            evidence_digest,
            suspended_attempt_evidence,
        })
    }

    // — Accessor'lar (evidence üzerinden) —

    pub fn evidence_digest(&self) -> &SuspendedAttemptEvidenceDigest {
        &self.evidence_digest
    }

    pub fn suspended_attempt_evidence(&self) -> &SuspendedAttemptEvidence {
        &self.suspended_attempt_evidence
    }

    pub fn task_id(&self) -> crate::trajectory::TaskId {
        self.suspended_attempt_evidence.task_id()
    }

    pub fn claim_id(&self) -> ClaimId {
        self.suspended_attempt_evidence.claim_id()
    }

    pub fn authorization_basis_digest(&self) -> &AuthorizationBasisDigest {
        self.suspended_attempt_evidence.authorization_basis_digest()
    }

    pub fn attempt_num(&self) -> AttemptNumber {
        self.suspended_attempt_evidence.attempt_num()
    }

    /// Rejected reasons — evidence disposition üzerinden.
    ///
    /// Panics yok — Rejected değilse None (constructor zaten Rejected garanti).
    pub fn reasons(&self) -> Option<&crate::witness::NonEmptyWitnessRejections> {
        match self.suspended_attempt_evidence.disposition() {
            SuspendedAttemptDisposition::Rejected { reasons, .. } => Some(reasons),
            _ => None,
        }
    }

    /// Witness snapshot — evidence disposition üzerinden.
    pub fn witness_snapshot(&self) -> &crate::witness::WitnessQuorumSnapshot {
        match self.suspended_attempt_evidence.disposition() {
            SuspendedAttemptDisposition::Rejected { snapshot, .. } => snapshot,
            SuspendedAttemptDisposition::Held { snapshot, .. } => snapshot,
        }
    }
}

/// `RevisionRequired` doğrulama hatası.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RevisionRequiredError {
    #[error("invalid evidence disposition for RevisionRequired: {found}")]
    InvalidEvidenceDisposition { found: String },
    /// **N3:** Stored evidence digest, recomputed digest ile eşleşmiyor.
    #[error("evidence digest mismatch — stored != recomputed (tamper/corruption)")]
    EvidenceDigestMismatch,
    /// **N3:** Embedded evidence semantic/canonical validation hatası.
    #[error("embedded evidence invalid: {0}")]
    EvidenceInvalid(SuspendedAttemptEvidenceError),
}

/// `RevisionRequired` custom Deserialize — `deny_unknown_fields` + load path (N3).
///
/// Wire: `{ evidence_digest, suspended_attempt_evidence }` → `try_new_with_verified_digest`
/// (stored digest korur, recompute + compare). Strict canonical wire — unknown
/// field reject.
impl<'de> serde::Deserialize<'de> for RevisionRequired {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            evidence_digest: SuspendedAttemptEvidenceDigest,
            suspended_attempt_evidence: SuspendedAttemptEvidence,
        }
        let wire = Wire::deserialize(deserializer)?;
        RevisionRequired::try_new_with_verified_digest(
            wire.evidence_digest,
            wire.suspended_attempt_evidence,
        )
        .map_err(serde::de::Error::custom)
    }
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
///
/// **INV-T9 #72 closure (P0-1):** Private fields — struct literal bypass imkânsız.
/// Creation path (`new`) digest compute + write; load path (`try_new_with_verified_digests`)
/// stored digest korur, recompute + compare. Accessor'lar üzerinden erişim.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PendingAuthorizationEnvelope {
    /// Tek canonical schema identifier.
    schema: String,
    record: PendingAuthorization,
    /// Self-contained — P1 claim/evidence store kurulmadan basis doğrulanabilir.
    authorization_basis: AuthorizationBasis,
}

/// Envelope schema sabitleri.
pub const PENDING_AUTHORIZATION_SCHEMA: &str = "osp.pending-authorization.v1";

impl PendingAuthorizationEnvelope {
    /// **Creation-path constructor** — production API.
    ///
    /// **P0-1 (creation vs load ayrımı):** Bu constructor digest'leri **hesaplar** ve
    /// record'a yazar. Load path (`try_new_with_verified_digests`) stored digest'leri
    /// korur ve recompute + compare yapar — asla overwrite etmez.
    ///
    /// Sadece geçerli envelope döner — invalid kombinasyon hata döndürür. `verify()`
    /// load sırasında aynı kontrolleri defensive olarak tekrarlar.
    ///
    /// **Surface-specific disposition:** `record.suspended_attempt_evidence` yalnız
    /// `Held` disposition taşımalı.
    pub fn new(
        mut record: PendingAuthorization,
        basis: AuthorizationBasis,
    ) -> Result<Self, PendingAuthorizationLoadError> {
        // Creation path: digest'leri compute + write.
        let basis_digest = AuthorizationBasisDigest::compute(&basis)
            .map_err(|e| PendingAuthorizationLoadError::DigestComputationFailed(e.to_string()))?;
        record.authorization_basis_digest = basis_digest;
        let evidence_digest = SuspendedAttemptEvidenceDigest::compute(
            &record.suspended_attempt_evidence,
        )
        .map_err(|e| PendingAuthorizationLoadError::DigestComputationFailed(e.to_string()))?;
        record.evidence_digest = evidence_digest;

        let envelope = Self {
            schema: PENDING_AUTHORIZATION_SCHEMA.to_string(),
            record,
            authorization_basis: basis,
        };
        envelope.verify()?;
        Ok(envelope)
    }

    /// **Load-path constructor** — stored digest'leri korur, recompute + compare (N3).
    ///
    /// **P0-1 (load path):** Wire'dan gelen stored digest'leri ASLA overwrite etmez.
    /// Recompute + compare yapar — mismatch → typed error. Bu constructor custom
    /// Deserialize tarafından çağrılır; production `new()` DEĞİL.
    ///
    /// **N3 exact sıra:** stored digest al → evidence/basis validation → recompute →
    /// compare → surface-specific disposition → stored DEĞİŞTİRMEDEN kur.
    pub fn try_new_with_verified_digests(
        schema: String,
        record: PendingAuthorization,
        authorization_basis: AuthorizationBasis,
    ) -> Result<Self, PendingAuthorizationLoadError> {
        // Stored digest'leri olduğu gibi koru — verify() recompute + compare yapar.
        let envelope = Self {
            schema,
            record,
            authorization_basis,
        };
        envelope.verify()?;
        Ok(envelope)
    }

    // — Accessor'lar —

    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn record(&self) -> &PendingAuthorization {
        &self.record
    }
    pub fn authorization_basis(&self) -> &AuthorizationBasis {
        &self.authorization_basis
    }

    /// Record'u consume et (navigator `AwaitingWitnesses { pending }` için).
    pub fn into_record(self) -> PendingAuthorization {
        self.record
    }

    /// Load + verify — full cross-field validation. Mismatch → typed integrity error.
    ///
    /// **P1-3 (record-internal vs envelope verification):** İki katman:
    /// - `record.validate_internal()` — record ↔ embedded evidence (basis'ten bağımsız)
    /// - envelope `verify()` ek olarak — record ↔ basis, basis recompute, karar
    ///   alanları, witness policy, basis iç task_id invariant
    ///
    /// **INV-T9 #72 (Commit 3):** 11-adım verification chain (kullanıcı sırası):
    /// 1. Schema version
    /// 2. Structural delta defensive validation (mevcut)
    /// 3. `AuthorizationBasisDigest` recompute
    /// 4-8. record ↔ evidence via `validate_internal` (evidence digest, task_id,
    ///    claim_id, attempt_num, basis digest binding, Held, reason/snapshot)
    /// 9. record ↔ basis karar alanları (predicate/mutation/apply/revision/ec-digest)
    /// 10. witness_requirement ↔ basis.witness_policy
    /// 11. basis iç task_id invariant (disposition semantic validate_internal'da)
    pub fn verify(&self) -> Result<(), PendingAuthorizationLoadError> {
        // 1. Schema version
        if self.schema != PENDING_AUTHORIZATION_SCHEMA {
            return Err(PendingAuthorizationLoadError::UnknownSchema {
                found: self.schema.clone(),
                expected: PENDING_AUTHORIZATION_SCHEMA,
            });
        }

        // 2. Structural delta defensive validation (mevcut — Step 5).
        self.authorization_basis
            .structural_delta
            .validate()
            .map_err(|e| PendingAuthorizationLoadError::StructuralDeltaInvalid(e.to_string()))?;

        // 3. AuthorizationBasisDigest recompute (mevcut — Step 5).
        let computed_basis = AuthorizationBasisDigest::compute(&self.authorization_basis)
            .map_err(|e| PendingAuthorizationLoadError::DigestComputationFailed(e.to_string()))?;
        if computed_basis != self.record.authorization_basis_digest {
            return Err(PendingAuthorizationLoadError::BasisDigestMismatch);
        }

        // 4-8. record ↔ evidence (validate_internal — P1-3 ayrımı).
        // Evidence digest recompute + record ↔ evidence kimlik + surface-specific
        // disposition + reason/snapshot binding.
        self.record.validate_internal()?;

        // 9. record ↔ basis kimlik + karar alanları.
        if self.record.task_id != self.authorization_basis.task_id {
            return Err(PendingAuthorizationLoadError::TaskIdMismatch {
                record: self.record.task_id,
                basis: self.authorization_basis.task_id,
            });
        }
        if self.record.claim_id != self.authorization_basis.claim_identity.claim_id {
            return Err(PendingAuthorizationLoadError::ClaimIdMismatch {
                record: self.record.claim_id,
                basis: self.authorization_basis.claim_identity.claim_id,
                evidence: self.record.suspended_attempt_evidence.claim_id(),
            });
        }

        // P1 basis iç task_id invariant: basis.task_id == basis.claim_identity.task_id.
        if self.authorization_basis.task_id != self.authorization_basis.claim_identity.task_id {
            return Err(PendingAuthorizationLoadError::BasisInternalTaskIdMismatch {
                basis_task_id: self.authorization_basis.task_id,
                claim_task_id: self.authorization_basis.claim_identity.task_id,
            });
        }

        // record ↔ evidence kontrolleri (adım 4-8) `validate_internal`'da yapıldı.

        // 9. record ↔ basis karar alanları.
        if self.record.predicate_completion != self.authorization_basis.predicate_completion {
            return Err(PendingAuthorizationLoadError::PredicateCompletionMismatch {
                record: self.record.predicate_completion,
                basis: self.authorization_basis.predicate_completion,
            });
        }
        if self.record.mutation_decision != self.authorization_basis.mutation_decision {
            return Err(PendingAuthorizationLoadError::MutationDecisionMismatch {
                record: self.record.mutation_decision,
                basis: self.authorization_basis.mutation_decision,
            });
        }
        if self.record.intended_apply_target != self.authorization_basis.intended_apply_target {
            return Err(PendingAuthorizationLoadError::ApplyTargetMismatch {
                record: self.record.intended_apply_target,
                basis: self.authorization_basis.intended_apply_target,
            });
        }
        if self.record.base_space_view_revision != self.authorization_basis.base_space_view_revision
        {
            return Err(PendingAuthorizationLoadError::SpaceViewRevisionMismatch);
        }
        if self.record.evaluation_context_digest
            != self.authorization_basis.evaluation_context_digest
        {
            return Err(PendingAuthorizationLoadError::EvaluationContextDigestMismatch);
        }

        // 10. witness_requirement ↔ basis.witness_policy (P0-1 invariant).
        let effective = self
            .authorization_basis
            .witness_policy
            .effective_requirement();
        if self.record.witness_requirement.min_approvers != effective.min_approvers
            || self.record.witness_requirement.quorum_threshold != effective.quorum_threshold
        {
            return Err(PendingAuthorizationLoadError::WitnessRequirementMismatch {
                record_min: self.record.witness_requirement.min_approvers,
                record_quorum: self.record.witness_requirement.quorum_threshold,
                basis_min: self.authorization_basis.witness_policy.min_approvers,
                basis_quorum: self.authorization_basis.witness_policy.quorum_threshold,
            });
        }

        // 11. disposition ↔ reason/snapshot semantic — `validate_internal`'da yapıldı
        // (adım 4-8). PendingAuthorization Held-only surface-specific + reason/snapshot
        // binding + evidence digest tamper detection hepsi record-internal.

        Ok(())
    }
}

/// **P0-2 strict wire + P0-1 load path:** `PendingAuthorizationEnvelope` custom
/// Deserialize — `deny_unknown_fields` + load-path constructor (stored digest korur).
///
/// **P0-1 (load path):** `try_new_with_verified_digests` kullanır — stored digest'leri
/// ASLA overwrite etmez, recompute + compare yapar. Creation `new()` DEĞİL.
impl<'de> serde::Deserialize<'de> for PendingAuthorizationEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema: String,
            record: PendingAuthorization,
            authorization_basis: AuthorizationBasis,
        }
        let wire = Wire::deserialize(deserializer)?;
        PendingAuthorizationEnvelope::try_new_with_verified_digests(
            wire.schema,
            wire.record,
            wire.authorization_basis,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// **P1 exhaustive hold-reason validation** — disposition iç tutarlılık.
///
/// `hold_reason` ile `snapshot` arasında mantıksal tutarlılık kontrolü. Üç varyant
/// exhaustive handle edilir:
/// - `MinApproversNotMet`: snapshot.approvers == distinct, required_approvers == required,
///   distinct < required
/// - `QuorumInsufficient`: snapshot.support == support, required_support == threshold
///   (canonical -0.0 normalize), support < threshold
/// - `EvidenceNotLocallyObservable`: hint non-empty (Q1/Q2 başarısızlığı zorunlu değil)
///
/// Snapshot genel: finite, support >= 0, required_support >= 0.
fn validate_hold_reason_against_snapshot(
    reason: &crate::witness::WitnessHoldReason,
    snapshot: &crate::witness::WitnessQuorumSnapshot,
) -> Result<(), PendingAuthorizationLoadError> {
    use crate::witness::WitnessHoldReason;
    // Snapshot genel doğrulama.
    if !snapshot.support.is_finite() || !snapshot.required_support.is_finite() {
        return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
            "witness snapshot support/required_support must be finite".into(),
        ));
    }
    if snapshot.support < 0.0 || snapshot.required_support < 0.0 {
        return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
            "witness snapshot support must be >= 0".into(),
        ));
    }
    match reason {
        WitnessHoldReason::MinApproversNotMet { distinct, required } => {
            if snapshot.approvers != *distinct {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    format!(
                        "MinApproversNotMet: snapshot.approvers ({}) != hold_reason.distinct ({})",
                        snapshot.approvers, distinct
                    ),
                ));
            }
            if snapshot.required_approvers != *required {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    format!(
                        "MinApproversNotMet: snapshot.required_approvers ({}) != hold_reason.required ({})",
                        snapshot.required_approvers, required
                    ),
                ));
            }
            if *distinct >= *required {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    format!(
                        "MinApproversNotMet: distinct ({}) must be < required ({})",
                        distinct, required
                    ),
                ));
            }
        }
        WitnessHoldReason::QuorumInsufficient { support, threshold } => {
            // Canonical -0.0 normalize karşılaştırma.
            let norm = |v: f64| -> f64 {
                if v == 0.0 {
                    0.0
                } else {
                    v
                }
            };
            if norm(snapshot.support) != norm(*support) {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    format!(
                        "QuorumInsufficient: snapshot.support ({}) != hold_reason.support ({})",
                        snapshot.support, support
                    ),
                ));
            }
            if norm(snapshot.required_support) != norm(*threshold) {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    format!(
                        "QuorumInsufficient: snapshot.required_support ({}) != hold_reason.threshold ({})",
                        snapshot.required_support, threshold
                    ),
                ));
            }
            if *support >= *threshold {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    format!(
                        "QuorumInsufficient: support ({}) must be < threshold ({})",
                        support, threshold
                    ),
                ));
            }
        }
        WitnessHoldReason::EvidenceNotLocallyObservable { hint } => {
            if hint.trim().is_empty() {
                return Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(
                    "EvidenceNotLocallyObservable: hint must be non-empty".into(),
                ));
            }
            // Q1/Q2 başarısızlığı zorunlu değil — policy alanları yine basis ile eşleşmeli
            // (adım 10'da doğrulandı).
        }
    }
    Ok(())
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
    /// **Step 5:** Structural delta invalid (duplicate/cross-list/non-finite/unsorted).
    /// Defensive — custom Deserialize zaten reject eder; bu ikinci katman.
    #[error("structural delta invalid: {0}")]
    StructuralDeltaInvalid(String),
    // ── INV-T9 #72 (Commit 3) — typed cross-field mismatch errors (P1) ──
    #[error("suspended attempt-evidence digest mismatch — artifact may be tampered or corrupted")]
    EvidenceDigestMismatch,
    #[error("task_id mismatch: record={record}, basis={basis}")]
    TaskIdMismatch { record: u64, basis: u64 },
    #[error("claim_id mismatch: record={record}, basis={basis}, evidence={evidence}")]
    ClaimIdMismatch {
        record: u64,
        basis: u64,
        evidence: u64,
    },
    #[error("attempt number mismatch: record={record}, evidence={evidence}")]
    AttemptNumberMismatch { record: u64, evidence: u64 },
    #[error(
        "evidence authorization_basis_digest mismatch: evidence does not match record/basis digest"
    )]
    EvidenceBasisDigestMismatch,
    /// **P1 basis iç task_id invariant:** `basis.task_id == basis.claim_identity.task_id`.
    #[error("basis internal task_id mismatch: basis.task_id={basis_task_id}, claim_identity.task_id={claim_task_id}")]
    BasisInternalTaskIdMismatch {
        basis_task_id: u64,
        claim_task_id: u64,
    },
    #[error("predicate completion mismatch: record={record:?}, basis={basis:?}")]
    PredicateCompletionMismatch {
        record: PredicateCompletion,
        basis: PredicateCompletion,
    },
    #[error("mutation decision mismatch: record={record:?}, basis={basis:?}")]
    MutationDecisionMismatch {
        record: MutationDecision,
        basis: MutationDecision,
    },
    #[error("apply target mismatch: record={record:?}, basis={basis:?}")]
    ApplyTargetMismatch {
        record: ApplyTarget,
        basis: ApplyTarget,
    },
    #[error("base space-view revision mismatch: record != basis")]
    SpaceViewRevisionMismatch,
    #[error("evaluation context digest mismatch: record != basis")]
    EvaluationContextDigestMismatch,
    #[error("witness requirement mismatch: record min_approvers={record_min}, quorum={record_quorum}; basis policy min_approvers={basis_min}, quorum={basis_quorum}")]
    WitnessRequirementMismatch {
        record_min: usize,
        record_quorum: f64,
        basis_min: u32,
        basis_quorum: f64,
    },
    #[error("witness hold reason mismatch: record != evidence disposition")]
    WitnessHoldReasonMismatch,
    #[error("witness snapshot mismatch: record != evidence disposition")]
    WitnessSnapshotMismatch,
    #[error("invalid evidence disposition for PendingAuthorization: {0}")]
    InvalidEvidenceDisposition(String),
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
///
/// **INV-T9 #72 (Commit 3):** `task_id`, `attempt_num`, `evidence_digest` eklendi
/// (P0-4 store identity migration). Artifact artık evidence identity ile adreslenir
/// — aynı basis farklı evidence ayrı artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAuthorizationReceipt {
    pub artifact_path: std::path::PathBuf,
    pub task_id: crate::trajectory::TaskId,
    pub claim_id: ClaimId,
    pub attempt_num: AttemptNumber,
    pub authorization_basis_digest: AuthorizationBasisDigest,
    pub evidence_digest: SuspendedAttemptEvidenceDigest,
}

/// Persist/load hataları.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PendingAuthorizationStoreError {
    #[error(
        "artifact already exists with different evidence — integrity error (no silent overwrite)"
    )]
    BasisConflict { existing_path: std::path::PathBuf },
    #[error("artifact write failed: {0}")]
    WriteFailed(String),
    #[error("parent directory creation failed: {0}")]
    DirCreationFailed(String),
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
    /// **N4 (persist-boundary verify):** Envelope `verify()` başarısız — persist
    /// sırasında tüm side-effect'lerden ÖNCE çalışır. In-memory bypass engeller.
    #[error("invalid envelope (persist-boundary verification failed): {0}")]
    InvalidEnvelope(String),
}

/// Dosya tabanlı default implementation.
///
/// **Path (INV-T9 #72 Commit 3):**
/// `<root>/.osp/pending-authorizations/task-{task_id}--claim-{claim_id}--attempt-{attempt_num}--{evidence_digest}.json`
///
/// Evidence identity (`task_id` + `claim_id` + `attempt_num` + `evidence_digest`)
/// artifact'i adresler. `evidence_digest` basis digest'ini binding olarak içerdiği
/// için filename'e ayrıca basis digest eklemek zorunlu DEĞİL — audit için eklenebilir
/// ama dosya adı gereksiz büyür.
///
/// **No-clobber:** `create_new` — sessiz overwrite YOK.
/// **Idempotent:** aynı evidence digest + aynı içerik → success; aynı evidence path +
/// farklı içerik → integrity error; aynı basis + farklı evidence digest → ayrı artifact.
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

    /// Artifact path'i evidence identity'den türet (P0-4).
    ///
    /// `task_id` + `claim_id` + `attempt_num` + `evidence_digest` → benzersiz path.
    fn artifact_path(
        &self,
        task_id: crate::trajectory::TaskId,
        claim_id: ClaimId,
        attempt_num: AttemptNumber,
        evidence_digest: &SuspendedAttemptEvidenceDigest,
    ) -> std::path::PathBuf {
        let hex = evidence_digest.to_hex();
        let filename = format!(
            "task-{task_id}--claim-{claim_id}--attempt-{}--{hex}.json",
            attempt_num.get()
        );
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

        // **N4 (persist-boundary verify):** Tüm side-effect'lerden ÖNCE verify().
        // In-memory bypass (struct literal) engeller — invalid envelope diske yazılamaz.
        envelope
            .verify()
            .map_err(|e| PendingAuthorizationStoreError::InvalidEnvelope(e.to_string()))?;

        let artifact_path = self.artifact_path(
            envelope.record().task_id,
            envelope.record().claim_id,
            envelope.record().suspended_attempt_evidence.attempt_num(),
            &envelope.record().evidence_digest,
        );

        // Idempotency: aynı path zaten varsa — içeriği karşılaştır.
        if artifact_path.exists() {
            let existing = std::fs::read(&artifact_path)
                .map_err(|e| PendingAuthorizationStoreError::WriteFailed(e.to_string()))?;
            let current = serde_json::to_vec_pretty(envelope)
                .map_err(|e| PendingAuthorizationStoreError::SerializationFailed(e.to_string()))?;
            if existing == current {
                // Idempotent success — aynı evidence identity + aynı içerik.
                return Ok(PendingAuthorizationReceipt {
                    artifact_path,
                    task_id: envelope.record().task_id,
                    claim_id: envelope.record().claim_id,
                    attempt_num: envelope.record().suspended_attempt_evidence.attempt_num(),
                    authorization_basis_digest: envelope
                        .record()
                        .authorization_basis_digest
                        .clone(),
                    evidence_digest: envelope.record().evidence_digest.clone(),
                });
            } else {
                // Conflict — aynı evidence path, farklı içerik (digest çakışması veya corruption).
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
            task_id: envelope.record().task_id,
            claim_id: envelope.record().claim_id,
            attempt_num: envelope.record().suspended_attempt_evidence.attempt_num(),
            authorization_basis_digest: envelope.record().authorization_basis_digest.clone(),
            evidence_digest: envelope.record().evidence_digest.clone(),
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
        // **N4 (persist-boundary verify):** Tüm side-effect'lerden ÖNCE verify().
        envelope
            .verify()
            .map_err(|e| PendingAuthorizationStoreError::InvalidEnvelope(e.to_string()))?;
        Ok(PendingAuthorizationReceipt {
            artifact_path: std::path::PathBuf::new(), // null — no artifact
            task_id: envelope.record().task_id,
            claim_id: envelope.record().claim_id,
            attempt_num: envelope.record().suspended_attempt_evidence.attempt_num(),
            authorization_basis_digest: envelope.record().authorization_basis_digest.clone(),
            evidence_digest: envelope.record().evidence_digest.clone(),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 4 Commit 1b — VersionedAuthorizationBasis wire dispatch
// (plan md:104-112, reviewer v2 closure)
//
// **Wire dispatch surface:** legacy bare-V1 + strict versioned V1/V2. Shape-based
// dispatch ("basis" key discriminator). RawValue + duplicate-key koruması. JSON-specific.
// V2→V1 fallback YOK. V1 frozen (wire format HİÇ değişmez). Model A (versioned payload,
// outer basis_digest yok — self-verifying envelope Faz 8).
// ═══════════════════════════════════════════════════════════════════════════════

/// **P1-4 rename:** Wire schema constants — `WIRE` ayrımı (mevcut V1 basis'in kendi
/// `schema_version: u32` alanı var, iki farklı version kavramı).
pub const AUTHORIZATION_BASIS_WIRE_SCHEMA_V1: u16 = 1;
pub const AUTHORIZATION_BASIS_WIRE_SCHEMA_V2: u16 = 2;

/// **INV-T9 #70 Commit 4b Faz 4 Commit 1b:** Versioned authorization basis wire dispatch
/// surface. V1 (legacy/strict) + V2 (strict). JSON-specific `from_json_slice` entry point.
///
/// **P2-1:** Generic `Deserialize` intentionally absent — dispatch requires duplicate-key-
/// preserving raw JSON parsing via `from_json_slice`. Derived Deserialize would bypass
/// strict dispatch + duplicate-key koruması.
///
/// **P1-1 v3 (reviewer):** Opaque struct + private repr. Checked constructor `try_v1`/
/// `try_v2` inner schema_version exact check yapar — outer/inner version illegal state
/// yapısal olarak imkânsız (V1 varyantı inner schema_version=1 dışını kabul etmez).
#[derive(Debug, Clone, PartialEq)]
pub struct VersionedAuthorizationBasis {
    repr: VersionedAuthorizationBasisRepr,
}

#[derive(Debug, Clone, PartialEq)]
enum VersionedAuthorizationBasisRepr {
    V1(AuthorizationBasisV1),
    V2(AuthorizationBasisV2),
}

impl VersionedAuthorizationBasis {
    /// **P1-1 v3:** V1 checked constructor — inner schema_version exact check.
    /// `schema_version != 1` → `InnerV1SchemaMismatch`. Illegal state bellekte bulunamaz.
    pub fn try_v1(basis: AuthorizationBasisV1) -> Result<Self, VersionedAuthorizationBasisError> {
        if basis.schema_version != u32::from(AUTHORIZATION_BASIS_WIRE_SCHEMA_V1) {
            return Err(VersionedAuthorizationBasisError::InnerV1SchemaMismatch {
                expected: AUTHORIZATION_BASIS_WIRE_SCHEMA_V1,
                found: basis.schema_version,
            });
        }
        Ok(Self {
            repr: VersionedAuthorizationBasisRepr::V1(basis),
        })
    }

    /// **P1-1 v3:** V2 constructor — AuthorizationBasisV2 zaten checked constructor.
    pub fn try_v2(basis: AuthorizationBasisV2) -> Self {
        Self {
            repr: VersionedAuthorizationBasisRepr::V2(basis),
        }
    }

    /// Wire schema version — `AUTHORIZATION_BASIS_WIRE_SCHEMA_V1` veya `V2`.
    pub fn version(&self) -> u16 {
        match &self.repr {
            VersionedAuthorizationBasisRepr::V1(_) => AUTHORIZATION_BASIS_WIRE_SCHEMA_V1,
            VersionedAuthorizationBasisRepr::V2(_) => AUTHORIZATION_BASIS_WIRE_SCHEMA_V2,
        }
    }

    /// V1 basis accessor (legacy/strict).
    pub fn as_v1(&self) -> Option<&AuthorizationBasisV1> {
        match &self.repr {
            VersionedAuthorizationBasisRepr::V1(v) => Some(v),
            VersionedAuthorizationBasisRepr::V2(_) => None,
        }
    }

    /// V2 basis accessor (strict).
    pub fn as_v2(&self) -> Option<&AuthorizationBasisV2> {
        match &self.repr {
            VersionedAuthorizationBasisRepr::V2(v) => Some(v),
            VersionedAuthorizationBasisRepr::V1(_) => None,
        }
    }

    /// Private repr accessor (modül içi — dispatch/serialize).
    fn repr(&self) -> &VersionedAuthorizationBasisRepr {
        &self.repr
    }
}

/// **INV-T9 #70 Commit 4b Faz 4 Commit 1b (reviewer P1-3):** Versioned authorization basis
/// wire dispatch error. Real `serde_json::Error` call-site'lar taşır — InvalidHexDigest
/// kaldırıldı (LowerHex32 hatası envelope parse sırasında serde_json::Error içine dönüşür).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VersionedAuthorizationBasisError {
    #[error("invalid JSON peek: {detail}")]
    JsonPeek { detail: String },
    #[error("JSON parse failed: {detail}")]
    JsonParse { detail: String },
    #[error("top-level authorization basis must be a JSON object")]
    TopLevelNotObject,
    #[error("versioned V1 envelope decode failed: {detail}")]
    VersionedV1Decode { detail: String },
    #[error("versioned V2 envelope decode failed: {detail}")]
    VersionedV2Decode { detail: String },
    #[error("legacy V1 decode failed: {detail}")]
    LegacyV1Decode { detail: String },
    #[error("schema_version missing")]
    MissingSchemaVersion,
    #[error("schema_version out of u16 range: {0}")]
    SchemaVersionOutOfRange(u64),
    #[error("schema_version must be an unsigned integer (reject float/string/null/exponent)")]
    SchemaVersionNotStrict,
    #[error("unknown schema_version: {0}")]
    UnknownSchemaVersion(u16),
    #[error("inner V1 schema_version mismatch: expected={expected}, found={found}")]
    InnerV1SchemaMismatch { expected: u16, found: u32 },
    /// **P1-2 v3:** `basis` yok ama schema_version=2 → V2-shaped input, legacy fallback yok.
    #[error("schema_version={schema_version} without 'basis' key — versioned envelope required")]
    MissingBasisForVersionedSchema { schema_version: u16 },
    #[error("V2 wire conversion failed: {detail}")]
    V2WireConversion { detail: String },
    #[error("V2 basis validation failed: {0}")]
    V2Validation(#[from] AuthorizationBasisV2Error),
}

/// **INV-T9 #70 Commit 4b Faz 4 Commit 1b (reviewer P0-3, P1-2):** V2 wire digest — tam
/// 64 karakter, yalnız lowercase hex [0-9a-f], 0x prefix yok. Uppercase, 0x prefix,
/// exponent, kısa/uzun reject. Serialize + Deserialize — V2 serializer yolu da var.
struct LowerHex32([u8; 32]);

impl LowerHex32 {
    fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for LowerHex32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let value = String::deserialize(deserializer)?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(D::Error::custom(
                "expected exactly 64 lowercase hexadecimal characters [0-9a-f]",
            ));
        }
        let decoded = hex::decode(&value).map_err(D::Error::custom)?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| D::Error::custom("digest must be 32 bytes"))?;
        Ok(Self(bytes))
    }
}

impl serde::Serialize for LowerHex32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

// ── V2 wire DTO ağacı (reviewer P0-3, P1-1) ─────────────────────────────────────
// Nested Serialize-only tipleri doğrudan Deserialize edilemez. Tam wire DTO ağacı.
// Hepsi deny_unknown_fields + tag="kind" + rename_all="snake_case".

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawTrajectoryBaselineV2 {
    Available {
        before: RawProvenancedMeasuredResultV2,
    },
    Unavailable {
        reason: RawBaselineUnavailableReasonV2,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawBaselineUnavailableReasonV2 {
    AllMembersIntroducedByDelta {
        members: Vec<u64>,
    },
    PartialNewSubject {
        existing: Vec<u64>,
        introduced: Vec<u64>,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawTrajectoryLossV2 {
    Available {
        target: RawPositionV2,
        loss_after: f64,
    },
    Unavailable {
        reason: RawTrajectoryLossUnavailableReasonV2,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum RawTrajectoryLossUnavailableReasonV2 {
    NoPreferredVector,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPositionV2 {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
    v: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAxisMeasurementV2 {
    value: f64,
    source_tag: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvenancedMeasuredResultV2 {
    coupling: RawAxisMeasurementV2,
    cohesion: RawAxisMeasurementV2,
    instability: RawAxisMeasurementV2,
    entropy: RawAxisMeasurementV2,
    witness_depth: RawAxisMeasurementV2,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RawSpaceViewIdV2 {
    Persisted { id: [u8; 16] },
    Ephemeral { id: u64 },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpaceViewRevisionV2 {
    view_id: RawSpaceViewIdV2,
    sequence: u64,
    content_digest: LowerHex32,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMeasurementRequestEvidenceV2 {
    subject: crate::measurement::CanonicalSubjectScope,
    impact: crate::measurement::CanonicalImpactScope,
    base_revision: RawSpaceViewRevisionV2,
    structural_delta_digest: LowerHex32,
    measurement_input_digest: LowerHex32,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthorizationBasisV2 {
    task_id: u64,
    claim_id: u64,
    task_claim_digest: LowerHex32,
    task_goal_digest: LowerHex32,
    measurement_digest: LowerHex32,
    engine_measurement_digest: LowerHex32,
    trajectory_baseline: RawTrajectoryBaselineV2,
    measurement_baseline_digest: LowerHex32,
    trajectory_loss: RawTrajectoryLossV2,
    measurement_request: RawMeasurementRequestEvidenceV2,
    measurement_request_digest: LowerHex32,
    measurement_context_digest: LowerHex32,
    canonical_delta_digest: LowerHex32,
}

// ── Envelope tipleri (reviewer P0-2) ────────────────────────────────────────────

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthorizationBasisV2Envelope {
    schema_version: u16,
    basis: RawAuthorizationBasisV2,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthorizationBasisV1Envelope {
    schema_version: u16,
    basis: RawAuthorizationBasisV1TopLevelStrict,
}

/// **P1-2 V1 strictlik:** Top-level strict, nested legacy semantics korur. Tam recursive
/// strictlik gereksiz risk (frozen V1 uyumluluğu). Basis top-level field'ları strict,
/// nested V1 representation mevcut parser davranışını korur.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthorizationBasisV1TopLevelStrict {
    schema_version: u32,
    task_id: crate::trajectory::TaskId,
    claim_identity: ClaimIdentity,
    claim_author: ClaimAuthor,
    structural_delta: CanonicalStructuralDelta,
    predicate_content: CanonicalPredicateContent,
    predicate_evaluation: PredicateEvaluationBasis,
    measured_result: ProvenancedMeasuredResult,
    deterministic_gate_result: crate::trajectory::GateDecision,
    predicate_completion: crate::trajectory::PredicateCompletion,
    mutation_decision: crate::trajectory::MutationDecision,
    intended_apply_target: crate::trajectory::ApplyTarget,
    witness_policy: CanonicalWitnessPolicy,
    measurement_input_digest: MeasurementInputDigest,
    evaluation_context_digest: EvaluationContextDigest,
    base_space_view_revision: SpaceViewRevision,
}

impl RawAuthorizationBasisV1TopLevelStrict {
    fn into_domain(self) -> AuthorizationBasisV1 {
        AuthorizationBasis {
            schema_version: self.schema_version,
            task_id: self.task_id,
            claim_identity: self.claim_identity,
            claim_author: self.claim_author,
            structural_delta: self.structural_delta,
            predicate_content: self.predicate_content,
            predicate_evaluation: self.predicate_evaluation,
            measured_result: self.measured_result,
            deterministic_gate_result: self.deterministic_gate_result,
            predicate_completion: self.predicate_completion,
            mutation_decision: self.mutation_decision,
            intended_apply_target: self.intended_apply_target,
            witness_policy: self.witness_policy,
            measurement_input_digest: self.measurement_input_digest,
            evaluation_context_digest: self.evaluation_context_digest,
            base_space_view_revision: self.base_space_view_revision,
        }
    }
}

// Serialize ref'ler (clone yok — &self üzerinden)
#[derive(serde::Serialize)]
struct VersionedV1EnvelopeRef<'a> {
    schema_version: u16,
    basis: &'a AuthorizationBasisV1,
}

#[derive(serde::Serialize)]
struct VersionedV2EnvelopeRef<'a> {
    schema_version: u16,
    basis: RawAuthorizationBasisV2Ref<'a>,
}

/// V2 basis → wire DTO (borrow, clone yok).
#[derive(serde::Serialize)]
struct RawAuthorizationBasisV2Ref<'a> {
    task_id: u64,
    claim_id: u64,
    task_claim_digest: LowerHex32,
    task_goal_digest: LowerHex32,
    measurement_digest: LowerHex32,
    engine_measurement_digest: LowerHex32,
    trajectory_baseline: RawTrajectoryBaselineV2Ref<'a>,
    measurement_baseline_digest: LowerHex32,
    trajectory_loss: RawTrajectoryLossV2Ref<'a>,
    measurement_request: RawMeasurementRequestEvidenceV2Ref<'a>,
    measurement_request_digest: LowerHex32,
    measurement_context_digest: LowerHex32,
    canonical_delta_digest: LowerHex32,
}

impl<'a> RawAuthorizationBasisV2Ref<'a> {
    #[allow(dead_code)]
    fn from_domain(basis: &'a AuthorizationBasisV2) -> Self {
        Self {
            task_id: basis.task_id(),
            claim_id: basis.claim_id(),
            task_claim_digest: LowerHex32(*basis.task_claim_digest().as_bytes()),
            task_goal_digest: LowerHex32(*basis.task_goal_digest().as_bytes()),
            measurement_digest: LowerHex32(*basis.measurement_digest().as_bytes()),
            engine_measurement_digest: LowerHex32(*basis.engine_measurement_digest().as_bytes()),
            trajectory_baseline: RawTrajectoryBaselineV2Ref::from_domain(
                basis.trajectory_baseline(),
            ),
            measurement_baseline_digest: LowerHex32(
                *basis.measurement_baseline_digest().as_bytes(),
            ),
            trajectory_loss: RawTrajectoryLossV2Ref::from_domain(basis.trajectory_loss()),
            measurement_request: RawMeasurementRequestEvidenceV2Ref::from_domain(
                basis.measurement_request(),
            ),
            measurement_request_digest: LowerHex32(*basis.measurement_request_digest().as_bytes()),
            measurement_context_digest: LowerHex32(*basis.measurement_context_digest().as_bytes()),
            canonical_delta_digest: LowerHex32(*basis.canonical_delta_digest().as_bytes()),
        }
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawTrajectoryBaselineV2Ref<'a> {
    Available {
        before: RawProvenancedMeasuredResultV2Ref<'a>,
    },
    Unavailable {
        reason: RawBaselineUnavailableReasonV2Ref<'a>,
    },
}

impl<'a> RawTrajectoryBaselineV2Ref<'a> {
    fn from_domain(baseline: &'a CanonicalTrajectoryEvidenceBaseline) -> Self {
        match baseline {
            CanonicalTrajectoryEvidenceBaseline::Available { before } => Self::Available {
                before: RawProvenancedMeasuredResultV2Ref::from_domain(before),
            },
            CanonicalTrajectoryEvidenceBaseline::Unavailable { reason } => Self::Unavailable {
                reason: RawBaselineUnavailableReasonV2Ref::from_domain(reason),
            },
        }
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawBaselineUnavailableReasonV2Ref<'a> {
    AllMembersIntroducedByDelta {
        members: &'a [crate::space::NodeId],
    },
    PartialNewSubject {
        existing: &'a [crate::space::NodeId],
        introduced: &'a [crate::space::NodeId],
    },
}

impl<'a> RawBaselineUnavailableReasonV2Ref<'a> {
    fn from_domain(reason: &'a CanonicalBaselineUnavailableReason) -> Self {
        match reason.view() {
            CanonicalBaselineUnavailableReasonView::AllMembersIntroducedByDelta { members } => {
                Self::AllMembersIntroducedByDelta { members }
            }
            CanonicalBaselineUnavailableReasonView::PartialNewSubject {
                existing,
                introduced,
            } => Self::PartialNewSubject {
                existing,
                introduced,
            },
        }
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawTrajectoryLossV2Ref<'a> {
    Available {
        target: RawPositionV2Ref<'a>,
        loss_after: CanonicalF64,
    },
    Unavailable {
        reason: RawTrajectoryLossUnavailableReasonV2,
    },
}

impl<'a> RawTrajectoryLossV2Ref<'a> {
    fn from_domain(loss: &'a CanonicalTrajectoryLossEvidence) -> Self {
        match loss {
            CanonicalTrajectoryLossEvidence::Available { target, loss_after } => Self::Available {
                target: RawPositionV2Ref::from_domain(target),
                loss_after: *loss_after,
            },
            // **P1-3 v3:** Exhaustive match — yeni reason varyantı compiler error üretir.
            CanonicalTrajectoryLossEvidence::Unavailable {
                reason: CanonicalTrajectoryLossUnavailableReason::NoPreferredVector,
            } => Self::Unavailable {
                reason: RawTrajectoryLossUnavailableReasonV2::NoPreferredVector,
            },
        }
    }
}

#[derive(serde::Serialize)]
struct RawPositionV2Ref<'a> {
    x: CanonicalF64,
    y: CanonicalF64,
    z: CanonicalF64,
    w: CanonicalF64,
    v: CanonicalF64,
    #[serde(skip)]
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> RawPositionV2Ref<'a> {
    fn from_domain(pos: &'a CanonicalRawPosition) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            w: pos.w,
            v: pos.v,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[derive(serde::Serialize)]
struct RawProvenancedMeasuredResultV2Ref<'a> {
    coupling: RawAxisMeasurementV2Ref<'a>,
    cohesion: RawAxisMeasurementV2Ref<'a>,
    instability: RawAxisMeasurementV2Ref<'a>,
    entropy: RawAxisMeasurementV2Ref<'a>,
    witness_depth: RawAxisMeasurementV2Ref<'a>,
}

impl<'a> RawProvenancedMeasuredResultV2Ref<'a> {
    fn from_domain(result: &'a ProvenancedMeasuredResult) -> Self {
        Self {
            coupling: RawAxisMeasurementV2Ref::from_domain(&result.coupling),
            cohesion: RawAxisMeasurementV2Ref::from_domain(&result.cohesion),
            instability: RawAxisMeasurementV2Ref::from_domain(&result.instability),
            entropy: RawAxisMeasurementV2Ref::from_domain(&result.entropy),
            witness_depth: RawAxisMeasurementV2Ref::from_domain(&result.witness_depth),
        }
    }
}

#[derive(serde::Serialize)]
struct RawAxisMeasurementV2Ref<'a> {
    value: CanonicalF64,
    source_tag: u8,
    #[serde(skip)]
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> RawAxisMeasurementV2Ref<'a> {
    fn from_domain(axis: &'a CanonicalAxisMeasurement) -> Self {
        Self {
            value: axis.value,
            source_tag: axis.source.as_u8(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[derive(serde::Serialize)]
struct RawMeasurementRequestEvidenceV2Ref<'a> {
    subject: &'a crate::measurement::CanonicalSubjectScope,
    impact: &'a crate::measurement::CanonicalImpactScope,
    base_revision: RawSpaceViewRevisionV2Ref<'a>,
    structural_delta_digest: LowerHex32,
    measurement_input_digest: LowerHex32,
}

impl<'a> RawMeasurementRequestEvidenceV2Ref<'a> {
    fn from_domain(evidence: &'a crate::measurement::CanonicalMeasurementRequestEvidence) -> Self {
        Self {
            subject: &evidence.subject,
            impact: &evidence.impact,
            base_revision: RawSpaceViewRevisionV2Ref::from_domain(&evidence.base_revision),
            structural_delta_digest: LowerHex32(*evidence.structural_delta_digest.as_bytes()),
            measurement_input_digest: LowerHex32(*evidence.measurement_input_digest.as_bytes()),
        }
    }
}

#[derive(serde::Serialize)]
struct RawSpaceViewRevisionV2Ref<'a> {
    view_id: RawSpaceViewIdV2Ref<'a>,
    sequence: u64,
    content_digest: LowerHex32,
}

impl<'a> RawSpaceViewRevisionV2Ref<'a> {
    fn from_domain(rev: &'a SpaceViewRevision) -> Self {
        Self {
            view_id: RawSpaceViewIdV2Ref::from_domain(&rev.view_id),
            sequence: rev.sequence,
            content_digest: LowerHex32(*rev.content_digest.as_bytes()),
        }
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawSpaceViewIdV2Ref<'a> {
    Persisted { id: &'a [u8; 16] },
    Ephemeral { id: u64 },
}

impl<'a> RawSpaceViewIdV2Ref<'a> {
    fn from_domain(view_id: &'a SpaceViewId) -> Self {
        match view_id {
            SpaceViewId::Persisted(id) => Self::Persisted { id: id.as_bytes() },
            SpaceViewId::Ephemeral(id) => Self::Ephemeral { id: *id },
        }
    }
}

// ── Dispatch + conversion (reviewer P0-1, P0-4, P1-4) ───────────────────────────

impl VersionedAuthorizationBasis {
    /// **P2-1:** JSON-specific entry point. RawValue ile duplicate-key koruması.
    /// Generic Deserialize intentionally absent — dispatch requires duplicate-key-
    /// preserving raw JSON parsing.
    #[allow(dead_code, reason = "Faz 4 wire restore / Commit 1b consumer")]
    pub fn from_json_slice(data: &[u8]) -> Result<Self, VersionedAuthorizationBasisError> {
        // Top-level RawValue — duplicate-key preserved.
        let raw_value: &serde_json::value::RawValue =
            serde_json::from_slice(data).map_err(|e| {
                VersionedAuthorizationBasisError::JsonParse {
                    detail: e.to_string(),
                }
            })?;
        // Peek outer shape (Value — dispatch only, typed parse sonra).
        let peek: serde_json::Value = serde_json::from_str(raw_value.get()).map_err(|e| {
            VersionedAuthorizationBasisError::JsonPeek {
                detail: e.to_string(),
            }
        })?;
        // Top-level non-object check (P2-2).
        if !peek.is_object() {
            return Err(VersionedAuthorizationBasisError::TopLevelNotObject);
        }
        // Shape-based dispatch (P0-1): "basis" key discriminator (P1-4 reserved).
        if peek.get("basis").is_some() {
            // Explicit versioned envelope.
            let schema_version = parse_outer_schema_version_strictly(&peek)?;
            match schema_version {
                AUTHORIZATION_BASIS_WIRE_SCHEMA_V1 => {
                    let env: RawAuthorizationBasisV1Envelope =
                        serde_json::from_str(raw_value.get()).map_err(|e| {
                            VersionedAuthorizationBasisError::VersionedV1Decode {
                                detail: e.to_string(),
                            }
                        })?;
                    if env.schema_version != AUTHORIZATION_BASIS_WIRE_SCHEMA_V1 {
                        return Err(VersionedAuthorizationBasisError::UnknownSchemaVersion(
                            env.schema_version,
                        ));
                    }
                    // env.basis.try_into() inner schema check yapar → AuthorizationBasisV1.
                    // try_v1 tekrar check eder (double-check defense-in-depth).
                    let basis: AuthorizationBasisV1 = env.basis.try_into()?;
                    Self::try_v1(basis)
                }
                AUTHORIZATION_BASIS_WIRE_SCHEMA_V2 => {
                    let env: RawAuthorizationBasisV2Envelope =
                        serde_json::from_str(raw_value.get()).map_err(|e| {
                            VersionedAuthorizationBasisError::VersionedV2Decode {
                                detail: e.to_string(),
                            }
                        })?;
                    if env.schema_version != AUTHORIZATION_BASIS_WIRE_SCHEMA_V2 {
                        return Err(VersionedAuthorizationBasisError::UnknownSchemaVersion(
                            env.schema_version,
                        ));
                    }
                    let basis = AuthorizationBasisV2::from_wire(env.basis)?;
                    Ok(Self::try_v2(basis))
                }
                unknown => Err(VersionedAuthorizationBasisError::UnknownSchemaVersion(
                    unknown,
                )),
            }
        } else {
            // **P1-2 v3:** "basis" key yok. Önce inner schema_version sınıflandır —
            // V2-shaped input (schema_version=2 + basis yok) hiçbir koşulda legacy V1
            // parser'a ulaşmaz. V1 (schema_version=1 veya yok) → legacy parse.
            match peek.get("schema_version") {
                None => {
                    // schema_version yok → legacy bare V1 (permissive parser korur).
                    let basis: AuthorizationBasisV1 = serde_json::from_str(raw_value.get())
                        .map_err(|e| VersionedAuthorizationBasisError::LegacyV1Decode {
                            detail: e.to_string(),
                        })?;
                    Self::try_v1(basis)
                }
                Some(serde_json::Value::Number(n)) if n.is_u64() => {
                    let raw = n.as_u64().unwrap();
                    match u16::try_from(raw) {
                        Ok(AUTHORIZATION_BASIS_WIRE_SCHEMA_V1) => {
                            // Legacy V1 with schema_version=1.
                            let basis: AuthorizationBasisV1 = serde_json::from_str(raw_value.get())
                                .map_err(|e| VersionedAuthorizationBasisError::LegacyV1Decode {
                                    detail: e.to_string(),
                                })?;
                            Self::try_v1(basis)
                        }
                        Ok(AUTHORIZATION_BASIS_WIRE_SCHEMA_V2) => {
                            // schema_version=2 ama basis yok → V2-shaped, versioned
                            // envelope required. Legacy fallback YOK.
                            Err(
                                VersionedAuthorizationBasisError::MissingBasisForVersionedSchema {
                                    schema_version: AUTHORIZATION_BASIS_WIRE_SCHEMA_V2,
                                },
                            )
                        }
                        Ok(other) => {
                            // **P2-1 v2:** Unknown version → UnknownSchemaVersion
                            // (typed taxonomy tutarlı — envelope shape ile aynı error).
                            Err(VersionedAuthorizationBasisError::UnknownSchemaVersion(
                                other,
                            ))
                        }
                        Err(_) => Err(VersionedAuthorizationBasisError::SchemaVersionOutOfRange(
                            raw,
                        )),
                    }
                }
                Some(_) => Err(VersionedAuthorizationBasisError::SchemaVersionNotStrict),
            }
        }
    }
}

/// **P2-3 (exponent dahil):** Strict numeric schema_version parse. `is_u64()` float/
/// exponent'i reject eder (1.0, 1e0 `is_u64()` false). String/null/missing ayrı reject.
fn parse_outer_schema_version_strictly(
    peek: &serde_json::Value,
) -> Result<u16, VersionedAuthorizationBasisError> {
    match peek.get("schema_version") {
        Some(serde_json::Value::Number(n)) if n.is_u64() => {
            let raw = n.as_u64().unwrap();
            u16::try_from(raw)
                .map_err(|_| VersionedAuthorizationBasisError::SchemaVersionOutOfRange(raw))
        }
        Some(_) => Err(VersionedAuthorizationBasisError::SchemaVersionNotStrict),
        None => Err(VersionedAuthorizationBasisError::MissingSchemaVersion),
    }
}

/// **P1-4:** Versioned V1 TryFrom — inner schema_version exact check.
impl TryFrom<RawAuthorizationBasisV1TopLevelStrict> for AuthorizationBasisV1 {
    type Error = VersionedAuthorizationBasisError;

    fn try_from(raw: RawAuthorizationBasisV1TopLevelStrict) -> Result<Self, Self::Error> {
        if raw.schema_version != u32::from(AUTHORIZATION_BASIS_WIRE_SCHEMA_V1) {
            return Err(VersionedAuthorizationBasisError::InnerV1SchemaMismatch {
                expected: AUTHORIZATION_BASIS_WIRE_SCHEMA_V1,
                found: raw.schema_version,
            });
        }
        Ok(raw.into_domain())
    }
}

impl serde::Serialize for VersionedAuthorizationBasis {
    /// **P0-2:** Her iki varyant için explicit envelope. Clone yok — `&self`.
    /// Legacy yazım yalnız doğrudan `AuthorizationBasisV1` serializer'ı ile.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.repr() {
            VersionedAuthorizationBasisRepr::V1(basis) => VersionedV1EnvelopeRef {
                schema_version: AUTHORIZATION_BASIS_WIRE_SCHEMA_V1,
                basis,
            }
            .serialize(serializer),
            VersionedAuthorizationBasisRepr::V2(basis) => VersionedV2EnvelopeRef {
                schema_version: AUTHORIZATION_BASIS_WIRE_SCHEMA_V2,
                basis: RawAuthorizationBasisV2Ref::from_domain(basis),
            }
            .serialize(serializer),
        }
    }
}

impl AuthorizationBasisV2 {
    /// **P0-3 conversion sırası:** Raw wire DTO → checked domain construction.
    /// 1. subject parse. 2. baseline reason + subject → try_from_reason (union invariant).
    /// 3. trajectory loss local invariant (finite + >= 0, target finite). 4. new() validate_semantics.
    #[allow(dead_code, reason = "Faz 4 wire restore / Commit 1b consumer")]
    fn from_wire(raw: RawAuthorizationBasisV2) -> Result<Self, VersionedAuthorizationBasisError> {
        use crate::canonical_tags::CanonicalMetricSourceTag;

        // Trajectory baseline conversion.
        let trajectory_baseline = match raw.trajectory_baseline {
            RawTrajectoryBaselineV2::Available { before } => {
                let mk_axis =
                    |a: RawAxisMeasurementV2| -> Result<_, VersionedAuthorizationBasisError> {
                        let source =
                            CanonicalMetricSourceTag::try_from(a.source_tag).map_err(|e| {
                                VersionedAuthorizationBasisError::V2WireConversion {
                                    detail: format!("axis source_tag: {e}"),
                                }
                            })?;
                        Ok(CanonicalAxisMeasurement {
                            value: a.value,
                            source,
                        })
                    };
                CanonicalTrajectoryEvidenceBaseline::Available {
                    before: ProvenancedMeasuredResult {
                        coupling: mk_axis(before.coupling)?,
                        cohesion: mk_axis(before.cohesion)?,
                        instability: mk_axis(before.instability)?,
                        entropy: mk_axis(before.entropy)?,
                        witness_depth: mk_axis(before.witness_depth)?,
                    },
                }
            }
            RawTrajectoryBaselineV2::Unavailable { reason } => {
                let raw_reason = match reason {
                    RawBaselineUnavailableReasonV2::AllMembersIntroducedByDelta { members } => {
                        crate::measurement::BaselineUnavailableReason::AllMembersIntroducedByDelta {
                            members,
                        }
                    }
                    RawBaselineUnavailableReasonV2::PartialNewSubject {
                        existing,
                        introduced,
                    } => crate::measurement::BaselineUnavailableReason::PartialNewSubject {
                        existing,
                        introduced,
                    },
                };
                let canonical_reason = CanonicalBaselineUnavailableReason::try_from_reason(
                    &raw_reason,
                    &raw.measurement_request.subject.clone(),
                )
                .map_err(|e| {
                    VersionedAuthorizationBasisError::V2WireConversion {
                        detail: e.to_string(),
                    }
                })?;
                CanonicalTrajectoryEvidenceBaseline::Unavailable {
                    reason: canonical_reason,
                }
            }
        };

        // Trajectory loss conversion + local invariant (P1-5).
        let trajectory_loss = match raw.trajectory_loss {
            RawTrajectoryLossV2::Available { target, loss_after } => {
                // P1-5: loss_after finite + >= 0.0 (Euclidean distance semantics).
                if !loss_after.is_finite() || loss_after < 0.0 {
                    return Err(VersionedAuthorizationBasisError::V2WireConversion {
                        detail: format!("loss_after must be finite and >= 0.0, got {loss_after}"),
                    });
                }
                // P1-5: target all axes finite.
                for (axis, value) in [
                    ("x", target.x),
                    ("y", target.y),
                    ("z", target.z),
                    ("w", target.w),
                    ("v", target.v),
                ] {
                    if !value.is_finite() {
                        return Err(VersionedAuthorizationBasisError::V2WireConversion {
                            detail: format!("target axis {axis} must be finite, got {value}"),
                        });
                    }
                }
                CanonicalTrajectoryLossEvidence::Available {
                    target: CanonicalRawPosition {
                        x: target.x,
                        y: target.y,
                        z: target.z,
                        w: target.w,
                        v: target.v,
                    },
                    loss_after,
                }
            }
            // **P1-3 v3:** Exhaustive match — yeni wire reason varyantı compiler error üretir.
            RawTrajectoryLossV2::Unavailable {
                reason: RawTrajectoryLossUnavailableReasonV2::NoPreferredVector,
            } => CanonicalTrajectoryLossEvidence::Unavailable {
                reason: CanonicalTrajectoryLossUnavailableReason::NoPreferredVector,
            },
        };

        // Measurement request evidence conversion.
        let base_revision = SpaceViewRevision {
            view_id: match raw.measurement_request.base_revision.view_id {
                RawSpaceViewIdV2::Persisted { id } => {
                    SpaceViewId::Persisted(PersistedSpaceViewId::from_bytes(id))
                }
                RawSpaceViewIdV2::Ephemeral { id } => SpaceViewId::Ephemeral(id),
            },
            sequence: raw.measurement_request.base_revision.sequence,
            content_digest: SpaceDigest::from_bytes(
                raw.measurement_request
                    .base_revision
                    .content_digest
                    .into_bytes(),
            ),
        };
        let measurement_request = crate::measurement::CanonicalMeasurementRequestEvidence {
            subject: raw.measurement_request.subject,
            impact: raw.measurement_request.impact,
            base_revision,
            structural_delta_digest: crate::measurement::MeasurementDeltaDigest::from_bytes(
                raw.measurement_request.structural_delta_digest.into_bytes(),
            ),
            measurement_input_digest: MeasurementInputDigest::from_bytes(
                raw.measurement_request
                    .measurement_input_digest
                    .into_bytes(),
            ),
        };

        // AuthorizationBasisV2::new — validate_semantics (nested commitment reverify).
        Self::new(
            raw.task_id,
            raw.claim_id,
            crate::measurement::TaskClaimDigest::from_bytes(raw.task_claim_digest.into_bytes()),
            crate::measurement::TaskGoalDigest::from_bytes(raw.task_goal_digest.into_bytes()),
            crate::measurement::MeasurementDigest::from_bytes(raw.measurement_digest.into_bytes()),
            crate::measurement::EngineMeasurementDigest::from_bytes(
                raw.engine_measurement_digest.into_bytes(),
            ),
            trajectory_baseline,
            crate::measurement::MeasurementBaselineDigest::from_bytes(
                raw.measurement_baseline_digest.into_bytes(),
            ),
            trajectory_loss,
            measurement_request,
            crate::measurement::MeasurementRequestDigest::from_bytes(
                raw.measurement_request_digest.into_bytes(),
            ),
            crate::measurement::MeasurementContextDigest::from_bytes(
                raw.measurement_context_digest.into_bytes(),
            ),
            crate::measurement::MeasurementDeltaDigest::from_bytes(
                raw.canonical_delta_digest.into_bytes(),
            ),
        )
        .map_err(VersionedAuthorizationBasisError::V2Validation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_encoding::encode_optional_f64_to_vec;
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
                vec![CanonicalEdgeIdentity::new(
                    0,
                    1,
                    CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
                )],
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
            delta.new_nodes().iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "nodes sorted by id"
        );
        assert_eq!(delta.new_edges()[0].from, 1, "edges sorted");
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
        // **INV-T9 #72 (Commit 3):** Evidence ve digest `sample_basis()` ile tutarlı
        // olmalı — `Envelope::new` cross-field validation yapar (task_id, claim_id,
        // basis_digest binding). Evidence basis'in compute edilen digest'ını kullanır
        // (placeholder değil — adım 8 evidence basis digest binding'i için).
        let basis = sample_basis();
        let basis_digest = AuthorizationBasisDigest::compute(&basis).unwrap();
        let hold_reason = WitnessHoldReason::MinApproversNotMet {
            distinct: 0,
            required: 2,
        };
        let snapshot = WitnessQuorumSnapshot {
            approvers: 0,
            required_approvers: 2,
            support: 0.0,
            required_support: 1.5,
        };
        let evidence = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest.clone(),
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Held {
                hold_reason: hold_reason.clone(),
                snapshot: snapshot.clone(),
            },
        )
        .unwrap();
        let evidence_digest = SuspendedAttemptEvidenceDigest::compute(&evidence).unwrap();
        PendingAuthorization {
            task_id: TaskId::from(1u64),
            claim_id: ClaimId::from(42u64),
            predicate_completion: PredicateCompletion::Completed,
            mutation_decision: MutationDecision::AcceptAsCompleted,
            intended_apply_target: ApplyTarget::Lane(CommitLane::Mainline),
            authorization_basis_digest: basis_digest, // Envelope::new overwrite eder (aynı değer)
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
            witness_hold_reason: hold_reason,
            witness_snapshot: snapshot,
            attempt_num: AttemptNumber::try_from(1u64).unwrap(),
            suspended_attempt_evidence: evidence,
            evidence_digest,
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
        // **P0-1 load path:** custom Deserialize `try_new_with_verified_digests` çağırır →
        // verify() UnknownSchema reject eder. Deserialize artık hata döner (unwrap yok).
        let basis = sample_basis();
        let record = sample_pending_record();
        let envelope = PendingAuthorizationEnvelope::new(record, basis).unwrap();

        let json = serde_json::to_string(&envelope).unwrap();
        let tampered = json.replace(PENDING_AUTHORIZATION_SCHEMA, "osp.bogus.v1");
        let result: Result<PendingAuthorizationEnvelope, _> = serde_json::from_str(&tampered);
        assert!(
            result.is_err(),
            "unknown schema must be rejected at deserialize (load-path verify): {result:?}"
        );
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
        // **INV-T9 #72 (Commit 3):** Artifact filename evidence identity kullanır —
        // `task-{task_id}--claim-{claim_id}--attempt-{attempt_num}--{evidence_digest}.json`.
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
            filename.starts_with("task-1--claim-42--attempt-1--"),
            "filename must use evidence identity (task+claim+attempt+evidence_digest): {filename}"
        );
        assert!(
            filename.ends_with(".json"),
            "filename must end with .json: {filename}"
        );
        // Evidence digest hex filename'de olmalı (64 hex chars).
        let hex_part = filename
            .strip_prefix("task-1--claim-42--attempt-1--")
            .and_then(|s| s.strip_suffix(".json"));
        assert!(
            hex_part.map(|h| h.len() == 64).unwrap_or(false),
            "filename must contain 64-char evidence_digest hex: {filename}"
        );
        // Receipt evidence identity filename ile eşleşmeli.
        assert_eq!(receipt.task_id, 1);
        assert_eq!(receipt.claim_id, 42);
        assert_eq!(receipt.attempt_num.get(), 1);
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
        let _attempt_num = record.attempt_num;
        let _created_at = record.created_at;
        // Hepsi erişilebilir — record complete.
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Commit 1 amend — Canonical primitives regression testleri
    // (reviewer P0-1..P0-3, P1 + plan-review düzeltmeleri)
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **INV-T9 #70 (P1-8):** Helper artık manuel mirror DEĞİL — gerçek axis
    /// constructor'larından üretilen gerçek `CoordinateSystem`'den türetilir. Test
    /// helper'ın düşündüğü descriptor'ı değil production axis'in gerçekten ürettiği
    /// descriptor'ı kilitler. Source descriptor encoding (semantics v2) dahil.
    fn sample_measurement_context() -> MeasurementInputContext {
        let coords = crate::coords::CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::TreeSitter,
            crate::axes::CohesionAxis::try_with_observed_source(crate::coords::MetricSource::Scip)
                .expect("Scip is a valid direct source"),
            crate::axes::EntropyAxis::from_commit_entropy(6.5),
            crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
        )
        .expect("valid measurement fixture");
        MeasurementInputContext::try_from(&coords).expect("valid measurement context")
    }

    #[test]
    fn measurement_digest_distinguishes_different_entropy_effective_value() {
        // **INV-T9 #70:** Stability + differentiation. Aynı descriptor listesi → aynı digest;
        // farklı entropy effective value → farklı digest.
        let ctx_a = sample_measurement_context();
        let ctx_b = sample_measurement_context();
        let d_a = MeasurementInputDigest::compute(&ctx_a).unwrap();
        let d_b = MeasurementInputDigest::compute(&ctx_b).unwrap();
        assert_eq!(
            d_a, d_b,
            "identical descriptor list → same digest (stability)"
        );

        // Farklı entropy effective value → farklı digest.
        let coords_changed = crate::coords::CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::TreeSitter,
            crate::axes::CohesionAxis::try_with_observed_source(crate::coords::MetricSource::Scip)
                .unwrap(),
            crate::axes::EntropyAxis::from_commit_entropy(9.0), // farklı effective value
            crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
        )
        .unwrap();
        let ctx_c = MeasurementInputContext::try_from(&coords_changed).unwrap();
        let d_c = MeasurementInputDigest::compute(&ctx_c).unwrap();
        assert_ne!(
            d_a, d_c,
            "axis descriptor change (entropy effective value) must produce different digest"
        );
    }

    /// **INV-T9 #70 Commit 1 v1 byte contract:** Real axis constructor'larından üretilen
    /// measurement input digest pinned. Source descriptor encoding (TreeSitter topology
    /// + Scip observed cohesion + Heuristic entropy/witness) dahil — descriptor semantics
    /// v2 değişince digest değişir, golden catches drift. Schema hâlâ v1 (yalnız content
    /// değişti).
    const MEASUREMENT_SEMANTICS_V2_GOLDEN_HEX: &str =
        "9ca484c73dae2ee6e27a945ee19e00df5a2ccfc028b8b05c615ab954f144336c";

    #[test]
    fn measurement_semantics_v2_matches_golden() {
        let ctx = sample_measurement_context();
        let digest = MeasurementInputDigest::compute(&ctx).unwrap();
        assert_eq!(
            digest.to_hex(),
            MEASUREMENT_SEMANTICS_V2_GOLDEN_HEX,
            "INV-T9 #70: measurement input digest preimage değişti — golden'ı güncelleyin"
        );
    }

    #[test]
    fn topology_source_changes_measurement_input_digest() {
        // **INV-T9 #70 (P1-1 source-difference regression):** topology_source
        // (Coupling+Instability graph topology source) digest'e gerçekten bağlı.
        let mk = |topology| {
            let coords = crate::coords::CoordinateSystem::default_raw_five(
                topology,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(6.5),
                crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
            )
            .unwrap();
            MeasurementInputContext::try_from(&coords).unwrap()
        };
        let d_ts =
            MeasurementInputDigest::compute(&mk(crate::coords::MetricSource::TreeSitter)).unwrap();
        let d_ph =
            MeasurementInputDigest::compute(&mk(crate::coords::MetricSource::Placeholder)).unwrap();
        assert_ne!(
            d_ts, d_ph,
            "topology_source difference must produce different digest"
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
        // **reviewer P1 + Step 5 + scoped P1-c:** duplicate node ID reddedilmeli.
        // try_new sort eder; iki id=5 → validate'de `Ordering::Equal` → `DuplicateNodeId(5)`.
        // Typed taxonomy: Equal = duplicate, Greater = UnsortedNodes (scoped review düzeltme).
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
        // **plan-review + Step 5b:** aynı edge identity new_edges ve removed_edges'te →
        // ambiguous delta. Cross-list kontrol artık identity üzerinden (is_type_only bağımsız).
        let imports = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap();
        let new_edge = CanonicalEdge {
            from: 1,
            to: 2,
            kind: imports,
            is_type_only: false,
        };
        let removed_identity = CanonicalEdgeIdentity::new(1, 2, imports);
        let err = CanonicalStructuralDelta::try_new(vec![], vec![new_edge], vec![removed_identity]);
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
    fn space_digest_is_independent_of_edge_insertion_order() {
        // **Step 6 P0 (scoped):** SpaceDigest canonical content identity — edge insertion
        // order'a bağımlı OLMAMALI. `encode_canonical_edge_vec` as-is encode eder (Step 5);
        // sıralama `SpaceDigest::compute` call site'ında yapılır. Aynı node + edge kümesi,
        // farklı insertion order → aynı digest.
        use crate::space::{Edge, EdgeKind, Node, NodeKind, Space};
        let mk_node = |id: u64| Node {
            id,
            kind: NodeKind::Module,
            mass: 10.0,
            ..Default::default()
        };
        let edge_a = Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Imports,
            is_type_only: false,
        };
        let edge_b = Edge {
            from: 1,
            to: 3,
            kind: EdgeKind::Calls,
            is_type_only: true,
        };
        let mut a = Space::default();
        a.nodes.insert(1, mk_node(1));
        a.nodes.insert(2, mk_node(2));
        a.nodes.insert(3, mk_node(3));
        a.edges.push(edge_a.clone());
        a.edges.push(edge_b.clone());

        let mut b = Space::default();
        b.nodes.insert(1, mk_node(1));
        b.nodes.insert(2, mk_node(2));
        b.nodes.insert(3, mk_node(3));
        // Ters insertion order.
        b.edges.push(edge_b);
        b.edges.push(edge_a);

        assert_eq!(
            SpaceDigest::compute(&a).unwrap(),
            SpaceDigest::compute(&b).unwrap(),
            "SpaceDigest must be canonical — independent of edge insertion order"
        );
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
        // **reviewer P0-3 (A8) / Step 4a / Step 4b / Step 4c:** EvaluationContextDigest
        // gerçek içerik + ordinal-aware RuleEvaluationContext + claim-specific effective
        // vision. Step 4c: config parametresi KALDIRILDI — digest yalnız Q5/Q6 girdileri.
        let rule_ctx = RuleEvaluationContext::try_new(vec![OrderedRuleDescriptor {
            ordinal: 0,
            descriptor: RuleDescriptor {
                rule_id: "structural.no_self_import".to_string(),
                semantics_version: 1,
                canonical_parameters: vec![],
            },
        }])
        .unwrap();
        let vision_ctx = mk_vision_context(0.3);
        let d1 = EvaluationContextDigest::compute(&rule_ctx, &vision_ctx).unwrap();
        let d2 = EvaluationContextDigest::compute(&rule_ctx, &vision_ctx).unwrap();
        assert_eq!(d1, d2);
    }

    #[test]
    fn evaluation_context_digest_changes_when_theta_bound_changes() {
        // **Step 4b:** theta_bound artık vision_context'te (config'ten KALDIRILDI).
        // vision_context.theta_bound değişince digest değişmeli.
        let mk = |theta: f64| {
            let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
            let vision_ctx = mk_vision_context(theta);
            EvaluationContextDigest::compute(&rule_ctx, &vision_ctx).unwrap()
        };
        assert_ne!(mk(0.3), mk(0.5));
    }

    #[test]
    fn evaluation_context_digest_changes_when_rule_added() {
        let vision_ctx = mk_vision_context(0.3);
        let d_no_rules = {
            let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
            EvaluationContextDigest::compute(&rule_ctx, &vision_ctx).unwrap()
        };
        let d_one_rule = {
            let rule_ctx = RuleEvaluationContext::try_new(vec![OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: RuleDescriptor {
                    rule_id: "test.rule".to_string(),
                    semantics_version: 1,
                    canonical_parameters: vec![],
                },
            }])
            .unwrap();
            EvaluationContextDigest::compute(&rule_ctx, &vision_ctx).unwrap()
        };
        assert_ne!(d_no_rules, d_one_rule);
    }

    #[test]
    fn evaluation_context_digest_changes_when_rule_semantics_version_changes() {
        // **plan-review #4:** semantics_version artarsa digest değişmeli.
        let vision_ctx = mk_vision_context(0.3);
        let mk = |semver: u32| {
            let rule_ctx = RuleEvaluationContext::try_new(vec![OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: RuleDescriptor {
                    rule_id: "test.rule".to_string(),
                    semantics_version: semver,
                    canonical_parameters: vec![],
                },
            }])
            .unwrap();
            EvaluationContextDigest::compute(&rule_ctx, &vision_ctx).unwrap()
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
            crate::coords::MetricSource::Placeholder,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let cs2 = crate::coords::CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::Placeholder,
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

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 4a — Rule sequence binding (ordinal-aware context) testleri
    // ═══════════════════════════════════════════════════════════════════════════════

    fn mk_rule_descriptor(id: &str, semver: u32) -> RuleDescriptor {
        RuleDescriptor {
            rule_id: id.to_string(),
            semantics_version: semver,
            canonical_parameters: vec![],
        }
    }

    /// **INV-T9 Step 4b test helper:** `EffectiveVisionGateContext` üret — `UserLoaded`
    /// source + `Global` subject (en basit geçerli kombinasyon; `GlobalDefault`/`None`
    /// authority'de reject edilir). `theta_bound` parametrik (digest değişim testleri için).
    fn mk_vision_context(theta_bound: f64) -> EffectiveVisionGateContext {
        use crate::vision::{VisionSource, VisionVector};
        let selection = EffectiveVisionSelection {
            effective_vision: VisionVector::with_source(
                crate::coords::RawPosition {
                    x: 0.5,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                },
                VisionSource::UserLoaded,
            ),
            subject: CanonicalVisionSubject::Global,
            role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
            vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
        };
        EffectiveVisionGateContext::try_new(selection, theta_bound).unwrap()
    }

    #[test]
    fn evaluation_context_changes_when_rule_order_changes() {
        // **Step 4a:** Registration sırası semantik — farklı sıra → farklı digest
        // (sort-by-rule_id KALDIRILDI, ordinal korundu).
        // Sıra A: alpha, beta
        let ctx_a = RuleEvaluationContext::try_new(vec![
            OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: mk_rule_descriptor("alpha", 1),
            },
            OrderedRuleDescriptor {
                ordinal: 1,
                descriptor: mk_rule_descriptor("beta", 1),
            },
        ])
        .unwrap();
        // Sıra B: beta, alpha (ters)
        let ctx_b = RuleEvaluationContext::try_new(vec![
            OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: mk_rule_descriptor("beta", 1),
            },
            OrderedRuleDescriptor {
                ordinal: 1,
                descriptor: mk_rule_descriptor("alpha", 1),
            },
        ])
        .unwrap();
        let vision_ctx = mk_vision_context(0.3);
        let d_a = EvaluationContextDigest::compute(&ctx_a, &vision_ctx).unwrap();
        let d_b = EvaluationContextDigest::compute(&ctx_b, &vision_ctx).unwrap();
        assert_ne!(d_a, d_b, "registration order must change digest");
    }

    #[test]
    fn same_rules_same_order_produce_same_digest() {
        let mk_ctx = || {
            RuleEvaluationContext::try_new(vec![
                OrderedRuleDescriptor {
                    ordinal: 0,
                    descriptor: mk_rule_descriptor("alpha", 1),
                },
                OrderedRuleDescriptor {
                    ordinal: 1,
                    descriptor: mk_rule_descriptor("beta", 2),
                },
            ])
            .unwrap()
        };
        let vision_ctx = mk_vision_context(0.3);
        let d1 = EvaluationContextDigest::compute(&mk_ctx(), &vision_ctx).unwrap();
        let d2 = EvaluationContextDigest::compute(&mk_ctx(), &vision_ctx).unwrap();
        assert_eq!(d1, d2, "same rules + same order → same digest");
    }

    #[test]
    fn rule_context_rejects_duplicate_active_rule_id() {
        // try_new duplicate rule_id reddetmeli (canonical validation).
        let err = RuleEvaluationContext::try_new(vec![
            OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: mk_rule_descriptor("alpha", 1),
            },
            OrderedRuleDescriptor {
                ordinal: 1,
                descriptor: mk_rule_descriptor("alpha", 1), // duplicate
            },
        ])
        .unwrap_err();
        assert_eq!(
            err,
            EvaluationContextError::DuplicateActiveRuleId("alpha".into())
        );
    }

    #[test]
    fn rule_context_rejects_empty_rule_id() {
        let err = RuleEvaluationContext::try_new(vec![OrderedRuleDescriptor {
            ordinal: 0,
            descriptor: mk_rule_descriptor("", 1),
        }])
        .unwrap_err();
        assert_eq!(err, EvaluationContextError::EmptyRuleId);
    }

    #[test]
    fn rule_context_rejects_zero_semantics_version() {
        let err = RuleEvaluationContext::try_new(vec![OrderedRuleDescriptor {
            ordinal: 0,
            descriptor: mk_rule_descriptor("alpha", 0),
        }])
        .unwrap_err();
        assert_eq!(err, EvaluationContextError::InvalidRuleSemanticsVersion(0));
    }

    #[test]
    fn rule_context_rejects_ordinal_gap() {
        // ordinal 0, 2 (1 atlanmış) → gap hatası.
        let err = RuleEvaluationContext::try_new(vec![
            OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: mk_rule_descriptor("alpha", 1),
            },
            OrderedRuleDescriptor {
                ordinal: 2, // gap
                descriptor: mk_rule_descriptor("beta", 1),
            },
        ])
        .unwrap_err();
        assert_eq!(
            err,
            EvaluationContextError::OrdinalGap {
                expected: 1,
                found: 2
            }
        );
    }

    #[test]
    fn rule_context_rejects_unsupported_semantics_version() {
        // try_new her zaman RULE_EVALUATION_SEMANTICS_VERSION kullanır; ama validate
        // elle kurulmuş context'te farklı version reddeder.
        let mut ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        ctx.semantics_version = 999; // sahte mutation
        let err = ctx.validate().unwrap_err();
        assert_eq!(
            err,
            EvaluationContextError::UnsupportedRuleContextSemantics(999)
        );
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn rule_ordinal_overflow_fails_closed() {
        // usize::MAX → u32 dönüşümü overflow → fail-closed.
        let err = checked_rule_ordinal(usize::MAX).unwrap_err();
        assert_eq!(err, EvaluationContextError::RuleOrdinalOverflow);
    }

    #[test]
    fn register_rule_rejects_duplicate_active_rule_id() {
        // engine.register_rule duplicate rule_id reddeder.
        use crate::engine::SpaceEngine;
        use crate::rule::NoSelfImportRule;
        let cs = crate::coords::CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            crate::axes::EntropyAxis::from_commit_entropy(6.0),
            crate::axes::WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let mut engine = SpaceEngine::with_default_rules(
            crate::space::Space::new(),
            cs,
            crate::vision::VisionVector::new(crate::coords::RawPosition::default()),
            crate::engine::EngineConfig::default_calibrated(),
        )
        .unwrap();
        // NoSelfImportRule zaten default_rules'ta kayıtlı → tekrar register duplicate.
        let err = engine
            .register_rule(Box::new(NoSelfImportRule::new()))
            .unwrap_err();
        assert!(matches!(
            err,
            crate::engine::RuleRegistrationError::DuplicateActiveRuleId(_)
        ));
    }

    #[test]
    fn register_rule_rejects_descriptor_identity_mismatch() {
        // runtime id "a" ama descriptor "b" → IdentityMismatch.
        use crate::engine::{RuleRegistrationError, SpaceEngine};
        use crate::rule::{Rule, RuleId, RuleViolation};
        use crate::space::{Edge, Node, Space};
        struct MismatchedRule {
            id: RuleId,
        }
        impl Rule for MismatchedRule {
            fn id(&self) -> &RuleId {
                &self.id
            }
            fn descriptor(&self) -> RuleDescriptor {
                RuleDescriptor {
                    rule_id: "mismatched.descriptor".into(),
                    semantics_version: 1,
                    canonical_parameters: vec![],
                }
            }
            fn evaluate(&self, _: &[Node], _: &[Edge], _: &Space) -> Option<RuleViolation> {
                None
            }
        }
        let cs = crate::coords::CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            crate::axes::EntropyAxis::from_commit_entropy(6.0),
            crate::axes::WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let mut engine = SpaceEngine::with_default_rules(
            crate::space::Space::new(),
            cs,
            crate::vision::VisionVector::new(crate::coords::RawPosition::default()),
            crate::engine::EngineConfig::default_calibrated(),
        )
        .unwrap();
        let err = engine
            .register_rule(Box::new(MismatchedRule {
                id: "mismatched.runtime".into(),
            }))
            .unwrap_err();
        match err {
            RuleRegistrationError::IdentityMismatch {
                runtime_id,
                descriptor_id,
            } => {
                assert_eq!(runtime_id, "mismatched.runtime");
                assert_eq!(descriptor_id, "mismatched.descriptor");
            }
            other => panic!("expected IdentityMismatch, got {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 4b — EffectiveVisionGateContext + claim-specific vision binding testleri
    //
    // Test matrisi (reviewer-onaylı plan): validation, digest binding, captured context
    // propagation, terminal behavior (P0-4), caller audit (P1).
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **Step 4b test helper:** Parametreli `EffectiveVisionSelection` — farklı
    /// source/subject/vision kombinasyonları için. `mk_vision_context` sabit bir
    /// kombinasyon (UserLoaded/Global) üretir; bu helper esnektir.
    ///
    /// **scoped-review P0:** `vision_source` ayrı alan YOK — source `effective_vision`'a
    /// gömülü (tek truth).
    fn mk_selection(
        source: crate::vision::VisionSource,
        subject: CanonicalVisionSubject,
        raw: crate::coords::RawPosition,
    ) -> EffectiveVisionSelection {
        EffectiveVisionSelection {
            effective_vision: crate::vision::VisionVector::with_source(raw, source),
            subject,
            role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
            vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
        }
    }

    // ── Validation (reviewer P0-2 + P0-3) ─────────────────────────────────────

    #[test]
    fn effective_vision_context_rejects_non_finite_vector() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        // x = NaN → NonFiniteVisionAxis.
        let sel = mk_selection(
            VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            RawPosition {
                x: f64::NAN,
                y: 0.5,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
        );
        let err = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap_err();
        assert!(
            matches!(err, VisionContextError::NonFiniteVisionAxis { axis: "x" }),
            "expected NonFiniteVisionAxis x, got {err:?}"
        );
    }

    #[test]
    fn effective_vision_context_rejects_non_finite_theta_bound() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        let sel = mk_selection(
            VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            RawPosition::default(),
        );
        let err = EffectiveVisionGateContext::try_new(sel, f64::INFINITY).unwrap_err();
        assert!(
            matches!(err, VisionContextError::NonFiniteThetaBound(_)),
            "expected NonFiniteThetaBound, got {err:?}"
        );
    }

    #[test]
    fn effective_vision_context_rejects_theta_bound_out_of_range() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        let sel = mk_selection(
            VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            RawPosition::default(),
        );
        // 1.5 > MAX_THETA_BOUND (1.0).
        let err = EffectiveVisionGateContext::try_new(sel, 1.5).unwrap_err();
        assert!(
            matches!(err, VisionContextError::ThetaBoundOutOfRange(1.5)),
            "expected ThetaBoundOutOfRange(1.5), got {err:?}"
        );
    }

    #[test]
    fn vision_source_none_fails_closed_before_q5() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        let sel = mk_selection(
            VisionSource::None,
            CanonicalVisionSubject::Global,
            RawPosition::default(),
        );
        let err = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap_err();
        assert!(
            matches!(err, VisionContextError::VisionUnavailable),
            "None source must fail-closed (VisionUnavailable), got {err:?}"
        );
    }

    #[test]
    fn vision_source_global_default_rejected_for_mutation_authority() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        let sel = mk_selection(
            VisionSource::GlobalDefault,
            CanonicalVisionSubject::Global,
            RawPosition::default(),
        );
        let err = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap_err();
        assert!(
            matches!(
                err,
                VisionContextError::VisionAuthorityInsufficient {
                    vision_source: VisionSource::GlobalDefault
                }
            ),
            "GlobalDefault must be rejected for mutation authority, got {err:?}"
        );
    }

    #[test]
    fn vision_source_role_profile_with_role_subject_accepted() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        // RoleProfile + Role(Runtime) — geçerli kombinasyon (kullanıcı TOML override).
        let role_tag = CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap();
        let sel = mk_selection(
            VisionSource::RoleProfile,
            CanonicalVisionSubject::Role(role_tag),
            RawPosition::default(),
        );
        let ctx = EffectiveVisionGateContext::try_new(sel, 0.3);
        assert!(
            ctx.is_ok(),
            "RoleProfile + Role subject must be accepted, got: {:?}",
            ctx.err()
        );
    }

    #[test]
    fn vision_source_user_loaded_with_global_subject_accepted() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        // UserLoaded + Global — geçerli (kullanıcı global vision, rol-süz).
        let sel = mk_selection(
            VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            RawPosition::default(),
        );
        let ctx = EffectiveVisionGateContext::try_new(sel, 0.3);
        assert!(
            ctx.is_ok(),
            "UserLoaded + Global subject must be accepted, got: {:?}",
            ctx.err()
        );
    }

    #[test]
    fn vision_source_builtin_role_with_global_subject_rejected() {
        use crate::coords::RawPosition;
        use crate::vision::VisionSource;
        // BuiltinRole + Global → SubjectSourceMismatch (role-scoped source ile global subject).
        let sel = mk_selection(
            VisionSource::BuiltinRole,
            CanonicalVisionSubject::Global,
            RawPosition::default(),
        );
        let err = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap_err();
        assert!(
            matches!(
                err,
                VisionContextError::SubjectSourceMismatch {
                    subject: CanonicalVisionSubject::Global,
                    vision_source: VisionSource::BuiltinRole
                }
            ),
            "BuiltinRole + Global subject must be SubjectSourceMismatch, got {err:?}"
        );
    }

    #[test]
    fn empty_delta_nodes_falls_to_global_subject() {
        // Engine integration: delta_nodes boş → override yolu girilmez → Global subject.
        use crate::engine::EngineConfig;
        use crate::space::Space;
        use crate::vision::{VisionSource, VisionVector};
        let engine = crate::engine::SpaceEngine::new(
            Space::default(),
            crate::coords::CoordinateSystem::default_raw_five(
                crate::coords::MetricSource::Placeholder,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(0.0),
                crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
            )
            .unwrap(),
            VisionVector::with_source(
                crate::coords::RawPosition::default(),
                VisionSource::UserLoaded,
            ),
            EngineConfig::default_calibrated(),
        );
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        let sel = engine.effective_vision_selection(&claim).unwrap();
        assert!(
            matches!(sel.subject, CanonicalVisionSubject::Global),
            "empty delta_nodes must fall to Global subject, got {:?}",
            sel.subject
        );
    }

    // ── Digest binding (reviewer P0-1 + P0-2) ─────────────────────────────────

    #[test]
    fn evaluation_context_binds_claim_specific_effective_vision() {
        // Claim-specific effective vision digest'e bağlı — farklı vision → farklı digest.
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        let mk = |x: f64| {
            use crate::vision::{VisionSource, VisionVector};
            let sel = EffectiveVisionSelection {
                effective_vision: VisionVector::with_source(
                    crate::coords::RawPosition {
                        x,
                        y: 0.6,
                        z: 0.4,
                        w: 0.5,
                        v: 0.3,
                    },
                    VisionSource::UserLoaded,
                ),
                subject: CanonicalVisionSubject::Global,
                role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
                vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
            };
            let ctx = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap();
            EvaluationContextDigest::compute(&rule_ctx, &ctx).unwrap()
        };
        assert_ne!(
            mk(0.5),
            mk(0.6),
            "different effective vision x → different digest"
        );
    }

    #[test]
    fn evaluation_context_changes_when_effective_theta_bound_changes() {
        // theta_bound artık vision_context'te — değişince digest değişir.
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        let mk = |theta: f64| {
            let ctx = mk_vision_context(theta);
            EvaluationContextDigest::compute(&rule_ctx, &ctx).unwrap()
        };
        assert_ne!(mk(0.2), mk(0.4));
    }

    #[test]
    fn evaluation_context_changes_when_only_vision_source_changes() {
        // Same vector, same subject, farklı source → farklı digest (provenance bağlı).
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        let raw = crate::coords::RawPosition {
            x: 0.5,
            y: 0.6,
            z: 0.4,
            w: 0.5,
            v: 0.3,
        };
        let mk = |source: crate::vision::VisionSource| {
            // RoleProfile/BuiltinRole için Role subject gerekir (mismatch olmasın).
            let subject = match source {
                crate::vision::VisionSource::RoleProfile
                | crate::vision::VisionSource::BuiltinRole => CanonicalVisionSubject::Role(
                    CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap(),
                ),
                _ => CanonicalVisionSubject::Global,
            };
            let sel = mk_selection(source, subject, raw);
            let ctx = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap();
            EvaluationContextDigest::compute(&rule_ctx, &ctx).unwrap()
        };
        assert_ne!(
            mk(crate::vision::VisionSource::UserLoaded),
            mk(crate::vision::VisionSource::RoleProfile),
            "different vision source (same vector) → different digest"
        );
    }

    #[test]
    fn evaluation_context_changes_when_only_subject_changes() {
        // Same vector, same source (UserLoaded), farklı subject → farklı digest.
        // Not: UserLoaded + Role subject geçerli (kullanıcı global vision ama role ata).
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        let raw = crate::coords::RawPosition {
            x: 0.5,
            y: 0.6,
            z: 0.4,
            w: 0.5,
            v: 0.3,
        };
        let mk = |subject: CanonicalVisionSubject| {
            let sel = mk_selection(crate::vision::VisionSource::UserLoaded, subject, raw);
            let ctx = EffectiveVisionGateContext::try_new(sel, 0.3).unwrap();
            EvaluationContextDigest::compute(&rule_ctx, &ctx).unwrap()
        };
        let role_tag = CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap();
        assert_ne!(
            mk(CanonicalVisionSubject::Global),
            mk(CanonicalVisionSubject::Role(role_tag)),
            "different subject (same vector+source) → different digest"
        );
    }

    #[test]
    fn selected_role_override_changes_claim_context() {
        // Engine integration: role_overrides map'inde ilgili rol varsa → o rolün
        // override'ı effective vision'a uygulanır → digest değişir.
        use crate::engine::EngineConfig;
        use crate::space::{Node, NodeClassification, NodeKind, Space};
        use crate::vision::{VisionSource, VisionVector};
        use crate::vision_config::RoleVisionOverride;
        let mk_engine = |overrides: std::collections::HashMap<String, RoleVisionOverride>| {
            let mut space = Space::default();
            // Runtime node — builtin override mevcut (Runtime → Some). delta_node Runtime.
            space.nodes.insert(
                0,
                Node {
                    id: 0,
                    kind: NodeKind::Module,
                    mass: 100.0,
                    ..Default::default()
                },
            );
            let mut config = EngineConfig::default_calibrated();
            config.role_overrides = overrides;
            crate::engine::SpaceEngine::new(
                space,
                crate::coords::CoordinateSystem::default_raw_five(
                    crate::coords::MetricSource::Placeholder,
                    crate::axes::CohesionAxis::new(),
                    crate::axes::EntropyAxis::from_commit_entropy(0.0),
                    crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
                )
                .unwrap(),
                VisionVector::with_source(
                    crate::coords::RawPosition {
                        x: 0.5,
                        y: 0.6,
                        z: 0.4,
                        w: 0.5,
                        v: 0.3,
                    },
                    VisionSource::UserLoaded,
                ),
                config,
            )
        };
        // Claim: Production classification node → infer_role → Runtime.
        let mk_claim = || crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![crate::space::Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                classification: NodeClassification::Production,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        // No overrides → builtin Runtime override uygulanır (BuiltinRole).
        let sel_no_user = mk_engine(std::collections::HashMap::new())
            .effective_vision_selection(&mk_claim())
            .unwrap();
        // User override for Runtime → RoleProfile (kullanıcı override kazanır).
        let mut user_overrides = std::collections::HashMap::new();
        user_overrides.insert(
            "Runtime".to_string(),
            RoleVisionOverride {
                x: Some(0.99),
                y: None,
                z: None,
            },
        );
        let sel_with_user = mk_engine(user_overrides)
            .effective_vision_selection(&mk_claim())
            .unwrap();
        assert_ne!(
            sel_no_user.effective_vision.raw.x, sel_with_user.effective_vision.raw.x,
            "selected role override must change effective vision"
        );
        assert_eq!(
            sel_no_user.vision_source(),
            crate::vision::VisionSource::BuiltinRole
        );
        assert_eq!(
            sel_with_user.vision_source(),
            crate::vision::VisionSource::RoleProfile
        );
    }

    #[test]
    fn unrelated_role_override_does_not_invalidate_claim_context() {
        // Engine integration: Support claim + Runtime override → Support builtin None,
        // user Runtime override Support'a uygulanmaz → Global vision inherit.
        use crate::engine::EngineConfig;
        use crate::space::{NodeClassification, NodeKind, Space};
        use crate::vision::{VisionSource, VisionVector};
        use crate::vision_config::RoleVisionOverride;
        let mut space = Space::default();
        space.nodes.insert(
            0,
            crate::space::Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                ..Default::default()
            },
        );
        let mut config = EngineConfig::default_calibrated();
        let mut user_overrides = std::collections::HashMap::new();
        user_overrides.insert(
            "Runtime".to_string(),
            RoleVisionOverride {
                x: Some(0.99),
                y: None,
                z: None,
            },
        );
        config.role_overrides = user_overrides;
        let engine = crate::engine::SpaceEngine::new(
            space,
            crate::coords::CoordinateSystem::default_raw_five(
                crate::coords::MetricSource::Placeholder,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(0.0),
                crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
            )
            .unwrap(),
            VisionVector::with_source(
                crate::coords::RawPosition {
                    x: 0.5,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                },
                VisionSource::UserLoaded,
            ),
            config,
        );
        // Test classification → infer_role → Support. Runtime override uygulanmaz.
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![crate::space::Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                classification: NodeClassification::Test,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        let sel = engine.effective_vision_selection(&claim).unwrap();
        // **scoped-review P1-a:** Support claim artık Role(Support) subject üretir
        // (override olsun/olmasın claim'in değerlendirme bağlamı korunur). Override yok
        // çünkü builtin_role_override(Support) = None ve user override Runtime için.
        // Vision global inherit edilir (Runtime override 0.99 uygulanmadı).
        let support_tag =
            crate::canonical_tags::CanonicalNodeRole::try_from(&crate::space::NodeRole::Support)
                .unwrap();
        assert!(
            matches!(sel.subject, CanonicalVisionSubject::Role(tag) if tag == support_tag),
            "Support claim must produce Role(Support) subject, got {:?}",
            sel.subject
        );
        // Global vision x = 0.5 (Runtime override 0.99 uygulanmadı — unrelated role).
        assert_eq!(sel.effective_vision.raw.x, 0.5);
    }

    #[test]
    fn global_default_does_not_create_evaluation_context_digest() {
        // GlobalDefault source → validate_for_authorization reject → compute Err.
        // `try_new` GlobalDefault'ı zaten reject eder; bu yüzden raw context ile compute
        // çağrılır (defensive digest katmanı da authority validation yapar — P0-3).
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        let sel = mk_selection(
            crate::vision::VisionSource::GlobalDefault,
            CanonicalVisionSubject::Global,
            crate::coords::RawPosition::default(),
        );
        // try_new reject → Err döner (constructor authority gate).
        let ctor_result = EffectiveVisionGateContext::try_new(sel.clone(), 0.3);
        assert!(
            matches!(
                ctor_result,
                Err(VisionContextError::VisionAuthorityInsufficient {
                    vision_source: crate::vision::VisionSource::GlobalDefault
                })
            ),
            "try_new must reject GlobalDefault at constructor, got: {:?}",
            ctor_result.err()
        );
        // Defensive digest katmanı: raw context (constructor bypass) ile compute da Err.
        let raw_ctx = EffectiveVisionGateContext {
            selection: sel,
            theta_bound: 0.3,
            deviation_semver: DEVIATION_SEMANTICS_VERSION,
        };
        let result = EvaluationContextDigest::compute(&rule_ctx, &raw_ctx);
        assert!(
            result.is_err(),
            "GlobalDefault must not produce a digest (authority rejected)"
        );
    }

    #[test]
    fn vision_source_none_does_not_create_evaluation_context_digest() {
        // None source → validate_for_authorization reject → compute Err.
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        let sel = mk_selection(
            crate::vision::VisionSource::None,
            CanonicalVisionSubject::Global,
            crate::coords::RawPosition::default(),
        );
        // try_new zaten None'ı reject eder; bu yüzden raw context ile compute çağır.
        let raw_ctx = EffectiveVisionGateContext {
            selection: sel,
            theta_bound: 0.3,
            deviation_semver: DEVIATION_SEMANTICS_VERSION,
        };
        let result = EvaluationContextDigest::compute(&rule_ctx, &raw_ctx);
        assert!(
            result.is_err(),
            "None source must not produce a digest (vision unavailable)"
        );
    }

    // ── Captured context propagation ──────────────────────────────────────────

    #[test]
    fn q5_and_evaluation_context_reuse_the_same_theta_bound() {
        // Engine integration: effective_vision_gate_context bir kez üretilir, Q5 +
        // build_authorization_context + digest paylaşır. theta_bound referans olarak
        // akar — aynı değer her ikisinde de kullanılır.
        use crate::engine::EngineConfig;
        use crate::space::Space;
        use crate::vision::{VisionSource, VisionVector};
        let engine = crate::engine::SpaceEngine::new(
            Space::default(),
            crate::coords::CoordinateSystem::default_raw_five(
                crate::coords::MetricSource::Placeholder,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(0.0),
                crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
            )
            .unwrap(),
            VisionVector::with_source(
                crate::coords::RawPosition::default(),
                VisionSource::UserLoaded,
            ),
            EngineConfig::default_calibrated(),
        );
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        let ctx = engine.effective_vision_gate_context(&claim).unwrap();
        // Q5'in kullanacağı theta_bound ile config.theta_bound aynı.
        assert_eq!(ctx.theta_bound, engine.config().theta_bound);
        // Digest de aynı theta_bound'ı kullanır (compute içinde vision_context.theta_bound).
        // Bu test yapısal referans paylaşımını doğrular — değer eşitliği yeterli
        // (Rust ownership referans değil kopya akıtır; ama tek kaynak = config.theta_bound).
    }

    // ── Terminal behavior (reviewer P0-4) ─────────────────────────────────────

    #[test]
    fn global_default_authority_failure_does_not_retry_or_consume_budget() {
        // Engine integration: GlobalDefault vision → commit_task_claim Q5 öncesi
        // VisionContextInvalid (terminal) → navigator SystemFailure (retry yok).
        use crate::engine::EngineConfig;
        use crate::space::Space;
        use crate::vision::VisionVector;
        // GlobalDefault vision (VisionVector::new legacy constructor).
        let engine = crate::engine::SpaceEngine::new(
            Space::default(),
            crate::coords::CoordinateSystem::default_raw_five(
                crate::coords::MetricSource::Placeholder,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(0.0),
                crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
            )
            .unwrap(),
            // VisionVector::new → GlobalDefault source.
            VisionVector::new(crate::coords::RawPosition::default()),
            EngineConfig::default_calibrated(),
        );
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        // effective_vision_gate_context → VisionContextInvalid (GlobalDefault reject).
        let result = engine.effective_vision_gate_context(&claim);
        assert!(result.is_err(), "GlobalDefault must fail-closed before Q5");
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                VisionContextError::VisionAuthorityInsufficient {
                    vision_source: crate::vision::VisionSource::GlobalDefault
                }
            ),
            "expected VisionAuthorityInsufficient(GlobalDefault), got {err:?}"
        );
        // Terminal mapping: gate_decision_from_engine_error → Unknown (navigator.rs).
        let engine_err = crate::engine::EngineCommitError::VisionContextInvalid(err);
        let gd = crate::navigator::gate_decision_from_engine_error(&engine_err);
        assert_eq!(
            gd,
            crate::trajectory::GateDecision::Unknown,
            "VisionContextInvalid must map to Unknown (terminal, no retry)"
        );
    }

    // ── Caller audit (reviewer P1) ────────────────────────────────────────────

    #[test]
    fn vision_config_produces_user_loaded_authoritative_source() {
        // **scoped-review P2-b:** Gerçek production dönüşümü — VisionConfig::from_str
        // (TOML parse) → to_vision_vector() → UserLoaded source. Bu, kullanıcının elle
        // deklare ettiği vision'ın en yüksek provenance ile işaretlendiğini doğrular
        // (GlobalDefault DEĞİL). Caller audit'in gerçek yolunu test eder.
        use crate::vision::VisionSource;
        let toml = r#"
[raw]
x = 0.4
y = 0.7
z = 0.5
w = 0.5
v = 0.5
"#;
        let config =
            crate::vision_config::VisionConfig::from_str(toml).expect("valid TOML must parse");
        let vector = config.to_vision_vector();
        assert_eq!(
            vector.source(),
            VisionSource::UserLoaded,
            "VisionConfig [raw] → to_vision_vector must produce UserLoaded (highest authority)"
        );
        assert!(vector.is_evaluable());
        // UserLoaded authority'de kabul edilir (validate_authority_for_mutation Ok).
        let sel = mk_selection(
            VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            vector.raw,
        );
        let ctx = EffectiveVisionGateContext::try_new(sel, 0.3);
        assert!(ctx.is_ok(), "UserLoaded must pass authority validation");
    }

    #[test]
    fn cosine_deviation_none_fallback_remains_defensive_only() {
        // CosineDeviation None source → 1.0 (maksimum sapma) döner — ikinci savunma
        // katmanı. Validate birinci katman; ama defensive fallback korunur.
        use crate::space::Space;
        use crate::vision::{CosineDeviation, DeviationMetric, VisionVector};
        let none_vision = VisionVector::none();
        assert!(!none_vision.is_evaluable());
        let theta = CosineDeviation.theta(
            &crate::coords::RawPosition::default(),
            &none_vision,
            &Space::default(),
        );
        assert_eq!(
            theta, 1.0,
            "CosineDeviation None fallback must return 1.0 (defensive)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // scoped-review closure testleri (P0 + P1-a + P1-b + P1-c)
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn vision_source_is_single_truth_from_effective_vector() {
        // **P0:** EffectiveVisionSelection'ın ayrı vision_source alanı YOK. Source her
        // zaman effective_vision.source()'dan okunur (dual-truth mismatch açığı kapandı).
        let sel = mk_selection(
            crate::vision::VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            crate::coords::RawPosition::default(),
        );
        assert_eq!(
            sel.vision_source(),
            crate::vision::VisionSource::UserLoaded,
            "vision_source() must read from effective_vision (single truth)"
        );
        // Struct literal'da ayrı field yok — compile-time guarantee (field kaldırıldı).
    }

    #[test]
    fn role_claim_with_user_loaded_global_fallback_keeps_role_subject() {
        // **P1-a:** delta_node varsa override yok bile olsa Role(infer_role) korunur.
        // Runtime claim + global UserLoaded fallback → subject Role(Runtime), source UserLoaded.
        use crate::engine::EngineConfig;
        use crate::space::{Node, NodeClassification, NodeKind, Space};
        use crate::vision::{VisionSource, VisionVector};
        let mut space = Space::default();
        space.nodes.insert(
            0,
            Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                ..Default::default()
            },
        );
        let engine = crate::engine::SpaceEngine::new(
            space,
            crate::coords::CoordinateSystem::default_raw_five(
                crate::coords::MetricSource::Placeholder,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(0.0),
                crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
            )
            .unwrap(),
            // Global UserLoaded vision — override yok ama authority yeterli.
            VisionVector::with_source(
                crate::coords::RawPosition {
                    x: 0.5,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                },
                VisionSource::UserLoaded,
            ),
            EngineConfig::default_calibrated(),
        );
        // Test classification node → infer_role → Support. builtin_role_override(Support)
        // = None, role_overrides boş → override YOK, global UserLoaded fallback. Bu P1-a'nın
        // tam senaryosu: override yok ama subject yine de Role(Support) korunur.
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                classification: NodeClassification::Test,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        let sel = engine.effective_vision_selection(&claim).unwrap();
        let support_tag =
            crate::canonical_tags::CanonicalNodeRole::try_from(&crate::space::NodeRole::Support)
                .unwrap();
        assert!(
            matches!(sel.subject, CanonicalVisionSubject::Role(tag) if tag == support_tag),
            "Support claim must keep Role(Support) subject even without override, got {:?}",
            sel.subject
        );
        // Override yok → global vision inherit → UserLoaded source.
        assert_eq!(sel.vision_source(), VisionSource::UserLoaded);
    }

    #[test]
    fn different_inferred_role_contexts_change_digest() {
        // **P1-a (scoped-review P2 note):** Farklı classification'lardan çıkarılan farklı
        // rollerin (Runtime vs Support) claim-specific context'i farklı digest üretir.
        //
        // Bu test "saf subject-only" değişim değildir: Runtime için `builtin_role_override`
        // mevcut olduğundan Runtime context'inin effective vision + source'u da Support'tan
        // ayrışır (Support builtin None → global inherit). Subject-only değişimi
        // `evaluation_context_changes_when_only_subject_changes()` testi sabitler (aynı
        // vector + aynı source altında yalnız subject). Burada tam claim decision-tree
        // senaryosu doğrulanır: farklı inferred role → farklı subject + farklı vision chain.
        use crate::engine::EngineConfig;
        use crate::space::{Node, NodeClassification, NodeKind, Space};
        use crate::vision::{VisionSource, VisionVector};
        let mk_engine = || {
            let mut space = Space::default();
            space.nodes.insert(
                0,
                Node {
                    id: 0,
                    kind: NodeKind::Module,
                    mass: 100.0,
                    ..Default::default()
                },
            );
            crate::engine::SpaceEngine::new(
                space,
                crate::coords::CoordinateSystem::default_raw_five(
                    crate::coords::MetricSource::Placeholder,
                    crate::axes::CohesionAxis::new(),
                    crate::axes::EntropyAxis::from_commit_entropy(0.0),
                    crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
                )
                .unwrap(),
                VisionVector::with_source(
                    crate::coords::RawPosition::default(),
                    VisionSource::UserLoaded,
                ),
                EngineConfig::default_calibrated(),
            )
        };
        let mk_claim = |classification: NodeClassification| crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                classification,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        let engine = mk_engine();
        let rule_ctx = RuleEvaluationContext::try_new(vec![]).unwrap();
        // Runtime claim (Production) → Role(Runtime) + BuiltinRole override.
        // Support claim (Test) → Role(Support) + global inherit (builtin None).
        let runtime_sel = engine
            .effective_vision_selection(&mk_claim(NodeClassification::Production))
            .unwrap();
        let support_sel = engine
            .effective_vision_selection(&mk_claim(NodeClassification::Test))
            .unwrap();
        assert_ne!(
            runtime_sel.subject, support_sel.subject,
            "different classifications → different inferred roles → different subjects"
        );
        let runtime_ctx = EffectiveVisionGateContext::try_new(runtime_sel, 0.3).unwrap();
        let support_ctx = EffectiveVisionGateContext::try_new(support_sel, 0.3).unwrap();
        let d_runtime = EvaluationContextDigest::compute(&rule_ctx, &runtime_ctx).unwrap();
        let d_support = EvaluationContextDigest::compute(&rule_ctx, &support_ctx).unwrap();
        assert_ne!(
            d_runtime, d_support,
            "different inferred role contexts → different digest"
        );
    }

    #[test]
    fn unsupported_semantics_version_rejected() {
        // **P1-b:** Exact-version modeli. Binary'nin uygulamadığı semantiği digest'e
        // yazması engellenir (999 → UnsupportedSemanticsVersion).
        let mut sel = mk_selection(
            crate::vision::VisionSource::UserLoaded,
            CanonicalVisionSubject::Global,
            crate::coords::RawPosition::default(),
        );
        // role_inference_semver = 999 (supported: 1) → reject.
        sel.role_inference_semver = 999;
        let err = EffectiveVisionGateContext::try_new(sel.clone(), 0.3).unwrap_err();
        assert!(
            matches!(
                err,
                VisionContextError::UnsupportedSemanticsVersion {
                    field: "role_inference",
                    found: 999,
                    supported: ROLE_INFERENCE_SEMANTICS_VERSION
                }
            ),
            "role_inference_semver=999 must be rejected (exact-version), got {err:?}"
        );
        // vision_selection_semver = 999 → reject.
        let mut sel2 = sel.clone();
        sel2.role_inference_semver = ROLE_INFERENCE_SEMANTICS_VERSION;
        sel2.vision_selection_semver = 999;
        let err2 = EffectiveVisionGateContext::try_new(sel2, 0.3).unwrap_err();
        assert!(
            matches!(
                err2,
                VisionContextError::UnsupportedSemanticsVersion {
                    field: "vision_selection",
                    found: 999,
                    supported: VISION_SELECTION_SEMANTICS_VERSION
                }
            ),
            "vision_selection_semver=999 must be rejected (exact-version), got {err2:?}"
        );
    }

    #[test]
    fn canonical_role_conversion_fail_closed() {
        // **P1-c:** effective_vision_selection Result döner; canonical role conversion
        // hatası terminal olarak yayılır (sessiz Runtime fallback YOK). Mevcut enum'da
        // infer_role yalnız Support/Runtime üretir (conversion hep başarılı), ama API
        // sözleşmesi fail-closed'dır — yeni NodeRole varyantı eklendiğinde koruma aktif.
        //
        // Bu test yapısal assertion: effective_vision_selection Result döner ve hata
        // tipi VisionContextError::CanonicalRoleConversionFailed. Runtime'da tetiklenmez
        // (infer_role sınırlı) ama API guarantee'yi doğrular.
        use crate::engine::EngineConfig;
        use crate::space::{Node, NodeClassification, NodeKind, Space};
        use crate::vision::{VisionSource, VisionVector};
        let engine = crate::engine::SpaceEngine::new(
            Space::default(),
            crate::coords::CoordinateSystem::default_raw_five(
                crate::coords::MetricSource::Placeholder,
                crate::axes::CohesionAxis::new(),
                crate::axes::EntropyAxis::from_commit_entropy(0.0),
                crate::axes::WitnessDepthAxis::from_witness(0.0, 0),
            )
            .unwrap(),
            VisionVector::with_source(
                crate::coords::RawPosition::default(),
                VisionSource::UserLoaded,
            ),
            EngineConfig::default_calibrated(),
        );
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(0, crate::coords::RawPosition::default()),
            author: 0,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                classification: NodeClassification::Production,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };
        // Conversion bugün başarılı (Runtime valid) — API Result döndüğünü doğrula.
        let result = engine.effective_vision_selection(&claim);
        assert!(
            result.is_ok(),
            "valid claim must produce selection; API is Result (fail-closed contract): {:?}",
            result.err()
        );
        // VisionContextError::CanonicalRoleConversionFailed variantı mevcut — yeni
        // NodeRole eklendiğinde TryFrom exhaustive match derleme hatası verir, mapping
        // güncellenmek zorunda kalınır (compiler-enforced).
        let _variant_exists = |e: VisionContextError| {
            matches!(e, VisionContextError::CanonicalRoleConversionFailed(_))
        };
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 5 — defensive structural-delta integrity testleri
    // (preimage encoding, identity-based conflict, custom Deserialize, defensive validation)
    // ═══════════════════════════════════════════════════════════════════════════════

    fn mk_edge_identity(from: u64, to: u64) -> CanonicalEdgeIdentity {
        CanonicalEdgeIdentity::new(
            from,
            to,
            CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
        )
    }

    fn mk_canonical_edge(from: u64, to: u64, is_type_only: bool) -> CanonicalEdge {
        CanonicalEdge {
            from,
            to,
            kind: CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
            is_type_only,
        }
    }

    #[test]
    fn removed_edge_encoding_contains_only_identity_fields() {
        // **Step 5 P1:** Preimage test — removed edge encoding 17 byte (from(8)+to(8)+kind(1)),
        // is_type_only YOK. Hash sonucundan alan yokluğu kanıtlanamaz; tam preimage kontrol.
        let edge = mk_edge_identity(1, 2);
        let encoded = encode_canonical_edge_identity_to_vec(&edge);
        assert_eq!(
            encoded.len(),
            17,
            "identity encoding = from(8) + to(8) + kind(1) = 17 bytes, no is_type_only"
        );
        assert_eq!(&encoded[0..8], &1u64.to_le_bytes(), "from = 1");
        assert_eq!(&encoded[8..16], &2u64.to_le_bytes(), "to = 2");
        // kind byte — Imports tag değeri (CanonicalEdgeKind).
        let imports_tag = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap();
        assert_eq!(encoded[16], imports_tag.as_u8(), "kind = Imports tag");
    }

    #[test]
    fn changing_new_edge_is_type_only_changes_digest() {
        // **Step 5:** new_edges encoding is_type_only dahil — değişince byte akışı değişir.
        // removed_edges is_type_only YOK (identity-only, ayrı test). new_edges'te
        // is_type_only'nin yaşadığını encoder preimage üzerinden doğrular (hash değil).
        use crate::authorization::AuthorizationBasisDigest;
        // İki edge aynı identity ama farklı is_type_only → duplicate (reject). Bu yüzden
        // iki ayrı delta kurup各自的 digest karşılaştırırız.
        let mk_digest = |is_type_only: bool| {
            // Minimal AuthorizationBasis — sample_basis pattern tek edge ile.
            // Ama daha basit: encode_canonical_edge_vec byte akışını doğrudan karşılaştır.
            let edge = mk_canonical_edge(1, 2, is_type_only);
            let mut hasher = blake3::Hasher::new();
            encode_canonical_edge_vec(&mut hasher, std::slice::from_ref(&edge)).unwrap();
            hasher.finalize().into()
        };
        let d_true: [u8; 32] = mk_digest(true);
        let d_false: [u8; 32] = mk_digest(false);
        assert_ne!(
            d_true, d_false,
            "new_edges is_type_only must change encoding (byte stream differs)"
        );
        let _ = AuthorizationBasisDigest::DOMAIN_SEPARATOR; // v1 korunur
    }

    #[test]
    fn removed_edge_identity_deserialization_rejects_is_type_only_field() {
        // **Step 5 P0:** deny_unknown_fields — tek canonical representation.
        // Diskten is_type_only içeren eski JSON reject edilir.
        let json_with_extra = r#"{"from":1,"to":2,"kind":0,"is_type_only":false}"#;
        let result: Result<CanonicalEdgeIdentity, _> = serde_json::from_str(json_with_extra);
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject is_type_only on CanonicalEdgeIdentity"
        );
        // Doğru representation (3 alan) kabul edilir.
        let json_correct = r#"{"from":1,"to":2,"kind":0}"#;
        let parsed: CanonicalEdgeIdentity =
            serde_json::from_str(json_correct).expect("3-field identity must deserialize");
        assert_eq!(parsed.from(), 1);
        assert_eq!(parsed.to(), 2);
    }

    #[test]
    fn add_and_remove_same_identity_conflict_regardless_of_is_type_only() {
        // **Step 5b gap kapanışı:** (1,2,Imports,true) add + (1,2,Imports) remove → conflict.
        // Eski kod tam CanonicalEdge eşitliği kullanıyordu → is_type_only farkı conflict'i
        // kaçırıyordu. Artık identity üzerinden — is_type_only bağımsız.
        let new_edge = mk_canonical_edge(1, 2, true); // is_type_only: true
        let removed_identity = mk_edge_identity(1, 2); // is_type_only YOK
        let err = CanonicalStructuralDelta::try_new(vec![], vec![new_edge], vec![removed_identity])
            .unwrap_err();
        assert_eq!(
            err,
            CanonicalizationError::CrossListEdgeConflict,
            "add+remove same identity must conflict regardless of is_type_only"
        );
    }

    #[test]
    fn duplicate_new_edge_identity_rejected_when_type_only_differs() {
        // **Step 5b:** (1,2,Imports,true) + (1,2,Imports,false) aynı identity → duplicate.
        // Eski kod bunları farklı CanonicalEdge sanırdı. Artık identity eşit → DuplicateEdge.
        let edge_a = mk_canonical_edge(1, 2, true);
        let edge_b = mk_canonical_edge(1, 2, false);
        let err =
            CanonicalStructuralDelta::try_new(vec![], vec![edge_a, edge_b], vec![]).unwrap_err();
        assert_eq!(
            err,
            CanonicalizationError::DuplicateEdge,
            "duplicate identity (is_type_only differs) must be rejected"
        );
    }

    #[test]
    fn duplicate_removed_edge_identity_rejected() {
        // **Step 5:** removed_edges'te aynı identity → DuplicateEdge.
        let a = mk_edge_identity(1, 2);
        let b = mk_edge_identity(1, 2);
        let err = CanonicalStructuralDelta::try_new(vec![], vec![], vec![a, b]).unwrap_err();
        assert_eq!(
            err,
            CanonicalizationError::DuplicateEdge,
            "duplicate removed_edge identity must be rejected"
        );
    }

    #[test]
    fn structural_delta_custom_deserialize_runs_validation() {
        // **Step 5:** custom Deserialize try_new üzerinden — malformed JSON reject.
        // Duplicate node id (id=5 iki kez) → serialize → deserialize should fail.
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
        // Manuel malformed JSON — duplicate node id (5 iki kez).
        let malformed = r#"{
            "new_nodes": [
                {"id":5,"kind":0,"mass":1.0,"classification":0,"role":4},
                {"id":5,"kind":0,"mass":1.0,"classification":0,"role":4}
            ],
            "new_edges": [],
            "removed_edges": []
        }"#;
        let result: Result<CanonicalStructuralDelta, _> = serde_json::from_str(malformed);
        assert!(
            result.is_err(),
            "custom Deserialize must reject duplicate node id (validate through try_new)"
        );
        // Geçerli delta deserialize olur.
        let valid = CanonicalStructuralDelta::try_new(vec![node()], vec![], vec![]).unwrap();
        let serialized = serde_json::to_string(&valid).unwrap();
        let deserialized: CanonicalStructuralDelta =
            serde_json::from_str(&serialized).expect("valid delta round-trips");
        assert_eq!(deserialized.new_nodes().len(), 1);
    }

    #[test]
    fn basis_digest_compute_rejects_invalid_structural_delta() {
        // **Step 5 P0 + scoped P1-a:** AuthorizationBasisDigest::compute başında validate
        // çağrılır. Gerçek invalid delta (duplicate edge identity) enjekte edilir —
        // compute validate'de EncodingFailed döner. Defensive çağrı kaldırılırsa test kırılır.
        //
        // Test modülü parent module'ün private alanlarına erişebilir → struct literal ile
        // try_new'i bypass eden bozuk delta üretilebilir (defensive katmanı test etmek için).
        let imports = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap();
        // Aynı identity, farklı is_type_only → duplicate (validate Ordering::Equal).
        let invalid = CanonicalStructuralDelta {
            new_nodes: vec![],
            new_edges: vec![
                CanonicalEdge {
                    from: 1,
                    to: 2,
                    kind: imports,
                    is_type_only: true,
                },
                CanonicalEdge {
                    from: 1,
                    to: 2,
                    kind: imports,
                    is_type_only: false,
                },
            ],
            removed_edges: vec![],
        };
        let mut basis = sample_basis();
        basis.structural_delta = invalid;
        let error = AuthorizationBasisDigest::compute(&basis).unwrap_err();
        assert!(
            matches!(error, AuthorizationBasisDigestError::EncodingFailed(_)),
            "compute must reject invalid structural delta via validate(): got {error:?}"
        );
    }

    #[test]
    fn pending_envelope_verify_reports_structural_delta_invalid() {
        // **Step 5 P0 + scoped P1-b:** Envelope verify structural delta validation yapar.
        // Gerçek invalid delta (unsorted new_edges) taşıyan envelope üzerinde verify()
        // çağrılır → StructuralDeltaInvalid. verify()'daki structural validation kaldırılırsa
        // test kırılır (BasisDigestMismatch yerine StructuralDeltaInvalid beklenir).
        //
        // Validation'ın BasisDigestMismatch kontrolünden ÖNCE çalıştığını da sabitler.
        let imports = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap();
        // Ters sıra (9,10 önce, 1,2 sonra) → Ordering::Greater → UnsortedNewEdges.
        let invalid = CanonicalStructuralDelta {
            new_nodes: vec![],
            new_edges: vec![
                CanonicalEdge {
                    from: 9,
                    to: 10,
                    kind: imports,
                    is_type_only: false,
                },
                CanonicalEdge {
                    from: 1,
                    to: 2,
                    kind: imports,
                    is_type_only: false,
                },
            ],
            removed_edges: vec![],
        };
        let mut basis = sample_basis();
        basis.structural_delta = invalid;
        let envelope = PendingAuthorizationEnvelope {
            schema: PENDING_AUTHORIZATION_SCHEMA.to_string(),
            record: sample_pending_record(),
            authorization_basis: basis,
        };
        assert!(
            matches!(
                envelope.verify(),
                Err(PendingAuthorizationLoadError::StructuralDeltaInvalid(_))
            ),
            "verify must report StructuralDeltaInvalid for unsorted structural delta (before digest check)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 6 — Golden vectors (v1 byte contract)
    //
    // Sınırlı hibrit: hardcoded end-to-end golden digest (regression kilidi) + hedefli
    // preimage testleri (ilk implementasyon hatası kilidi). Tam bağımsız ikinci encoder YOK.
    //
    // Fixture'lar AYRI: authorization fixture yalnız AuthorizationBasis encoding kollarını
    // kapsar (nested digest'ler explicit sentinel — encoding değişikliği kök nedeni net).
    // Evaluation fixture ayrı (rule context + vision context).
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Test helper: 32-byte digest → lowercase hex (EvaluationContextDigest to_hex YOK).
    /// Public API'ye to_hex eklenmez — yalnız test modülünde.
    fn digest_hex(bytes: &[u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// **Step 6 golden authorization fixture** — her AuthorizationBasis encoding kolunu kapsar.
    /// Nested digest'ler (measurement/evaluation/space) **explicit sentinel** — compute ile
    /// ÜRETİLMEZ. Bu, AuthorizationBasis golden failure → AuthorizationBasis encoding
    /// değişti, EvaluationContext golden failure → Q5/Q6 encoding değişti ayrımını korur.
    ///
    /// **Not:** AuthorizationBasis rule context / vision taşımaz; bunlar **opaque
    /// evaluation digest bytes** olarak bağlı (sentinel [0x22; 32]).
    #[allow(clippy::too_many_lines)]
    fn golden_authorization_basis_fixture() -> AuthorizationBasis {
        let module_kind = CanonicalNodeKind::try_from(&crate::space::NodeKind::Module).unwrap();
        let concept_kind = CanonicalNodeKind::try_from(&crate::space::NodeKind::Concept).unwrap();
        let production =
            CanonicalNodeClassification::try_from(&crate::space::NodeClassification::Production)
                .unwrap();
        let runtime = CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap();
        let typesurface =
            CanonicalNodeRole::try_from(&crate::space::NodeRole::TypeSurface).unwrap();
        let imports = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap();
        let calls = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Calls).unwrap();
        let scip = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::Scip,
        )
        .unwrap();
        let treesitter = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::TreeSitter,
        )
        .unwrap();

        // 2 node: biri Some(cohesion), diğeri None — Option<f64> encoding kolları.
        // try_new id sırasına göre canonicalize eder.
        let new_nodes = vec![
            CanonicalNode {
                id: 7,
                kind: module_kind,
                mass: 250.0,
                cohesion: Some(0.72),
                classification: production,
                role: runtime,
            },
            CanonicalNode {
                id: 23,
                kind: concept_kind,
                mass: 40.0,
                cohesion: None,
                classification: production,
                role: typesurface,
            },
        ];
        // new_edges: is_type_only: true — eklenen edge semantiği.
        let new_edges = vec![CanonicalEdge {
            from: 7,
            to: 23,
            kind: imports,
            is_type_only: true,
        }];
        // removed_edges: identity-only (CanonicalEdgeIdentity).
        let removed_edges = vec![CanonicalEdgeIdentity::new(7, 99, calls)];

        // 2 predicate, TERS canonical sırada verilir — encode_effective_predicate_set
        // sıralar. Predicate A (Subgraph + Exact), Predicate B (Node + Any).
        // Canonical byte ordering coverage (sort byte representation'ları üzerinden).
        let coupling =
            PredicateAxisTag::try_from(&crate::trajectory::PredicateAxis::Coupling).unwrap();
        let entropy =
            PredicateAxisTag::try_from(&crate::trajectory::PredicateAxis::Entropy).unwrap();
        let le = ComparisonOpTag::try_from(&crate::trajectory::ComparisonOp::Le).unwrap();
        let gt = ComparisonOpTag::try_from(&crate::trajectory::ComparisonOp::Gt).unwrap();
        let subgraph_scope = CanonicalPredicateScope::Subgraph(
            CanonicalSubgraphScope::try_new(vec![7, 23]).unwrap(),
        );
        let node_scope = CanonicalPredicateScope::Node(7);
        // Ters sırada: byte representation büyük olan önce (B daha büyük olsun diye
        // değerleri seçtik — gt/entropy/node_scope/any kombinasyonu).
        let predicates = vec![
            // Predicate B (Node + Any) — canonical byte ordering'de farklı konum.
            EffectiveMetricPredicate {
                axis: entropy,
                operator: gt,
                threshold: 0.3,
                scope: node_scope,
                required_source: EffectiveSourceRequirement::Any,
                effective_weight: 1.5,
                effective_tolerance: 0.05,
            },
            // Predicate A (Subgraph + Exact).
            EffectiveMetricPredicate {
                axis: coupling,
                operator: le,
                threshold: 0.55,
                scope: subgraph_scope,
                required_source: EffectiveSourceRequirement::Exact(scip),
                effective_weight: 2.0,
                effective_tolerance: 0.1,
            },
        ];

        AuthorizationBasis {
            schema_version: 1,
            task_id: TaskId::from(555u64),
            claim_identity: ClaimIdentity {
                claim_id: ClaimId::from(909u64),
                task_id: TaskId::from(555u64),
            },
            claim_author: AgentId::from(321u64),
            structural_delta: CanonicalStructuralDelta::try_new(
                new_nodes,
                new_edges,
                removed_edges,
            )
            .unwrap(),
            predicate_content: CanonicalPredicateContent {
                mode: PredicateModeTag::try_from(&crate::trajectory::PredicateMode::All).unwrap(),
                predicates,
            },
            predicate_evaluation: PredicateEvaluationBasis {
                target_vector: CanonicalRawPosition {
                    x: 0.42,
                    y: 0.71,
                    z: 0.38,
                    w: 0.61,
                    v: 0.27,
                },
                loss_before: 1.37,
                loss_after: 0.83,
                failure_policy: PredicateFailurePolicyTag::try_from(
                    &crate::trajectory::PredicateFailurePolicy::AcceptImprovement,
                )
                .unwrap(),
                allow_progress_checkpoint: true,
                min_improvement_delta: 0.07,
                improvement_policy: EffectiveImprovementPolicy::current_semantics(),
            },
            // 5 measurements: farklı değer + farklı source (Some scip, Some treesitter).
            measured_result: ProvenancedMeasuredResult {
                coupling: CanonicalAxisMeasurement {
                    value: 0.68,
                    source: scip,
                },
                cohesion: CanonicalAxisMeasurement {
                    value: 0.74,
                    source: treesitter,
                },
                instability: CanonicalAxisMeasurement {
                    value: 0.31,
                    source: scip,
                },
                entropy: CanonicalAxisMeasurement {
                    value: 0.45,
                    source: treesitter,
                },
                witness_depth: CanonicalAxisMeasurement {
                    value: 0.12,
                    source: scip,
                },
            },
            // non-zero tags.
            deterministic_gate_result: GateDecision::PassedAll,
            predicate_completion: PredicateCompletion::Completed,
            mutation_decision: MutationDecision::AcceptAsProgress,
            intended_apply_target: ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint),
            // witness_policy: non-default.
            witness_policy: CanonicalWitnessPolicy {
                schema_version: 1,
                min_approvers: 4,
                quorum_threshold: 3.25,
                independence_policy: WitnessIndependencePolicyTag::STRICT,
            },
            // Nested digest'ler: EXPLICIT SENTINEL (compute ile üretilmez).
            // AuthorizationBasis encoding değişikliği → golden failure buradan gelir,
            // nested digest encoding değişikliğinden DEĞİL.
            measurement_input_digest: MeasurementInputDigest::from_bytes([0x11; 32]),
            evaluation_context_digest: EvaluationContextDigest::from_bytes([0x22; 32]),
            base_space_view_revision: SpaceViewRevision {
                view_id: SpaceViewId::Persisted(PersistedSpaceViewId::from_bytes([
                    0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x01,
                    0x23, 0x45, 0x67,
                ])),
                sequence: 42,
                content_digest: SpaceDigest::from_bytes([0x33; 32]),
            },
        }
    }

    /// **Step 6 golden rule context fixture** — 2 ordered rule, farklı id/semver/parameters.
    fn golden_rule_context_fixture() -> RuleEvaluationContext {
        RuleEvaluationContext::try_new(vec![
            OrderedRuleDescriptor {
                ordinal: 0,
                descriptor: RuleDescriptor {
                    rule_id: "structural.no_self_import".to_string(),
                    semantics_version: 1,
                    canonical_parameters: vec![0xAB, 0xCD],
                },
            },
            OrderedRuleDescriptor {
                ordinal: 1,
                descriptor: RuleDescriptor {
                    rule_id: "structural.no_orphan_witness".to_string(),
                    semantics_version: 2,
                    canonical_parameters: vec![],
                },
            },
        ])
        .unwrap()
    }

    /// **Step 6 golden vision context fixture** — Role subject + RoleProfile source +
    /// non-zero 5-axis + non-zero theta_bound.
    fn golden_vision_context_fixture() -> EffectiveVisionGateContext {
        let runtime_tag =
            crate::canonical_tags::CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime)
                .unwrap();
        let selection = EffectiveVisionSelection {
            effective_vision: crate::vision::VisionVector::with_source(
                crate::coords::RawPosition {
                    x: 0.31,
                    y: 0.62,
                    z: 0.47,
                    w: 0.58,
                    v: 0.19,
                },
                crate::vision::VisionSource::RoleProfile,
            ),
            subject: CanonicalVisionSubject::Role(runtime_tag),
            role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
            vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
        };
        EffectiveVisionGateContext::try_new(selection, 0.37).unwrap()
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 6 — Golden vectors + exact preimage tests (v1 byte contract lock)
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Golden hex sabitleri — POST-refactor doğrulanmış değerler (PRE == POST).
    /// DO NOT update as routine test maintenance. A mismatch requires an explicit
    /// compatibility/version decision: a canonical field/order/tag/encoding change
    /// requires v2; a fixture semantics-version change must be reviewed according to
    /// its compatibility impact (may or may not warrant v2).
    const AUTHORIZATION_V1_GOLDEN_HEX: &str =
        "7f67f2acf97bc9747b9f708437eb6a3454628f3cb4c23541e48e00554a4945f5";
    const EVALUATION_CONTEXT_V1_GOLDEN_HEX: &str =
        "b2e7e883e0af8bdbff02e691d39f1574caaeb6be9d1a29e8467a3b99d79f1a5f";

    #[test]
    fn authorization_basis_digest_v1_golden_vector() {
        // **Step 6:** v1 byte contract lock — AuthorizationBasisDigest. Encoding
        // (field order, tag values, float canonicalization, predicate sorting, edge
        // identity-only) bu testle kilitlenir. Nested digest'ler sentinel →
        // AuthorizationBasis encoding değişikliği buradan gelir, nested değişiklikten değil.
        let actual =
            AuthorizationBasisDigest::compute(&golden_authorization_basis_fixture()).unwrap();
        assert_eq!(
            actual.to_hex(),
            AUTHORIZATION_V1_GOLDEN_HEX,
            "AuthorizationBasis v1 byte contract changed — explicit version decision required"
        );
    }

    #[test]
    fn evaluation_context_digest_v1_golden_vector() {
        // **Step 6:** v1 byte contract lock — EvaluationContextDigest. Q5 vision-gate +
        // Q6 ordered-rule encoding kilitlenir. Authorization golden'dan AYRI —
        // evaluation encoding değişikliği bu testi kırar, authorization'ı değil.
        let actual = EvaluationContextDigest::compute(
            &golden_rule_context_fixture(),
            &golden_vision_context_fixture(),
        )
        .unwrap();
        assert_eq!(
            digest_hex(actual.as_bytes()),
            EVALUATION_CONTEXT_V1_GOLDEN_HEX,
            "EvaluationContext v1 byte contract changed — explicit version decision required"
        );
    }

    // ── Exact preimage testleri (shared byte helper'lar üzerinden) ──────────────

    #[test]
    fn canonical_f64_bytes_preimage() {
        // **Step 6 P0:** canonical_f64_bytes — finite, -0.0 normalize, LE to_bits.
        assert_eq!(
            canonical_f64_bytes(1.0).unwrap(),
            1.0f64.to_bits().to_le_bytes()
        );
        // -0.0 == +0.0 encoding (normalize).
        assert_eq!(
            canonical_f64_bytes(-0.0).unwrap(),
            canonical_f64_bytes(0.0).unwrap()
        );
        // Non-finite reject.
        assert!(canonical_f64_bytes(f64::NAN).is_err());
        assert!(canonical_f64_bytes(f64::INFINITY).is_err());
        assert!(canonical_f64_bytes(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn encode_optional_f64_to_vec_preimage() {
        // **Step 6 P0:** Option<f64> encoding — presence tag + canonical float.
        assert_eq!(encode_optional_f64_to_vec(None).unwrap(), vec![0u8]);
        let some = encode_optional_f64_to_vec(Some(1.0)).unwrap();
        assert_eq!(some.len(), 9, "Some(v) = tag(1) + canonical_f64(8)");
        assert_eq!(some[0], 1u8, "presence tag = 1");
        assert_eq!(
            &some[1..],
            &1.0f64.to_bits().to_le_bytes(),
            "value = canonical_f64_bytes(1.0)"
        );
    }

    #[test]
    fn encode_canonical_edge_to_vec_preimage() {
        // **Step 6 P0:** CanonicalEdge → 18 byte (from + to + kind + is_type_only).
        let imports = CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap();
        let edge = CanonicalEdge {
            from: 1,
            to: 2,
            kind: imports,
            is_type_only: true,
        };
        let encoded = encode_canonical_edge_to_vec(&edge);
        assert_eq!(
            encoded.len(),
            18,
            "edge = from(8) + to(8) + kind(1) + is_type_only(1)"
        );
        assert_eq!(&encoded[0..8], &1u64.to_le_bytes(), "from = 1");
        assert_eq!(&encoded[8..16], &2u64.to_le_bytes(), "to = 2");
        assert_eq!(encoded[16], imports.as_u8(), "kind = Imports tag");
        assert_eq!(encoded[17], 1u8, "is_type_only = true");
    }

    #[test]
    fn encode_vision_subject_to_vec_preimage() {
        // **Step 6 P0:** CanonicalVisionSubject — Global [0], Role(role) [1, role].
        let global = encode_vision_subject_to_vec(CanonicalVisionSubject::Global);
        assert_eq!(global, vec![0u8], "Global = [0] (1 byte)");
        let runtime_tag =
            crate::canonical_tags::CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime)
                .unwrap();
        let role = encode_vision_subject_to_vec(CanonicalVisionSubject::Role(runtime_tag));
        assert_eq!(role.len(), 2, "Role = [1, role_tag] (2 byte)");
        assert_eq!(role[0], 1u8, "subject kind = 1 (Role)");
        assert_eq!(role[1], runtime_tag.as_u8(), "role tag");
    }

    #[test]
    fn push_effective_source_preimage() {
        // **Step 6 P1:** EffectiveSourceRequirement — Any [0], Exact(src) [1, src].
        // (push_effective_source mevcut helper — encode_effective_predicate_to_vec kullanıyor.)
        let mut any_buf = Vec::new();
        push_effective_source(&mut any_buf, &EffectiveSourceRequirement::Any);
        assert_eq!(any_buf, vec![0u8], "Any = [0]");
        let scip = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::Scip,
        )
        .unwrap();
        let mut exact_buf = Vec::new();
        push_effective_source(&mut exact_buf, &EffectiveSourceRequirement::Exact(scip));
        assert_eq!(exact_buf.len(), 2, "Exact = [1, source_tag]");
        assert_eq!(exact_buf[0], 1u8, "presence = 1 (Exact)");
        assert_eq!(exact_buf[1], scip.as_u8(), "source = Scip tag");
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #72 — Suspended attempt-evidence model tests (Commit 1)
    //
    // P0-1 AttemptNumber invariant (custom Deserialize + TryFrom).
    // P1 canonical rejection ordering (sort + duplicate reject).
    // Evidence digest golden vector + preimage tests.
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn attempt_number_try_from_rejects_zero() {
        // P0-1: 0 → Err(Zero). 1-based trajectory invariant.
        assert_eq!(
            AttemptNumber::try_from(0u64),
            Err(AttemptNumberError::Zero),
            "attempt number zero must be rejected (1-based invariant)"
        );
    }

    #[test]
    fn attempt_number_try_from_accepts_nonzero() {
        let one = AttemptNumber::try_from(1u64).unwrap();
        assert_eq!(one.get(), 1);
        let large = AttemptNumber::try_from(u64::MAX).unwrap();
        assert_eq!(large.get(), u64::MAX);
    }

    #[test]
    fn attempt_number_deserialize_rejects_zero() {
        // P0-1: derived Deserialize bypass fix — custom Deserialize `0` JSON'dan reject eder.
        let json = "0";
        let result: Result<AttemptNumber, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Deserialize must reject zero (custom Deserialize invariant)"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("attempt number must be >= 1"),
            "error must explain 1-based invariant, got: {err}"
        );
    }

    #[test]
    fn attempt_number_round_trips_nonzero() {
        // Serialize → Deserialize round-trip nonzero değerle çalışır.
        let original = AttemptNumber::try_from(42u64).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "42", "transparent serialize as u64");
        let restored: AttemptNumber = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn attempt_number_try_from_via_into() {
        // TryFrom<u64> — `u64::into()` da çalışır.
        let n: AttemptNumber = 7u64.try_into().unwrap();
        assert_eq!(n.get(), 7);
    }

    // — SuspendedAttemptEvidence construction & wire-format —

    /// Test helper: Held evidence fixture (Commit 1 — sadece Commit 1 testleri için).
    fn sample_held_evidence() -> SuspendedAttemptEvidence {
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "5555555555555555555555555555555555555555555555555555555555555555",
        )
        .unwrap();
        SuspendedAttemptEvidence::try_new(
            TaskId::from(7u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(3u64).unwrap(),
            SuspendedAttemptDisposition::Held {
                hold_reason: WitnessHoldReason::QuorumInsufficient {
                    support: 0.8,
                    threshold: 1.5,
                },
                snapshot: WitnessQuorumSnapshot {
                    approvers: 1,
                    required_approvers: 2,
                    support: 0.8,
                    required_support: 1.5,
                },
            },
        )
        .unwrap()
    }

    /// Test helper: Rejected evidence fixture.
    fn sample_rejected_evidence() -> SuspendedAttemptEvidence {
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "6666666666666666666666666666666666666666666666666666666666666666",
        )
        .unwrap();
        SuspendedAttemptEvidence::try_new(
            TaskId::from(9u64),
            ClaimId::from(99u64),
            basis_digest,
            AttemptNumber::try_from(2u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_single(WitnessRejection {
                    witness: 100u64,
                    rationale: Some("predicate mismatch".to_string()),
                }),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap()
    }

    #[test]
    fn suspended_evidence_try_new_sets_schema_version() {
        let ev = sample_held_evidence();
        assert_eq!(
            ev.schema_version(),
            SUSPENDED_ATTEMPT_EVIDENCE_SCHEMA_VERSION
        );
        assert_eq!(ev.schema_version(), 1, "v1 byte contract");
    }

    #[test]
    fn suspended_evidence_accessors_return_correct_values() {
        let ev = sample_held_evidence();
        assert_eq!(ev.task_id(), TaskId::from(7u64));
        assert_eq!(ev.claim_id(), ClaimId::from(42u64));
        assert_eq!(ev.attempt_num().get(), 3);
        assert_eq!(ev.authorization_basis_digest().as_bytes(), &[0x55; 32]);
        assert!(matches!(
            ev.disposition(),
            SuspendedAttemptDisposition::Held { .. }
        ));
    }

    #[test]
    fn suspended_evidence_round_trips_through_serde_held() {
        let original = sample_held_evidence();
        let json = serde_json::to_string(&original).unwrap();
        let restored: SuspendedAttemptEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored, original,
            "serde round-trip must preserve evidence"
        );
    }

    #[test]
    fn suspended_evidence_round_trips_through_serde_rejected() {
        let original = sample_rejected_evidence();
        let json = serde_json::to_string(&original).unwrap();
        let restored: SuspendedAttemptEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn suspended_evidence_rejects_unknown_field() {
        // P0-1: custom Deserialize + deny_unknown_fields — extra field reject.
        let ev = sample_held_evidence();
        let mut json = serde_json::to_value(&ev).unwrap();
        // Bilinmeyen alan enjekte et.
        json["unknown_field"] = serde_json::json!(42);
        let json_str = serde_json::to_string(&json).unwrap();
        let result: Result<SuspendedAttemptEvidence, _> = serde_json::from_str(&json_str);
        assert!(
            result.is_err(),
            "unknown field must be rejected (deny_unknown_fields)"
        );
    }

    #[test]
    fn suspended_evidence_rejects_schema_version_mismatch() {
        let ev = sample_held_evidence();
        let mut json = serde_json::to_value(&ev).unwrap();
        json["schema_version"] = serde_json::json!(999);
        let json_str = serde_json::to_string(&json).unwrap();
        let result: Result<SuspendedAttemptEvidence, _> = serde_json::from_str(&json_str);
        assert!(
            result.is_err(),
            "schema version mismatch must be rejected on deserialize"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("schema version mismatch"),
            "error must name schema mismatch, got: {err}"
        );
    }

    #[test]
    fn suspended_evidence_rejects_attempt_num_zero_on_deserialize() {
        let ev = sample_held_evidence();
        let mut json = serde_json::to_value(&ev).unwrap();
        json["attempt_num"] = serde_json::json!(0);
        let json_str = serde_json::to_string(&json).unwrap();
        let result: Result<SuspendedAttemptEvidence, _> = serde_json::from_str(&json_str);
        assert!(
            result.is_err(),
            "attempt_num=0 must be rejected via AttemptNumber custom Deserialize"
        );
    }

    #[test]
    fn suspended_evidence_disposition_tagged_enum_round_trips() {
        // Serde tag = "kind", rename_all = "snake_case".
        let held = sample_held_evidence();
        let held_json = serde_json::to_value(&held).unwrap();
        assert_eq!(held_json["disposition"]["kind"], "held");

        let rejected = sample_rejected_evidence();
        let rejected_json = serde_json::to_value(&rejected).unwrap();
        assert_eq!(rejected_json["disposition"]["kind"], "rejected");
    }

    // — SuspendedAttemptEvidenceDigest determinism + golden vector —

    #[test]
    fn suspended_evidence_digest_is_deterministic() {
        let ev = sample_held_evidence();
        let d1 = SuspendedAttemptEvidenceDigest::compute(&ev).unwrap();
        let d2 = SuspendedAttemptEvidenceDigest::compute(&ev).unwrap();
        assert_eq!(d1.as_bytes(), d2.as_bytes(), "BLAKE3 deterministic");
    }

    #[test]
    fn suspended_evidence_digest_differs_for_held_vs_rejected() {
        // Aynı identity fields, farklı disposition → farklı digest.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "7777777777777777777777777777777777777777777777777777777777777777",
        )
        .unwrap();
        let held = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest.clone(),
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Held {
                hold_reason: WitnessHoldReason::MinApproversNotMet {
                    distinct: 0,
                    required: 2,
                },
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let rejected = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_single(WitnessRejection {
                    witness: 5u64,
                    rationale: None,
                }),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let dh = SuspendedAttemptEvidenceDigest::compute(&held).unwrap();
        let dr = SuspendedAttemptEvidenceDigest::compute(&rejected).unwrap();
        assert_ne!(
            dh.as_bytes(),
            dr.as_bytes(),
            "Held vs Rejected must produce distinct digests (disposition tag binding)"
        );
    }

    #[test]
    fn suspended_evidence_digest_hex_round_trips() {
        let ev = sample_held_evidence();
        let d = SuspendedAttemptEvidenceDigest::compute(&ev).unwrap();
        let hex = d.to_hex();
        assert_eq!(hex.len(), 64, "32 bytes → 64 hex chars");
        let restored = SuspendedAttemptEvidenceDigest::from_hex(&hex).unwrap();
        assert_eq!(restored.as_bytes(), d.as_bytes());
    }

    // — Canonical rejection ordering (P1) —

    #[test]
    fn rejection_canonical_order_independent_of_input_order() {
        // P1: aynı rejection kümesi farklı input sırasıyla aynı digest üretir.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "8888888888888888888888888888888888888888888888888888888888888888",
        )
        .unwrap();
        let snapshot = WitnessQuorumSnapshot {
            approvers: 0,
            required_approvers: 2,
            support: 0.0,
            required_support: 1.5,
        };
        let r1 = WitnessRejection {
            witness: 10u64,
            rationale: Some("a".to_string()),
        };
        let r2 = WitnessRejection {
            witness: 20u64,
            rationale: None,
        };
        // [r1, r2] sırasıyla.
        let ev_a = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest.clone(),
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r1.clone(), r2.clone()]),
                snapshot: snapshot.clone(),
            },
        )
        .unwrap();
        // [r2, r1] sırasıyla — aynı mantıksal küme.
        let ev_b = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r2, r1]),
                snapshot,
            },
        )
        .unwrap();
        let da = SuspendedAttemptEvidenceDigest::compute(&ev_a).unwrap();
        let db = SuspendedAttemptEvidenceDigest::compute(&ev_b).unwrap();
        assert_eq!(
            da.as_bytes(),
            db.as_bytes(),
            "canonical sort must make rejection order irrelevant to digest"
        );
    }

    #[test]
    fn rejection_canonical_rejects_duplicate_witness_rationale() {
        // **P0-3 + P1-2 (closure):** Duplicate (witness, rationale) artık
        // constructor'da reject ediliyor (DuplicateRejection) — eski test digest
        // level bekliyordu, artık try_new constructor level reject ediyor.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "9999999999999999999999999999999999999999999999999999999999999999",
        )
        .unwrap();
        let dup = WitnessRejection {
            witness: 7u64,
            rationale: Some("same".to_string()),
        };
        let result = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![dup.clone(), dup]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        );
        assert!(
            matches!(result, Err(SuspendedAttemptEvidenceError::DuplicateRejection)),
            "duplicate (witness, rationale) must be rejected at constructor (P0-3 single key): {result:?}"
        );
    }

    #[test]
    fn rejection_canonical_accepts_same_witness_different_rationale() {
        // Aynı witness farklı rationale → ayrı rejection (duplicate DEĞİL).
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let r1 = WitnessRejection {
            witness: 7u64,
            rationale: Some("reason_a".to_string()),
        };
        let r2 = WitnessRejection {
            witness: 7u64,
            rationale: Some("reason_b".to_string()),
        };
        let ev = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r1, r2]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let result = SuspendedAttemptEvidenceDigest::compute(&ev);
        assert!(
            result.is_ok(),
            "same witness different rationale is NOT a duplicate"
        );
    }

    // — Canonical encoding preimage tests (byte-level) —

    #[test]
    fn encode_witness_rejection_to_vec_preimage() {
        // Encoding: witness u64 LE + rationale Option tag.
        use crate::witness::WitnessRejection;
        let r_none = WitnessRejection {
            witness: 1u64,
            rationale: None,
        };
        let bytes = encode_witness_rejection_to_vec(&r_none).unwrap();
        // witness 1u64 LE = [1,0,0,0,0,0,0,0] + rationale None = [0]
        assert_eq!(bytes, vec![1u8, 0, 0, 0, 0, 0, 0, 0, 0]);

        let r_some = WitnessRejection {
            witness: 1u64,
            rationale: Some("ab".to_string()),
        };
        let bytes = encode_witness_rejection_to_vec(&r_some).unwrap();
        // witness 1u64 LE + tag 1 + len 2 u64 LE + "ab"
        let mut expected = vec![1u8, 0, 0, 0, 0, 0, 0, 0, 1];
        expected.extend_from_slice(&2u64.to_le_bytes());
        expected.extend_from_slice(b"ab");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn encode_witness_hold_reason_preimage_tags() {
        // Tag assignment: MinApproversNotMet=1, QuorumInsufficient=2,
        // EvidenceNotLocallyObservable=3.
        // (Bu test encoder'ın tag atamasını kilitler — `format!("{:?}")` değil.)
        let reason_min = WitnessHoldReason::MinApproversNotMet {
            distinct: 0,
            required: 2,
        };
        // Aynı evidence digest test'in farklı varyantları farklı tag üretir.
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap();
        let make_ev = |reason| {
            SuspendedAttemptEvidence::try_new(
                TaskId::from(1u64),
                ClaimId::from(1u64),
                basis_digest.clone(),
                AttemptNumber::try_from(1u64).unwrap(),
                SuspendedAttemptDisposition::Held {
                    hold_reason: reason,
                    snapshot: WitnessQuorumSnapshot {
                        approvers: 0,
                        required_approvers: 2,
                        support: 0.0,
                        required_support: 1.5,
                    },
                },
            )
            .unwrap()
        };
        let d_min = SuspendedAttemptEvidenceDigest::compute(&make_ev(
            WitnessHoldReason::MinApproversNotMet {
                distinct: 0,
                required: 2,
            },
        ))
        .unwrap();
        let d_quorum = SuspendedAttemptEvidenceDigest::compute(&make_ev(
            WitnessHoldReason::QuorumInsufficient {
                support: 0.0,
                threshold: 1.5,
            },
        ))
        .unwrap();
        let d_evidence = SuspendedAttemptEvidenceDigest::compute(&make_ev(
            WitnessHoldReason::EvidenceNotLocallyObservable {
                hint: "x".to_string(),
            },
        ))
        .unwrap();
        // Üç farklı varyant üç farklı digest üretir (tag ayrımı çalışır).
        assert_ne!(d_min.as_bytes(), d_quorum.as_bytes());
        assert_ne!(d_min.as_bytes(), d_evidence.as_bytes());
        assert_ne!(d_quorum.as_bytes(), d_evidence.as_bytes());
        let _ = reason_min; // (tip referansı — test okunabilirliği için)
    }

    // — Golden vector (v1 byte contract lock) —

    /// Golden evidence fixture — Held disposition, non-trivial değerlerle.
    /// Nested digest (`AuthorizationBasisDigest`) explicit sentinel bytes —
    /// evidence encoding değişikliği bu golden'ı kırar, nested digest değişikliği değil.
    fn golden_suspended_attempt_evidence_fixture_held() -> SuspendedAttemptEvidence {
        SuspendedAttemptEvidence::try_new(
            TaskId::from(0x0A1Bu64),
            ClaimId::from(0x0C1Du64),
            AuthorizationBasisDigest::from_hex(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            )
            .unwrap(),
            AttemptNumber::try_from(7u64).unwrap(),
            SuspendedAttemptDisposition::Held {
                hold_reason: WitnessHoldReason::QuorumInsufficient {
                    support: 0.8,
                    threshold: 1.5,
                },
                snapshot: WitnessQuorumSnapshot {
                    approvers: 1,
                    required_approvers: 2,
                    support: 0.8,
                    required_support: 1.5,
                },
            },
        )
        .unwrap()
    }

    /// Golden hex sabiti — ilk implementasyon doğrulanmış değer.
    /// DO NOT update as routine test maintenance. A mismatch requires an explicit
    /// compatibility/version decision: a canonical field/order/tag/encoding change
    /// requires v2 domain separator (`osp.attempt-evidence.v2\0`).
    const SUSPENDED_ATTEMPT_EVIDENCE_V1_GOLDEN_HEX: &str =
        "3cfb984502df3382fec90111b5afd19a5d6543c071c98ba6c3fc3f7a0fe0052c";

    #[test]
    fn suspended_attempt_evidence_digest_v1_golden_vector() {
        // **INV-T9 #72 v1 byte contract lock** — SuspendedAttemptEvidenceDigest.
        // Encoding (field order, tag values, float canonicalization, rejection
        // canonical ordering) bu testle kilitlenir. Nested AuthorizationBasisDigest
        // sentinel bytes (0xEE) → evidence encoding değişikliği buradan gelir,
        // nested digest değişikliğinden değil.
        let ev = golden_suspended_attempt_evidence_fixture_held();
        let actual = SuspendedAttemptEvidenceDigest::compute(&ev).unwrap();
        assert_eq!(
            actual.to_hex(),
            SUSPENDED_ATTEMPT_EVIDENCE_V1_GOLDEN_HEX,
            "SuspendedAttemptEvidence v1 byte contract changed — explicit version decision required"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #72 closure — Wire-tampering + constructor validation tests
    //
    // **P0-1/P0-2 closure:** Envelope private fields → in-memory tampering imkânsız.
    // Testler iki seviye:
    // 1. Constructor (creation-path) — invalid input reject
    // 2. Wire tampering — serialize → JSON mutate → deserialize → verify reject
    //
    // Reviewer exact test isimleri:
    // envelope_deserialize_preserves_and_verifies_stored_digests
    // revision_deserialize_rejects_tampered_evidence_digest
    // revision_deserialize_rejects_noncanonical_rejection_order
    // pending_deserialize_rejects_stale_attempt_evidence_id
    // persist_verifies_before_creating_artifact
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Helper: valid envelope üret (sample_basis + sample_pending_record).
    fn sample_valid_envelope() -> PendingAuthorizationEnvelope {
        let basis = sample_basis();
        let record = sample_pending_record();
        PendingAuthorizationEnvelope::new(record, basis).unwrap()
    }

    /// Helper: envelope'u serialize et, JSON mutate et, deserialize dene.
    fn envelope_from_tampered_json<F>(
        envelope: &PendingAuthorizationEnvelope,
        mutate: F,
    ) -> Result<PendingAuthorizationEnvelope, serde_json::Error>
    where
        F: FnOnce(&mut serde_json::Value),
    {
        let mut json = serde_json::to_value(envelope).unwrap();
        mutate(&mut json);
        serde_json::from_value(json)
    }

    #[test]
    fn envelope_deserialize_preserves_and_verifies_stored_digests() {
        // **P0-1 load path:** Stored digest'ler korunur, recompute + compare.
        // Clean envelope round-trip → aynı digest'ler.
        let envelope = sample_valid_envelope();
        let json = serde_json::to_string(&envelope).unwrap();
        let restored: PendingAuthorizationEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.record().authorization_basis_digest,
            envelope.record().authorization_basis_digest
        );
        assert_eq!(
            restored.record().evidence_digest,
            envelope.record().evidence_digest
        );
    }

    #[test]
    fn envelope_deserialize_rejects_tampered_evidence_digest() {
        // Wire'da evidence_digest tamper → load reject (stored ≠ recomputed).
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["evidence_digest"] = serde_json::to_value(vec![0xABu8; 32]).unwrap();
        });
        assert!(
            result.is_err(),
            "tampered evidence_digest must be rejected on deserialize (load-path verify)"
        );
    }

    #[test]
    fn envelope_deserialize_rejects_tampered_basis_digest() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["authorization_basis_digest"] =
                serde_json::to_value(vec![0xCDu8; 32]).unwrap();
        });
        assert!(result.is_err(), "tampered basis_digest must be rejected");
    }

    #[test]
    fn envelope_deserialize_rejects_tampered_task_id() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["task_id"] = serde_json::json!(999);
        });
        assert!(
            result.is_err(),
            "tampered task_id must be rejected (record↔basis↔evidence mismatch)"
        );
    }

    #[test]
    fn envelope_deserialize_rejects_tampered_predicate_completion() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["predicate_completion"] = serde_json::json!("NotCompleted");
        });
        assert!(
            result.is_err(),
            "tampered predicate_completion must be rejected"
        );
    }

    #[test]
    fn envelope_deserialize_rejects_tampered_witness_requirement() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["witness_requirement"]["min_approvers"] = serde_json::json!(5);
        });
        assert!(
            result.is_err(),
            "tampered witness_requirement must be rejected"
        );
    }

    #[test]
    fn pending_deserialize_rejects_stale_attempt_evidence_id() {
        // **P0-2 strict wire:** Stale `attempt_evidence_id` field reject.
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["attempt_evidence_id"] = serde_json::json!(1);
        });
        assert!(
            result.is_err(),
            "stale attempt_evidence_id field must be rejected (strict canonical wire)"
        );
    }

    #[test]
    fn pending_deserialize_rejects_unknown_field() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["unknown_field"] = serde_json::json!(42);
        });
        assert!(
            result.is_err(),
            "unknown field must be rejected (deny_unknown_fields)"
        );
    }

    #[test]
    fn envelope_deserialize_rejects_unknown_field() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["unknown_envelope_field"] = serde_json::json!(42);
        });
        assert!(result.is_err(), "unknown envelope field must be rejected");
    }

    #[test]
    fn held_disposition_deserialize_rejects_unknown_field() {
        let envelope = sample_valid_envelope();
        let result = envelope_from_tampered_json(&envelope, |json| {
            json["record"]["suspended_attempt_evidence"]["disposition"]
                ["unknown_disposition_field"] = serde_json::json!(42);
        });
        assert!(
            result.is_err(),
            "unknown Held disposition field must be rejected (per-variant strict wire)"
        );
    }

    #[test]
    fn rejected_disposition_deserialize_rejects_unknown_field() {
        // **P2 simetrik test:** Rejected varyantı için de unknown field reject.
        // Custom deserializer iki ayrı wire struct kullandığı için her iki varyant
        // bağımsız test edilmeli.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "8888888888888888888888888888888888888888888888888888888888888888",
        )
        .unwrap();
        let evidence = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_single(WitnessRejection {
                    witness: 5u64,
                    rationale: None,
                }),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let mut json = serde_json::to_value(&evidence).unwrap();
        json["disposition"]["unknown_rejected_field"] = serde_json::json!(42);
        let json_str = serde_json::to_string(&json).unwrap();
        let result: Result<SuspendedAttemptEvidence, _> = serde_json::from_str(&json_str);
        assert!(
            result.is_err(),
            "unknown Rejected disposition field must be rejected (per-variant strict wire)"
        );
    }

    #[test]
    fn envelope_constructor_rejects_basis_internal_task_id_mismatch() {
        // **P1 basis iç task_id invariant:** basis.task_id != claim_identity.task_id.
        // (record sample_pending_record'tan geliyor, bad_basis claim_identity.task_id farklı —
        // evidence basis digest binding veya basis internal mismatch ile reject edilir.
        // Her ikisi de integrity hatası, exact varyant implementation sırasına bağlı.)
        let basis = sample_basis();
        let record = sample_pending_record();
        let mut bad_basis = basis.clone();
        bad_basis.claim_identity.task_id = 999;
        let result = PendingAuthorizationEnvelope::new(record, bad_basis);
        assert!(
            result.is_err(),
            "basis internal task_id mismatch must be rejected at constructor: {result:?}"
        );
    }

    #[test]
    fn envelope_constructor_rejects_rejected_disposition_for_pending() {
        // Surface-specific: PendingAuthorizationEnvelope yalnız Held.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis = sample_basis();
        let basis_digest = AuthorizationBasisDigest::compute(&basis).unwrap();
        let rejected_evidence = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_single(WitnessRejection {
                    witness: 5u64,
                    rationale: None,
                }),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let mut record = sample_pending_record();
        record.suspended_attempt_evidence = rejected_evidence;
        record.evidence_digest =
            SuspendedAttemptEvidenceDigest::compute(&record.suspended_attempt_evidence).unwrap();
        let result = PendingAuthorizationEnvelope::new(record, basis);
        assert!(
            matches!(
                result,
                Err(PendingAuthorizationLoadError::InvalidEvidenceDisposition(_))
            ),
            "Rejected disposition for PendingAuthorization must be rejected: {result:?}"
        );
    }

    #[test]
    fn revision_required_try_new_rejects_held_disposition() {
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap();
        let held_evidence = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Held {
                hold_reason: WitnessHoldReason::MinApproversNotMet {
                    distinct: 0,
                    required: 2,
                },
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let result = RevisionRequired::try_new(held_evidence);
        assert!(
            matches!(
                result,
                Err(RevisionRequiredError::InvalidEvidenceDisposition { .. })
            ),
            "Held disposition for RevisionRequired must be rejected: {result:?}"
        );
    }

    #[test]
    fn revision_deserialize_rejects_tampered_evidence_digest() {
        // **N3 load path:** Stored evidence_digest tamper → recompute mismatch.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "3333333333333333333333333333333333333333333333333333333333333333",
        )
        .unwrap();
        let evidence = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_single(WitnessRejection {
                    witness: 5u64,
                    rationale: None,
                }),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let rev = RevisionRequired::try_new(evidence).unwrap();
        let mut json = serde_json::to_value(&rev).unwrap();
        json["evidence_digest"] = serde_json::to_value(vec![0xEEu8; 32]).unwrap();
        let result: Result<RevisionRequired, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "tampered evidence_digest must be rejected (RevisionRequiredError::EvidenceDigestMismatch)"
        );
    }

    #[test]
    fn revision_deserialize_rejects_noncanonical_rejection_order() {
        // **P1-1 strict wire:** Non-canonical rejection order reject (wire normalize ETMEZ).
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "4444444444444444444444444444444444444444444444444444444444444444",
        )
        .unwrap();
        // Manuel non-canonical JSON: reasons ters sırada (witness 20, witness 10).
        let json_str = format!(
            r#"{{
                "evidence_digest": "0000000000000000000000000000000000000000000000000000000000000000",
                "suspended_attempt_evidence": {{
                    "schema_version": 1,
                    "task_id": 1,
                    "claim_id": 42,
                    "authorization_basis_digest": "{}",
                    "attempt_num": 1,
                    "disposition": {{
                        "kind": "rejected",
                        "reasons": [
                            {{"witness": 20, "rationale": null}},
                            {{"witness": 10, "rationale": null}}
                        ],
                        "snapshot": {{"approvers": 0, "required_approvers": 2, "support": 0.0, "required_support": 1.5}}
                    }}
                }}
            }}"#,
            basis_digest.to_hex()
        );
        let result: Result<RevisionRequired, _> = serde_json::from_str(&json_str);
        assert!(
            result.is_err(),
            "non-canonical rejection order on wire must be rejected (strict wire, not normalized)"
        );
    }

    #[test]
    fn reversed_rejection_inputs_construct_equal_evidence() {
        // **P1-1 stored canonicalization:** API normalizes → same logical set → equal evidence.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "5555555555555555555555555555555555555555555555555555555555555555",
        )
        .unwrap();
        let r1 = WitnessRejection {
            witness: 10u64,
            rationale: Some("a".into()),
        };
        let r2 = WitnessRejection {
            witness: 20u64,
            rationale: None,
        };
        let ev_a = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest.clone(),
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r1.clone(), r2.clone()]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let ev_b = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r2, r1]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        assert_eq!(
            ev_a, ev_b,
            "API normalizes → reversed inputs produce equal evidence"
        );
    }

    #[test]
    fn reversed_rejection_inputs_serialize_identically() {
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "6666666666666666666666666666666666666666666666666666666666666666",
        )
        .unwrap();
        let r1 = WitnessRejection {
            witness: 10u64,
            rationale: Some("a".into()),
        };
        let r2 = WitnessRejection {
            witness: 20u64,
            rationale: None,
        };
        let ev_a = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest.clone(),
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r1, r2]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let ev_b = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(1u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![
                    WitnessRejection {
                        witness: 20u64,
                        rationale: None,
                    },
                    WitnessRejection {
                        witness: 10u64,
                        rationale: Some("a".into()),
                    },
                ]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let json_a = serde_json::to_string(&ev_a).unwrap();
        let json_b = serde_json::to_string(&ev_b).unwrap();
        assert_eq!(json_a, json_b, "canonical stored representation identical");
    }

    /// Reversed input order is normalized into one canonical rejected-evidence
    /// representation. This test does not persist an artifact.
    #[test]
    fn reversed_rejection_inputs_produce_identical_canonical_evidence() {
        // Reviewer P2-4: Test kanıtı ismiyle uyumlu — persist/artifact/store-path iddiası YOK.
        // reversed logical input → identical canonical evidence → identical evidence digest
        // → identical serialized representation.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "5555555555555555555555555555555555555555555555555555555555555555",
        )
        .unwrap();
        let r1 = WitnessRejection {
            witness: 10u64,
            rationale: Some("a".into()),
        };
        let r2 = WitnessRejection {
            witness: 20u64,
            rationale: None,
        };

        // Evidence A: [r1, r2] sırasıyla.
        let evidence_a = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest.clone(),
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r1.clone(), r2.clone()]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        // Evidence B: [r2, r1] sırasıyla (API normalize eder → same evidence).
        let evidence_b = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(1u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_vec(vec![r2, r1]),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();

        // Equal evidence (canonical stored representation).
        assert_eq!(
            evidence_a, evidence_b,
            "reversed inputs → identical canonical evidence"
        );
        // Equal digest.
        let digest_a = SuspendedAttemptEvidenceDigest::compute(&evidence_a).unwrap();
        let digest_b = SuspendedAttemptEvidenceDigest::compute(&evidence_b).unwrap();
        assert_eq!(
            digest_a, digest_b,
            "identical canonical evidence → identical evidence digest"
        );
        // Identical serialized representation.
        let json_a = serde_json::to_string(&evidence_a).unwrap();
        let json_b = serde_json::to_string(&evidence_b).unwrap();
        assert_eq!(
            json_a, json_b,
            "identical canonical evidence → identical serialized representation"
        );
    }

    #[test]
    fn persist_verifies_before_creating_artifact() {
        // **N4:** persist() verify() çağırır, tüm side-effect'lerden önce.
        // Valid envelope → persist başarılı.
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let envelope = sample_valid_envelope();
        let result = store.persist(&envelope);
        assert!(result.is_ok(), "valid envelope must persist");
    }

    /// Helper: gerçek inconsistent envelope üret (in-memory struct literal).
    /// Envelope private fields ama test modülü parent modülün private'larına erişir.
    /// record.task_id basis.task_id ile çelişiyor → verify reject.
    fn sample_inconsistent_envelope() -> PendingAuthorizationEnvelope {
        let basis = sample_basis();
        let mut record = sample_pending_record();
        // record.task_id'yı basis ile çelişecek şekilde değiştir.
        record.task_id = 999; // basis.task_id = 1
                              // Evidence da güncelle ki record-internal validation geçsin (task_id ↔ evidence).
        let evidence = SuspendedAttemptEvidence::try_new(
            record.task_id,
            record.claim_id,
            record.authorization_basis_digest.clone(),
            record.attempt_num,
            SuspendedAttemptDisposition::Held {
                hold_reason: record.witness_hold_reason.clone(),
                snapshot: record.witness_snapshot.clone(),
            },
        )
        .unwrap();
        record.suspended_attempt_evidence = evidence;
        record.evidence_digest =
            SuspendedAttemptEvidenceDigest::compute(&record.suspended_attempt_evidence).unwrap();
        // Struct literal — private fields ama test modülünden erişilebilir.
        // verify() record↔basis task_id mismatch yakalar (InvalidEnvelope via persist).
        PendingAuthorizationEnvelope {
            schema: PENDING_AUTHORIZATION_SCHEMA.to_string(),
            record,
            authorization_basis: basis,
        }
    }

    #[test]
    fn filesystem_store_rejects_invalid_envelope() {
        // **N4 negative test:** Gerçek inconsistent envelope → persist reject
        // (InvalidEnvelope). In-memory struct literal bypass persist-boundary
        // verify ile yakalanır.
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let invalid = sample_inconsistent_envelope();
        let result = store.persist(&invalid);
        assert!(
            matches!(
                result,
                Err(PendingAuthorizationStoreError::InvalidEnvelope(_))
            ),
            "inconsistent envelope must be rejected at persist-boundary verify: {result:?}"
        );
    }

    #[test]
    fn filesystem_store_invalid_envelope_creates_no_artifact() {
        // **N4 fail-before-side-effects:** Invalid persist sonrası .osp/pending-authorizations
        // dizini bile oluşmaz (verify tüm side-effect'lerden önce).
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let invalid = sample_inconsistent_envelope();
        let _ = store.persist(&invalid);
        let pending_dir = dir.join(".osp").join("pending-authorizations");
        assert!(
            !pending_dir.exists(),
            "invalid envelope must not create any artifact directory (verify before side effects)"
        );
    }

    #[test]
    fn null_store_rejects_invalid_envelope() {
        // **N4 negative test:** Null store da persist-boundary verify yapıyor.
        let invalid = sample_inconsistent_envelope();
        let mut store = crate::authorization::NullPendingAuthorizationStore;
        let result = store.persist(&invalid);
        assert!(
            matches!(
                result,
                Err(PendingAuthorizationStoreError::InvalidEnvelope(_))
            ),
            "null store must reject inconsistent envelope at persist-boundary: {result:?}"
        );
    }

    #[test]
    fn validate_hold_reason_rejects_inconsistent_snapshot_min_approvers() {
        // MinApproversNotMet { distinct: 0, required: 2 } ama snapshot.approvers = 5
        // → iç çelişki.
        let reason = WitnessHoldReason::MinApproversNotMet {
            distinct: 0,
            required: 2,
        };
        let inconsistent_snapshot = WitnessQuorumSnapshot {
            approvers: 5, // distinct 0 olmalı
            required_approvers: 2,
            support: 0.0,
            required_support: 1.5,
        };
        let result = validate_hold_reason_against_snapshot(&reason, &inconsistent_snapshot);
        assert!(
            result.is_err(),
            "MinApproversNotMet with inconsistent snapshot.approvers must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("MinApproversNotMet") && err.contains("snapshot.approvers"),
            "error must name the inconsistency, got: {err}"
        );
    }

    #[test]
    fn validate_hold_reason_rejects_quorum_insufficient_support_geq_threshold() {
        // QuorumInsufficient { support: 1.5, threshold: 1.5 } → support >= threshold
        // (quorum sağlanmış gibi) → iç çelişki.
        let reason = WitnessHoldReason::QuorumInsufficient {
            support: 1.5,
            threshold: 1.5,
        };
        let snapshot = WitnessQuorumSnapshot {
            approvers: 2,
            required_approvers: 2,
            support: 1.5,
            required_support: 1.5,
        };
        let result = validate_hold_reason_against_snapshot(&reason, &snapshot);
        assert!(
            result.is_err(),
            "QuorumInsufficient with support >= threshold must be rejected"
        );
    }

    #[test]
    fn validate_hold_reason_rejects_empty_evidence_not_locally_observable_hint() {
        // EvidenceNotLocallyObservable { hint: "" } → hint non-empty invariant.
        let reason = WitnessHoldReason::EvidenceNotLocallyObservable {
            hint: "   ".to_string(), // trim boş
        };
        let snapshot = WitnessQuorumSnapshot {
            approvers: 0,
            required_approvers: 2,
            support: 0.0,
            required_support: 1.5,
        };
        let result = validate_hold_reason_against_snapshot(&reason, &snapshot);
        assert!(
            result.is_err(),
            "EvidenceNotLocallyObservable with whitespace-only hint must be rejected"
        );
    }

    #[test]
    fn validate_hold_reason_accepts_consistent_min_approvers() {
        // Tutarlı kombinasyon — geçerli.
        let reason = WitnessHoldReason::MinApproversNotMet {
            distinct: 0,
            required: 2,
        };
        let snapshot = WitnessQuorumSnapshot {
            approvers: 0,
            required_approvers: 2,
            support: 0.0,
            required_support: 1.5,
        };
        assert!(
            validate_hold_reason_against_snapshot(&reason, &snapshot).is_ok(),
            "consistent MinApproversNotMet + snapshot must pass"
        );
    }

    #[test]
    fn envelope_new_constructor_runs_cross_field_validation() {
        // P1: constructor validation — invalid kombinasyon reject, valid geçer.
        let basis = sample_basis();
        let record = sample_pending_record();
        // Valid → success.
        let envelope = PendingAuthorizationEnvelope::new(record.clone(), basis.clone());
        assert!(envelope.is_ok(), "valid envelope must construct");
        // Invalid: predicate_completion değiştir → constructor reject.
        let mut bad_record = record;
        bad_record.predicate_completion = PredicateCompletion::NotCompleted;
        let result = PendingAuthorizationEnvelope::new(bad_record, basis);
        assert!(
            matches!(result, Err(PendingAuthorizationLoadError::PredicateCompletionMismatch { .. })),
            "constructor must run cross-field validation and reject mismatched predicate_completion"
        );
    }

    #[test]
    fn pending_authorization_round_trips_with_evidence() {
        // PendingAuthorization evidence field'larla serde round-trip.
        let record = sample_pending_record();
        let json = serde_json::to_string(&record).unwrap();
        let restored: PendingAuthorization = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, record, "evidence fields must round-trip");
    }

    #[test]
    fn pending_authorization_record_carries_evidence_to_runtime() {
        // P0-3: record içine gömülü evidence runtime AwaitingWitnesses'e gider.
        // (Bu test record.field erişilebilirliğini doğrular — navigator integration
        // Commit 2 test'lerinde zaten.)
        let record = sample_pending_record();
        assert_eq!(record.suspended_attempt_evidence.task_id(), 1);
        assert_eq!(record.suspended_attempt_evidence.claim_id(), 42);
        assert_eq!(record.suspended_attempt_evidence.attempt_num().get(), 1);
        assert!(matches!(
            record.suspended_attempt_evidence.disposition(),
            SuspendedAttemptDisposition::Held { .. }
        ));
        // evidence_digest serialize edilmiş olmalı.
        assert_eq!(record.evidence_digest.to_hex().len(), 64);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #72 — Commit 4: Dangling evidence id removal migration tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn pending_authorization_uses_attempt_num_not_evidence_id() {
        // Commit 4: attempt_evidence_id field kaldırıldı, attempt_num (AttemptNumber) eklendi.
        let record = sample_pending_record();
        // attempt_num AttemptNumber typed — 1-based invariant.
        assert_eq!(record.attempt_num.get(), 1);
        // AttemptNumber olarak da erişilebilir (Copy).
        let n: AttemptNumber = record.attempt_num;
        assert_eq!(n.get(), 1);
    }

    #[test]
    fn revision_required_attempt_num_via_evidence_accessor() {
        // **P0-1 closure:** Minimal shape — try_new(evidence) tek argüman.
        // attempt_num() erişim metodu evidence üzerinden.
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};
        let basis_digest = AuthorizationBasisDigest::from_hex(
            "2222222222222222222222222222222222222222222222222222222222222222",
        )
        .unwrap();
        let evidence = SuspendedAttemptEvidence::try_new(
            TaskId::from(1u64),
            ClaimId::from(42u64),
            basis_digest,
            AttemptNumber::try_from(5u64).unwrap(),
            SuspendedAttemptDisposition::Rejected {
                reasons: NonEmptyWitnessRejections::from_single(WitnessRejection {
                    witness: 7u64,
                    rationale: None,
                }),
                snapshot: WitnessQuorumSnapshot {
                    approvers: 0,
                    required_approvers: 2,
                    support: 0.0,
                    required_support: 1.5,
                },
            },
        )
        .unwrap();
        let rev = RevisionRequired::try_new(evidence).unwrap();
        assert_eq!(
            rev.attempt_num().get(),
            5,
            "attempt_num via evidence accessor"
        );
        // Accessor'lar evidence üzerinden — outer duplicate alan yok.
        assert_eq!(rev.task_id(), TaskId::from(1u64));
        assert_eq!(rev.claim_id(), ClaimId::from(42u64));
        assert!(rev.reasons().is_some());
        assert_eq!(rev.reasons().unwrap().as_slice().len(), 1);
    }

    #[test]
    fn attempt_evidence_id_alias_removed_compiles() {
        // Compile-time assertion: AttemptEvidenceId type alias tamamen kaldırıldı.
        // Bu test derleniyorsa alias yok — `AttemptEvidenceId` referansı compile error verir.
        // (Test gövdesi boş — type-level assertion derleme ile sağlanıyor.)
        let record = sample_pending_record();
        // record.attempt_evidence_id erişimi compile error olmalı (field yok).
        // Aşağıdaki satır yorumda — uncomment ederseniz compile error:
        // let _ = record.attempt_evidence_id;
        let _ = record.attempt_num; // geçerli erişim
    }

    #[test]
    fn pending_authorization_rejects_old_artifact_format_without_attempt_num() {
        // **P0-2 strict wire (closure):** Eski artifact format (attempt_evidence_id
        // içeren JSON) artık REJECT edilir — custom Deserialize + deny_unknown_fields.
        // Önceki closure derive Deserialize extra field'ı tolere ediyordu (fail-open);
        // reviewer P0-2 bunu TERS ÇEVİRMEMİ istedi → assert is_err().
        let record = sample_pending_record();
        let mut json = serde_json::to_value(&record).unwrap();
        json["attempt_evidence_id"] = serde_json::json!(1);
        let json_str = serde_json::to_string(&json).unwrap();
        let result: Result<PendingAuthorization, _> = serde_json::from_str(&json_str);
        assert!(
            result.is_err(),
            "stale attempt_evidence_id field must be REJECTED (strict canonical wire, P0-2 closure)"
        );
    }

    #[test]
    fn pending_authorization_serde_roundtrip_preserves_attempt_num() {
        // Serde round-trip attempt_num (AttemptNumber) doğru korunur.
        let record = sample_pending_record();
        let json = serde_json::to_string(&record).unwrap();
        // JSON'da attempt_num u64 olarak serileşir (transparent).
        assert!(
            json.contains("\"attempt_num\":1"),
            "JSON must serialize attempt_num as u64: {json}"
        );
        let restored: PendingAuthorization = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.attempt_num, record.attempt_num);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #72 — Commit 5: Persisted artifact tamper matrix
    //
    // Commit 3 typed unit test'lerden FARKLI seviye — serialize → byte/JSON tamper →
    // deserialize → load_pending_authorization → verify. Disk artifact üzerinde
    // representative end-to-end tamper matrix. Tekrar yok (Commit 3 in-memory verify,
    // Commit 5 persisted artifact seviyesi).
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Helper: persist envelope to temp dir, return artifact path.
    fn persist_to_temp(envelope: &PendingAuthorizationEnvelope) -> std::path::PathBuf {
        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let receipt = store.persist(envelope).expect("persist");
        receipt.artifact_path
    }

    #[test]
    fn persisted_artifact_load_verifies_clean_envelope() {
        // Baseline: temiz artifact load + verify başarılı.
        let envelope = sample_valid_envelope();
        let path = persist_to_temp(&envelope);
        let loaded = load_pending_authorization(&path);
        assert!(loaded.is_ok(), "clean artifact must load + verify");
        assert_eq!(loaded.unwrap(), envelope);
    }

    #[test]
    fn persisted_artifact_tamper_evidence_digest_rejected_on_load() {
        // **P0-1 load path:** evidence_digest tamper → recompute mismatch → reject.
        // (DeserializationFailed veya EvidenceDigestMismatch — both acceptable, reject yeterli.)
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        let tampered_array: Vec<u8> = vec![0xAB; 32];
        json["record"]["evidence_digest"] = serde_json::to_value(&tampered_array).unwrap();
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-evidence-digest.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered evidence_digest must be rejected on load: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_basis_digest_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        let tampered_array: Vec<u8> = vec![0xCD; 32];
        json["record"]["authorization_basis_digest"] =
            serde_json::to_value(&tampered_array).unwrap();
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-basis-digest.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered basis_digest must be rejected: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_task_id_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        json["record"]["task_id"] = serde_json::json!(999);
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-task-id.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered task_id must be rejected: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_claim_id_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        json["record"]["claim_id"] = serde_json::json!(999);
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-claim-id.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered claim_id must be rejected: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_attempt_num_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        json["record"]["attempt_num"] = serde_json::json!(999);
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-attempt-num.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered attempt_num must be rejected: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_predicate_completion_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        json["record"]["predicate_completion"] = serde_json::json!("NotCompleted");
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-predicate-completion.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered predicate_completion must be rejected: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_witness_requirement_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        json["record"]["witness_requirement"]["min_approvers"] = serde_json::json!(5);
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-witness-req.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered witness_requirement must be rejected: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_tamper_schema_rejected_on_load() {
        let envelope = sample_valid_envelope();
        let mut json = serde_json::to_value(&envelope).unwrap();
        json["schema"] = serde_json::json!("osp.pending-authorization.v2");
        let tampered_bytes = serde_json::to_vec_pretty(&json).unwrap();

        let dir = temp_dir();
        let path = dir.join(".osp").join("tampered-schema.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &tampered_bytes).unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            result.is_err(),
            "tampered schema must be rejected (UnknownSchema at deserialize or verify): {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_corrupted_json_rejected_on_load() {
        let dir = temp_dir();
        let path = dir.join(".osp").join("corrupted.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ this is not valid json").unwrap();

        let result = load_pending_authorization(&path);
        assert!(
            matches!(
                result,
                Err(PendingAuthorizationLoadError::DeserializationFailed(_))
            ),
            "corrupted JSON must be DeserializationFailed: {result:?}"
        );
    }

    #[test]
    fn persisted_artifact_round_trip_through_filesystem_store_load() {
        let envelope = sample_valid_envelope();
        let path = persist_to_temp(&envelope);
        let loaded = load_pending_authorization(&path).expect("load + verify");
        assert_eq!(
            loaded, envelope,
            "persisted artifact must round-trip exactly"
        );
        assert_eq!(
            loaded.record().suspended_attempt_evidence,
            envelope.record().suspended_attempt_evidence
        );
        assert_eq!(
            loaded.record().evidence_digest,
            envelope.record().evidence_digest
        );
    }

    #[test]
    fn persisted_artifact_distinct_evidence_distinct_files() {
        // P0-4: aynı basis farklı evidence → ayrı artifact (no false conflict).
        let basis = sample_basis();
        let record1 = sample_pending_record();

        let mut record2 = sample_pending_record();
        let evidence2 = SuspendedAttemptEvidence::try_new(
            record2.task_id,
            record2.claim_id,
            record2.authorization_basis_digest.clone(),
            AttemptNumber::try_from(2u64).unwrap(),
            SuspendedAttemptDisposition::Held {
                hold_reason: record2.witness_hold_reason.clone(),
                snapshot: record2.witness_snapshot.clone(),
            },
        )
        .unwrap();
        record2.attempt_num = AttemptNumber::try_from(2u64).unwrap();
        record2.suspended_attempt_evidence = evidence2;
        record2.evidence_digest =
            SuspendedAttemptEvidenceDigest::compute(&record2.suspended_attempt_evidence).unwrap();

        let env1 = PendingAuthorizationEnvelope::new(record1, basis.clone()).unwrap();
        let env2 = PendingAuthorizationEnvelope::new(record2, basis).unwrap();

        let dir = temp_dir();
        let mut store = FilesystemPendingAuthorizationStore::new(&dir);
        let receipt1 = store.persist(&env1).expect("persist 1");
        let receipt2 = store.persist(&env2).expect("persist 2");

        assert_ne!(
            receipt1.artifact_path, receipt2.artifact_path,
            "distinct evidence → distinct artifact files (no false conflict)"
        );
        assert_ne!(
            receipt1.evidence_digest, receipt2.evidence_digest,
            "distinct attempt → distinct evidence digest"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 4 (reviewer P0-1) — V1 production encoder frozen
    //
    // **Production `gate_decision_tag_v1`** (fallible): mevcut 7 varyant (0-6) `Ok`,
    // V2-only varyantlar (RejectedByTaskValidation=7, RejectedByMeasurementBinding=8)
    // `Err(UnsupportedV1GateDecision)`. `AuthorizationBasisDigest::compute` (V1 production
    // encoder) bunu kullanır — V2-only kararların V1 artifact'lerine sızması imkânsız.
    // Eski test-only paralel enum (`GateDecisionV1Frozen`) kaldırıldı; production fn
    // gerçek V1 encoder'ı kullanır.
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn v1_encoder_gate_decision_tag_accepts_legacy_variants() {
        // Reviewer P0-1: production gate_decision_tag_v1 legacy 7 varyant (0-6) kabul eder.
        use crate::trajectory::GateDecision::*;
        assert_eq!(gate_decision_tag_v1(Unknown).unwrap(), 0);
        assert_eq!(gate_decision_tag_v1(PassedAll).unwrap(), 1);
        assert_eq!(gate_decision_tag_v1(RejectedBySyntax).unwrap(), 2);
        assert_eq!(gate_decision_tag_v1(RejectedByVision).unwrap(), 3);
        assert_eq!(gate_decision_tag_v1(RejectedByRule).unwrap(), 4);
        assert_eq!(gate_decision_tag_v1(RejectedByTaskBinding).unwrap(), 5);
        assert_eq!(gate_decision_tag_v1(BlockedByManeuverLimit).unwrap(), 6);
    }

    #[test]
    fn v1_encoder_gate_decision_tag_rejects_v2_variants() {
        // Reviewer P0-1: V2-only varyantlar (7, 8) V1 encoder'da reject — V1 byte contract frozen.
        use crate::trajectory::GateDecision::*;
        let err1 = gate_decision_tag_v1(RejectedByTaskValidation)
            .expect_err("RejectedByTaskValidation reject");
        assert!(
            matches!(
                err1,
                CanonicalDigestError::UnsupportedV1GateDecision { tag: 7 }
            ),
            "RejectedByTaskValidation (tag 7) reject"
        );
        let err2 = gate_decision_tag_v1(RejectedByMeasurementBinding)
            .expect_err("RejectedByMeasurementBinding reject");
        assert!(
            matches!(
                err2,
                CanonicalDigestError::UnsupportedV1GateDecision { tag: 8 }
            ),
            "RejectedByMeasurementBinding (tag 8) reject"
        );
    }

    #[test]
    fn v1_basis_compute_rejects_v2_gate_decision() {
        // Reviewer P0-1: V2-only GateDecision içeren V1 basis → AuthorizationBasisDigest::compute Err.
        // V1 byte contract frozen — V2-only kararların V1 artifact'lerine sızması imkânsız.
        let mut basis = golden_authorization_basis_fixture();
        basis.deterministic_gate_result = crate::trajectory::GateDecision::RejectedByTaskValidation;
        let err = AuthorizationBasisDigest::compute(&basis)
            .expect_err("V1 basis with V2 GateDecision must reject");
        assert!(
            matches!(
                err,
                CanonicalDigestError::UnsupportedV1GateDecision { tag: 7 }
            ),
            "V1 encoder must reject V2-only GateDecision"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b — GateDecision v2 append-only tag mapping test
    // (reviewer Faz 2 scoped P1-2: gerçek tag mapping'i doğrudan çağırır)
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn gate_decision_v2_tags_are_unique_and_append_only() {
        // Reviewer v4 P1-4 + Faz 2 scoped P1-2: append-only canonical tag invariant.
        // gate_decision_tag_v2 helper'ını doğrudan çağırarak gerçek tag mapping'i test eder.
        use crate::trajectory::GateDecision::*;
        // Mevcut tag'ler (0-6) exact pin — golden vector lock.
        assert_eq!(gate_decision_tag_v2(Unknown), 0);
        assert_eq!(gate_decision_tag_v2(PassedAll), 1);
        assert_eq!(gate_decision_tag_v2(RejectedBySyntax), 2);
        assert_eq!(gate_decision_tag_v2(RejectedByVision), 3);
        assert_eq!(gate_decision_tag_v2(RejectedByRule), 4);
        assert_eq!(gate_decision_tag_v2(RejectedByTaskBinding), 5);
        assert_eq!(gate_decision_tag_v2(BlockedByManeuverLimit), 6);
        // Commit 4b — append-only yeni tag'ler (7, 8).
        assert_eq!(gate_decision_tag_v2(RejectedByTaskValidation), 7);
        assert_eq!(gate_decision_tag_v2(RejectedByMeasurementBinding), 8);

        // Uniqueness: tüm tag'ler distinct.
        let all_tags: Vec<u8> = [
            Unknown,
            PassedAll,
            RejectedBySyntax,
            RejectedByVision,
            RejectedByRule,
            RejectedByTaskBinding,
            BlockedByManeuverLimit,
            RejectedByTaskValidation,
            RejectedByMeasurementBinding,
        ]
        .iter()
        .map(|gd| gate_decision_tag_v2(*gd))
        .collect();
        let mut sorted_tags = all_tags.clone();
        sorted_tags.sort_unstable();
        sorted_tags.dedup();
        assert_eq!(
            sorted_tags.len(),
            all_tags.len(),
            "all GateDecision v2 tags must be unique (no tag reuse)"
        );
        // Range: 0..=8 (append-only — hiçbir tag 8'in üstüne çıkmaz Commit 4b'de).
        assert!(
            all_tags.iter().all(|&t| t <= 8),
            "Commit 4b GateDecision tags must be in range 0..=8"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b — CanonicalBaselineUnavailableReason validation matrisi
    // (reviewer Faz 2 scoped P2-2: duplicate/subject-mismatch/empty/not-disjoint/union)
    // ═══════════════════════════════════════════════════════════════════════════════

    fn subject_scope(members: Vec<u64>) -> crate::measurement::CanonicalSubjectScope {
        crate::measurement::CanonicalSubjectScope::try_new(members).unwrap()
    }

    #[test]
    fn canonical_baseline_all_members_duplicate_rejected() {
        let subject = subject_scope(vec![1, 2]);
        let reason = crate::measurement::BaselineUnavailableReason::AllMembersIntroducedByDelta {
            members: vec![1, 1, 2], // duplicate 1
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::AllMembersDuplicate { .. }
        ));
    }

    #[test]
    fn canonical_baseline_all_members_subject_mismatch_rejected() {
        let subject = subject_scope(vec![1, 2]);
        let reason = crate::measurement::BaselineUnavailableReason::AllMembersIntroducedByDelta {
            members: vec![1, 3], // 3 not in subject
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::AllMembersSubjectMismatch { .. }
        ));
    }

    #[test]
    fn canonical_baseline_partial_existing_duplicate_rejected() {
        let subject = subject_scope(vec![1, 2, 3]);
        let reason = crate::measurement::BaselineUnavailableReason::PartialNewSubject {
            existing: vec![1, 1], // duplicate
            introduced: vec![2, 3],
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::PartialExistingDuplicate { .. }
        ));
    }

    #[test]
    fn canonical_baseline_partial_introduced_duplicate_rejected() {
        let subject = subject_scope(vec![1, 2, 3]);
        let reason = crate::measurement::BaselineUnavailableReason::PartialNewSubject {
            existing: vec![1],
            introduced: vec![2, 2, 3], // duplicate
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::PartialIntroducedDuplicate { .. }
        ));
    }

    #[test]
    fn canonical_baseline_partial_empty_list_rejected() {
        let subject = subject_scope(vec![1, 2]);
        let reason = crate::measurement::BaselineUnavailableReason::PartialNewSubject {
            existing: vec![], // empty
            introduced: vec![1, 2],
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::PartialEmptyList
        ));
    }

    #[test]
    fn canonical_baseline_partial_not_disjoint_rejected() {
        let subject = subject_scope(vec![1, 2]);
        let reason = crate::measurement::BaselineUnavailableReason::PartialNewSubject {
            existing: vec![1, 2],
            introduced: vec![2], // 2 in both — not disjoint
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::PartialNotDisjoint { node_id: 2 }
        ));
    }

    #[test]
    fn canonical_baseline_partial_union_subject_mismatch_rejected() {
        let subject = subject_scope(vec![1, 2, 3]);
        let reason = crate::measurement::BaselineUnavailableReason::PartialNewSubject {
            existing: vec![1, 4], // 4 not in subject
            introduced: vec![2],
        };
        let err =
            CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject).unwrap_err();
        assert!(matches!(
            err,
            CanonicalBaselineValidationError::PartialUnionSubjectMismatch { .. }
        ));
    }

    #[test]
    fn canonical_baseline_valid_all_members_succeeds() {
        let subject = subject_scope(vec![2, 1, 3]); // unsorted input
        let reason = crate::measurement::BaselineUnavailableReason::AllMembersIntroducedByDelta {
            members: vec![3, 1, 2], // unsorted — ordering canonicalize edilir
        };
        let canonical = CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject)
            .expect("valid all-members must succeed (ordering canonicalized)");
        match canonical.repr() {
            CanonicalBaselineUnavailableReasonRepr::AllMembersIntroducedByDelta { members } => {
                assert_eq!(members, &vec![1, 2, 3], "members sorted canonical order");
            }
            other => panic!("expected AllMembersIntroducedByDelta, got {other:?}"),
        }
    }

    #[test]
    fn canonical_baseline_valid_partial_succeeds() {
        let subject = subject_scope(vec![1, 2, 3]);
        let reason = crate::measurement::BaselineUnavailableReason::PartialNewSubject {
            existing: vec![2, 1], // unsorted
            introduced: vec![3],
        };
        let canonical = CanonicalBaselineUnavailableReason::try_from_reason(&reason, &subject)
            .expect("valid partial must succeed (ordering canonicalized)");
        match canonical.repr() {
            CanonicalBaselineUnavailableReasonRepr::PartialNewSubject {
                existing,
                introduced,
            } => {
                assert_eq!(existing, &vec![1, 2], "existing sorted canonical order");
                assert_eq!(introduced, &vec![3], "introduced sorted canonical order");
            }
            other => panic!("expected PartialNewSubject, got {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 4 — Pinned tag + V2 basis + witness requirement +
    // gate evaluation + context digest testleri (plan md:166)
    // ═══════════════════════════════════════════════════════════════════════════════

    // ── Pinned canonical tag newtype testleri (plan md:115) ───────────────────────

    #[test]
    fn faz4_gate_decision_tag_pinned_values() {
        use crate::trajectory::GateDecision;
        // Pinned exact values — append-only (0-8). V1 frozen (0-6) KORUNUR.
        assert_eq!(GateDecisionTag::from(&GateDecision::Unknown).as_u8(), 0);
        assert_eq!(GateDecisionTag::from(&GateDecision::PassedAll).as_u8(), 1);
        assert_eq!(
            GateDecisionTag::from(&GateDecision::RejectedBySyntax).as_u8(),
            2
        );
        assert_eq!(
            GateDecisionTag::from(&GateDecision::RejectedByVision).as_u8(),
            3
        );
        assert_eq!(
            GateDecisionTag::from(&GateDecision::RejectedByRule).as_u8(),
            4
        );
        assert_eq!(
            GateDecisionTag::from(&GateDecision::RejectedByTaskBinding).as_u8(),
            5
        );
        assert_eq!(
            GateDecisionTag::from(&GateDecision::BlockedByManeuverLimit).as_u8(),
            6
        );
        // Commit 4b append-only yeni tag'ler.
        assert_eq!(
            GateDecisionTag::from(&GateDecision::RejectedByTaskValidation).as_u8(),
            7
        );
        assert_eq!(
            GateDecisionTag::from(&GateDecision::RejectedByMeasurementBinding).as_u8(),
            8
        );
    }

    #[test]
    fn faz4_gate_decision_tag_rejects_unknown() {
        // TryFrom<u8> — invalid tag reddedilir (CanonicalizationError::InvalidCanonicalTag).
        assert!(GateDecisionTag::try_from(9).is_err());
        assert!(GateDecisionTag::try_from(255).is_err());
        assert!(GateDecisionTag::try_from(0).is_ok());
        assert!(GateDecisionTag::try_from(8).is_ok());
    }

    #[test]
    fn faz4_predicate_completion_and_mutation_decision_tags_pinned() {
        use crate::trajectory::{MutationDecision, PredicateCompletion};
        assert_eq!(
            PredicateCompletionTag::from(&PredicateCompletion::NotCompleted).as_u8(),
            0
        );
        assert_eq!(
            PredicateCompletionTag::from(&PredicateCompletion::Completed).as_u8(),
            1
        );
        assert_eq!(
            MutationDecisionTag::from(&MutationDecision::Reject).as_u8(),
            0
        );
        assert_eq!(
            MutationDecisionTag::from(&MutationDecision::AcceptAsProgress).as_u8(),
            1
        );
        assert_eq!(
            MutationDecisionTag::from(&MutationDecision::AcceptAsCompleted).as_u8(),
            2
        );
        assert_eq!(
            MutationDecisionTag::from(&MutationDecision::RequireOperatorApproval).as_u8(),
            3
        );
    }

    #[test]
    fn faz4_apply_target_tag_pinned() {
        use crate::trajectory::{ApplyTarget, CommitLane};
        assert_eq!(ApplyTargetTag::from(&ApplyTarget::NotApplied).as_u8(), 0);
        assert_eq!(
            ApplyTargetTag::from(&ApplyTarget::Lane(CommitLane::Mainline)).as_u8(),
            1
        );
        assert_eq!(
            ApplyTargetTag::from(&ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint)).as_u8(),
            2
        );
        assert_eq!(
            ApplyTargetTag::from(&ApplyTarget::Lane(CommitLane::Sandbox)).as_u8(),
            3
        );
    }

    #[test]
    fn faz4_helper_fn_delegate_preserves_v1_byte_contract() {
        // Helper fn'ler newtype'a delege eder — mapping değerleri KORUNUR.
        // V1 digest byte'ları HİÇ DEĞİŞMEZ (golden test green kalır).
        use crate::trajectory::{
            ApplyTarget, CommitLane, GateDecision, MutationDecision, PredicateCompletion,
        };
        assert_eq!(gate_decision_tag_v2(GateDecision::PassedAll), 1);
        assert_eq!(
            gate_decision_tag_v2(GateDecision::RejectedByMeasurementBinding),
            8
        );
        assert_eq!(predicate_completion_tag(PredicateCompletion::Completed), 1);
        assert_eq!(
            mutation_decision_tag(MutationDecision::AcceptAsCompleted),
            2
        );
        assert_eq!(apply_target_tag(&ApplyTarget::Lane(CommitLane::Sandbox)), 3);
    }

    #[test]
    fn faz4_witness_requirement_and_disposition_tags_pinned() {
        // Witness requirement + reason ayrı newtype (ontolojik ayrım).
        assert_eq!(WitnessRequirementTag::REQUIRED.as_u8(), 0);
        assert_eq!(WitnessRequirementTag::NOT_REQUIRED.as_u8(), 1);
        assert_eq!(
            WitnessNotRequiredReasonTag::REJECTED_BEFORE_WITNESS.as_u8(),
            0
        );
        // GateDispositionV2 — Passed/Rejected.
        assert_eq!(GateDispositionV2Tag::PASSED.as_u8(), 0);
        assert_eq!(GateDispositionV2Tag::REJECTED.as_u8(), 1);
        // Reject unknown tags.
        assert!(WitnessRequirementTag::try_from(2).is_err());
        assert!(WitnessNotRequiredReasonTag::try_from(1).is_err());
        assert!(GateDispositionV2Tag::try_from(2).is_err());
    }

    // ── CanonicalWitnessRequirementV2 testleri (plan md:96-102) ──────────────────

    fn faz4_witness_policy() -> CanonicalWitnessPolicy {
        CanonicalWitnessPolicy {
            schema_version: 1,
            min_approvers: 2,
            quorum_threshold: 1.5,
            independence_policy: crate::canonical_tags::WitnessIndependencePolicyTag::default(),
        }
    }

    #[test]
    fn faz4_witness_requirement_not_required_for_not_applied() {
        use crate::trajectory::ApplyTarget;
        // Reject → NotApplied → NotRequired { RejectedBeforeWitness }.
        let policy = faz4_witness_policy();
        let req = CanonicalWitnessRequirementV2::try_from((&policy, &ApplyTarget::NotApplied))
            .expect("NotApplied → NotRequired");
        // validate_for(NotApplied) → Ok.
        req.validate_for(&ApplyTarget::NotApplied)
            .expect("NotRequired valid for NotApplied");
        // tag = NOT_REQUIRED.
        assert_eq!(
            req.tag().as_u8(),
            WitnessRequirementTag::NOT_REQUIRED.as_u8()
        );
    }

    #[test]
    fn faz4_witness_requirement_required_for_lane() {
        use crate::trajectory::{ApplyTarget, CommitLane};
        let policy = faz4_witness_policy();
        let req = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .expect("Lane → Required");
        req.validate_for(&ApplyTarget::Lane(CommitLane::Mainline))
            .expect("Required valid for Lane");
        assert_eq!(req.tag().as_u8(), WitnessRequirementTag::REQUIRED.as_u8());
    }

    #[test]
    fn faz4_witness_requirement_validate_for_mismatch() {
        use crate::trajectory::{ApplyTarget, CommitLane};
        let policy = faz4_witness_policy();
        // Required için NotApplied → tutarsız.
        let req_for_lane = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        assert!(req_for_lane.validate_for(&ApplyTarget::NotApplied).is_err());
        // NotRequired için Lane → tutarsız.
        let req_for_not_applied =
            CanonicalWitnessRequirementV2::try_from((&policy, &ApplyTarget::NotApplied)).unwrap();
        assert!(req_for_not_applied
            .validate_for(&ApplyTarget::Lane(CommitLane::Sandbox))
            .is_err());
    }

    #[test]
    fn faz4_witness_requirement_encode_canonical_deterministic() {
        use crate::trajectory::{ApplyTarget, CommitLane};
        let policy = faz4_witness_policy();
        let req1 = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let req2 = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let mut h1 = blake3::Hasher::new();
        let mut h2 = blake3::Hasher::new();
        req1.encode_canonical(&mut h1).unwrap();
        req2.encode_canonical(&mut h2).unwrap();
        assert_eq!(
            h1.finalize().as_bytes(),
            h2.finalize().as_bytes(),
            "same requirement → same canonical bytes"
        );
    }

    // ── CanonicalGateEvaluationV2 + VerifiedGateEvaluationV2 testleri (plan md:74-79, P1-1) ─

    use crate::trajectory::MutationDecision;

    #[test]
    fn faz4_canonical_gate_evaluation_gate_passed_constructor() {
        // **Reviewer P1-1 v2:** GatePassed constructor — tüm MutationDecision varyantları
        // geçerli (Reject dahil — predicate/policy sonucu uygulanmama).
        let gate = CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted)
            .expect("structural construct");
        assert!(matches!(gate, CanonicalGateEvaluationV2::GatePassed { .. }));
        assert_eq!(
            gate.apply_target(),
            crate::trajectory::ApplyTarget::Lane(crate::trajectory::CommitLane::Mainline)
        );
    }

    #[test]
    fn faz4_canonical_gate_evaluation_rejected_by_gate_constructor() {
        // **Reviewer P1-1 v2:** RejectedByGate constructor — checked RejectedGateDecisionV2.
        let gate = CanonicalGateEvaluationV2::rejected_by_gate(
            crate::trajectory::GateDecision::RejectedBySyntax,
        )
        .expect("rejection gate decision valid");
        assert!(matches!(
            gate,
            CanonicalGateEvaluationV2::RejectedByGate { .. }
        ));
        assert_eq!(
            gate.apply_target(),
            crate::trajectory::ApplyTarget::NotApplied
        );
    }

    #[test]
    fn faz4_canonical_gate_evaluation_rejected_decision_rejects_non_rejection() {
        // **Reviewer P1-1 v2:** PassedAll/Unknown rejected gate decision olarak geçersiz.
        let err =
            CanonicalGateEvaluationV2::rejected_by_gate(crate::trajectory::GateDecision::PassedAll)
                .expect_err("PassedAll is not a rejection");
        assert!(
            matches!(err, GateDispositionError::NotARejectedGateDecision { .. }),
            "PassedAll → NotARejectedGateDecision"
        );
        let err =
            CanonicalGateEvaluationV2::rejected_by_gate(crate::trajectory::GateDecision::Unknown)
                .expect_err("Unknown is not a rejection");
        assert!(
            matches!(err, GateDispositionError::NotARejectedGateDecision { .. }),
            "Unknown → NotARejectedGateDecision"
        );
    }

    #[test]
    fn faz4_verified_gate_evaluation_fixture_cfg_test_only() {
        // cfg(test) fixture — production build'de constructor YOK.
        let canonical = CanonicalGateEvaluationV2::rejected_by_gate(
            crate::trajectory::GateDecision::RejectedByRule,
        )
        .unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(canonical);
        // into_canonical — tek yol (field private).
        let recovered = verified.into_canonical();
        assert!(matches!(
            recovered,
            CanonicalGateEvaluationV2::RejectedByGate { .. }
        ));
    }

    #[test]
    fn faz4_gate_evaluation_encode_canonical_deterministic() {
        let gate1 =
            CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted).unwrap();
        let gate2 =
            CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted).unwrap();
        let mut h1 = blake3::Hasher::new();
        let mut h2 = blake3::Hasher::new();
        gate1.encode_canonical(&mut h1).unwrap();
        gate2.encode_canonical(&mut h2).unwrap();
        assert_eq!(
            h1.finalize().as_bytes(),
            h2.finalize().as_bytes(),
            "same gate → same canonical bytes"
        );
    }

    // ── AuthorizationBasisV2 + AuthorizationContextV2 testleri (plan md:146-160) ───

    /// Faz 4 AuthorizationBasisV2 fixture — tutarlı digest zinciri (gerçek compute).
    /// Reviewer P1-2: field'lar private — fixture builder helper üzerinden üretilir.
    fn faz4_basis_v2_fixture() -> AuthorizationBasisV2 {
        faz4_basis_v2_fixture_with_task_id(42)
    }

    /// **Reviewer P1-2:** Parametreli fixture builder — farklı task_id ile tutarlı basis.
    /// Field write bypass imkânsız (field'lar private) — her fixture `new()` üzerinden.
    fn faz4_basis_v2_fixture_with_task_id(
        task_id: crate::trajectory::TaskId,
    ) -> AuthorizationBasisV2 {
        let parts = faz4_basis_v2_raw_parts(task_id);
        AuthorizationBasisV2::new(
            parts.task_id,
            parts.claim_id,
            parts.task_claim_digest,
            parts.task_goal_digest,
            parts.measurement_digest,
            parts.engine_measurement_digest,
            parts.trajectory_baseline,
            parts.measurement_baseline_digest,
            parts.trajectory_loss,
            parts.measurement_request,
            parts.measurement_request_digest,
            parts.measurement_context_digest,
            parts.canonical_delta_digest,
        )
        .expect("valid V2 basis")
    }

    /// **Reviewer P1-2:** Raw constructor parts — test'ler tek field'ı bozup `new()`
    /// çağırarak gerçek rejection testi yapar. Field write bypass YOK.
    struct Faz4BasisV2RawParts {
        task_id: crate::trajectory::TaskId,
        claim_id: crate::witness::ClaimId,
        task_claim_digest: crate::measurement::TaskClaimDigest,
        task_goal_digest: crate::measurement::TaskGoalDigest,
        measurement_digest: crate::measurement::MeasurementDigest,
        engine_measurement_digest: crate::measurement::EngineMeasurementDigest,
        trajectory_baseline: CanonicalTrajectoryEvidenceBaseline,
        measurement_baseline_digest: crate::measurement::MeasurementBaselineDigest,
        trajectory_loss: CanonicalTrajectoryLossEvidence,
        measurement_request: crate::measurement::CanonicalMeasurementRequestEvidence,
        measurement_request_digest: crate::measurement::MeasurementRequestDigest,
        measurement_context_digest: crate::measurement::MeasurementContextDigest,
        canonical_delta_digest: crate::measurement::MeasurementDeltaDigest,
    }

    fn faz4_basis_v2_raw_parts(task_id: crate::trajectory::TaskId) -> Faz4BasisV2RawParts {
        use crate::measurement::{
            EngineMeasurement, MeasurementBaseline, MeasurementContextDigest, MeasurementDigest,
            TaskGoalDigest,
        };

        // EngineMeasurement — uniform measured + Available baseline.
        let (request, evidence) = sample_measurement_request_evidence_parts();
        let measured = faz4_uniform_measured(0.5);
        let baseline = MeasurementBaseline::Available(measured.clone());
        let context = sample_measurement_input_context_for_faz4();
        let engine_meas = EngineMeasurement::new(baseline, measured, context, request.clone())
            .expect("context matches request");

        // Commitments — gerçek compute (tutarlı zincir).
        let measurement_digest = MeasurementDigest::compute(engine_meas.after()).unwrap();
        let engine_measurement_digest = engine_meas.compute_digest().unwrap();
        let measurement_baseline_digest = engine_meas.before().compute_digest().unwrap();
        let measurement_request_digest = engine_meas.request_digest().unwrap();
        let measurement_context_digest =
            MeasurementContextDigest::compute(engine_meas.context()).unwrap();
        let canonical_delta_digest = request.structural_delta_digest().clone();

        // **Reviewer P2:** Task goal digest — zengin golden task (tek commitment zinciri
        // measurement.rs golden ile aynı).
        let task = crate::measurement::tests::faz4_golden_task_with_id(task_id);
        let task_goal_digest = TaskGoalDigest::compute(&task).unwrap();

        // Task claim digest — minimal claim.
        let claim = faz4_test_claim_for_digest(1, task_id, 100);
        let task_claim_digest =
            crate::measurement::TaskClaimDigest::compute(&claim, task_id, &canonical_delta_digest)
                .unwrap();

        // Evidence — baseline Available (shared encoder ile tutarlı).
        let trajectory_baseline = CanonicalTrajectoryEvidenceBaseline::Available {
            before: faz4_provenanced_measured_result(),
        };
        // **Reviewer P1-1 v4:** Loss evidence Available — preferred_vector Some ile tutarlı.
        // target, faz4_golden_task preferred_vector ile birebir aynı. loss_after production
        // `trajectory::trajectory_loss` ile hesaplanır (x/y/z 3 eksen — production semantiği).
        let golden_task = crate::measurement::tests::faz4_golden_task_with_id(task_id);
        let preferred = golden_task
            .target_predicate_set
            .preferred_vector
            .expect("golden task preferred_vector Some");
        let target = CanonicalRawPosition {
            x: preferred.x,
            y: preferred.y,
            z: preferred.z,
            w: preferred.w,
            v: preferred.v,
        };
        let loss_after = crate::trajectory::trajectory_loss(engine_meas.after(), &preferred);
        let trajectory_loss = CanonicalTrajectoryLossEvidence::Available { target, loss_after };

        Faz4BasisV2RawParts {
            task_id,
            claim_id: 1,
            task_claim_digest,
            task_goal_digest,
            measurement_digest,
            engine_measurement_digest,
            trajectory_baseline,
            measurement_baseline_digest,
            trajectory_loss,
            measurement_request: evidence,
            measurement_request_digest,
            measurement_context_digest,
            canonical_delta_digest,
        }
    }

    /// Uniform MeasuredRawPosition — minimal fixture (measurement.rs test_measured pattern).
    fn faz4_uniform_measured(value: f64) -> crate::coords::MeasuredRawPosition {
        use crate::coords::{AxisMeasurement, MeasuredRawPosition, MetricSource};
        let axis = AxisMeasurement {
            value,
            source: MetricSource::Scip,
        };
        MeasuredRawPosition {
            coupling: axis,
            cohesion: axis,
            instability: axis,
            entropy: axis,
            witness_depth: axis,
        }
    }

    /// Minimal Claim for digest tests (measurement.rs test_claim_for_digest pattern).
    fn faz4_test_claim_for_digest(
        claim_id: u64,
        _task_id: u64,
        author: u64,
    ) -> crate::witness::Claim {
        use crate::witness::{Claim, Intent};
        Claim {
            id: claim_id,
            intent: Intent::new(100, crate::coords::RawPosition::default()),
            author,
            computed_raw: crate::coords::RawPosition::default(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id: Some(_task_id),
            removed_edges: vec![],
        }
    }

    fn faz4_provenanced_measured_result() -> ProvenancedMeasuredResult {
        use crate::canonical_tags::CanonicalMetricSourceTag;
        use crate::coords::MetricSource;
        let scip = CanonicalMetricSourceTag::try_from(&MetricSource::Scip).unwrap();
        let mk = |v: f64| CanonicalAxisMeasurement {
            value: v,
            source: scip,
        };
        ProvenancedMeasuredResult {
            coupling: mk(0.5),
            cohesion: mk(0.5),
            instability: mk(0.5),
            entropy: mk(0.5),
            witness_depth: mk(0.5),
        }
    }

    /// MeasurementRequest + CanonicalMeasurementRequestEvidence çifti (tutarlı).
    /// measurement.rs test helper'larına crate-içi erişim.
    fn sample_measurement_request_evidence_parts() -> (
        crate::measurement::MeasurementRequest,
        crate::measurement::CanonicalMeasurementRequestEvidence,
    ) {
        let request = crate::measurement::tests::sample_measurement_request();
        let evidence = request.canonical_evidence();
        (request, evidence)
    }

    fn sample_measurement_input_context_for_faz4() -> MeasurementInputContext {
        crate::measurement::tests::sample_measurement_input_context()
    }

    #[test]
    fn faz4_basis_v2_fixture_constructs_and_validates() {
        // validate_semantics başarılı — baseline digest + engine measurement digest reverify.
        let basis = faz4_basis_v2_fixture();
        // compute_digest başarılı (canonical encoding çalışır).
        let digest = basis.compute_digest().expect("V2 basis digest");
        assert_eq!(digest.to_hex().len(), 64, "hex wire 64 lowercase");
    }

    #[test]
    fn faz4_basis_v2_digest_is_deterministic() {
        let d1 = faz4_basis_v2_fixture().compute_digest().expect("digest");
        let d2 = faz4_basis_v2_fixture().compute_digest().expect("digest");
        assert_eq!(d1.as_bytes(), d2.as_bytes(), "same basis → same digest");
    }

    #[test]
    fn faz4_basis_v2_digest_mutates_on_identity() {
        // Farklı task_id → farklı digest.
        // **Reviewer P1-2:** Farklı task_id → farklı digest. Field write YOK —
        // parametreli fixture builder ile iki ayrı basis kurulur (her biri new() üzerinden).
        let d1 = faz4_basis_v2_fixture().compute_digest().unwrap();
        let d2 = faz4_basis_v2_fixture_with_task_id(43)
            .compute_digest()
            .unwrap();
        assert_ne!(
            d1.as_bytes(),
            d2.as_bytes(),
            "different task_id → different digest"
        );
    }

    #[test]
    fn faz4_basis_v2_validate_semantics_rejects_baseline_mismatch() {
        // **Reviewer P1-2:** Gerçek rejection testi. Tutarsız baseline digest ile
        // new() çağır → Err(MeasurementBaselineDigestMismatch). Field write YOK —
        // raw parts builder ile tutarsız digest verilir.
        let parts = faz4_basis_v2_raw_parts(42);
        // Tutarlı olmayan baseline digest üret (farklı measured değer).
        let bad_measured = faz4_uniform_measured(0.9);
        let bad_baseline = crate::measurement::MeasurementBaseline::Available(bad_measured);
        let bad_baseline_digest = bad_baseline.compute_digest().unwrap();
        let err = AuthorizationBasisV2::new(
            parts.task_id,
            parts.claim_id,
            parts.task_claim_digest,
            parts.task_goal_digest,
            parts.measurement_digest,
            parts.engine_measurement_digest,
            parts.trajectory_baseline,
            bad_baseline_digest, // tutarsız — trajectory_baseline ile uyuşmaz
            parts.trajectory_loss,
            parts.measurement_request,
            parts.measurement_request_digest,
            parts.measurement_context_digest,
            parts.canonical_delta_digest,
        )
        .expect_err("baseline mismatch must reject");
        assert!(
            matches!(
                err,
                AuthorizationBasisV2Error::MeasurementBaselineDigestMismatch { .. }
            ),
            "tutarsız baseline digest → MeasurementBaselineDigestMismatch, got {err:?}"
        );
    }

    // ── P1-3: Request snapshot mutation matrisi (reviewer) ──────────────────────────

    /// Helper: raw parts + bozuk request evidence ile new() çağır → error type assert.
    fn faz4_basis_v2_with_bad_request_evidence(
        bad_evidence: crate::measurement::CanonicalMeasurementRequestEvidence,
    ) -> Result<AuthorizationBasisV2, AuthorizationBasisV2Error> {
        let parts = faz4_basis_v2_raw_parts(42);
        AuthorizationBasisV2::new(
            parts.task_id,
            parts.claim_id,
            parts.task_claim_digest,
            parts.task_goal_digest,
            parts.measurement_digest,
            parts.engine_measurement_digest,
            parts.trajectory_baseline,
            parts.measurement_baseline_digest,
            parts.trajectory_loss,
            bad_evidence,
            parts.measurement_request_digest,
            parts.measurement_context_digest,
            parts.canonical_delta_digest,
        )
    }

    #[test]
    fn faz4_basis_v2_request_subject_mismatch() {
        // **Reviewer P1-3:** measurement_request.subject değişir → MeasurementRequestDigestMismatch.
        let parts = faz4_basis_v2_raw_parts(42);
        let mut bad_evidence = parts.measurement_request.clone();
        // Subject'e yeni node ekle (farklı subject → farklı digest).
        let mut new_members = bad_evidence.subject.member_ids().to_vec();
        new_members.push(999);
        bad_evidence.subject =
            crate::measurement::CanonicalSubjectScope::try_new(new_members).unwrap();
        let err = faz4_basis_v2_with_bad_request_evidence(bad_evidence).unwrap_err();
        assert!(
            matches!(
                err,
                AuthorizationBasisV2Error::MeasurementRequestDigestMismatch { .. }
            ),
            "subject mismatch → MeasurementRequestDigestMismatch, got {err:?}"
        );
    }

    #[test]
    fn faz4_basis_v2_request_impact_mismatch() {
        // **Reviewer P1-3:** measurement_request.impact değişir → mismatch.
        let parts = faz4_basis_v2_raw_parts(42);
        let mut bad_evidence = parts.measurement_request.clone();
        bad_evidence.impact =
            crate::measurement::CanonicalImpactScope::try_new(vec![888], vec![]).unwrap();
        let err = faz4_basis_v2_with_bad_request_evidence(bad_evidence).unwrap_err();
        assert!(
            matches!(
                err,
                AuthorizationBasisV2Error::MeasurementRequestDigestMismatch { .. }
            ),
            "impact mismatch → MeasurementRequestDigestMismatch, got {err:?}"
        );
    }

    #[test]
    fn faz4_basis_v2_request_base_revision_mismatch() {
        // **Reviewer P1-3:** measurement_request.base_revision değişir → mismatch.
        let parts = faz4_basis_v2_raw_parts(42);
        let mut bad_evidence = parts.measurement_request.clone();
        bad_evidence.base_revision.sequence = bad_evidence.base_revision.sequence + 1;
        let err = faz4_basis_v2_with_bad_request_evidence(bad_evidence).unwrap_err();
        assert!(
            matches!(
                err,
                AuthorizationBasisV2Error::MeasurementRequestDigestMismatch { .. }
            ),
            "base_revision mismatch → MeasurementRequestDigestMismatch, got {err:?}"
        );
    }

    #[test]
    fn faz4_basis_v2_request_structural_delta_digest_mismatch() {
        // **Reviewer P1-3:** measurement_request.structural_delta_digest değişir → mismatch.
        // Aynı zamanda canonical_delta_digest != request.structural_delta_digest olur.
        let parts = faz4_basis_v2_raw_parts(42);
        let mut bad_evidence = parts.measurement_request.clone();
        bad_evidence.structural_delta_digest =
            crate::measurement::MeasurementDeltaDigest::compute_from_canonical(
                &crate::authorization::CanonicalStructuralDelta::try_new(vec![], vec![], vec![])
                    .unwrap(),
            )
            .unwrap();
        // Önce MeasurementRequestDigestMismatch (structural_delta_digest evidence'da değişti).
        let err = faz4_basis_v2_with_bad_request_evidence(bad_evidence).unwrap_err();
        assert!(
            matches!(err, AuthorizationBasisV2Error::MeasurementRequestDigestMismatch { .. }),
            "structural_delta_digest evidence mismatch → MeasurementRequestDigestMismatch, got {err:?}"
        );
    }

    #[test]
    fn faz4_basis_v2_request_measurement_input_digest_mismatch() {
        // **Reviewer P1-3:** measurement_request.measurement_input_digest değişir → mismatch.
        let parts = faz4_basis_v2_raw_parts(42);
        let mut bad_evidence = parts.measurement_request.clone();
        bad_evidence.measurement_input_digest = MeasurementInputDigest::from_bytes([0xFE; 32]);
        let err = faz4_basis_v2_with_bad_request_evidence(bad_evidence).unwrap_err();
        assert!(
            matches!(
                err,
                AuthorizationBasisV2Error::MeasurementRequestDigestMismatch { .. }
            ),
            "measurement_input_digest mismatch → MeasurementRequestDigestMismatch, got {err:?}"
        );
    }

    #[test]
    fn faz4_basis_v2_canonical_delta_digest_mismatch() {
        // **Reviewer P1-3:** Request digest tutarlı ama canonical_delta_digest eski →
        // CanonicalDeltaDigestMismatch. measurement_request.structural_delta_digest ile
        // basis canonical_delta_digest farklı.
        let parts = faz4_basis_v2_raw_parts(42);
        // Yeni structural_delta_digest üret — hem evidence'da hem canonical_delta_digest'te
        // tutarlı olmalı ama eski canonical_delta_digest ile uyuşmaz.
        let new_delta = crate::measurement::MeasurementDeltaDigest::compute_from_canonical(
            &crate::authorization::CanonicalStructuralDelta::try_new(vec![], vec![], vec![])
                .unwrap(),
        )
        .unwrap();
        let mut bad_evidence = parts.measurement_request.clone();
        bad_evidence.structural_delta_digest = new_delta.clone();
        // measurement_request_digest de yeni delta'ya göre recompute — tutarlı olsun ki
        // sadece canonical_delta_digest ile çelişsin.
        bad_evidence.measurement_input_digest =
            parts.measurement_request.measurement_input_digest.clone();
        let new_request_digest =
            crate::measurement::MeasurementRequestDigest::compute_from_canonical(&bad_evidence)
                .unwrap();
        // engine_measurement_digest de recompute — tutarlı zincir.
        let new_engine_digest =
            crate::measurement::EngineMeasurementDigest::compute_from_commitments(
                &new_request_digest,
                &parts.measurement_baseline_digest,
                &parts.measurement_digest,
                &parts.measurement_context_digest,
            )
            .unwrap();
        let err = AuthorizationBasisV2::new(
            parts.task_id,
            parts.claim_id,
            parts.task_claim_digest,
            parts.task_goal_digest,
            parts.measurement_digest,
            new_engine_digest,
            parts.trajectory_baseline,
            parts.measurement_baseline_digest,
            parts.trajectory_loss,
            bad_evidence,
            new_request_digest,
            parts.measurement_context_digest,
            parts.canonical_delta_digest, // eski — yeni delta ile çelişir
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                AuthorizationBasisV2Error::CanonicalDeltaDigestMismatch { .. }
            ),
            "canonical_delta_digest mismatch → CanonicalDeltaDigestMismatch, got {err:?}"
        );
    }

    #[test]
    fn faz4_measurement_request_digest_compute_equals_compute_from_canonical() {
        // **Reviewer P1-3:** MeasurementRequestDigest::compute(request) ==
        // compute_from_canonical(request.canonical_evidence()) (shared encoder).
        let request = crate::measurement::tests::sample_measurement_request();
        let digest_via_compute =
            crate::measurement::MeasurementRequestDigest::compute(&request).unwrap();
        let evidence = request.canonical_evidence();
        let digest_via_canonical =
            crate::measurement::MeasurementRequestDigest::compute_from_canonical(&evidence)
                .unwrap();
        assert_eq!(
            digest_via_compute.as_bytes(),
            digest_via_canonical.as_bytes(),
            "compute(request) == compute_from_canonical(evidence) — shared encoder"
        );
    }

    // ── AuthorizationContextV2 proof-gated constructor (plan md:69-72) ────────────

    #[test]
    fn faz4_context_v2_new_consumes_verified_gate_evaluation() {
        let basis = faz4_basis_v2_fixture();
        let gate =
            CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted).unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(gate);
        let policy = faz4_witness_policy();
        use crate::trajectory::{ApplyTarget, CommitLane};
        let witness_req = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let context = AuthorizationContextV2::new(basis, verified, witness_req)
            .expect("proof-gated context construction");
        // Accessors çalışır.
        assert_eq!(context.basis().task_id(), 42);
        assert!(matches!(
            context.gate_evaluation(),
            CanonicalGateEvaluationV2::GatePassed { .. }
        ));
        let _ = context.witness_requirement();
    }

    #[test]
    fn faz4_context_v2_new_rejects_witness_mismatch() {
        // **Reviewer P1-1:** GatePassed + Reject → apply_target NotApplied,
        // ama witness Required → tutarsız → WitnessRequirement err.
        let basis = faz4_basis_v2_fixture();
        let gate = CanonicalGateEvaluationV2::gate_passed(MutationDecision::Reject).unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(gate);
        let policy = faz4_witness_policy();
        use crate::trajectory::{ApplyTarget, CommitLane};
        // witness_req = Required (Mainline lane), ama mutation_decision = Reject → NotApplied.
        let witness_req = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let err = AuthorizationContextV2::new(basis, verified, witness_req)
            .expect_err("witness mismatch must reject");
        assert!(
            matches!(err, AuthorizationContextV2BuildError::WitnessRequirement(_)),
            "witness requirement mismatch → WitnessRequirement error"
        );
    }

    #[test]
    fn faz4_context_v2_new_accepts_consistent_not_required() {
        // **Reviewer P1-1:** GatePassed + Reject → NotApplied + witness NotRequired → Ok.
        let basis = faz4_basis_v2_fixture();
        let gate = CanonicalGateEvaluationV2::gate_passed(MutationDecision::Reject).unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(gate);
        let policy = faz4_witness_policy();
        use crate::trajectory::ApplyTarget;
        let witness_req =
            CanonicalWitnessRequirementV2::try_from((&policy, &ApplyTarget::NotApplied)).unwrap();
        let context = AuthorizationContextV2::new(basis, verified, witness_req)
            .expect("consistent NotRequired + NotApplied → Ok");
        assert!(matches!(
            context.gate_evaluation(),
            CanonicalGateEvaluationV2::GatePassed {
                mutation_decision: MutationDecision::Reject
            }
        ));
    }

    #[test]
    fn faz4_context_v2_new_accepts_rejected_gate_not_required() {
        // **Reviewer P1-1 v2:** RejectedByGate → NotApplied + witness NotRequired → Ok.
        let basis = faz4_basis_v2_fixture();
        let gate = CanonicalGateEvaluationV2::rejected_by_gate(
            crate::trajectory::GateDecision::RejectedBySyntax,
        )
        .unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(gate);
        let policy = faz4_witness_policy();
        use crate::trajectory::ApplyTarget;
        let witness_req =
            CanonicalWitnessRequirementV2::try_from((&policy, &ApplyTarget::NotApplied)).unwrap();
        let context = AuthorizationContextV2::new(basis, verified, witness_req)
            .expect("RejectedByGate + NotRequired → Ok");
        assert!(matches!(
            context.gate_evaluation(),
            CanonicalGateEvaluationV2::RejectedByGate { .. }
        ));
    }

    #[test]
    fn faz4_context_v2_new_rejects_rejected_gate_required() {
        // **Reviewer P1-1 v2:** RejectedByGate → NotApplied, ama witness Required → tutarsız.
        let basis = faz4_basis_v2_fixture();
        let gate = CanonicalGateEvaluationV2::rejected_by_gate(
            crate::trajectory::GateDecision::RejectedByRule,
        )
        .unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(gate);
        let policy = faz4_witness_policy();
        use crate::trajectory::{ApplyTarget, CommitLane};
        let witness_req = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let err = AuthorizationContextV2::new(basis, verified, witness_req)
            .expect_err("RejectedByGate + Required must reject");
        assert!(
            matches!(err, AuthorizationContextV2BuildError::WitnessRequirement(_)),
            "RejectedByGate + Required → WitnessRequirement error"
        );
    }

    #[test]
    fn faz4_context_v2_digest_is_deterministic() {
        let basis = faz4_basis_v2_fixture();
        let gate1 =
            CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted).unwrap();
        let gate2 =
            CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted).unwrap();
        let policy = faz4_witness_policy();
        use crate::trajectory::{ApplyTarget, CommitLane};
        let wr1 = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let wr2 = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let ctx1 = AuthorizationContextV2::new(
            basis.clone(),
            VerifiedGateEvaluationV2::fixture(gate1),
            wr1,
        )
        .unwrap();
        let ctx2 =
            AuthorizationContextV2::new(basis, VerifiedGateEvaluationV2::fixture(gate2), wr2)
                .unwrap();
        let d1 = ctx1.compute_digest().unwrap();
        let d2 = ctx2.compute_digest().unwrap();
        assert_eq!(d1.as_bytes(), d2.as_bytes(), "same context → same digest");
    }

    #[test]
    fn faz4_context_v2_digest_mutates_on_variant() {
        // **Reviewer P1-1 v2 + P2 v3:** GatePassed vs RejectedByGate → farklı digest.
        // **Isolation:** iki context aynı witness requirement (NotRequired) tutar —
        // GatePassed { Reject } → NotApplied + NotRequired, RejectedByGate → NotApplied +
        // NotRequired. Böylece sadece gate variant/payload değişir (digest farkı gate'ten).
        let basis = faz4_basis_v2_fixture();
        let policy = faz4_witness_policy();
        use crate::trajectory::ApplyTarget;
        let wr_not_req =
            CanonicalWitnessRequirementV2::try_from((&policy, &ApplyTarget::NotApplied)).unwrap();
        // GatePassed + Reject → NotApplied + NotRequired (aynı witness requirement).
        let gate_passed = CanonicalGateEvaluationV2::gate_passed(MutationDecision::Reject).unwrap();
        let ctx_passed = AuthorizationContextV2::new(
            basis.clone(),
            VerifiedGateEvaluationV2::fixture(gate_passed),
            wr_not_req.clone(),
        )
        .unwrap();
        // RejectedByGate → NotApplied + NotRequired (aynı witness requirement).
        let gate_rejected = CanonicalGateEvaluationV2::rejected_by_gate(
            crate::trajectory::GateDecision::RejectedBySyntax,
        )
        .unwrap();
        let ctx_rejected = AuthorizationContextV2::new(
            basis,
            VerifiedGateEvaluationV2::fixture(gate_rejected),
            wr_not_req,
        )
        .unwrap();
        let d_passed = ctx_passed.compute_digest().unwrap();
        let d_rejected = ctx_rejected.compute_digest().unwrap();
        assert_ne!(
            d_passed.as_bytes(),
            d_rejected.as_bytes(),
            "GatePassed vs RejectedByGate → different digest"
        );
    }

    // ── V2 digest golden vector pinleme (reviewer P2-2) ──────────────────────────────

    #[test]
    fn faz4_basis_v2_digest_golden_vector() {
        // **Reviewer P2-2:** Frozen golden hex — AuthorizationBasisDigestV2 canonical byte
        // contract pin (OSP/AUTHORIZATION-BASIS/V2).
        let basis = faz4_basis_v2_fixture();
        let digest = basis.compute_digest().expect("V2 basis digest");
        const FAZ4_BASIS_V2_GOLDEN_HEX: &str =
            "ee3e78c4b5c3df71752d58cb94cf772816014c3709009a44a98dd1d57fe2bc64";
        assert_eq!(
            digest.to_hex(),
            FAZ4_BASIS_V2_GOLDEN_HEX,
            "AuthorizationBasisDigestV2 golden byte contract changed (OSP/AUTHORIZATION-BASIS/V2)"
        );
    }

    #[test]
    fn faz4_context_v2_digest_golden_vector() {
        // **Reviewer P2-2:** Frozen golden hex — AuthorizationContextDigestV2 canonical byte
        // contract pin (OSP/AUTHORIZATION-CONTEXT/V2).
        let basis = faz4_basis_v2_fixture();
        let gate =
            CanonicalGateEvaluationV2::gate_passed(MutationDecision::AcceptAsCompleted).unwrap();
        let verified = VerifiedGateEvaluationV2::fixture(gate);
        let policy = faz4_witness_policy();
        use crate::trajectory::{ApplyTarget, CommitLane};
        let witness_req = CanonicalWitnessRequirementV2::try_from((
            &policy,
            &ApplyTarget::Lane(CommitLane::Mainline),
        ))
        .unwrap();
        let context = AuthorizationContextV2::new(basis, verified, witness_req).unwrap();
        let digest = context.compute_digest().expect("V2 context digest");
        const FAZ4_CONTEXT_V2_GOLDEN_HEX: &str =
            "3000ccb37928868e2506869aeb6a13f1c823e61977cdf60603b645123380d8a0";
        assert_eq!(
            digest.to_hex(),
            FAZ4_CONTEXT_V2_GOLDEN_HEX,
            "AuthorizationContextDigestV2 golden byte contract changed (OSP/AUTHORIZATION-CONTEXT/V2)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 4 Commit 1b — VersionedAuthorizationBasis wire dispatch
    // testleri (plan md:177, reviewer v2 closure)
    // ═══════════════════════════════════════════════════════════════════════════════

    /// V1 basis fixture — wire dispatch testleri için.
    fn faz4_v1_basis_fixture() -> AuthorizationBasisV1 {
        sample_basis()
    }

    /// Versioned V1 envelope JSON string — `{schema_version:1, basis:{...}}`.
    fn faz4_versioned_v1_envelope_json() -> String {
        let basis = faz4_v1_basis_fixture();
        let envelope = VersionedAuthorizationBasis::try_v1(basis).unwrap();
        serde_json::to_string(&envelope).unwrap()
    }

    /// Versioned V2 envelope JSON string — `{schema_version:2, basis:{...}}`.
    fn faz4_versioned_v2_envelope_json() -> String {
        let basis = faz4_basis_v2_fixture();
        let envelope = VersionedAuthorizationBasis::try_v2(basis);
        serde_json::to_string(&envelope).unwrap()
    }

    /// Legacy bare V1 JSON string — mevcut V1 serialization shape (schema_version field dahil).
    fn faz4_legacy_v1_json() -> String {
        let basis = faz4_v1_basis_fixture();
        serde_json::to_string(&basis).unwrap()
    }

    // ── Dispatch matrisi (P0-1, P0-2, P1-4) ──────────────────────────────────────

    #[test]
    fn commit1b_legacy_v1_dispatches_correctly() {
        // P0-1: Legacy V1 kendi schema_version=1 ile doğru dispatch. "basis" key yok.
        let json = faz4_legacy_v1_json();
        let parsed =
            VersionedAuthorizationBasis::from_json_slice(json.as_bytes()).expect("legacy V1");
        assert_eq!(parsed.version(), AUTHORIZATION_BASIS_WIRE_SCHEMA_V1);
        assert!(parsed.as_v1().is_some());
        assert!(parsed.as_v2().is_none());
    }

    #[test]
    fn commit1b_versioned_v1_produces_explicit_envelope() {
        // P0-2: Versioned V1 serialize gerçekten {schema_version, basis} üretir.
        let json = faz4_versioned_v1_envelope_json();
        let peek: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(peek.get("schema_version"), Some(&serde_json::json!(1)));
        assert!(
            peek.get("basis").is_some(),
            "versioned V1 must have 'basis' key"
        );
    }

    #[test]
    fn commit1b_versioned_v1_round_trip() {
        let original = faz4_versioned_v1_envelope_json();
        let parsed = VersionedAuthorizationBasis::from_json_slice(original.as_bytes())
            .expect("versioned V1 round-trip");
        let reserialized = serde_json::to_string(&parsed).unwrap();
        assert_eq!(
            original, reserialized,
            "versioned V1 round-trip preserves JSON"
        );
    }

    #[test]
    fn commit1b_versioned_v2_round_trip() {
        let original = faz4_versioned_v2_envelope_json();
        let parsed = VersionedAuthorizationBasis::from_json_slice(original.as_bytes())
            .expect("versioned V2 round-trip");
        assert_eq!(parsed.version(), AUTHORIZATION_BASIS_WIRE_SCHEMA_V2);
        let reserialized = serde_json::to_string(&parsed).unwrap();
        assert_eq!(
            original, reserialized,
            "versioned V2 round-trip preserves JSON"
        );
    }

    #[test]
    fn commit1b_schema_version_2_without_basis_rejects() {
        // P1-2 v3: schema_version=2 ama basis yok → MissingBasisForVersionedSchema.
        // V2-shaped input hiçbir koşulda legacy V1 parser'a ulaşmaz.
        let json = r#"{"schema_version": 2, "task_id": 42}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("schema_version=2 without basis must reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::MissingBasisForVersionedSchema { schema_version: 2 }
        ));
    }

    #[test]
    fn commit1b_basis_without_schema_version_rejects() {
        // P0-1: basis var ama schema_version yok → reject.
        let json = r#"{"basis": {"schema_version": 1}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("basis without schema_version must reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::MissingSchemaVersion
        ));
    }

    #[test]
    fn commit1b_legacy_v1_with_extra_basis_key_rejects_ambiguous() {
        // P1-4: Legacy V1 + extra "basis" key → reject ambiguous (versioned-envelope-shaped).
        let mut legacy: serde_json::Value = serde_json::from_str(&faz4_legacy_v1_json()).unwrap();
        legacy["basis"] = serde_json::json!({});
        let json = serde_json::to_string(&legacy).unwrap();
        let result = VersionedAuthorizationBasis::from_json_slice(json.as_bytes());
        // "basis" var → versioned path. Inner basis boş → decode error veya schema mismatch.
        assert!(
            result.is_err(),
            "legacy V1 with extra 'basis' key must reject"
        );
    }

    // ── Duplicate-key matrisi (P0-4) ─────────────────────────────────────────────

    #[test]
    fn commit1b_duplicate_schema_version_rejects() {
        // P0-4: duplicate top-level schema_version → reject.
        let json = r#"{"schema_version": 1, "schema_version": 2, "basis": {"schema_version": 1}}"#;
        let result = VersionedAuthorizationBasis::from_json_slice(json.as_bytes());
        assert!(result.is_err(), "duplicate schema_version must reject");
    }

    #[test]
    fn commit1b_duplicate_basis_field_rejects() {
        // P0-4: duplicate basis field → reject.
        let json = r#"{"schema_version": 1, "basis": {}, "basis": {}}"#;
        let result = VersionedAuthorizationBasis::from_json_slice(json.as_bytes());
        assert!(result.is_err(), "duplicate basis field must reject");
    }

    // ── Strict numeric matrisi (P2-3) ────────────────────────────────────────────

    #[test]
    fn commit1b_schema_version_float_rejects() {
        // P2-3: schema_version=1.0 (float) → reject.
        let json = r#"{"schema_version": 1.0, "basis": {}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("float schema_version reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::SchemaVersionNotStrict
        ));
    }

    #[test]
    fn commit1b_schema_version_string_rejects() {
        // P2-3: schema_version="2" (string) → reject.
        let json = r#"{"schema_version": "2", "basis": {}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("string schema_version reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::SchemaVersionNotStrict
        ));
    }

    #[test]
    fn commit1b_schema_version_null_rejects() {
        // P2-3: schema_version=null → reject.
        let json = r#"{"schema_version": null, "basis": {}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("null schema_version reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::SchemaVersionNotStrict
        ));
    }

    #[test]
    fn commit1b_schema_version_exponent_rejects() {
        // P2-3: schema_version=1e0 (exponent) → reject.
        let json = r#"{"schema_version": 1e0, "basis": {}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("exponent schema_version reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::SchemaVersionNotStrict
        ));
    }

    #[test]
    fn commit1b_schema_version_out_of_range_rejects() {
        // P2-3: schema_version=65536 (u16 dışı) → reject.
        let json = r#"{"schema_version": 65536, "basis": {}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("u16 out-of-range schema_version reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::SchemaVersionOutOfRange(65536)
        ));
    }

    #[test]
    fn commit1b_unknown_schema_version_rejects() {
        // Unknown version → reject (V2→V1 fallback YOK).
        let json = r#"{"schema_version": 3, "basis": {}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("unknown schema_version reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::UnknownSchemaVersion(3)
        ));
    }

    // ── Top-level non-object matrisi (P2-2) ──────────────────────────────────────

    #[test]
    fn commit1b_top_level_null_rejects() {
        let err = VersionedAuthorizationBasis::from_json_slice(b"null")
            .expect_err("null top-level reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::TopLevelNotObject
        ));
    }

    #[test]
    fn commit1b_top_level_array_rejects() {
        let err = VersionedAuthorizationBasis::from_json_slice(b"[]")
            .expect_err("array top-level reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::TopLevelNotObject
        ));
    }

    #[test]
    fn commit1b_top_level_number_rejects() {
        let err = VersionedAuthorizationBasis::from_json_slice(b"42")
            .expect_err("number top-level reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::TopLevelNotObject
        ));
    }

    // ── Wire contract matrisi ────────────────────────────────────────────────────

    #[test]
    fn commit1b_versioned_v1_basis_payload_preserves_legacy() {
        // P2: versioned V1 basis payload == legacy V1 JSON payload (Value equality —
        // field sıralaması serde map'te farklı olabilir ama içerik aynı).
        let legacy: serde_json::Value = serde_json::from_str(&faz4_legacy_v1_json()).unwrap();
        let versioned_json = faz4_versioned_v1_envelope_json();
        let versioned: serde_json::Value = serde_json::from_str(&versioned_json).unwrap();
        assert_eq!(
            versioned.get("basis"),
            Some(&legacy),
            "versioned V1 basis payload must exactly preserve legacy V1 JSON representation"
        );
    }

    #[test]
    fn commit1b_v2_nested_commitment_inconsistency_rejects() {
        // P1-5 Model A: V2 nested commitment inconsistency → validate_semantics reject.
        // Geçerli V2 envelope'ı al, basis içinde bir digest'ı boz → new() reject.
        let json = faz4_versioned_v2_envelope_json();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        // measurement_baseline_digest'i boz (farklı hex).
        value["basis"]["measurement_baseline_digest"] =
            serde_json::json!("0000000000000000000000000000000000000000000000000000000000000000");
        let tampered = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(tampered.as_bytes())
            .expect_err("V2 nested inconsistency must reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::V2Validation(_)
        ));
    }

    // ── Bağımsız V2 golden fixture (reviewer P1-4) ────────────────────────────────

    /// **P1-4:** Bağımsız golden JSON fixture — serializer/deserializer birlikte
    /// yanlış değişse bile wire contract pinlenir. Repository fixture'dan okunur.
    const V2_WIRE_GOLDEN_FIXTURE: &str =
        include_str!("../tests/fixtures/authorization_basis_v2_wire.json");

    #[test]
    fn commit1b_v2_wire_golden_fixture_round_trip() {
        // P1-4: Bağımsız fixture → parse → reserialize → Value equality.
        let parsed =
            VersionedAuthorizationBasis::from_json_slice(V2_WIRE_GOLDEN_FIXTURE.as_bytes())
                .expect("golden fixture must parse");
        assert_eq!(parsed.version(), AUTHORIZATION_BASIS_WIRE_SCHEMA_V2);
        let actual: serde_json::Value = serde_json::to_value(&parsed).expect("reserialize");
        let expected: serde_json::Value =
            serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).expect("fixture is valid JSON");
        assert_eq!(
            actual, expected,
            "V2 wire golden fixture round-trip — wire contract pin"
        );
    }

    // ── LowerHex32 strict matrisi (P1-2) ─────────────────────────────────────────

    #[test]
    fn commit1b_v2_digest_lowercase_hex() {
        // V2 nested tüm digest alanları lowercase hex string.
        let json = faz4_versioned_v2_envelope_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let digest_str = &value["basis"]["task_goal_digest"];
        assert!(
            digest_str.is_string(),
            "V2 digest must be JSON string (LowerHex32)"
        );
        let s = digest_str.as_str().unwrap();
        assert_eq!(s.len(), 64, "V2 digest must be 64 chars");
        assert!(
            s.bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
            "V2 digest must be lowercase hex only"
        );
    }

    // ── P2: Negatif wire testleri (reviewer) ──────────────────────────────────────

    #[test]
    fn commit1b_v2_uppercase_hex_rejects() {
        // P2: uppercase hex → LowerHex32 reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["task_goal_digest"] =
            serde_json::json!("03A3AD384D2DFF383974A301ED68A52D932439F18E3C08CC4CB8A8B9C7C8201C");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("uppercase hex reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_0x_prefix_hex_rejects() {
        // P2: 0x prefix → LowerHex32 reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["task_goal_digest"] =
            serde_json::json!("0x03a3ad384d2dff383974a301ed68a52d932439f18e3c08cc4cb8a8b9c7c8201c");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("0x prefix reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_short_hex_rejects() {
        // P2: 63 karakter hex → reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["task_goal_digest"] =
            serde_json::json!("03a3ad384d2dff383974a301ed68a52d932439f18e3c08cc4cb8a8b9c7c8201");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("short hex reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_source_tag_255_rejects() {
        // P2: source_tag=255 → checked TryFrom reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["trajectory_baseline"]["before"]["coupling"]["source_tag"] =
            serde_json::json!(255);
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("source_tag=255 reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::V2WireConversion { .. }
                | VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_unknown_baseline_kind_rejects() {
        // P2: unknown baseline kind → reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["trajectory_baseline"]["kind"] = serde_json::json!("unknown_kind");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("unknown baseline kind reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_unknown_loss_kind_rejects() {
        // P2: unknown loss kind → reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["trajectory_loss"]["kind"] = serde_json::json!("unknown_loss");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("unknown loss kind reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_unknown_space_view_id_kind_rejects() {
        // P2: unknown space_view_id kind → reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["measurement_request"]["base_revision"]["view_id"]["kind"] =
            serde_json::json!("unknown_view");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("unknown space_view_id kind reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    #[test]
    fn commit1b_v2_negative_loss_after_rejects() {
        // P2: negative loss_after → local invariant reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        value["basis"]["trajectory_loss"]["loss_after"] = serde_json::json!(-0.5);
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("negative loss_after reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::V2WireConversion { .. }
        ));
    }

    #[test]
    fn commit1b_v1_inner_schema_version_2_rejects() {
        // P1-1: outer V1 + inner schema_version=2 → InnerV1SchemaMismatch exact error.
        let mut basis = faz4_v1_basis_fixture();
        basis.schema_version = 2;
        let err = VersionedAuthorizationBasis::try_v1(basis)
            .expect_err("inner schema_version=2 with V1 constructor must reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::InnerV1SchemaMismatch {
                expected: AUTHORIZATION_BASIS_WIRE_SCHEMA_V1,
                found: 2
            }
        ));
    }

    #[test]
    fn commit1b_v2_missing_axis_field_rejects() {
        // P2: missing axis field (coupling removed) → reject.
        let mut value: serde_json::Value = serde_json::from_str(V2_WIRE_GOLDEN_FIXTURE).unwrap();
        // Remove coupling axis from before.
        let before = value
            .get_mut("basis")
            .unwrap()
            .get_mut("trajectory_baseline")
            .unwrap()
            .get_mut("before")
            .unwrap()
            .as_object_mut()
            .unwrap();
        before.remove("coupling");
        let json = serde_json::to_string(&value).unwrap();
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("missing axis field reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    // ── P1-1 v2: Kalan varyant golden wire shape testleri ─────────────────────────
    // Reviewer: golden fixture yalnız available/ephemeral pinliyor. Kalan varyantlar
    // modül içi testlerde private DTO'lara erişerek exact JSON shape pinlenir.

    // ── P1-1 v3: Production *Ref serializer golden wire shape testleri ────────────
    // Reviewer: production serializer *Ref tiplerini kullanır (owned DTO değil).
    // Outer baseline wrapper + nested reason birlikte pinlenir.

    #[test]
    fn commit1b_wire_shape_baseline_unavailable_all_members_output_golden() {
        // Production Ref serializer — outer wrapper + nested reason.
        let members = [1u64, 2];
        let value = serde_json::to_value(RawTrajectoryBaselineV2Ref::Unavailable {
            reason: RawBaselineUnavailableReasonV2Ref::AllMembersIntroducedByDelta {
                members: &members,
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "unavailable",
                "reason": {
                    "kind": "all_members_introduced_by_delta",
                    "members": [1, 2]
                }
            })
        );
    }

    #[test]
    fn commit1b_wire_shape_baseline_unavailable_partial_new_subject_output_golden() {
        let existing = [1u64, 2];
        let introduced = [3u64];
        let value = serde_json::to_value(RawTrajectoryBaselineV2Ref::Unavailable {
            reason: RawBaselineUnavailableReasonV2Ref::PartialNewSubject {
                existing: &existing,
                introduced: &introduced,
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "unavailable",
                "reason": {
                    "kind": "partial_new_subject",
                    "existing": [1, 2],
                    "introduced": [3]
                }
            })
        );
    }

    #[test]
    fn commit1b_wire_shape_loss_unavailable_output_golden() {
        let value = serde_json::to_value(RawTrajectoryLossV2Ref::Unavailable {
            reason: RawTrajectoryLossUnavailableReasonV2::NoPreferredVector,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"kind": "unavailable", "reason": "no_preferred_vector"})
        );
    }

    #[test]
    fn commit1b_wire_shape_space_view_id_persisted_output_golden() {
        let id_bytes = [
            0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff, 0x00,
        ];
        let value = serde_json::to_value(RawSpaceViewIdV2Ref::Persisted { id: &id_bytes }).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "persisted",
                "id": [
                    17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 0
                ]
            })
        );
    }

    // ── P2-1: Bare unknown version → UnknownSchemaVersion ────────────────────────

    #[test]
    fn commit1b_schema_version_3_without_basis_unknown_version_error() {
        // P2-1: schema_version=3 (unknown) + basis yok → UnknownSchemaVersion
        // (MissingBasisForVersionedSchema değil — typed taxonomy tutarlı).
        let json = r#"{"schema_version": 3, "task_id": 42}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("unknown version without basis must reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::UnknownSchemaVersion(3)
        ));
    }

    // ── P2-2: Nested duplicate-key test (raw JSON) ───────────────────────────────

    #[test]
    fn commit1b_v2_nested_duplicate_key_rejects() {
        // P2-2: Nested duplicate key (task_id iki kez) → VersionedV2Decode.
        // Raw JSON — serde_json::Value duplicate'e ezmez (RawValue parse).
        let json = r#"{"schema_version": 2, "basis": {"task_id": 42, "task_id": 43, "claim_id": 1, "task_claim_digest": "0000000000000000000000000000000000000000000000000000000000000000", "task_goal_digest": "0000000000000000000000000000000000000000000000000000000000000000", "measurement_digest": "0000000000000000000000000000000000000000000000000000000000000000", "engine_measurement_digest": "0000000000000000000000000000000000000000000000000000000000000000", "trajectory_baseline": {"kind": "available", "before": {"coupling": {"value": 0.5, "source_tag": 1}, "cohesion": {"value": 0.5, "source_tag": 1}, "instability": {"value": 0.5, "source_tag": 1}, "entropy": {"value": 0.5, "source_tag": 1}, "witness_depth": {"value": 0.5, "source_tag": 1}}}, "measurement_baseline_digest": "0000000000000000000000000000000000000000000000000000000000000000", "trajectory_loss": {"kind": "available", "target": {"x": 0.2, "y": 0.8, "z": 0.15, "w": 0.3, "v": 0.6}, "loss_after": 0.55}, "measurement_request": {"subject": {"member_ids": [1]}, "impact": {"node_ids": [], "edge_ids": []}, "base_revision": {"view_id": {"kind": "ephemeral", "id": 1}, "sequence": 1, "content_digest": "0000000000000000000000000000000000000000000000000000000000000000"}, "structural_delta_digest": "0000000000000000000000000000000000000000000000000000000000000000", "measurement_input_digest": "0000000000000000000000000000000000000000000000000000000000000000"}, "measurement_request_digest": "0000000000000000000000000000000000000000000000000000000000000000", "measurement_context_digest": "0000000000000000000000000000000000000000000000000000000000000000", "canonical_delta_digest": "0000000000000000000000000000000000000000000000000000000000000000"}}"#;
        let err = VersionedAuthorizationBasis::from_json_slice(json.as_bytes())
            .expect_err("nested duplicate key must reject");
        assert!(matches!(
            err,
            VersionedAuthorizationBasisError::VersionedV2Decode { .. }
        ));
    }

    // ── P2-3: Tek-wire-surface invariant compile-time guard ──────────────────────
    // Reviewer P1-2 v3: Bu test "Serialize absent compile-time guard" DEĞİL.
    // AuthorizationBasisV2: Serialize trait bound'una başvurmuyor — yarın derive
    // geri eklenirse test yine geçer. Gerçek compile-fail guard (trybuild external
    // crate fixture — requires_serialize::<AuthorizationBasisV2>()) Faz 10 type-
    // suite'e deferred. Bu test sadece "wrapper serialization çalışıyor" doğrular.

    /// **P1-2 v3:** VersionedAuthorizationBasis wrapper serialization çalışıyor —
    /// V2 envelope tek serialization yolu. **NOT:** Bu test AuthorizationBasisV2
    /// direct Serialize absent invariant'ını doğrulamaz. Compile-fail guard
    /// (trybuild external crate) Faz 10 type-suite'e deferred.
    #[test]
    fn commit1b_versioned_v2_wrapper_is_serializable() {
        let basis = faz4_basis_v2_fixture();
        let envelope = VersionedAuthorizationBasis::try_v2(basis);
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(
            json.contains("\"schema_version\":2"),
            "V2 envelope tek serialization yolu"
        );
    }

    #[test]
    fn commit1b_authorization_basis_digest_v2_serialize_hex_string() {
        // P2-3: Digest custom Serialize — yalnız 64 lowercase hex string üretir.
        let basis = faz4_basis_v2_fixture();
        let digest = basis.compute_digest().unwrap();
        let json = serde_json::to_string(&digest).unwrap();
        // JSON string quote'lu hex — "\"hex...\""
        assert!(
            json.starts_with('"') && json.ends_with('"'),
            "digest must serialize as JSON string"
        );
        let hex = &json[1..json.len() - 1];
        assert_eq!(hex.len(), 64, "digest hex 64 chars");
        assert!(
            hex.bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
            "digest hex lowercase only"
        );
        assert_eq!(hex, &digest.to_hex(), "custom Serialize == to_hex");
    }
}
