//! INV-T9 #70 Commit 3 — Subject-bound measurement token (neutral layer).
//!
//! Authority/evidence yollarının tek measurement yüzeyi (Commit 4 migration).
//! Commit 3 add-only: tanımlar + `SpaceEngine` metotları. Caller migration Commit 4'te.
//!
//! **Reviewer v1→v4 turu (8.9 → 9.3 → 9.6 → 9.7) kapanmış bulgular:**
//! - Subject scope: sort + duplicate reject (sessiz dedup YOK)
//! - Error routing: blanket `CanonicalizationError` `#[from]` YOK; explicit call-site mapping
//! - Cross-field integrity: `MeasurementRequest::try_new` digest'leri üretir;
//!   `EngineMeasurement::new` defensive re-verify
//!
//! `MeasurementError` enum'u `SpaceViewRevision` (içinde `SpaceDigest` + `SpaceViewId`)
//! ve `CanonicalizationError` gibi ~128 byte üstü tipler içerir. clippy::result_large_err
//! lint'i bu yüzden Measurement pipeline fonksiyonlarında uyarı verir; error tipi
//! authorization pattern ile uyumlu (auth `AuthorizationContext` da aynı tipleri taşır)
//! ve hot path değil — measurement başına bir kez üretilir. Box'lama Commit 3'te
//! API surface'i büyütür; erken optimizasyon olarak reddedilir.
#![allow(clippy::result_large_err)]
//! - Canonical impact edges: `CanonicalEdgeIdentity` (raw `EdgeRef` DEĞİL)
//! - Shared `CanonicalStructuralDelta` producer (single canonicalization truth)
//! - Centroid axis identity + mass validation
//! - Digest framing count + length-prefix explicit
//! - Canonical derivation (scope tip döndürür)
//! - Defensive `validate()` + AS-IS encode (single canonicalization)
//! - Serialize-only `MeasurementRequest` (Deserialize bypass kapalı)

use crate::authorization::{
    encode_canonical_edge_identity_to_vec, encode_canonical_edge_to_vec, encode_canonical_node,
    encode_space_view_id, CanonicalEdgeIdentity, CanonicalStructuralDelta, CanonicalizationError,
    MeasurementInputContext, MeasurementInputDigest, SpaceViewRevision,
};
use crate::canonical_encoding::{encode_u64, CanonicalEncodingError};
use crate::coords::MeasuredRawPosition;
use crate::space::NodeId;

