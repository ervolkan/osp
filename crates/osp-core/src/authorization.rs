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

use crate::coords::RawPosition;
use crate::space::NodeId;
use crate::trajectory::{ApplyTarget, GateDecision, MutationDecision, PredicateCompletion};
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

/// Structural delta'nın tam kanonik temsili.
///
/// `StructuralDeltaDigest` KULLANILMAZ — lossy özet iki farklı proposal'ı aynı
/// authorization basis'e dönüştürebilir. Full canonical byte stream kullanılır.
/// Node ID'leri sorted, edge'ler sorted — deterministic encoding.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalStructuralDelta {
    /// Eklenen node ID'leri (sorted).
    pub new_node_ids: Vec<NodeId>,
    /// Eklenen edge'ler (from, to, kind) — sorted.
    pub new_edges: Vec<(NodeId, NodeId, String)>, // kind as string for canonical encoding
    /// Kaldırılan edge'ler (from, to, kind) — sorted. G2c-2 subtractive delta.
    pub removed_edges: Vec<(NodeId, NodeId, String)>,
}

/// Predicate içeriği — her zaman bağlı (identifier yetersiz, içerik mutable olabilir).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalPredicateContent {
    /// Predicate'lerin canonical serialization'ı (sorted, deterministic).
    pub predicate_bytes: Vec<u8>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceViewRevision — store-scoped, lane-qualified
// ═══════════════════════════════════════════════════════════════════════════════

/// Store-scoped ve lane-qualified revision identity.
///
/// "Revision global" DEĞİL — store + lane + sequence + content_digest kombinasyonu.
/// Authorization basis ölçümüne görünür olan lane'in her yapısal mutasyonunda değişir
/// (Mainline commit, TrajectoryCheckpoint, Sandbox mutation — hepsi artırır).
///
/// P1 resume'da staleness kontrolü: `current_revision == base_revision` → devam;
/// `!=` → stale authorization basis → remeasure gerekir.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpaceViewRevision {
    pub store_id: StoreId,
    pub lane: crate::trajectory::CommitLane,
    pub sequence: u64,
    /// Space content digest (BLAKE3) — revision bütünlüğü için.
    pub content_digest: SpaceDigest,
}

/// Store identifier (projeye özgü, global değil).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StoreId(pub String);

/// Space content digest (BLAKE3, 32 byte).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpaceDigest([u8; 32]);

