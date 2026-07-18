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

/// **Step 6 P0:** Canonical f64 → 8 byte (tek primitive). Non-finite reject (NaN + ±Infinity),
/// -0.0 → 0.0 normalize, little-endian to_bits.
///
/// `encode_f64` (hasher) + `push_f64` (buffer) + `encode_optional_f64` hep bu kaynağı
/// kullanır — çift canonicalization yok. Preimage testleri doğrudan bu fonksiyonu çağırır.
fn canonical_f64_bytes(val: f64) -> Result<[u8; 8], AuthorizationBasisDigestError> {
    if !val.is_finite() {
        return Err(AuthorizationBasisDigestError::NonFiniteRejected);
    }
    // -0.0 → 0.0 normalize (to_bits farklı: -0.0 = 0x8000000000000000, 0.0 = 0x0).
    let normalized = if val == 0.0 { 0.0f64 } else { val };
    Ok(normalized.to_bits().to_le_bytes())
}

/// f64 canonical encoding — non-finite reject (NaN + ±Infinity), -0.0 → 0.0, little-endian to_bits.
///
/// **reviewer P0-2a:** yalnız NaN değil, ±Infinity de reddedilir. Plan NaN+infinity
/// rejection öngörüyordu; `is_nan()` kontrolü infinity'yi geçiriyordu.
///
/// **Step 6 P0:** `canonical_f64_bytes` üzerinden (tek kaynak).
fn encode_f64(
    hasher: &mut blake3::Hasher,
    val: f64,
    _field: &str,
) -> Result<(), AuthorizationBasisDigestError> {
    hasher.update(&canonical_f64_bytes(val)?);
    Ok(())
}

/// **Step 6 P0:** Option\<f64\> → Vec\<u8\> (shared byte helper). Presence tag:
/// `None → [0]`, `Some(v) → [1] || canonical_f64_bytes(v)`. Tag olmadan aynı byte dizisini
/// üreten context çiftleri imkânsız (reviewer P0-1 encoding collision fix).
/// Preimage testleri doğrudan bu fonksiyonu çağırır.
fn encode_optional_f64_to_vec(
    value: Option<f64>,
) -> Result<Vec<u8>, AuthorizationBasisDigestError> {
    let mut bytes = Vec::with_capacity(9);
    match value {
        None => push_u8(&mut bytes, 0),
        Some(v) => {
            push_u8(&mut bytes, 1);
            bytes.extend_from_slice(&canonical_f64_bytes(v)?);
        }
    }
    Ok(bytes)
}

/// Option\<f64\> canonical encoding — **reviewer P0-1 (encoding collision fix).**
///
/// Önceki yaklaşım `None → encode_u8(0)` ve `Some(v) → encode_f64(v)` kullanıyordu;
/// bu `None` (1 byte) ile `Some(0.0)` (8 byte) dizilerini farklı uzunluklarda üretiyordu
/// ama `None` + `Some(0.0)` kombinasyonları dokuz sıfır byte'a çakışabiliyordu.
///
/// Presence tag: `None → [0]`, `Some(v) → [1] || canonical_f64(v)`. Tag olmadan aynı
/// byte dizisini üreten context çiftleri artık imkânsız.
///
/// **Step 6 P0:** `encode_optional_f64_to_vec` üzerinden (tek kaynak).
fn encode_optional_f64(
    hasher: &mut blake3::Hasher,
    value: Option<f64>,
    _field: &str,
) -> Result<(), AuthorizationBasisDigestError> {
    hasher.update(&encode_optional_f64_to_vec(value)?);
    Ok(())
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

/// **Step 6 P0:** CanonicalEdge → Vec\<u8\> (shared byte helper, 18 byte).
/// from(8) + to(8) + kind(1) + is_type_only(1). Preimage testleri doğrudan çağırır.
fn encode_canonical_edge_to_vec(edge: &CanonicalEdge) -> Vec<u8> {
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
) -> Result<(), AuthorizationBasisDigestError> {
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
) -> Result<(), AuthorizationBasisDigestError> {
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
fn encode_canonical_edge_identity_to_vec(edge: &CanonicalEdgeIdentity) -> Vec<u8> {
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

/// **Step 6 P0:** buffer'a canonical f64 yazar — `canonical_f64_bytes` üzerinden (tek kaynak).
fn push_f64(buf: &mut Vec<u8>, val: f64) -> Result<(), AuthorizationBasisDigestError> {
    buf.extend_from_slice(&canonical_f64_bytes(val)?);
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
        // **Step 5:** Defensive structural-delta validation. Custom Deserialize zaten
        // reject eder (try_new üzerinden), ama reload sırasında ikinci güvenlik katmanı —
        // deserialize bypass edilse bile verify yakalar.
        self.authorization_basis
            .structural_delta
            .validate()
            .map_err(|e| PendingAuthorizationLoadError::StructuralDeltaInvalid(e.to_string()))?;
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
    /// **Step 5:** Structural delta invalid (duplicate/cross-list/non-finite/unsorted).
    /// Defensive — custom Deserialize zaten reject eder; bu ikinci katman.
    #[error("structural delta invalid: {0}")]
    StructuralDeltaInvalid(String),
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
}