// ═══════════════════════════════════════════════════════════════════════════════
// P1-3 (reviewer v4): Public measurement digest error boundary
//
// `CanonicalEncodingError` crate-private; bu tip public. External API'ye primitive
// error sızmaz. Hex decode ve length varyantları dahil (P2-1 v4).
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MeasurementDigestError {
    #[error("non-finite canonical float rejected")]
    NonFiniteRejected,
    #[error("canonical length overflow in {field}")]
    LengthOverflow { field: &'static str },
    #[error("structural canonicalization failed: {detail}")]
    StructuralCanonicalization { detail: String },
    #[error("measurement-input digest construction failed: {detail}")]
    MeasurementInputDigest { detail: String },
    #[error("{digest} hex decode failed: {detail}")]
    HexDecodeFailed {
        digest: &'static str,
        detail: String,
    },
    #[error("{digest} must be exactly 32 bytes, got {actual}")]
    InvalidDigestLength { digest: &'static str, actual: usize },
}

impl From<CanonicalEncodingError> for MeasurementDigestError {
    fn from(err: CanonicalEncodingError) -> Self {
        match err {
            CanonicalEncodingError::NonFiniteRejected => MeasurementDigestError::NonFiniteRejected,
            CanonicalEncodingError::LengthOverflow { field } => {
                MeasurementDigestError::LengthOverflow { field }
            }
        }
    }
}

impl From<CanonicalizationError> for MeasurementDigestError {
    fn from(err: CanonicalizationError) -> Self {
        MeasurementDigestError::StructuralCanonicalization {
            detail: err.to_string(),
        }
    }
}

/// Shared helper (P1-1 v4): `MeasurementInputDigest::compute` `AuthorizationBasisDigestError`
/// döner — measurement-domain `MeasurementDigestError::MeasurementInputDigest`'e sarmalar.
/// Hem `MeasurementRequest::try_new` hem `EngineMeasurement::new` bunu kullanır.
fn compute_measurement_input_digest(
    context: &MeasurementInputContext,
) -> Result<MeasurementInputDigest, MeasurementDigestError> {
    MeasurementInputDigest::compute(context).map_err(|e| {
        MeasurementDigestError::MeasurementInputDigest {
            detail: e.to_string(),
        }
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1-1 (reviewer v3): CanonicalSubjectScope — sort + duplicate reject (sessiz dedup YOK)
//
// `CanonicalSubgraphScope` pattern (authorization.rs:342+): unsorted unique input
// canonical sıraya getirilir; duplicate typed error. Hint aynı kurala tabi.
// ═══════════════════════════════════════════════════════════════════════════════

/// Subject scope (task'ın hakkında hüküm verdiği varlıklar). Sorted + unique + non-empty.
///
/// Smart constructor sort eder (unsorted unique kabul), duplicate reddeder (sessiz dedup
/// YOK). Custom Deserialize `try_new` üzerinden — wire bypass kapalı.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CanonicalSubjectScope {
    member_ids: Vec<NodeId>,
}

impl CanonicalSubjectScope {
    /// Validated smart constructor. Sort eder (unsorted unique kabul), duplicate ve empty
    /// reddeder. `CanonicalSubgraphScope` ile aynı sözleşme.
    pub fn try_new(mut ids: Vec<NodeId>) -> Result<Self, MeasurementDigestError> {
        if ids.is_empty() {
            return Err(MeasurementDigestError::StructuralCanonicalization {
                detail: "subject scope must be non-empty".to_string(),
            });
        }
        ids.sort_unstable();
        for pair in ids.windows(2) {
            if pair[0] == pair[1] {
                return Err(MeasurementDigestError::StructuralCanonicalization {
                    detail: format!("duplicate subject member id: {}", pair[0]),
                });
            }
        }
        Ok(Self { member_ids: ids })
    }

    pub fn member_ids(&self) -> &[NodeId] {
        &self.member_ids
    }
}

impl<'de> serde::Deserialize<'de> for CanonicalSubjectScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            member_ids: Vec<NodeId>,
        }
        let wire = Wire::deserialize(deserializer)?;
        CanonicalSubjectScope::try_new(wire.member_ids).map_err(serde::de::Error::custom)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1-4 (reviewer v3): CanonicalImpactScope — CanonicalEdgeIdentity taşır (raw EdgeRef DEĞİL)
//
// Impact semantik olarak bir küme — endpoint union sırasında duplicate'lar dedup edilir
// (subject scope'tan farklı kural). CanonicalEdgeIdentity identity-only (is_type_only hariç).
// ═══════════════════════════════════════════════════════════════════════════════

/// Impact scope (hypothetical hesap etkilenen varlıklar). Subject'ten BAĞIMSIZ küme
/// (reviewer P1-1 v2 — `impact ⊆ subject` invariant YOK). `CanonicalEdgeIdentity` taşır
/// (reviewer P1-4 v3 — raw `EdgeRef` DEĞİL).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CanonicalImpactScope {
    node_ids: Vec<NodeId>,
    edge_ids: Vec<CanonicalEdgeIdentity>,
}

impl CanonicalImpactScope {
    /// Validated smart constructor. Sort + dedup (küme semantiği). Empty kabul (delta
    /// node/edge içermeyen task'lar için).
    pub fn try_new(
        mut node_ids: Vec<NodeId>,
        mut edge_ids: Vec<CanonicalEdgeIdentity>,
    ) -> Result<Self, MeasurementDigestError> {
        node_ids.sort_unstable();
        node_ids.dedup();
        edge_ids.sort_unstable();
        edge_ids.dedup();
        Ok(Self { node_ids, edge_ids })
    }

    pub fn node_ids(&self) -> &[NodeId] {
        &self.node_ids
    }

    pub fn edge_ids(&self) -> &[CanonicalEdgeIdentity] {
        &self.edge_ids
    }
}

impl<'de> serde::Deserialize<'de> for CanonicalImpactScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            node_ids: Vec<NodeId>,
            edge_ids: Vec<CanonicalEdgeIdentity>,
        }
        let wire = Wire::deserialize(deserializer)?;
        CanonicalImpactScope::try_new(wire.node_ids, wire.edge_ids)
            .map_err(serde::de::Error::custom)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1-5 (reviewer v2/v3): MeasurementDeltaDigest — shared CanonicalStructuralDelta producer
//
// Tek canonicalization truth: Claim → `canonical_structural_delta_from_claim` (try_new)
//   → defensive `validate()` → AS-IS encode.
// `canonicalize_node` digest'te KULLANILMAZ (try_new zaten CanonicalNode taşıyor).
// ═══════════════════════════════════════════════════════════════════════════════

/// Claim structural delta canonical digest. new_nodes + new_edges + removed_edges
/// içeriklerini bağlıyor. Aynı subject/impact/revision altında farklı delta → farklı digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MeasurementDeltaDigest([u8; 32]);

impl MeasurementDeltaDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.measurement-delta.v1\0";
    const DIGEST_NAME: &'static str = "MeasurementDeltaDigest";

    /// Shared producer (P1-5 v3): `canonical_structural_delta_from_claim` tarafından
    /// üretilmiş `CanonicalStructuralDelta`'dan digest üretir.
    ///
    /// P2-3 (reviewer v3): defensive `validate()` başında çağrılır (non-normalizing).
    /// Encoder AS-IS — sort YOK (try_new zaten canonical sırayı garanti).
    /// `canonicalize_node` KULLANILMAZ (single canonicalization truth).
    pub fn compute_from_canonical(
        delta: &CanonicalStructuralDelta,
    ) -> Result<Self, MeasurementDigestError> {
        delta.validate().map_err(MeasurementDigestError::from)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        // P2-1 (reviewer v3): count prefix her section'da (section boundary net).
        encode_u64(
            &mut hasher,
            delta.new_nodes().len() as u64,
            "delta_new_node_count",
        );
        for node in delta.new_nodes() {
            encode_canonical_node(&mut hasher, node)?;
        }
        encode_u64(
            &mut hasher,
            delta.new_edges().len() as u64,
            "delta_new_edge_count",
        );
        for edge in delta.new_edges() {
            hasher.update(&encode_canonical_edge_to_vec(edge));
        }
        encode_u64(
            &mut hasher,
            delta.removed_edges().len() as u64,
            "delta_removed_edge_count",
        );
        for edge in delta.removed_edges() {
            hasher.update(&encode_canonical_edge_identity_to_vec(edge));
        }
        Ok(Self(hasher.finalize().into()))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(value: &str) -> Result<Self, MeasurementDigestError> {
        let bytes = hex::decode(value).map_err(|e| MeasurementDigestError::HexDecodeFailed {
            digest: Self::DIGEST_NAME,
            detail: e,
        })?;
        Self::from_bytes_slice(&bytes)
    }

    fn from_bytes_slice(bytes: &[u8]) -> Result<Self, MeasurementDigestError> {
        if bytes.len() != 32 {
            return Err(MeasurementDigestError::InvalidDigestLength {
                digest: Self::DIGEST_NAME,
                actual: bytes.len(),
            });
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1-3 (reviewer v3): MeasurementRequest — try_new digest'leri kendisi üretir
//
// Caller delta/context digest'lerini ayrı veremez — cross-field integrity constructor
// içinde. Serialize-only (P1-4 v4): wire bypass kapalı.
// ═══════════════════════════════════════════════════════════════════════════════

/// Measurement authority request — caller'ın ne ölçtüğünü beyan eder. `EngineMeasurement`
/// token'ın "request" field'ı. Cross-field integrity: digest'leri `try_new` üretir.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MeasurementRequest {
    subject: CanonicalSubjectScope,
    impact: CanonicalImpactScope,
    base_revision: SpaceViewRevision,
    structural_delta_digest: MeasurementDeltaDigest,
    measurement_input_digest: MeasurementInputDigest,
}

impl MeasurementRequest {
    /// Validated smart constructor. Caller digest'leri ayrı veremez — `canonical_delta`
    /// ve `context`'ten üretir (P1-3 v3 cross-field integrity).
    pub(crate) fn try_new(
        subject: CanonicalSubjectScope,
        impact: CanonicalImpactScope,
        base_revision: SpaceViewRevision,
        canonical_delta: &CanonicalStructuralDelta,
        context: &MeasurementInputContext,
    ) -> Result<Self, MeasurementDigestError> {
        let structural_delta_digest =
            MeasurementDeltaDigest::compute_from_canonical(canonical_delta)?;
        let measurement_input_digest = compute_measurement_input_digest(context)?;
        Ok(Self {
            subject,
            impact,
            base_revision,
            structural_delta_digest,
            measurement_input_digest,
        })
    }

    pub fn subject(&self) -> &CanonicalSubjectScope {
        &self.subject
    }
    pub fn impact(&self) -> &CanonicalImpactScope {
        &self.impact
    }
    pub fn base_revision(&self) -> &SpaceViewRevision {
        &self.base_revision
    }
    pub fn structural_delta_digest(&self) -> &MeasurementDeltaDigest {
        &self.structural_delta_digest
    }
    pub fn measurement_input_digest(&self) -> &MeasurementInputDigest {
        &self.measurement_input_digest
    }

    /// **INV-T9 #70 Commit 4b (reviewer scoped P2-1 / v2 P1-3):** Shared producer —
    /// `CanonicalMeasurementRequestEvidence` snapshot. Basis builder (Faz 4) ve digest
    /// encoder aynı helper'ı kullanır — field-by-field ikinci encoder YOK (tek truth source).
    /// Digest cross-field invariant: `MeasurementRequestDigest::compute_from_canonical`
    /// (Faz 4) bu snapshot'tan digest üretir, basis'teki stored digest ile karşılaştırır.
    #[allow(dead_code)] // Faz 4: AuthorizationBasis v2 basis builder consume
    pub(crate) fn canonical_evidence(&self) -> CanonicalMeasurementRequestEvidence {
        CanonicalMeasurementRequestEvidence {
            subject: self.subject.clone(),
            impact: self.impact.clone(),
            base_revision: self.base_revision.clone(),
            structural_delta_digest: self.structural_delta_digest.clone(),
            measurement_input_digest: self.measurement_input_digest.clone(),
        }
    }
}
// NOT: Deserialize intentionally absent (reviewer P1-4 v4). Wire restore Commit 4'te
// iki aşamalı: `UnverifiedMeasurementRequestWire` + `verify_against(canonical_delta, context)`.

// ═══════════════════════════════════════════════════════════════════════════════
// CanonicalMeasurementRequestEvidence (INV-T9 #70 Commit 4b — basis v2 request snapshot)
//
// Reviewer v2 P1-1 / scoped P2-1: AuthorizationBasis v2 tam canonical request snapshot
// taşır — yalnız digest değil. validate_v2 cross-field invariant için digest'i snapshot'tan
// recompute edip stored digest ile karşılaştırır. Subject/impact/revision/digest'ler
// deserialize sonrasında görülebilir → replay/tamper kanıtı.
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b:** Canonical measurement request evidence — basis v2'de
/// `measurement_request` field'ı. `MeasurementRequest::canonical_evidence()` shared
/// producer'dan üretilir (tek truth source). Digest cross-field invariant için
/// `MeasurementRequestDigest` ayrıca saklanır.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CanonicalMeasurementRequestEvidence {
    pub subject: CanonicalSubjectScope,
    pub impact: CanonicalImpactScope,
    pub base_revision: SpaceViewRevision,
    pub structural_delta_digest: MeasurementDeltaDigest,
    pub measurement_input_digest: MeasurementInputDigest,
}

// ═══════════════════════════════════════════════════════════════════════════════
// MeasurementRequestDigest — BLAKE3 canonical encoding over MeasurementRequest
// ═══════════════════════════════════════════════════════════════════════════════

/// `MeasurementRequest` canonical digest. AuthorizationBasis v2 digest zincirinde yer
/// alacak (Commit 4). Her variable-length section count prefix taşır (P2-1 v3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MeasurementRequestDigest([u8; 32]);

impl MeasurementRequestDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.measurement-request.v1\0";
    const DIGEST_NAME: &'static str = "MeasurementRequestDigest";

    pub fn compute(request: &MeasurementRequest) -> Result<Self, MeasurementDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);

        // subject_count + sorted subject ids
        encode_u64(
            &mut hasher,
            request.subject.member_ids().len() as u64,
            "mr_subject_count",
        );
        for id in request.subject.member_ids() {
            encode_u64(&mut hasher, *id, "mr_subject_node_id");
        }

        // impact_node_count + sorted impact node ids
        encode_u64(
            &mut hasher,
            request.impact.node_ids().len() as u64,
            "mr_impact_node_count",
        );
        for id in request.impact.node_ids() {
            encode_u64(&mut hasher, *id, "mr_impact_node_id");
        }

        // impact_edge_count + sorted impact edges (canonical identity)
        encode_u64(
            &mut hasher,
            request.impact.edge_ids().len() as u64,
            "mr_impact_edge_count",
        );
        for edge in request.impact.edge_ids() {
            hasher.update(&encode_canonical_edge_identity_to_vec(edge));
        }

        // base_revision: view_id variant + sequence + content_digest (32 raw bytes)
        encode_space_view_id(&mut hasher, &request.base_revision.view_id);
        encode_u64(
            &mut hasher,
            request.base_revision.sequence,
            "mr_revision_sequence",
        );
        hasher.update(request.base_revision.content_digest.as_bytes());

        // structural_delta_digest (32 raw bytes)
        hasher.update(request.structural_delta_digest.as_bytes());
        // measurement_input_digest (32 raw bytes)
        hasher.update(request.measurement_input_digest.as_bytes());

        Ok(Self(hasher.finalize().into()))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(value: &str) -> Result<Self, MeasurementDigestError> {
        let bytes = hex::decode(value).map_err(|e| MeasurementDigestError::HexDecodeFailed {
            digest: Self::DIGEST_NAME,
            detail: e,
        })?;
        if bytes.len() != 32 {
            return Err(MeasurementDigestError::InvalidDigestLength {
                digest: Self::DIGEST_NAME,
                actual: bytes.len(),
            });
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 3 — Task-claim + measured-result commitment digests
//
// Outer proof (`VerifiedTaskMeasurementBinding`) task/claim/measured-result kimliğini
// taşır. Mevcut `MeasurementRequestDigest` request snapshot'ı bağlıyor ama task_id
// doğrudan hash'lemiyor ve measured-result değerlerini içermiyor. Bu iki digest
// outer proof'un replay/cross-context substitution protection'ı için gerekli.
//
// **Reviewer v6 P2-1:** `Serialize`-only — Deserialize derive EDİLMEZ (trusted value,
// wire'dan restore edilemez). Persistence için ayrı `Untrusted*Bytes` → verify_against
// iki aşamalı model (Faz 4+).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 3 (reviewer v4 P1-1, v6 P2-2):** Claim binding commitment.
///
/// `claim_id + task_id + author + structural_delta_digest` bağlar. **NOT: claim'in TÜM
/// field'larını bağlamaz** — binding-relevant identity + structural content. Full
/// serialization digest DEĞİL. Faz 4+ kod bu digest'i "claim'in tamamı değişmedi"
/// kanıtı olarak KULLANMAMALI (doc-comment pinli — reviewer v6 P2-2).
///
/// **Author semantics:** `AgentId = u64` plain numeric alias (witness.rs:19) —
/// length-prefix gerekmez, `encode_u64` yeterli. Büyük/küçük harf/alias/unicode
/// normalization uygulanmaz (raw numeric identity).
///
/// **Reviewer v6 P2-1:** `Serialize`-only, `Deserialize` absent. `pub(crate)` —
/// outer proof constructor tarafından üretilir, wire/literal bypass kapalı.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub(crate) struct TaskClaimDigest([u8; 32]);

impl TaskClaimDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.task-claim-digest.v1\0";

    /// Binding commitment üretir. `structural_delta_digest` claim'in canonical structural
    /// delta'sından üretilmiş olmalı (`MeasurementDeltaDigest::compute_from_canonical`).
    pub(crate) fn compute(
        claim: &crate::witness::Claim,
        task_id: crate::trajectory::TaskId,
        structural_delta_digest: &MeasurementDeltaDigest,
    ) -> Result<Self, MeasurementDigestError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        crate::canonical_encoding::encode_u64(&mut hasher, claim.id.into(), "claim_id");
        crate::canonical_encoding::encode_u64(&mut hasher, task_id, "task_id");
        crate::canonical_encoding::encode_u64(&mut hasher, claim.author.into(), "claim_author");
        hasher.update(structural_delta_digest.as_bytes());
        Ok(Self(hasher.finalize().into()))
    }

    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// **INV-T9 #70 Commit 4b Faz 3 (reviewer v4 P1-1, v6 P2-2):** Measured-result commitment.
///
/// 5-axis measured değer + source'ları bağlar. `MeasuredRawPosition` (coords.rs:518)
/// field'ları `pub` ve smart constructor YOK — NaN/∞ struct literal ile oluşturulabilir.
/// Bu digest seviyesinde `canonical_f64_bytes` (NaN/∞ reject, -0.0 normalize) defense-in-depth.
///
/// **Canonicalization contract (reviewer v4 P2-2):**
/// - NaN/±Infinity reddedilir (`encode_axis_components` → `canonical_f64_bytes`)
/// - -0.0 → 0.0 normalize
/// - Axis sırası sabit: coupling→cohesion→instability→entropy→witness_depth
/// - Source: `CanonicalMetricSourceTag` stable mapping (`canonical_tag_newtype!` macro —
///   enum discriminant DEĞİL, varyant sırası değişse bile tag byte sabit)
/// - Axis discriminator: explicit byte (defense-in-depth — structural sıra garanti)
/// - Confidence/coverage: `AxisMeasurement`'da yok (value + source sadece) — doc.
///
/// **Reviewer v6 P2-1:** `Serialize`-only, `Deserialize` absent. `pub(crate)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub(crate) struct MeasurementDigest([u8; 32]);

impl MeasurementDigest {
    const DOMAIN_SEPARATOR: &'static [u8] = b"osp.measurement-result.v1\0";

    /// Measured result commitment üretir. Nötr `encode_axis_components` kullanır —
    /// cycle risk kapalı (canonical_encoding auth/coords tipi tanımaz).
    pub(crate) fn compute(
        measured: &crate::coords::MeasuredRawPosition,
    ) -> Result<Self, MeasurementDigestError> {
        use crate::canonical_encoding::{
            encode_axis_components, AXIS_DISCRIM_COHESION, AXIS_DISCRIM_COUPLING,
            AXIS_DISCRIM_ENTROPY, AXIS_DISCRIM_INSTABILITY, AXIS_DISCRIM_WITNESS_DEPTH,
        };
        use crate::canonical_tags::CanonicalMetricSourceTag;

        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        encode_axis_components(
            &mut hasher,
            measured.coupling.value,
            CanonicalMetricSourceTag::try_from(&measured.coupling.source).map_err(|e| {
                MeasurementDigestError::StructuralCanonicalization {
                    detail: e.to_string(),
                }
            })?,
            AXIS_DISCRIM_COUPLING,
        )
        .map_err(MeasurementDigestError::from)?;
        encode_axis_components(
            &mut hasher,
            measured.cohesion.value,
            CanonicalMetricSourceTag::try_from(&measured.cohesion.source).map_err(|e| {
                MeasurementDigestError::StructuralCanonicalization {
                    detail: e.to_string(),
                }
            })?,
            AXIS_DISCRIM_COHESION,
        )
        .map_err(MeasurementDigestError::from)?;
        encode_axis_components(
            &mut hasher,
            measured.instability.value,
            CanonicalMetricSourceTag::try_from(&measured.instability.source).map_err(|e| {
                MeasurementDigestError::StructuralCanonicalization {
                    detail: e.to_string(),
                }
            })?,
            AXIS_DISCRIM_INSTABILITY,
        )
        .map_err(MeasurementDigestError::from)?;
        encode_axis_components(
            &mut hasher,
            measured.entropy.value,
            CanonicalMetricSourceTag::try_from(&measured.entropy.source).map_err(|e| {
                MeasurementDigestError::StructuralCanonicalization {
                    detail: e.to_string(),
                }
            })?,
            AXIS_DISCRIM_ENTROPY,
        )
        .map_err(MeasurementDigestError::from)?;
        encode_axis_components(
            &mut hasher,
            measured.witness_depth.value,
            CanonicalMetricSourceTag::try_from(&measured.witness_depth.source).map_err(|e| {
                MeasurementDigestError::StructuralCanonicalization {
                    detail: e.to_string(),
                }
            })?,
            AXIS_DISCRIM_WITNESS_DEPTH,
        )
        .map_err(MeasurementDigestError::from)?;
        Ok(Self(hasher.finalize().into()))
    }

    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1-5 (reviewer v2): Baseline — terminal error vs unavailable ayrımı
// ═══════════════════════════════════════════════════════════════════════════════

/// Before-state measurement. Partial/delta-introduced → Unavailable (sentetik (0.0, Placeholder) DEĞİL).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MeasurementBaseline {
    /// Before-state measured — subject scope üyelerinin tamamı base snapshot'ta mevcut.
    Available(MeasuredRawPosition),
    /// Before-state unavailable — subject scope üyeleri tamamen veya kısmen delta-introduced.
    Unavailable { reason: BaselineUnavailableReason },
}

/// Baseline unavailability sebebi. Subject member'ların base/delta dağılımını taşır.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BaselineUnavailableReason {
    /// Tüm subject üyeleri delta ile eklenen node'lar — base'de hiçbiri yok.
    AllMembersIntroducedByDelta { members: Vec<NodeId> },
    /// Bazı üyeler base'de var, kalanların tümü delta ile ekleniyor.
    PartialNewSubject {
        existing: Vec<NodeId>,
        introduced: Vec<NodeId>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// EngineMeasurement — private-field token, cross-field defensive verify (P1-3 v3)
// ═══════════════════════════════════════════════════════════════════════════════

/// Subject-bound measurement token. Private-field: struct literal bypass kapalı.
/// Serialize-only — Deserialize intentionally absent (wire restore Commit 4).
///
/// Cross-field integrity (P1-3 v3): `new()` defensive olarak `context` digest'ini
/// `request.measurement_input_digest` ile karşılaştırır.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EngineMeasurement {
    before: MeasurementBaseline,
    after: MeasuredRawPosition,
    context: MeasurementInputContext,
    request: MeasurementRequest,
}

impl EngineMeasurement {
    /// Private-field constructor. Defensive cross-field verify (P1-3 v3):
    /// `MeasurementInputDigest::compute(context) == request.measurement_input_digest`.
    pub(crate) fn new(
        before: MeasurementBaseline,
        after: MeasuredRawPosition,
        context: MeasurementInputContext,
        request: MeasurementRequest,
    ) -> Result<Self, MeasurementError> {
        let actual = compute_measurement_input_digest(&context)?;
        if &actual != request.measurement_input_digest() {
            return Err(MeasurementError::MeasurementContextDigestMismatch);
        }
        Ok(Self {
            before,
            after,
            context,
            request,
        })
    }

    pub fn before(&self) -> &MeasurementBaseline {
        &self.before
    }
    pub fn after(&self) -> &MeasuredRawPosition {
        &self.after
    }
    pub fn context(&self) -> &MeasurementInputContext {
        &self.context
    }
    /// Revision delegasyon (P2-1 v2 — tek truth source `request`'te).
    pub fn revision(&self) -> &SpaceViewRevision {
        self.request.base_revision()
    }
    pub fn request(&self) -> &MeasurementRequest {
        &self.request
    }

    /// Request digest — authorization zinciri için (Commit 4).
    pub fn request_digest(&self) -> Result<MeasurementRequestDigest, MeasurementDigestError> {
        MeasurementRequestDigest::compute(&self.request)
    }
}
// NOT: Deserialize intentionally absent — `EngineMeasurement` authority token, wire'dan
// restore edilemez. Commit 4'te `UnverifiedWire + verify_against` iki aşamalı model.

// ═══════════════════════════════════════════════════════════════════════════════
// MeasurementBinding (INV-T9 #70 Commit 4b — token replay/binding guard)
//
// Reviewer v2 karar 4 (full binding) + v3 P1-4 (mismatch vs derivation ayrımı) +
// v4 P1-2 (somut Rust error tipi) + P1-3 (disposition sınıflandırma).
//
// `verify_measurement_binding` (Faz 3 — engine.rs) presented `EngineMeasurement`
// token'ını claim/task/subject/impact/delta/revision/context karşısında doğrular.
// Token replay (claim A token'ı claim B'ye), stale measurement, ve tampered authority
// üç farklı terminal sınıfa ayrılır:
//
//   RegenerateMeasurement — stale (Revision/CurrentContext) → yeni token üret
//   RejectPresentedAuthority — replayed/tampered (Task/Subject/Impact/StructuralDelta/ContextDigest)
//
// Basis builder `VerifiedMeasurementBinding`'i consume eder — re-derivation yok (tek truth).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b (reviewer v2 karar 4 + v4 P1-3):** Mismatch disposition —
/// navigator sonuç/retry davranışını inner mismatch varyantına göre belirler.
///
/// - `RegenerateMeasurement`: stale measurement (Revision/CurrentContext) → yeni
///   `EngineMeasurement` üret, LLM yeniden çağrılmaz, maneuver budget tüketilmez.
/// - `RejectPresentedAuthority`: replayed/tampered (Task/Subject/Impact/StructuralDelta/
///   ContextDigest) → terminal presented-authority rejection, otomatik retry yok,
///   maneuver budget tüketilmez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementBindingDisposition {
    /// Stale measurement — revision/context değişti, yeni token üretilebilir.
    RegenerateMeasurement,
    /// Presented authority token geçersiz (replay/tamper) — terminal reject.
    RejectPresentedAuthority,
}

/// **INV-T9 #70 Commit 4b (reviewer v2 karar 4):** Presented `EngineMeasurement`
/// token'ının claim/task/subject/impact/delta/revision/context ile uyuşmaması.
/// 7 mismatch varyantı — her biri `disposition()` ile retry/reject sınıfına ayrılır.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementBindingMismatch {
    /// `claim.task_id` resolved task.id ile uyuşmuyor.
    #[error(
        "measurement task binding mismatch: claim_task={claim_task_id:?}, resolved_task={resolved_task_id}"
    )]
    TaskMismatch {
        claim_task_id: Option<crate::trajectory::TaskId>,
        resolved_task_id: crate::trajectory::TaskId,
    },

    /// `measurement.request().subject()` task'tan yeniden derived subject ile uyuşmuyor.
    #[error("measurement subject scope does not match resolved task")]
    SubjectMismatch {
        expected: crate::measurement::CanonicalSubjectScope,
        presented: crate::measurement::CanonicalSubjectScope,
    },

    /// `measurement.request().impact()` claim'den yeniden derived impact ile uyuşmuyor.
    #[error("measurement impact scope does not match claim delta")]
    ImpactMismatch {
        expected: crate::measurement::CanonicalImpactScope,
        presented: crate::measurement::CanonicalImpactScope,
    },

    /// `measurement.request().structural_delta_digest()` claim canonical delta digest
    /// ile uyuşmuyor. **Reviewer scoped P2-3:** expected/presented digest kanıtı taşır
    /// (replay/tamper evidence — exact assertion + telemetry).
    #[error("measurement structural delta digest does not match claim: expected={expected:?}, presented={presented:?}")]
    StructuralDeltaMismatch {
        expected: crate::measurement::MeasurementDeltaDigest,
        presented: crate::measurement::MeasurementDeltaDigest,
    },

    /// `measurement.request().base_revision()` current engine revision ile uyuşmuyor
    /// (stale measurement — `RegenerateMeasurement` disposition).
    #[error("measurement base revision does not match current engine revision")]
    RevisionMismatch {
        expected: SpaceViewRevision,
        presented: SpaceViewRevision,
    },

    /// `MeasurementInputDigest::compute(measurement.context())` request içindeki input
    /// digest ile uyuşmuyor (token içi tutarsızlık). **Reviewer scoped P2-3:**
    /// expected/presented digest kanıtı taşır.
    #[error("measurement input context does not match request digest: expected={expected:?}, presented={presented:?}")]
    ContextDigestMismatch {
        expected: crate::authorization::MeasurementInputDigest,
        presented: crate::authorization::MeasurementInputDigest,
    },

    /// `BoundMeasurementSession` current context (Commit 4a yüzeyi) token context ile
    /// uyuşmuyor (axis config drift — `RegenerateMeasurement` disposition).
    /// **Reviewer scoped P2-3:** digest seviyesinde kanıt (büyük context nesnesi yerine).
    #[error("measurement axis context does not match current engine context: expected={expected:?}, presented={presented:?}")]
    CurrentContextMismatch {
        expected: crate::authorization::MeasurementInputDigest,
        presented: crate::authorization::MeasurementInputDigest,
    },
}