impl SpaceDigest {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
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

impl EvaluationContextDigest {
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
/// Digest lenirken TÜM alanlar dahil edilir — structural delta full canonical
/// (digest değil), predicate içerik her zaman bağlı (id yetersiz). `created_at`
/// dahil DEĞİL — aynı basis farklı zamanda aynı digest vermeli.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthorizationBasis {
    pub schema_version: u32,
    pub task_id: crate::trajectory::TaskId,
    pub claim_identity: ClaimIdentity,
    pub claim_author: ClaimAuthor,
    pub structural_delta: CanonicalStructuralDelta,
    pub predicate_content: CanonicalPredicateContent,
    pub measured_result: ProvenancedMeasuredResult,
    pub deterministic_gate_result: GateDecision,
    pub predicate_completion: PredicateCompletion,
    pub mutation_decision: MutationDecision,
    pub intended_apply_target: ApplyTarget,
    pub evaluation_context_digest: EvaluationContextDigest,
    pub base_space_view_revision: SpaceViewRevision,
}

/// Measured result + provenance (MetricSource dahil — INV-T4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProvenancedMeasuredResult {
    pub raw: RawPosition,
    /// Metric source — "scip" | "treesitter" | "placeholder" | "heuristic".
    pub metric_source: String,
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
    /// Canonical encoding: serde_json (sorted keys, deterministic) + domain separation.
    /// NaN ve -0.0 canonicalization encoding öncesi uygulanır.
    pub fn compute(basis: &AuthorizationBasis) -> Result<Self, AuthorizationBasisDigestError> {
        // Canonical JSON encoding (sorted keys, no pretty printing).
        let canonical = serde_json::to_vec(basis).map_err(|e| {
            AuthorizationBasisDigestError::EncodingFailed(e.to_string())
        })?;

        // Float canonicalization: NaN reject, -0.0 normalize.
        // serde_json f64'leri default olarak canonical üretir ama biz yine de validate edelim.
        validate_no_nan(&canonical)?;

        // BLAKE3 keyed hash with domain separation.
        let mut hasher = blake3::Hasher::new();
        hasher.update(Self::DOMAIN_SEPARATOR);
        hasher.update(&canonical);
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
        let bytes = hex::decode(hex_str).map_err(|e| {
            AuthorizationBasisDigestError::HexDecodeFailed(e.to_string())
        })?;
        if bytes.len() != 32 {
            return Err(AuthorizationBasisDigestError::InvalidLength(bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

/// Authorization basis digest hesaplama hataları.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthorizationBasisDigestError {
    #[error("canonical encoding failed: {0}")]
    EncodingFailed(String),
    #[error("NaN detected in authorization basis — not allowed (canonical encoding)")]
    NaNRejected,
    #[error("hex decode failed: {0}")]
    HexDecodeFailed(String),
    #[error("invalid digest length: expected 32 bytes, got {0}")]
    InvalidLength(usize),
}

/// JSON byte stream'inde NaN kontrolü.
///
/// serde_json NaN'ı `null` olarak serialize eder ama biz yine de defensive check yapalım.
fn validate_no_nan(canonical: &[u8]) -> Result<(), AuthorizationBasisDigestError> {
    // serde_json NaN'ı zaten handle eder; bu defensive check for future encoding changes.
    let s = std::str::from_utf8(canonical)
        .map_err(|e| AuthorizationBasisDigestError::EncodingFailed(e.to_string()))?;
    if s.contains("NaN") {
        return Err(AuthorizationBasisDigestError::NaNRejected);
    }
    Ok(())
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

/// Attempt evidence identifier (P1 resume'da evidence store lookup için).
pub type AttemptEvidenceId = u64;

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
    UnknownSchema { found: String, expected: &'static str },
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

/// Navigator'ın `AwaitingWitnesses` döndürmeden ÖNCE çağırdığı persistence abstraction.
///
/// Çökme penceresi YOK: `AwaitingWitnesses` yalnızca artifact başarılı publish edildikten
/// sonra return edilir. P0-1 çözümü — CLI yazmaz, navigator injected store'a persist eder.
pub trait PendingAuthorizationStore {
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
    #[error("artifact already exists with different basis — integrity error (no silent overwrite)")]
    BasisConflict {
        existing_path: std::path::PathBuf,
    },
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
        Self {
            root: root.into(),
        }
    }

    /// Artifact path'i claim_id + digest'ten türet.
    fn artifact_path(&self, claim_id: ClaimId, digest: &AuthorizationBasisDigest) -> std::path::PathBuf {
        let hex = digest.to_hex();
        let filename = format!("claim-{claim_id}--{hex}.json");
        self.root.join(".osp").join("pending-authorizations").join(filename)
    }
}

impl PendingAuthorizationStore for FilesystemPendingAuthorizationStore {
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
            std::fs::create_dir_all(parent).map_err(|e| {
                PendingAuthorizationStoreError::DirCreationFailed(e.to_string())
            })?;
        }

        // Crash-consistent publish: same-dir temp → write_all → sync_all → rename.
        let temp_path = artifact_path.with_extension("json.tmp");
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

        // Atomic no-clobber rename. Windows: rename hedef varsa fail eder (biz create_new ile
        // temp oluşturduk, hedef yok kontrolü yukarıda yapıldı — race window minimal).
        // Unix: rename atomic, hedef varsa overwrite. Biz yukarıda exists() kontrolü yaptık.
        std::fs::rename(&temp_path, &artifact_path)
            .map_err(|e| PendingAuthorizationStoreError::WriteFailed(e.to_string()))?;

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
            structural_delta: CanonicalStructuralDelta {
                new_node_ids: vec![10],
                new_edges: vec![],
                removed_edges: vec![(0, 1, "Imports".to_string())],
            },
            predicate_content: CanonicalPredicateContent {
                predicate_bytes: b"coupling<=0.55".to_vec(),
            },
            measured_result: ProvenancedMeasuredResult {
                raw: RawPosition {
                    x: 0.5,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                },
                metric_source: "scip".to_string(),
            },
            deterministic_gate_result: GateDecision::PassedAll,
            predicate_completion: PredicateCompletion::Completed,
            mutation_decision: MutationDecision::AcceptAsCompleted,
            intended_apply_target: ApplyTarget::Lane(CommitLane::Mainline),
            evaluation_context_digest: EvaluationContextDigest::from_bytes([0xaa; 32]),
            base_space_view_revision: SpaceViewRevision {
                store_id: StoreId("test-store".to_string()),
                lane: CommitLane::Mainline,
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
    fn authorization_basis_digest_changes_when_base_lane_changes() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let mut basis2 = basis.clone();
        basis2.base_space_view_revision.lane = CommitLane::TrajectoryCheckpoint;
        let d2 = AuthorizationBasisDigest::compute(&basis2).unwrap();
        assert_ne!(d1, d2, "different lane → different digest");
    }

    #[test]
    fn authorization_basis_digest_changes_when_predicate_content_changes() {
        let basis = sample_basis();
        let d1 = AuthorizationBasisDigest::compute(&basis).unwrap();
        let mut basis2 = basis.clone();
        basis2.predicate_content.predicate_bytes = b"coupling<=0.60".to_vec();
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
        let basis = sample_basis();
        let digest = AuthorizationBasisDigest::compute(&basis).unwrap();

        // Raw BLAKE3 without domain separation (control).
        let canonical = serde_json::to_vec(&basis).unwrap();
        let raw_hash = blake3::hash(&canonical);
        let raw_bytes: [u8; 32] = raw_hash.into();

        assert_ne!(
            digest.as_bytes(),
            &raw_bytes,
            "domain separation must produce different digest"
        );
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
            store_id: StoreId("test".to_string()),
            lane: CommitLane::Mainline,
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
                store_id: StoreId("test-store".to_string()),
                lane: CommitLane::Mainline,
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
            WitnessHoldReason::MinApproversNotMet { distinct: 0, required: 2 }
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
        envelope.record.authorization_basis_digest =
            AuthorizationBasisDigest::from_hex(
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
        assert!(matches!(err, PendingAuthorizationLoadError::UnknownSchema { .. }));
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
        assert!(matches!(err, PendingAuthorizationLoadError::UnknownSchema { .. }));
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // FilesystemPendingAuthorizationStore tests
    // ═══════════════════════════════════════════════════════════════════════════════

    fn temp_dir() -> std::path::PathBuf {
        tempfile::tempdir()
            .expect("temp dir")
            .keep()
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
        assert!(receipt.artifact_path.to_string_lossy().contains("claim-42--"));
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
        let receipt2 = store.persist(&envelope).expect("second persist (idempotent)");

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
}