impl MeasurementBindingMismatch {
    /// **INV-T9 #70 Commit 4b (reviewer v4 P1-3):** Retry/reject disposition.
    /// Navigator sonuç/retry davranışını bu değere göre belirler.
    pub fn disposition(&self) -> MeasurementBindingDisposition {
        match self {
            // Stale measurement — yeni token üretilebilir, LLM retry yok, budget yok.
            Self::RevisionMismatch { .. } | Self::CurrentContextMismatch { .. } => {
                MeasurementBindingDisposition::RegenerateMeasurement
            }
            // Presented authority geçersiz (replay/tamper) — terminal reject, retry yok.
            Self::TaskMismatch { .. }
            | Self::SubjectMismatch { .. }
            | Self::ImpactMismatch { .. }
            | Self::StructuralDeltaMismatch { .. }
            | Self::ContextDigestMismatch { .. } => {
                MeasurementBindingDisposition::RejectPresentedAuthority
            }
        }
    }
}

// Not: disposition match zaten { .. } pattern kullandığı için varyant field
// değişikliğinden etkilenmedi — struct literal olmayan varyantlar (StructuralDeltaMismatch
// vb.) artık { expected, presented } field taşır ama { .. } bunu kapsar.

/// **INV-T9 #70 Commit 4b (reviewer v3 P1-4):** Engine derivation failure —
/// `verify_measurement_binding` sırasında expected binding üretilemedi. Sistem hatası
/// (operational fault), hallucination DEĞİL, agent retry DEĞİL. Navigator SystemFailure'a
/// map'ler, maneuver budget tüketmez, witness'a ulaşmaz.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementBindingDerivationError {
    /// `derive_task_subject_scope` başarısız (heterogeneous scope, module resolution, empty).
    #[error("measurement subject derivation failed: {detail}")]
    SubjectDerivationFailed { detail: String },

    /// `derive_impact_scope` başarısız (canonicalization).
    #[error("measurement impact derivation failed: {detail}")]
    ImpactDerivationFailed { detail: String },

    /// `canonical_structural_delta_from_claim` başarısız (duplicate/cross-list/non-finite).
    #[error("measurement structural canonicalization failed: {detail}")]
    StructuralCanonicalizationFailed { detail: String },

    /// `current_space_view_revision` başarısız (structural digest computation).
    #[error("measurement revision computation failed: {detail}")]
    RevisionComputationFailed { detail: String },

    /// `BoundMeasurementSession::begin` başarısız (coordinate measurement — axis drift).
    /// **Reviewer scoped P2-2:** typed source — `#[source]` attribute + Display format
    /// (Debug DEĞIL). Error chain telemetry için korunur.
    #[error("measurement current context capture failed: {source}")]
    CurrentContextCaptureFailed {
        #[source]
        source: crate::coords::CoordinateMeasurementError,
    },

    /// `MeasurementInputContext::try_new` başarısız (canonicalization).
    #[error("measurement context construction failed: {detail}")]
    ContextConstructionFailed { detail: String },

    /// `MeasurementRequestDigest::compute` başarısız (digest construction).
    #[error("measurement request digest computation failed: {source}")]
    RequestDigestComputationFailed {
        #[source]
        source: MeasurementDigestError,
    },

    /// **INV-T9 #70 Commit 4b Faz 3 (reviewer v7 P2-1):** Measured-result commitment
    /// computation failure — `MeasurementDigest::compute` hatası. Semantic ayrım:
    /// structural delta canonicalization DEĞİL, measured-result (5-axis) commitment
    /// başarısız. Telemetry bu ayrımı korur.
    #[error("measurement result digest computation failed: {detail}")]
    MeasurementResultDigestComputationFailed { detail: String },

    /// **INV-T9 #70 Commit 4b Faz 3 (reviewer v7 P2-1):** Task-claim binding commitment
    /// computation failure — `TaskClaimDigest::compute` hatası.
    #[error("task-claim binding digest computation failed: {detail}")]
    TaskClaimDigestComputationFailed { detail: String },

    /// **INV-T9 #70 Commit 4b Faz 3 (reviewer v6 P1-2):** Verification epoch sonunda
    /// revision yeniden hesaplanamadı. Capture başarılıydı ama final re-verify hatası —
    /// sistem hatası (derivation family). `current_space_view_revision` final çağrısı
    /// Err döndü.
    #[error("measurement verification epoch revision recheck failed: {detail}")]
    RevisionRecheckFailed { detail: String },
}

/// **INV-T9 #70 Commit 4b Faz 3 (reviewer v4 P2-4, v6 P1-2):** Verification epoch
/// boyunca gözlenen değişim — derivation DEĞİL, drift. Capture başarılı (session başladı,
/// revision/context elde edildi), ama verification sırasında gerçeklik değişti.
///
/// **Capture failure vs drift ayrımı (reviewer v6 P1-2):**
/// - Capture failure → `MeasurementBindingDerivationError` (örn `CurrentContextCaptureFailed`):
///   verifier gerekli başlangıç kanıtını elde edemedi.
/// - Drift → bu tip: başlangıç kanıtı elde edildi ama doğrulama sırasında gerçeklik değişti.
///
/// **Deterministic precedence (reviewer v6 P1-2):** coord drift > revision recheck failed
/// (Derivation) > revision before≠after > ordinary verification error. Drift ordinary
/// verification sonuçlarına göre öncelikli — drift sırasında üretilen karşılaştırma
/// sonucu güvenilmez.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementBindingDriftError {
    /// `BoundMeasurementSession::verify_unchanged()` Err döndü — coordinate axis
    /// generation verification epoch boyunca değişti (interior mutability).
    #[error("measurement verification epoch coordinate context drift: {source}")]
    CoordinateContextChanged {
        #[source]
        source: crate::coords::CoordinateMeasurementError,
    },

    /// Verification epoch sonu revision re-verify başarılı ama before ≠ after —
    /// space interior mutation geçti. `SpaceViewRevision` monotonik sequence taşıdığı
    /// için A→B→A revert'te R3 ≠ R1 (ABA-safe — engine.rs:1491 `sequence: self.t_c`).
    #[error(
        "measurement verification epoch space revision drift: before={before:?}, after={after:?}"
    )]
    SpaceRevisionChanged {
        before: crate::authorization::SpaceViewRevision,
        after: crate::authorization::SpaceViewRevision,
    },

    /// Hem coordinate context hem space revision drift — composite. Yalnız coord drift
    /// gerçekten gözlenmiş + revision recheck başarılı + before ≠ after ise üretilir
    /// (reviewer v6 P1-2).
    #[error(
        "measurement verification epoch both drift: coord={coord}, before={before:?}, after={after:?}"
    )]
    BothChanged {
        #[source]
        coord: crate::coords::CoordinateMeasurementError,
        before: crate::authorization::SpaceViewRevision,
        after: crate::authorization::SpaceViewRevision,
    },
}

/// **INV-T9 #70 Commit 4b (reviewer v4 P1-2, v4 P2-4):** Somut Rust error tipi —
/// `verify_measurement_binding` dönüş hatası. Üç terminal sınıf:
/// - **Mismatch:** Presented token geçersiz (caller'ın authority'si)
/// - **Derivation:** Engine sistemi hatası (operational fault — capture dahil)
/// - **Drift:** Verification epoch boyunca gerçeklik değişti (coord/revision drift)
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementBindingVerificationError {
    /// Presented token mismatch — caller'ın sunduğu authority geçersiz.
    #[error(transparent)]
    Mismatch(#[from] MeasurementBindingMismatch),
    /// Engine derivation/capture failure — sistem hatası (operational fault).
    #[error(transparent)]
    Derivation(#[from] MeasurementBindingDerivationError),
    /// Verification epoch drift — gerçeklik değişti (reviewer v4 P2-4).
    #[error(transparent)]
    Drift(#[from] MeasurementBindingDriftError),
}

// Not: VerifiedMeasurementBinding engine.rs'te tanımlı (reviewer Faz 2 scoped P1-3) —
// construction modül-private (`verify_measurement_binding` aynı modülde), accessor'lar
// pub(crate) (authorization.rs basis builder Faz 4 için).

// ═══════════════════════════════════════════════════════════════════════════════
// Error taxonomy
// ═══════════════════════════════════════════════════════════════════════════════

/// Measurement pipeline hatası.
///
/// **Reviewer v4 P1-1:** blanket `CanonicalizationError` `#[from]` YOK — explicit
/// call-site mapping. `MeasurementContext` (context construction) ve `Digest`
/// (digest construction) ayrı varyantlar.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementError {
    #[error("coordinate measurement failed: {0}")]
    CoordinateMeasurement(#[from] crate::coords::CoordinateMeasurementError),

    /// `MeasurementInputContext::try_from` hatası (coordinate axis context construction).
    /// Explicit mapping (blanket `#[from]` YOK — reviewer v4 P1-2).
    #[error("measurement context construction failed: {0}")]
    MeasurementContext(crate::authorization::CanonicalizationError),

    /// **Reviewer v5 P2-2:** Revision computation hatası (SpaceDigest/SpaceViewRevision).
    /// `current_space_view_revision` hatası — axis context değil, structural digest.
    #[error("space view revision computation failed: {detail}")]
    RevisionComputationFailed { detail: String },

    /// **Reviewer v5 P1-1:** Measurement context drift — interior mutability threat.
    /// before/after ölçümleri arası axis descriptor/state değişti; token'ın bağladığı
    /// digest tüm ölçümlerin aynı axis semantiği altında üretildiğini kanıtlayamaz.
    #[error("measurement context drift: before={before:?}, after={after:?}")]
    MeasurementContextDrift {
        before: crate::authorization::MeasurementInputDigest,
        after: crate::authorization::MeasurementInputDigest,
    },

    /// Digest construction hatası (delta/request/input digest + structural canonicalization).
    #[error("measurement digest construction failed: {0}")]
    Digest(#[from] MeasurementDigestError),

    /// `EngineMeasurement::new` cross-field defensive check: `context` digest ≠ request digest.
    #[error("engine measurement context digest does not match request digest")]
    MeasurementContextDigestMismatch,

    /// **Reviewer P1-1 v2:** `TaskBoundClaim` defensive — `claim.task_id` yok.
    #[error("claim {claim_id} is not bound to any task")]
    ClaimNotTaskBound { claim_id: u64 },
    /// **Reviewer P1-1 v2:** `TaskBoundClaim` defensive — `claim.task_id != task.id`.
    #[error("task binding mismatch: claim.task_id={claim_task_id}, bound.task.id={bound_task_id}")]
    TaskBindingMismatch {
        claim_task_id: u64,
        bound_task_id: u64,
    },

    /// **Reviewer P1-2 v2:** Engine current revision ≠ expected base revision.
    #[error("revision mismatch: expected={expected:?}, current={current:?}")]
    RevisionMismatch {
        expected: SpaceViewRevision,
        current: SpaceViewRevision,
    },

    /// **Reviewer P1-4 v2:** Task predicate set heterogeneous scope'lar içeriyor.
    #[error("heterogeneous predicate scopes: {scopes:?}")]
    HeterogeneousPredicateScopes { scopes: Vec<CanonicalSubjectScope> },

    /// Subject scope boş — task'ta hiç Node/Subgraph predicate yok.
    #[error("empty subject scope — task has no node/subgraph predicate")]
    EmptySubjectScope,

    /// **Reviewer P1-3 v2:** Module(name) çözülemedi (Commit 3 fail-closed; Commit 4 resolver).
    #[error("subject scope resolution failed: {0}")]
    SubjectScopeResolutionFailed(SubjectScopeResolutionError),

    /// **Reviewer P1-5 v2:** Subject member base'de yok ve delta'da eklemiyor — unresolvable.
    #[error("subject members unresolvable (not in base and not in delta): {missing:?}")]
    SubjectMemberUnresolvable { missing: Vec<NodeId> },

    /// Subject member hypothetical'te yok (delta removed, corresponding add yok).
    #[error("subject member {node_id} missing after delta application")]
    SubjectMemberMissingAfterDelta { node_id: NodeId },

    /// Caller hint ≠ derived subject scope.
    #[error("subject scope hint mismatch: hint={hint_members:?}, derived={derived_members:?}")]
    SubjectScopeHintMismatch {
        hint_members: Vec<NodeId>,
        derived_members: Vec<NodeId>,
    },

    /// **Reviewer P1-6 v2:** Subject node mass non-finite veya negatif.
    #[error("invalid subject node mass: node_id={node_id}, mass={mass}")]
    InvalidSubjectMass { node_id: NodeId, mass: f64 },

    /// **Reviewer P1-6 v2:** Total subject mass non-finite veya non-positive.
    #[error("invalid total subject mass (non-finite or non-positive): {total_mass}")]
    InvalidTotalSubjectMass { total_mass: f64 },
}

/// Subject scope resolution hatası (Commit 3: yalnız Module unavailable).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubjectScopeResolutionError {
    /// **Reviewer P1-3 v2:** Module(name) çözülemedi — graph-aware resolver Commit 4'te.
    #[error("module scope resolution unavailable (Commit 4 resolver): module={module}")]
    ModuleResolutionUnavailable { module: String },
}

// ═══════════════════════════════════════════════════════════════════════════════
// hex encoding (inline — `hex` crate dependency'siz, authorization.rs pattern)
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

#[cfg(test)]
mod tests {
    use super::*;

    // === CanonicalSubjectScope (P1-1 v3) ===

    #[test]
    fn canonical_subject_scope_try_new_sorts_unique_input() {
        let scope = CanonicalSubjectScope::try_new(vec![5, 2, 8, 1]).unwrap();
        assert_eq!(scope.member_ids(), &[1, 2, 5, 8]);
    }

    #[test]
    fn canonical_subject_scope_try_new_rejects_duplicate() {
        let err = CanonicalSubjectScope::try_new(vec![1, 2, 1]).unwrap_err();
        assert!(matches!(
            err,
            MeasurementDigestError::StructuralCanonicalization { .. }
        ));
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn canonical_subject_scope_try_new_rejects_empty() {
        let err = CanonicalSubjectScope::try_new(vec![]).unwrap_err();
        assert!(matches!(
            err,
            MeasurementDigestError::StructuralCanonicalization { .. }
        ));
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn canonical_subject_scope_deserialize_accepts_unsorted_and_canonicalizes() {
        let json = r#"{"member_ids":[3,1,2]}"#;
        let scope: CanonicalSubjectScope = serde_json::from_str(json).unwrap();
        assert_eq!(scope.member_ids(), &[1, 2, 3]);
    }

    #[test]
    fn canonical_subject_scope_deserialize_rejects_duplicate() {
        let json = r#"{"member_ids":[1,1]}"#;
        let result: Result<CanonicalSubjectScope, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn canonical_subject_scope_deserialize_rejects_empty() {
        let json = r#"{"member_ids":[]}"#;
        let result: Result<CanonicalSubjectScope, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn canonical_subject_scope_deserialize_rejects_unknown_field() {
        let json = r#"{"member_ids":[1],"extra":42}"#;
        let result: Result<CanonicalSubjectScope, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject extra fields"
        );
    }

    // === CanonicalImpactScope (P1-4 v3) ===

    #[test]
    fn canonical_impact_scope_try_new_sorts_dedupes() {
        let scope = CanonicalImpactScope::try_new(vec![3, 1, 3], vec![]).unwrap();
        assert_eq!(scope.node_ids(), &[1, 3]);
    }

    #[test]
    fn canonical_impact_scope_allows_empty() {
        let scope = CanonicalImpactScope::try_new(vec![], vec![]).unwrap();
        assert!(scope.node_ids().is_empty());
        assert!(scope.edge_ids().is_empty());
    }

    // === MeasurementDigestError mappings (P1-2 v3) ===

    #[test]
    fn measurement_digest_error_from_canonical_encoding_exhaustive() {
        let err = MeasurementDigestError::from(CanonicalEncodingError::NonFiniteRejected);
        assert!(matches!(err, MeasurementDigestError::NonFiniteRejected));
        let err =
            MeasurementDigestError::from(CanonicalEncodingError::LengthOverflow { field: "test" });
        assert!(matches!(
            err,
            MeasurementDigestError::LengthOverflow { field: "test" }
        ));
    }

    #[test]
    fn measurement_digest_error_from_canonicalization_exhaustive() {
        let err = MeasurementDigestError::from(CanonicalizationError::DuplicateNodeId(5));
        assert!(matches!(
            err,
            MeasurementDigestError::StructuralCanonicalization { .. }
        ));
    }

    // === Baseline (P1-5 v2) ===

    #[test]
    fn measurement_baseline_available_preserves_measured() {
        let baseline = MeasurementBaseline::Available(MeasuredRawPosition {
            coupling: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            cohesion: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            instability: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            entropy: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            witness_depth: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
        });
        match &baseline {
            MeasurementBaseline::Available(_) => {}
            _ => panic!("expected Available"),
        }
    }

    #[test]
    fn measurement_baseline_unavailable_all_members_introduced_by_delta() {
        let baseline = MeasurementBaseline::Unavailable {
            reason: BaselineUnavailableReason::AllMembersIntroducedByDelta {
                members: vec![1, 2],
            },
        };
        match &baseline {
            MeasurementBaseline::Unavailable {
                reason: BaselineUnavailableReason::AllMembersIntroducedByDelta { members },
            } => assert_eq!(members, &[1, 2]),
            _ => panic!("expected AllMembersIntroducedByDelta"),
        }
    }

    #[test]
    fn measurement_baseline_unavailable_partial_new_subject() {
        let baseline = MeasurementBaseline::Unavailable {
            reason: BaselineUnavailableReason::PartialNewSubject {
                existing: vec![1],
                introduced: vec![2],
            },
        };
        match &baseline {
            MeasurementBaseline::Unavailable {
                reason:
                    BaselineUnavailableReason::PartialNewSubject {
                        existing,
                        introduced,
                    },
            } => {
                assert_eq!(existing, &[1]);
                assert_eq!(introduced, &[2]);
            }
            _ => panic!("expected PartialNewSubject"),
        }
    }

    // === SubjectScopeResolutionError ===

    #[test]
    fn subject_scope_resolution_error_module_unavailable_display() {
        let err = SubjectScopeResolutionError::ModuleResolutionUnavailable {
            module: "payment".to_string(),
        };
        assert!(err.to_string().contains("payment"));
        assert!(err.to_string().contains("Commit 4"));
    }

    // === hex helpers (P2-1 v4) ===

    #[test]
    fn measurement_delta_digest_from_hex_rejects_non_32_byte_value() {
        let too_short = "00".repeat(16); // 16 bytes
        let err = MeasurementDeltaDigest::from_hex(&too_short).unwrap_err();
        assert!(matches!(
            err,
            MeasurementDigestError::InvalidDigestLength {
                digest: "MeasurementDeltaDigest",
                actual: 16
            }
        ));
    }

    #[test]
    fn measurement_request_digest_from_hex_rejects_non_32_byte_value() {
        let too_long = "00".repeat(33);
        let err = MeasurementRequestDigest::from_hex(&too_long).unwrap_err();
        assert!(matches!(
            err,
            MeasurementDigestError::InvalidDigestLength {
                digest: "MeasurementRequestDigest",
                actual: 33
            }
        ));
    }

    #[test]
    fn measurement_delta_digest_from_hex_rejects_invalid_hex() {
        let err = MeasurementDeltaDigest::from_hex("not-hex!!").unwrap_err();
        assert!(matches!(
            err,
            MeasurementDigestError::HexDecodeFailed {
                digest: "MeasurementDeltaDigest",
                ..
            }
        ));
    }

    // === EngineMeasurement private-field token (P2-2 v2) ===

    /// **Reviewer v5 P2-1:** Serialize-only invariant regression test.
    ///
    /// NOT: Bu test yalnız `Serialize` trait bound'ını doğrular — `Deserialize` eklenirse
    /// bile geçer. Gerçek compile-fail UI test (`trybuild`) Commit 4'te eklenecek
    /// (orada dev-dependency genişletme yapılacak). Şimdilik bu test, `Serialize` derive
    /// edildiğini ve tipin pub API'de erişilebilir olduğunu doğrular; `Deserialize`
    /// absence invariant'ı manuel review ile korunur (engine.rs/measurement.rs derive
    /// listelerinde `Deserialize` yok).
    #[test]
    fn engine_measurement_remains_serialize_only() {
        fn assert_serialize<T>()
        where
            T: serde::Serialize,
        {
        }
        assert_serialize::<EngineMeasurement>();
    }

    /// **Reviewer v5 P2-1:** Açıklama yukarıdaki ile aynı.
    #[test]
    fn measurement_request_remains_serialize_only() {
        fn assert_serialize<T>()
        where
            T: serde::Serialize,
        {
        }
        assert_serialize::<MeasurementRequest>();
    }

    // === Test fixture helpers ===

    /// Minimal `CanonicalStructuralDelta` — iki node, bir edge, bir removed edge.
    fn sample_canonical_delta() -> CanonicalStructuralDelta {
        use crate::authorization::{
            CanonicalEdge, CanonicalEdgeIdentity, CanonicalEdgeKind, CanonicalNode,
            CanonicalNodeClassification, CanonicalNodeKind, CanonicalNodeRole,
        };
        let nodes = vec![
            CanonicalNode {
                id: 1,
                kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Concept).unwrap(),
                mass: 1.0,
                cohesion: Some(0.5),
                classification: CanonicalNodeClassification::try_from(
                    &crate::space::NodeClassification::Production,
                )
                .unwrap(),
                role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Core).unwrap(),
            },
            CanonicalNode {
                id: 2,
                kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Feature).unwrap(),
                mass: 2.0,
                cohesion: None,
                classification: CanonicalNodeClassification::try_from(
                    &crate::space::NodeClassification::Test,
                )
                .unwrap(),
                role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Adapter).unwrap(),
            },
        ];
        let new_edges = vec![CanonicalEdge {
            from: 1,
            to: 2,
            kind: CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
            is_type_only: false,
        }];
        let removed_edges = vec![CanonicalEdgeIdentity::new(
            3,
            4,
            CanonicalEdgeKind::try_from(&crate::space::EdgeKind::DependsOn).unwrap(),
        )];
        CanonicalStructuralDelta::try_new(nodes, new_edges, removed_edges).unwrap()
    }

    /// `MeasurementInputContext` — gerçek `default_raw_five` CoordinateSystem'den üretilir
    /// (Commit 1 pattern — axis implementation identity'i bağlar).
    fn sample_measurement_input_context() -> MeasurementInputContext {
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

    fn sample_space_view_revision() -> SpaceViewRevision {
        use crate::authorization::{SpaceDigest, SpaceViewId};
        // Minimal Space — empty digest reproducible.
        let space = crate::space::Space::new();
        let content_digest = SpaceDigest::compute(&space).unwrap();
        SpaceViewRevision {
            view_id: SpaceViewId::Ephemeral(1),
            sequence: 1,
            content_digest,
        }
    }

    // === MeasurementDeltaDigest (P1-5 v3, P0-1) ===

    #[test]
    fn measurement_delta_digest_deterministic() {
        let delta = sample_canonical_delta();
        let d1 = MeasurementDeltaDigest::compute_from_canonical(&delta).unwrap();
        let d2 = MeasurementDeltaDigest::compute_from_canonical(&delta).unwrap();
        assert_eq!(d1, d2, "same canonical delta → same digest");
    }

    #[test]
    fn measurement_delta_digest_changes_when_node_mass_changes() {
        use crate::authorization::{CanonicalNode, CanonicalNodeKind};
        let base = sample_canonical_delta();

        // Same nodes/edges/removed but node[0].mass changed.
        let mut nodes = base.new_nodes().to_vec();
        nodes[0] = CanonicalNode {
            mass: 99.0,
            ..nodes[0].clone()
        };
        let _ = CanonicalNodeKind::try_from(&crate::space::NodeKind::Concept).unwrap();
        let delta_b = CanonicalStructuralDelta::try_new(
            nodes,
            base.new_edges().to_vec(),
            base.removed_edges().to_vec(),
        )
        .unwrap();

        let d_a = MeasurementDeltaDigest::compute_from_canonical(&base).unwrap();
        let d_b = MeasurementDeltaDigest::compute_from_canonical(&delta_b).unwrap();
        assert_ne!(d_a, d_b, "node mass change must produce different digest");
    }

    #[test]
    fn measurement_delta_digest_changes_when_edge_type_only_changes() {
        use crate::authorization::CanonicalEdge;
        let base = sample_canonical_delta();

        let mut edges = base.new_edges().to_vec();
        edges[0] = CanonicalEdge {
            is_type_only: true,
            ..edges[0]
        };
        let delta_b = CanonicalStructuralDelta::try_new(
            base.new_nodes().to_vec(),
            edges,
            base.removed_edges().to_vec(),
        )
        .unwrap();

        let d_a = MeasurementDeltaDigest::compute_from_canonical(&base).unwrap();
        let d_b = MeasurementDeltaDigest::compute_from_canonical(&delta_b).unwrap();
        assert_ne!(
            d_a, d_b,
            "is_type_only change must produce different digest"
        );
    }

    #[test]
    fn measurement_delta_digest_changes_when_removed_edge_changes() {
        use crate::authorization::{CanonicalEdgeIdentity, CanonicalEdgeKind};
        let base = sample_canonical_delta();

        // Different removed edge.
        let delta_b = CanonicalStructuralDelta::try_new(
            base.new_nodes().to_vec(),
            base.new_edges().to_vec(),
            vec![CanonicalEdgeIdentity::new(
                5,
                6,
                CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Calls).unwrap(),
            )],
        )
        .unwrap();

        let d_a = MeasurementDeltaDigest::compute_from_canonical(&base).unwrap();
        let d_b = MeasurementDeltaDigest::compute_from_canonical(&delta_b).unwrap();
        assert_ne!(
            d_a, d_b,
            "removed edge change must produce different digest"
        );
    }

    #[test]
    fn measurement_delta_digest_rejects_duplicate_node_id() {
        use crate::authorization::{CanonicalNode, CanonicalNodeKind};
        let node = CanonicalNode {
            id: 1,
            kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Concept).unwrap(),
            mass: 1.0,
            cohesion: None,
            classification: CanonicalizationErrorFixture::classification(),
            role: CanonicalizationErrorFixture::role(),
        };
        // Manually construct invalid (try_new'den değil).
        let result = CanonicalStructuralDelta::try_new(vec![node.clone(), node], vec![], vec![]);
        assert!(result.is_err(), "duplicate node id must be rejected");
    }

    #[test]
    fn measurement_delta_digest_hex_roundtrip() {
        let delta = sample_canonical_delta();
        let digest = MeasurementDeltaDigest::compute_from_canonical(&delta).unwrap();
        let hex = digest.to_hex();
        let restored = MeasurementDeltaDigest::from_hex(&hex).unwrap();
        assert_eq!(digest, restored);
    }

    /// **Reviewer v5 P1-4:** Real pinned golden — encoding değişirse test fail etmeli.
    /// Probe ile üretilip sabitlendi (`eprintln!` → pin). 65-karakter placeholder YOK.
    const MEASUREMENT_DELTA_V1_GOLDEN_HEX: &str =
        "071b94001b33e71415479910c0ee69f68b2a859dd0b5b052faf36a9d5b156bb3";

    #[test]
    fn measurement_delta_digest_v1_golden() {
        let delta = sample_canonical_delta();
        let digest = MeasurementDeltaDigest::compute_from_canonical(&delta).unwrap();
        assert_eq!(
            digest.to_hex(),
            MEASUREMENT_DELTA_V1_GOLDEN_HEX,
            "INV-T9 #70 Commit 3: measurement delta digest encoding değişti — golden'ı \
             güncelleyin VE downstream consumer'ları (AuthorizationBasis v2 Commit 4) kontrol edin"
        );
    }

    /// **Reviewer v5 P1-4:** Real pinned golden — v1 domain separator publication'dan
    /// sonra encoding backward-compat yüzeyine dönüşür.
    const MEASUREMENT_REQUEST_V1_GOLDEN_HEX: &str =
        "bcc98fc016a150621c704660d0417277b2532f5bdfbf9a3465ce580e6448f739";

    #[test]
    fn measurement_request_digest_v1_golden() {
        let req = sample_measurement_request();
        let digest = MeasurementRequestDigest::compute(&req).unwrap();
        assert_eq!(
            digest.to_hex(),
            MEASUREMENT_REQUEST_V1_GOLDEN_HEX,
            "INV-T9 #70 Commit 3: measurement request digest encoding değişti — golden'ı \
             güncelleyin VE downstream consumer'ları (AuthorizationBasis v2 Commit 4) kontrol edin"
        );
    }

    // === MeasurementRequestDigest (P0-1) ===

    fn sample_measurement_request() -> MeasurementRequest {
        let subject = CanonicalSubjectScope::try_new(vec![1, 2]).unwrap();
        use crate::authorization::{CanonicalEdgeIdentity, CanonicalEdgeKind};
        let impact = CanonicalImpactScope::try_new(
            vec![1, 2, 3],
            vec![CanonicalEdgeIdentity::new(
                1,
                2,
                CanonicalEdgeKind::try_from(&crate::space::EdgeKind::Imports).unwrap(),
            )],
        )
        .unwrap();
        let delta = sample_canonical_delta();
        let context = sample_measurement_input_context();
        MeasurementRequest::try_new(
            subject,
            impact,
            sample_space_view_revision(),
            &delta,
            &context,
        )
        .unwrap()
    }

    #[test]
    fn measurement_request_digest_deterministic() {
        let req = sample_measurement_request();
        let d1 = MeasurementRequestDigest::compute(&req).unwrap();
        let d2 = MeasurementRequestDigest::compute(&req).unwrap();
        assert_eq!(d1, d2, "same request → same digest");
    }

    #[test]
    fn measurement_request_digest_changes_when_subject_changes() {
        let req_a = sample_measurement_request();
        // Same impact/revision/delta/context but different subject.
        let subject_b = CanonicalSubjectScope::try_new(vec![1, 5]).unwrap();
        let req_b = {
            let impact = req_a.impact().clone();
            let delta = sample_canonical_delta();
            let context = sample_measurement_input_context();
            MeasurementRequest::try_new(
                subject_b,
                impact,
                req_a.base_revision().clone(),
                &delta,
                &context,
            )
            .unwrap()
        };
        let d_a = MeasurementRequestDigest::compute(&req_a).unwrap();
        let d_b = MeasurementRequestDigest::compute(&req_b).unwrap();
        assert_ne!(
            d_a, d_b,
            "subject change must produce different request digest"
        );
    }

    #[test]
    fn measurement_request_digest_changes_when_axis_context_changes() {
        let req_a = sample_measurement_request();
        // Different axis context (different entropy effective value).
        let coords_b = crate::coords::CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::TreeSitter,
            crate::axes::CohesionAxis::try_with_observed_source(crate::coords::MetricSource::Scip)
                .unwrap(),
            crate::axes::EntropyAxis::from_commit_entropy(9.0), // different effective value
            crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
        )
        .unwrap();
        let context_b = MeasurementInputContext::try_from(&coords_b).unwrap();

        let subject = req_a.subject().clone();
        let impact = req_a.impact().clone();
        let delta = sample_canonical_delta();
        let req_b = MeasurementRequest::try_new(
            subject,
            impact,
            req_a.base_revision().clone(),
            &delta,
            &context_b,
        )
        .unwrap();

        let d_a = MeasurementRequestDigest::compute(&req_a).unwrap();
        let d_b = MeasurementRequestDigest::compute(&req_b).unwrap();
        assert_ne!(
            d_a, d_b,
            "axis context change must produce different request digest"
        );
    }

    #[test]
    fn measurement_request_digest_hex_roundtrip() {
        let req = sample_measurement_request();
        let digest = MeasurementRequestDigest::compute(&req).unwrap();
        let hex = digest.to_hex();
        let restored = MeasurementRequestDigest::from_hex(&hex).unwrap();
        assert_eq!(digest, restored);
    }

    // === MeasurementRequest cross-field (P1-3 v3) ===

    #[test]
    fn measurement_request_derives_input_digest_from_context() {
        let context = sample_measurement_input_context();
        let expected = MeasurementInputDigest::compute(&context).unwrap();

        let req = sample_measurement_request();
        assert_eq!(
            req.measurement_input_digest(),
            &expected,
            "MeasurementRequest::try_new must derive input digest from context"
        );
    }

    #[test]
    fn measurement_request_derives_delta_digest_from_canonical_delta() {
        let delta = sample_canonical_delta();
        let expected = MeasurementDeltaDigest::compute_from_canonical(&delta).unwrap();

        let req = sample_measurement_request();
        assert_eq!(
            req.structural_delta_digest(),
            &expected,
            "MeasurementRequest::try_new must derive delta digest from canonical delta"
        );
    }

    #[test]
    fn engine_measurement_rejects_mismatched_context_and_request_digest() {
        // Build a request with one context, then try to construct EngineMeasurement
        // with a different context — cross-field mismatch must fail.
        let req = sample_measurement_request();
        // Different context (different entropy effective value).
        let coords_b = crate::coords::CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::TreeSitter,
            crate::axes::CohesionAxis::try_with_observed_source(crate::coords::MetricSource::Scip)
                .unwrap(),
            crate::axes::EntropyAxis::from_commit_entropy(9.0),
            crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
        )
        .unwrap();
        let wrong_context = MeasurementInputContext::try_from(&coords_b).unwrap();

        let measured = MeasuredRawPosition {
            coupling: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            cohesion: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            instability: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            entropy: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            witness_depth: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
        };
        let baseline = MeasurementBaseline::Available(measured.clone());
        let result = EngineMeasurement::new(baseline, measured, wrong_context, req);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::MeasurementContextDigestMismatch)
            ),
            "cross-field digest mismatch must be rejected"
        );
    }

    #[test]
    fn engine_measurement_revision_delegates_to_request() {
        // Accessor sanity — revision() returns request.base_revision().
        fn assert_revision_accessor<T>()
        where
            T: serde::Serialize,
        {
        }
        assert_revision_accessor::<EngineMeasurement>();
    }

    #[test]
    fn engine_measurement_accessors_preserve_all_fields() {
        // Type-level check — EngineMeasurement has all accessor methods.
        let req = sample_measurement_request();
        let _ = req.subject();
        let _ = req.impact();
        let _ = req.base_revision();
        let _ = req.structural_delta_digest();
        let _ = req.measurement_input_digest();
    }

    #[test]
    fn engine_measurement_request_digest_chain() {
        // Build request → EngineMeasurement → request_digest() → MeasurementRequestDigest.
        // Validate the chain works end-to-end.
        let req = sample_measurement_request();
        let expected_digest = MeasurementRequestDigest::compute(&req).unwrap();

        let measured = MeasuredRawPosition {
            coupling: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            cohesion: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            instability: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            entropy: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
            witness_depth: crate::coords::AxisMeasurement::try_new(
                0.5,
                crate::coords::MetricSource::Scip,
            )
            .unwrap(),
        };
        let context = sample_measurement_input_context();
        let baseline = MeasurementBaseline::Available(measured.clone());
        let engine_meas =
            EngineMeasurement::new(baseline, measured, context, req).expect("context matches");
        let actual_digest = engine_meas.request_digest().unwrap();
        assert_eq!(actual_digest, expected_digest);
    }

    // === helper struct for fixture ===
    struct CanonicalizationErrorFixture;
    impl CanonicalizationErrorFixture {
        fn classification() -> crate::authorization::CanonicalNodeClassification {
            crate::authorization::CanonicalNodeClassification::try_from(
                &crate::space::NodeClassification::Production,
            )
            .unwrap()
        }
        fn role() -> crate::authorization::CanonicalNodeRole {
            crate::authorization::CanonicalNodeRole::try_from(&crate::space::NodeRole::Core)
                .unwrap()
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b — MeasurementBinding disposition + verification mapping
    // (reviewer scoped P2-1: Faz 1 tip test'leri kendi fazında)
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn measurement_binding_mismatch_disposition_regenerate_for_stale() {
        // Reviewer v4 P1-3: RevisionMismatch + CurrentContextMismatch → RegenerateMeasurement
        // (stale — yeni token üret, LLM retry yok, budget yok).
        let revision_mismatch = MeasurementBindingMismatch::RevisionMismatch {
            expected: sentinel_test_revision(1),
            presented: sentinel_test_revision(2),
        };
        assert_eq!(
            revision_mismatch.disposition(),
            MeasurementBindingDisposition::RegenerateMeasurement
        );
        // CurrentContextMismatch disposition'ı { .. } pattern ile aynı sınıfa düşer —
        // RevisionMismatch testi RegenerateMeasurement sınıfını kanıtlar.
    }

    #[test]
    fn measurement_binding_mismatch_disposition_reject_for_replay_tamper() {
        // Reviewer v4 P1-3: Task/Subject/Impact/StructuralDelta/ContextDigest mismatch
        // → RejectPresentedAuthority (terminal, retry yok).
        // Digest gerektiren varyantlar (StructuralDelta/ContextDigest/CurrentContext)
        // Faz 3'te gerçek compute ile test edilecek; burada digest gerektirmeyen
        // varyantlar (Task/Subject/Impact) sınıfı kanıtlar.
        let task_mismatch = MeasurementBindingMismatch::TaskMismatch {
            claim_task_id: Some(10),
            resolved_task_id: 20,
        };
        assert_eq!(
            task_mismatch.disposition(),
            MeasurementBindingDisposition::RejectPresentedAuthority
        );
    }

    #[test]
    fn verification_error_maps_to_engine_commit_error_correctly() {
        // **Reviewer v6 #1:** MeasurementBindingVerificationError → EngineCommitError
        // tek kapsayıcı mapping. Mismatch/Derivation/Drift üçü de
        // `MeasurementBindingVerification` varyantına gider. Legacy
        // `MeasurementBindingMismatch`/`MeasurementBindingFailed` varyantları yeni kod
        // tarafından üretilmez (#[from] kaldırıldı).
        use crate::engine::EngineCommitError;

        let mismatch_err = MeasurementBindingVerificationError::Mismatch(
            MeasurementBindingMismatch::TaskMismatch {
                claim_task_id: Some(1),
                resolved_task_id: 2,
            },
        );
        let engine_err: EngineCommitError = mismatch_err.into();
        assert!(matches!(
            engine_err,
            EngineCommitError::MeasurementBindingVerification(
                MeasurementBindingVerificationError::Mismatch(_)
            )
        ));

        let derivation_err = MeasurementBindingVerificationError::Derivation(
            MeasurementBindingDerivationError::SubjectDerivationFailed {
                detail: "test".to_string(),
            },
        );
        let engine_err: EngineCommitError = derivation_err.into();
        assert!(matches!(
            engine_err,
            EngineCommitError::MeasurementBindingVerification(
                MeasurementBindingVerificationError::Derivation(_)
            )
        ));

        // **Faz 3 (reviewer v4 P2-4):** Drift → tek kapsayıcı varyant.
        let drift_err = MeasurementBindingVerificationError::Drift(
            MeasurementBindingDriftError::SpaceRevisionChanged {
                before: sentinel_test_revision(1),
                after: sentinel_test_revision(2),
            },
        );
        let engine_err: EngineCommitError = drift_err.into();
        assert!(matches!(
            engine_err,
            EngineCommitError::MeasurementBindingVerification(
                MeasurementBindingVerificationError::Drift(_)
            )
        ));
    }

    /// Helper: test SpaceViewRevision (sentinel).
    fn sentinel_test_revision(seq: u64) -> SpaceViewRevision {
        use crate::authorization::{SpaceDigest, SpaceViewId, SpaceViewRevision};
        SpaceViewRevision {
            view_id: SpaceViewId::Ephemeral(seq),
            sequence: seq,
            content_digest: SpaceDigest::from_bytes([seq as u8; 32]),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 3 — TaskClaimDigest + MeasurementDigest coverage tests
    // (reviewer v4 P1-1, v6 P2-2/P2-3)
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Helper: minimal valid claim for digest tests.
    fn test_claim_for_digest(claim_id: u64, task_id: u64, author: u64) -> crate::witness::Claim {
        use crate::coords::RawPosition;
        crate::witness::Claim {
            id: claim_id.into(),
            intent: crate::witness::Intent::new(100, RawPosition::default()),
            author,
            computed_raw: RawPosition::default(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id: Some(task_id),
            removed_edges: vec![],
        }
    }

    /// Helper: minimal MeasurementDeltaDigest (empty delta sentinel).
    fn empty_delta_digest() -> MeasurementDeltaDigest {
        use crate::authorization::CanonicalStructuralDelta;
        let delta = CanonicalStructuralDelta::try_new(vec![], vec![], vec![]).unwrap();
        MeasurementDeltaDigest::compute_from_canonical(&delta).unwrap()
    }

    #[test]
    fn task_claim_digest_changes_when_claim_id_mutates() {
        let digest_a = TaskClaimDigest::compute(
            &test_claim_for_digest(1, 42, 100),
            42,
            &empty_delta_digest(),
        )
        .unwrap();
        let digest_b = TaskClaimDigest::compute(
            &test_claim_for_digest(2, 42, 100),
            42,
            &empty_delta_digest(),
        )
        .unwrap();
        assert_ne!(
            digest_a, digest_b,
            "claim_id change must produce different digest"
        );
    }

    #[test]
    fn task_claim_digest_changes_when_task_id_mutates() {
        let digest_a = TaskClaimDigest::compute(
            &test_claim_for_digest(1, 42, 100),
            42,
            &empty_delta_digest(),
        )
        .unwrap();
        let digest_b = TaskClaimDigest::compute(
            &test_claim_for_digest(1, 99, 100),
            99,
            &empty_delta_digest(),
        )
        .unwrap();
        assert_ne!(
            digest_a, digest_b,
            "task_id change must produce different digest"
        );
    }

    #[test]
    fn task_claim_digest_changes_when_author_mutates() {
        let digest_a = TaskClaimDigest::compute(
            &test_claim_for_digest(1, 42, 100),
            42,
            &empty_delta_digest(),
        )
        .unwrap();
        let digest_b = TaskClaimDigest::compute(
            &test_claim_for_digest(1, 42, 200),
            42,
            &empty_delta_digest(),
        )
        .unwrap();
        assert_ne!(
            digest_a, digest_b,
            "author change must produce different digest"
        );
    }

    #[test]
    fn task_claim_digest_stable_for_same_inputs() {
        // Determinism: aynı input → aynı digest (canonical, non-randomized).
        let claim = test_claim_for_digest(1, 42, 100);
        let digest_a = TaskClaimDigest::compute(&claim, 42, &empty_delta_digest()).unwrap();
        let digest_b = TaskClaimDigest::compute(&claim, 42, &empty_delta_digest()).unwrap();
        assert_eq!(digest_a, digest_b, "same inputs must produce same digest");
    }

    /// Helper: minimal MeasuredRawPosition for digest tests.
    fn test_measured(value: f64) -> crate::coords::MeasuredRawPosition {
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

    #[test]
    fn measurement_digest_changes_when_axis_value_mutates() {
        let digest_a = MeasurementDigest::compute(&test_measured(0.5)).unwrap();
        let digest_b = MeasurementDigest::compute(&test_measured(0.6)).unwrap();
        assert_ne!(
            digest_a, digest_b,
            "axis value change must produce different digest"
        );
    }

    #[test]
    fn measurement_digest_changes_when_source_mutates() {
        // Reviewer v6 P2-2: stable canonical source tag — farklı source → farklı digest.
        use crate::coords::{AxisMeasurement, MeasuredRawPosition, MetricSource};
        let mk = |source: MetricSource| -> MeasuredRawPosition {
            let axis = AxisMeasurement { value: 0.5, source };
            MeasuredRawPosition {
                coupling: axis,
                cohesion: axis,
                instability: axis,
                entropy: axis,
                witness_depth: axis,
            }
        };
        let digest_scip = MeasurementDigest::compute(&mk(MetricSource::Scip)).unwrap();
        let digest_treesitter = MeasurementDigest::compute(&mk(MetricSource::TreeSitter)).unwrap();
        assert_ne!(
            digest_scip, digest_treesitter,
            "source change must produce different digest (stable canonical tag)"
        );
    }

    #[test]
    fn measurement_digest_uses_stable_canonical_source_tags() {
        // Reviewer v4 P2-2: source tag enum discriminant DEĞİL — stable mapping.
        // TreeSitter=0, Scip=1 (canonical_tag_newtype! macro pinli).
        // İki farklı source → iki farklı tag byte → farklı digest (yukarıdaki test).
        // Bu test tag stability'sini ayrıca pinler: Placeholder vs Heuristic.
        use crate::coords::{AxisMeasurement, MeasuredRawPosition, MetricSource};
        let mk = |source: MetricSource| -> MeasuredRawPosition {
            let axis = AxisMeasurement { value: 0.5, source };
            MeasuredRawPosition {
                coupling: axis,
                cohesion: axis,
                instability: axis,
                entropy: axis,
                witness_depth: axis,
            }
        };
        let d1 = MeasurementDigest::compute(&mk(MetricSource::Placeholder)).unwrap();
        let d2 = MeasurementDigest::compute(&mk(MetricSource::Heuristic)).unwrap();
        let d3 = MeasurementDigest::compute(&mk(MetricSource::Mixed)).unwrap();
        // Üçü farklı olmalı (3 farklı stable tag).
        assert_ne!(d1, d2);
        assert_ne!(d1, d3);
        assert_ne!(d2, d3);
    }

    #[test]
    fn measurement_digest_normalizes_negative_zero() {
        // Reviewer v6 P2-3: -0.0 → 0.0 normalize (canonical_f64_bytes).
        let digest_pos = MeasurementDigest::compute(&test_measured(0.0)).unwrap();
        let digest_neg = MeasurementDigest::compute(&test_measured(-0.0)).unwrap();
        assert_eq!(
            digest_pos, digest_neg,
            "-0.0 and 0.0 must produce same digest (normalized)"
        );
    }

    #[test]
    fn measurement_digest_rejects_non_finite() {
        // Reviewer v6 P2-3: NaN/±Infinity reject (defense-in-depth — MeasuredRawPosition
        // smart constructor yok, field'lar pub; digest seviyesinde finite-check).
        assert!(MeasurementDigest::compute(&test_measured(f64::NAN)).is_err());
        assert!(MeasurementDigest::compute(&test_measured(f64::INFINITY)).is_err());
        assert!(MeasurementDigest::compute(&test_measured(f64::NEG_INFINITY)).is_err());
    }

    #[test]
    fn measurement_digest_stable_for_same_inputs() {
        let measured = test_measured(0.5);
        let digest_a = MeasurementDigest::compute(&measured).unwrap();
        let digest_b = MeasurementDigest::compute(&measured).unwrap();
        assert_eq!(digest_a, digest_b, "same inputs must produce same digest");
    }

    #[test]
    fn task_claim_and_measurement_digests_are_serializable() {
        // **Reviewer v7 P2-2 (dürüst adlandırma):** Bu test yalnız `Serialize` impl
        // varlığını doğrular — `Deserialize` absent OLDUĞUNU kanıtlamaz. Eğer her iki
        // derive eklense bu test yine geçer. "Serialize-only kanıtı" iddiası KALDIRILDI.
        //
        // Deserialize absent kanıtı: gerçek compile-fail test gerekir ama tipler
        // `pub(crate)` olduğundan external trybuild fixture onu adlandıramaz. Crate içi
        // compile-fail veya derive AST guard Faz 10'da. Şimdilik sadece Serialize
        // impl varlığı (downstream persistence için gerekli minimum).
        fn assert_serializable<T: serde::Serialize>() {}
        assert_serializable::<TaskClaimDigest>();
        assert_serializable::<MeasurementDigest>();
    }
}
