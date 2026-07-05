//! Concept Anchoring runtime domain tipleri (Faz 1).
//!
//! [`crate::anchoring`] kökündeki enum'ların (Faz 0) üzerinde yaşayan struct'lar.
//! Faz 1 §11: "ConceptPacket, AnchorCandidate, AnchorPlan, PositionSnapshot tipleri".

use crate::anchoring::{
    AnchorDecisionKind, ConceptEdgeKind, ConceptPacketType, DecisionStatus, PositionFamily,
    ThresholdBand,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Newtype ID'ler — typed-prefix string ("Concept:Payment" vb.)
// ═══════════════════════════════════════════════════════════════════════════════

/// ConceptPacket newtype ID'si.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ConceptPacketId(pub String);

/// ConceptNode newtype ID'si — typed-prefix formatı ("Concept:Payment").
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ConceptNodeId(pub String);

/// PositionSnapshot newtype ID'si.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct PositionSnapshotId(pub String);

// ═══════════════════════════════════════════════════════════════════════════════
// NonEmptyExplanation — INV-C7 newtype (Faz 2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Boş olmayan explanation (INV-C7 — high-stake edge'lerde zorunlu).
///
/// # Yapısal garanti
/// Private inner `String` + fallible `new(...)`. Boş/whitespace string reject edilir
/// (`EmptyExplanation`). Bu, "boş explanation graph'a girer" hatasını type-level engeller:
/// `Option<NonEmptyExplanation>` ya `None`'dır ya da kesinlikle boş olmayan bir açıklama.
///
/// High-stake edge'lerde explanation *presence* hâlâ runtime (gate, `is_high_stake()`
/// runtime) — type-level olan emptiness kontrolüdür.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmptyExplanation(String);

impl NonEmptyExplanation {
    /// Boş/whitespace string reject. Başarılıysa `NonEmptyExplanation`.
    pub fn new(s: impl Into<String>) -> Result<Self, EmptyExplanation> {
        let s = s.into();
        if s.trim().is_empty() {
            Err(EmptyExplanation)
        } else {
            Ok(Self(s))
        }
    }

    /// TCB içi: validation'sız construct (extractor zaten non-empty üretir).
    pub(crate) fn from_validated(s: String) -> Self {
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NonEmptyExplanation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for NonEmptyExplanation {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl serde::Serialize for NonEmptyExplanation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for NonEmptyExplanation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Boş explanation hatası.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyExplanation;

impl std::fmt::Display for EmptyExplanation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "explanation boş/whitespace olamaz (INV-C7 NonEmptyExplanation)"
        )
    }
}

impl std::error::Error for EmptyExplanation {}

// ═══════════════════════════════════════════════════════════════════════════════
// ScalarSimilarity — INV-C1 newtype (Faz 2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Skalar semantic similarity [0,1] — INV-C1: embedding vektörü DEĞİL.
///
/// # INV-C1 yapısal garanti
/// [`AnchorScorer`](crate::anchoring::scorer::AnchorScorer) embedding **vektörünü**
/// görmez; sadece bu skalar newtype'ı alır. `new()` `[0,1]` range-check yapar —
/// değer aralığı dışı → `SimilarityOutOfRange`. Bu, scorer'a "embedding benzerliği"
/// adı altında rastgele değer sızmasını type-level engeller.
///
/// Faz 7'de gerçek embedding geldiğinde `Embedding` type doldurulur (sealed mod),
/// `cosine` private kalır, scorer hala `ScalarSimilarity` görür.
///
/// # Serde boundary (hardening PR — EvidenceStrength R1 ile aynı açık)
/// `Serialize`/`Deserialize` derive **kullanılmaz** — derive, `new()` constructor'ını
/// bypass edip range-check'i deler (`serde_json::from_str("2.0")` geçerli üretir).
/// Custom `Deserialize` impl `new()` üzerinden geçer; range-dışı değer reject.
/// Bu, EvidenceStrength (INV-C6) ile aynı serde hijyeni standardını INV-C1'e taşır.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarSimilarity(f64);

impl ScalarSimilarity {
    /// `[0,1]` range-check. Dışarıda → `SimilarityOutOfRange`.
    pub fn new(value: f64) -> Result<Self, SimilarityOutOfRange> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(SimilarityOutOfRange { value })
        }
    }

    /// TCB içi: validation'sız construct (Faz 7 embedding cosine sonucu için hazır API).
    #[allow(dead_code)] // Faz 7 placeholder — scorer şu an zero() kullanır
    pub(crate) fn from_validated(value: f64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0.0)
    }
    pub fn one() -> Self {
        Self(1.0)
    }
    pub fn get(&self) -> f64 {
        self.0
    }
}

/// Custom Serialize — inner f64'yi transparent serialize (round-trip uyumlu).
impl serde::Serialize for ScalarSimilarity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

/// Custom Deserialize — `new()` üzerinden range-check (serde hijyeni, EvidenceStrength R1 ile aynı).
/// `serde_json::from_str("2.0")` / `"-1.0"` / `"NaN"` reject edilir; constructor bypass
/// edilemez.
impl<'de> serde::Deserialize<'de> for ScalarSimilarity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        ScalarSimilarity::new(value).map_err(serde::de::Error::custom)
    }
}

/// `ScalarSimilarity` değer aralığı dışı hatası.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimilarityOutOfRange {
    pub value: f64,
}

impl std::fmt::Display for SimilarityOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScalarSimilarity [0,1] dışı: {} (INV-C1)", self.value)
    }
}

impl std::error::Error for SimilarityOutOfRange {}

// ═══════════════════════════════════════════════════════════════════════════════
// ObservedCodeMetricSource + EvidenceStrength + ObservedCodeEvidence — INV-C6 (Faz 4)
// ═══════════════════════════════════════════════════════════════════════════════
//
// INV-C6 (§7.2/§9, düzeltilmiş): kod metric'leri = **Observed** (Paper 1 ontolojisi);
// koddan çıkarılan *niyet* = **Inferred** (Candidate). İkisi karıştırılamaz.
//
// Faz 4 modelleme kararı (D15 — provenance yorumu): "Observed" yeni bir `DecisionStatus`
// variantı DEĞİLDİR. İki lane net ayrılır:
//
//   DecisionStatus        = graph acceptance lane (Candidate→Accepted)
//   ObservedCodeEvidence  = epistemik provenance lane (MetricSource'tan)
//
// "Observed code reality is evidence, not acceptance." — bir CodeEntity node'unun
// observed olması operator-accepted decision anlamına gelmez; Candidate kalır, observed
// olma durumu `ObservedCodeEvidence` içinde taşınır (Patch 5).

/// Observed kod kanıtının metric kaynağı (INV-C6 — type-level filtre).
///
/// Genel [`crate::coords::MetricSource`] (Paper 1'de `Placeholder`/`Heuristic` içerir)
/// yerine yalnızca gözlemlenmiş kaynakları kabul eden typed enum. Bu, "bir LLM çıktısı
/// veya insan tahmini asla kod kanıtı sayılamaz" kuralını tip sistemine taşır (Patch 2):
/// `HumanVision`/`LlmGuess`/`InferredIntent` imkansız.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ObservedCodeMetricSource {
    /// SCIP semantic index (en yüksek güven — gerçek field-access/cross-ref).
    Scip,
    /// tree-sitter syntactic extraction (imports, class defs).
    TreeSitter,
    /// Diğer static analyzer (Faz 4 stub sonrası, osp-analyzer bridge).
    StaticAnalyzer,
}

/// Observed kod kanıt gücü `[0,1]` (INV-C6 type-level, Patch 3).
///
/// `ScalarSimilarity` (INV-C1) paterninin "kanıt gücü" için uygulanması. Çıplak `f64`
/// tekrar riskini (`-1.0`, `2.0`, `NaN`) type-level engeller. [`crate::anchoring`] (scorer
/// `code_evidence_score`, gate `evidence_strength`) bu newtype'ı görür.
///
/// # Serde boundary (review patch R1)
/// `Serialize`/`Deserialize` derive **kullanılmaz** — derive, `new()` constructor'ını
/// bypass edip range-check'i deler (`serde_json::from_str("2.0")` geçerli üretir).
/// Bunun yerine **custom impl** ile Deserialize `new()` üzerinden geçer; range-dışı
/// değer reject edilir. Bu, "evidence strength serde ile forged edilemez" garantisi.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceStrength(f64);

impl EvidenceStrength {
    /// `[0,1]` range-check + finiteness. NaN, ±∞, negatif, >1 → `EvidenceStrengthOutOfRange`.
    /// Not 1: `is_finite()` öncelikli — NaN/inf range-check'i yanıltmasın.
    pub fn new(value: f64) -> Result<Self, EvidenceStrengthOutOfRange> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(EvidenceStrengthOutOfRange { value })
        }
    }

    pub fn zero() -> Self {
        Self(0.0)
    }
    pub fn one() -> Self {
        Self(1.0)
    }
    pub fn get(&self) -> f64 {
        self.0
    }
}

/// Custom Serialize — inner f64'yi transparent serialize (round-trip uyumlu).
impl serde::Serialize for EvidenceStrength {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

/// Custom Deserialize — `new()` üzerinden range-check (R1 review patch).
/// `serde_json::from_str("2.0")` / `"-1.0"` / `"NaN"` reject edilir; constructor bypass
/// edilemez.
impl<'de> serde::Deserialize<'de> for EvidenceStrength {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        EvidenceStrength::new(value).map_err(serde::de::Error::custom)
    }
}

/// `EvidenceStrength` değer aralığı dışı hatası.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceStrengthOutOfRange {
    pub value: f64,
}

impl std::fmt::Display for EvidenceStrengthOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EvidenceStrength [0,1] dışı veya non-finite: {} (INV-C6)",
            self.value
        )
    }
}

impl std::error::Error for EvidenceStrengthOutOfRange {}

/// Bir CodeEntity için observed (ölçülmüş) kod kanıtı (INV-C6 — Patch 1/2/5).
///
/// # INV-C6 yapısal garanti
/// - **Private field'lar + public smart constructor `new`** (Patch 1): dışarıdan struct
///   literal construct edilemez (trybuild `c6_observed_evidence_literal`); ama **gelecekteki
///   osp-analyzer bridge crate'i `new(...)` çağırabilir** (`pub(crate)` DEĞİL — dış provider
///   geçerli evidence üretebilir). İnvariant'lar constructor içinde korunur.
/// - **`ObservedCodeMetricSource` typed enum** (Patch 2): `MetricSource::Placeholder`/
///   `Heuristic` imkansız — sadece gözlemlenmiş kaynaklar.
/// - **`confidence: EvidenceStrength`** (Not 2): çak `f64` değil, range-checked newtype.
/// - **Serialize-only** (Not 3): `Deserialize` YOK — diskten evidence reconstruct edip
///   INV-C6 boundary'yi bypass etmeyi engeller (PR30 serde boundary paterni).
/// - **`physical_vector: PhysicalCodeVector`**: PhysicalCode family (INV-C2). Intent
///   (`ConceptualIntentVector`) buraya TYPE-LEVEL verilmez (trybuild
///   `c6_intent_carries_physical_vector`).
///
/// # Observed ≠ Accepted (Patch 5)
/// Bu evidence bir CodeEntity'nin *ölçülmüş fiziksel yapısını* taşır. Node'un graph
/// acceptance status'unu (Candidate→Accepted) değiştirmez. Operator acceptance ayrı
/// lane'dir (INV-C3).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ObservedCodeEvidence {
    code_entity_id: ConceptNodeId,
    physical_vector: PhysicalCodeVector,
    metric_source: ObservedCodeMetricSource,
    confidence: EvidenceStrength,
    /// Unix epoch saniye (chrono bağımlılığı yok — Faz 1 stratejisi).
    measured_at: u64,
}

impl ObservedCodeEvidence {
    /// Public smart constructor — invariant'lar burada korunur (Patch 1).
    ///
    /// Dış crate (osp-analyzer bridge, test) geçerli observed evidence üretebilir; ama
    /// field'lar private olduğu için struct literal ile *geçersiz* evidence enjekte edemez.
    pub fn new(
        code_entity_id: ConceptNodeId,
        physical_vector: PhysicalCodeVector,
        metric_source: ObservedCodeMetricSource,
        confidence: EvidenceStrength,
        measured_at: u64,
    ) -> Self {
        Self {
            code_entity_id,
            physical_vector,
            metric_source,
            confidence,
            measured_at,
        }
    }

    /// Bu evidence'ın bağlı olduğu CodeEntity node ID'si.
    pub fn code_entity_id(&self) -> &ConceptNodeId {
        &self.code_entity_id
    }

    /// Ölçülen fiziksel yapı (Paper 1 eksenleri, INV-C2 PhysicalCode family).
    pub fn physical_vector(&self) -> &PhysicalCodeVector {
        &self.physical_vector
    }

    /// Metric kaynağı (INV-C6 — observed provenance).
    pub fn metric_source(&self) -> ObservedCodeMetricSource {
        self.metric_source
    }

    /// Kanıt güven katsayısı `[0,1]` (EvidenceStrength newtype).
    pub fn confidence(&self) -> EvidenceStrength {
        self.confidence
    }

    /// Ölçüm zaman damgası (Unix epoch saniye).
    pub fn measured_at(&self) -> u64 {
        self.measured_at
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptNodeKind — Concept graph node türü
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept graph node türü. Fixture target prefix'leriyle uyumlu
/// (`Concept:`, `RuleCandidate:`, `RiskCandidate:`, `Decision:`,
///  `CodeEntity:`, `CodeEntityCandidate:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConceptNodeKind {
    Concept,
    RuleCandidate,
    TaskCandidate,
    RiskCandidate,
    Risk,
    Decision,
    CodeEntity,
    /// Faz 1'de gerçek code analizi yok — ExpectedImplementation hedefi.
    CodeEntityCandidate,
}

impl ConceptNodeKind {
    /// Typed-prefix string ("Concept", "CodeEntityCandidate").
    pub fn as_prefix(self) -> &'static str {
        match self {
            Self::Concept => "Concept",
            Self::RuleCandidate => "RuleCandidate",
            Self::TaskCandidate => "TaskCandidate",
            Self::RiskCandidate => "RiskCandidate",
            Self::Risk => "Risk",
            Self::Decision => "Decision",
            Self::CodeEntity => "CodeEntity",
            Self::CodeEntityCandidate => "CodeEntityCandidate",
        }
    }

    /// Prefix string → kind. Fixture/given parse için.
    pub fn from_prefix(s: &str) -> Option<Self> {
        Some(match s {
            "Concept" => Self::Concept,
            "RuleCandidate" => Self::RuleCandidate,
            "TaskCandidate" => Self::TaskCandidate,
            "RiskCandidate" => Self::RiskCandidate,
            "Risk" => Self::Risk,
            "Decision" => Self::Decision,
            "CodeEntity" => Self::CodeEntity,
            "CodeEntityCandidate" => Self::CodeEntityCandidate,
            _ => return None,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PacketSource — girdi kaynağı provenance
// ═══════════════════════════════════════════════════════════════════════════════

/// ConceptPacket kaynağı — temporal_trust_score için (§8.1, §6.2 hiyerarşi).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PacketSource {
    /// Kullanıcıdan gelen açık vizyon (§6.2 seviye 2).
    ExplicitUser,
    /// Operator kabul etti (seviye 1).
    Operator,
    /// Doküman/ADR (seviye 5 — stale olabilir).
    Document,
    /// Agent hipotezi (seviye 7 — düşük güven).
    Agent,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptPacket — insan/metin girdisinin ontolojik paketi
// ═══════════════════════════════════════════════════════════════════════════════

/// İnsan/metin girdisinin ontolojik paketi (§3.1, §11).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptPacket {
    pub id: ConceptPacketId,
    pub packet_type: ConceptPacketType,
    pub text: String,
    pub language: String,
    pub position_family: PositionFamily,
    /// INV-C5: türetilen başlangıçta Candidate.
    pub decision_status: DecisionStatus,
    pub source: PacketSource,
}

impl ConceptPacket {
    /// Yeni packet — default Candidate (INV-C5), ConceptualIntent family.
    pub fn new(
        id: impl Into<String>,
        packet_type: ConceptPacketType,
        text: impl Into<String>,
        language: impl Into<String>,
        source: PacketSource,
    ) -> Self {
        Self {
            id: ConceptPacketId(id.into()),
            packet_type,
            text: text.into(),
            language: language.into(),
            position_family: PositionFamily::ConceptualIntent,
            decision_status: DecisionStatus::Candidate,
            source,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptNode — graph node
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept graph node (Concept/RuleCandidate/TaskCandidate/Risk/Decision/CodeEntity).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptNode {
    pub id: ConceptNodeId,
    pub canonical: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub node_kind: ConceptNodeKind,
    pub decision_status: DecisionStatus,
    pub position_family: PositionFamily,
}

impl ConceptNode {
    /// Yeni Concept node — default Candidate (INV-C5), ConceptualIntent.
    pub fn new_concept(canonical: impl Into<String>) -> Self {
        let canonical = canonical.into();
        Self {
            id: ConceptNodeId(format!("Concept:{canonical}")),
            canonical,
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptEdge — graph edge
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept graph edge. INV-C7: high-stake kind'larda explanation zorunlu.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptEdge {
    pub from: ConceptNodeId,
    pub to: ConceptNodeId,
    pub kind: ConceptEdgeKind,
    pub decision_status: DecisionStatus,
    /// INV-C7 — high-stake edge'lerde zorunlu (gate doğrular). Faz 2: NonEmptyExplanation
    /// newtype (boş string type-level engelli).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<NonEmptyExplanation>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ExtractedAnchorCandidate — extractor çıktısı (score'suz)
// ═══════════════════════════════════════════════════════════════════════════════

/// Extractor tarafından üretilen, henüz skorlanmamış aday.
/// Pipeline: extract → [`ExtractedAnchorCandidate`] → score → [`AnchorCandidate`].
///
/// # INV-C8 (Faz 2 — type-level)
/// Field'lar `pub(crate)` — crate içi TCB erişir; external crate struct literal
/// construct edemez (Rust tüm field'ların visible olmasını gerektirir). Constructor
/// `pub(crate) fn new(...)` — sadece extractor üretir.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedAnchorCandidate {
    pub(crate) packet_id: ConceptPacketId,
    pub(crate) target_node_id: ConceptNodeId,
    pub(crate) edge_kind: ConceptEdgeKind,
    /// High-stake edge'lerde doldurulur (gate INV-C7 doğrular). Faz 2: NonEmptyExplanation.
    pub(crate) explanation: Option<NonEmptyExplanation>,
}

impl ExtractedAnchorCandidate {
    /// Extractor (TCB içi) constructor.
    pub(crate) fn new(
        packet_id: ConceptPacketId,
        target_node_id: ConceptNodeId,
        edge_kind: ConceptEdgeKind,
        explanation: Option<NonEmptyExplanation>,
    ) -> Self {
        Self {
            packet_id,
            target_node_id,
            edge_kind,
            explanation,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorScoreBreakdown — §8.1 hibrit skor (7 pozitif + 2 penalty)
// ═══════════════════════════════════════════════════════════════════════════════

/// 7 bileşenli hibrit skor + 2 penalty (§8.1, D5). INV-C1: `semantic_similarity`
/// skalar — embedding vektörü görülmez (Faz 7 embedding).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnchorScoreBreakdown {
    /// Faz 1: 0.0 placeholder (lexical classifier, embedding yok). Faz 2: ScalarSimilarity
    /// newtype (INV-C1 — type-level vector değil scalar).
    pub semantic_similarity: ScalarSimilarity,
    pub ontology_type_compatibility: f64,
    pub graph_context_score: f64,
    pub domain_term_match: f64,
    /// Faz 1: 0.0 (code analizi Faz 4).
    pub code_evidence_score: f64,
    pub temporal_trust_score: f64,
    pub decision_status_score: f64,
    /// §8.1 penalty — accepted decision ile çelişki (INV-C4).
    #[serde(default)]
    pub contradiction_penalty: f64,
    /// §8.1 penalty — eski bilgi.
    #[serde(default)]
    pub staleness_penalty: f64,
}

impl Default for AnchorScoreBreakdown {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl AnchorScoreBreakdown {
    /// Tüm bileşenler 0.0 (Faz 1 başlangıç).
    pub fn zeroed() -> Self {
        Self {
            semantic_similarity: ScalarSimilarity::zero(),
            ontology_type_compatibility: 0.0,
            graph_context_score: 0.0,
            domain_term_match: 0.0,
            code_evidence_score: 0.0,
            temporal_trust_score: 0.0,
            decision_status_score: 0.0,
            contradiction_penalty: 0.0,
            staleness_penalty: 0.0,
        }
    }

    /// Ham toplam — penalty dahil, clamp YOK (negatif olabilir, debug için).
    pub fn raw_total(&self) -> f64 {
        0.25 * self.semantic_similarity.get()
            + 0.20 * self.ontology_type_compatibility
            + 0.15 * self.graph_context_score
            + 0.15 * self.domain_term_match
            + 0.10 * self.code_evidence_score
            + 0.10 * self.temporal_trust_score
            + 0.05 * self.decision_status_score
            - self.contradiction_penalty
            - self.staleness_penalty
    }

    /// Threshold policy için clamp'li toplam [0,1] (§8.2).
    pub fn total_clamped(&self) -> f64 {
        self.raw_total().clamp(0.0, 1.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorCandidate — scorer çıktısı (score'lu)
// ═══════════════════════════════════════════════════════════════════════════════

/// Skorlanmış aday. Gate bunları işleyip [`AnchorPlan`] üretir.
///
/// # INV-C8 (Faz 2 — type-level)
/// Field'lar `pub(crate)` — crate içi TCB erişir; external crate literal construct edemez.
/// Faz 3: Serialize var (audit), Deserialize YOK (INV-C8).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnchorCandidate {
    pub(crate) packet_id: ConceptPacketId,
    pub(crate) target_node_id: ConceptNodeId,
    pub(crate) edge_kind: ConceptEdgeKind,
    pub(crate) score: AnchorScoreBreakdown,
    /// Extractor'dan gelir; high-stake'te zorunlu (INV-C7). Faz 2: NonEmptyExplanation.
    pub(crate) explanation: Option<NonEmptyExplanation>,
}

impl AnchorCandidate {
    /// Scorer (TCB içi) constructor — Extracted adaydan + skor.
    pub(crate) fn from_scored(
        extracted: ExtractedAnchorCandidate,
        score: AnchorScoreBreakdown,
    ) -> Self {
        Self {
            packet_id: extracted.packet_id,
            target_node_id: extracted.target_node_id,
            edge_kind: extracted.edge_kind,
            score,
            explanation: extracted.explanation,
        }
    }

    /// Read accessor — external crate erişebilir (INV-C8: write/construct `pub(crate)`).
    pub fn edge_kind(&self) -> ConceptEdgeKind {
        self.edge_kind
    }
    pub fn target_node_id(&self) -> &ConceptNodeId {
        &self.target_node_id
    }
    pub fn explanation(&self) -> Option<&NonEmptyExplanation> {
        self.explanation.as_ref()
    }
    pub fn score(&self) -> &AnchorScoreBreakdown {
        &self.score
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorPlan — gate çıktısı
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir ConceptPacket'ten üretilen toplam karar planı.
///
/// # INV-C8 (Faz 2 — type-level)
/// Field'lar `pub(crate)` — crate içi TCB erişir. External crate literal construct edemez
/// (Rust tüm field'ların visible olmasını gerektirir). Tek constructor `pub(crate) from_gate`
/// — sadece [`crate::anchoring::gate::AnchorGate::decide`] üretir. **"AnchorPlan almak =
/// canon gate'ten geçmiş olmak"** (yapısal garanti). INV-C8 by-pass: harici kod
/// `AnchorPlan { ... }` uydurup `apply_plan` çağıramaz.
///
/// # Faz 3 serde boundary
/// `Serialize` var (audit write). `Deserialize` **YOK** — serde ile reconstruct
/// engelli (INV-C8). DB read için [`PersistedAnchorPlanAudit`] (apply edilemez audit).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnchorPlan {
    pub(crate) packet_id: ConceptPacketId,
    /// Graph'a yazılacak edge'ler (Candidate status, INV-C3).
    pub(crate) candidates: Vec<AnchorCandidate>,
    /// §8.2 threshold kararı.
    pub(crate) decision: AnchorDecisionKind,
    pub(crate) threshold_band: ThresholdBand,
    /// INV-C7 ↔ D6: high-stake edge varsa true.
    pub(crate) requires_operator_review: bool,
    /// §6.4.1 mapping (fix_007 — kod gerçekliği SUPERSEDES yapamaz).
    pub(crate) negative_assertions: Vec<String>,
    /// INV-C8 canon gate intercept'leri (hata DEĞİL — başarılı redirect).
    pub(crate) redirects: Vec<CanonicalRedirect>,
}

impl AnchorPlan {
    /// Gate (TCB içi) constructor. External crate erişemez.
    pub(crate) fn from_gate(
        packet_id: ConceptPacketId,
        candidates: Vec<AnchorCandidate>,
        decision: AnchorDecisionKind,
        threshold_band: ThresholdBand,
        requires_operator_review: bool,
        negative_assertions: Vec<String>,
        redirects: Vec<CanonicalRedirect>,
    ) -> Self {
        Self {
            packet_id,
            candidates,
            decision,
            threshold_band,
            requires_operator_review,
            negative_assertions,
            redirects,
        }
    }

    /// Read accessor'lar — external crate erişebilir (INV-C8: write/construct `pub(crate)`).
    pub fn candidates(&self) -> &[AnchorCandidate] {
        &self.candidates
    }
    pub fn decision(&self) -> AnchorDecisionKind {
        self.decision
    }
    pub fn threshold_band(&self) -> ThresholdBand {
        self.threshold_band
    }
    pub fn requires_operator_review(&self) -> bool {
        self.requires_operator_review
    }
    pub fn negative_assertions(&self) -> &[String] {
        &self.negative_assertions
    }
    pub fn redirects(&self) -> &[CanonicalRedirect] {
        &self.redirects
    }
    pub fn packet_id(&self) -> &ConceptPacketId {
        &self.packet_id
    }
}

impl AnchorPlan {
    /// Okunabilir multi-line rapor — Paper 3 evidence / debug için.
    /// Anchor kararının tüm bileşenlerini (candidates, decision, redirects, negative
    /// assertions) human-readable biçimde listeler.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("AnchorPlan[{}]", self.packet_id.0));
        lines.push(format!(
            "  decision: {:?} (band: {:?}, review: {})",
            self.decision, self.threshold_band, self.requires_operator_review
        ));
        if self.candidates.is_empty() {
            lines.push("  candidates: (none — unanchored)".to_string());
        } else {
            lines.push(format!("  candidates ({}):", self.candidates.len()));
            for c in &self.candidates {
                let expl = if c.explanation.is_some() {
                    " [explained]"
                } else {
                    ""
                };
                lines.push(format!(
                    "    - {:?} → {} (score={:.3}){}",
                    c.edge_kind,
                    c.target_node_id.0,
                    c.score.total_clamped(),
                    expl
                ));
            }
        }
        if !self.redirects.is_empty() {
            lines.push(format!("  redirects ({}):", self.redirects.len()));
            for r in &self.redirects {
                lines.push(format!(
                    "    - '{}' → {} ({:?})",
                    r.attempted, r.existing_node.0, r.reason
                ));
            }
        }
        if !self.negative_assertions.is_empty() {
            lines.push(format!(
                "  negative_assertions ({}):",
                self.negative_assertions.len()
            ));
            for n in &self.negative_assertions {
                lines.push(format!("    - {}", n));
            }
        }
        lines.join("\n")
    }

    /// JSON audit raporu — Paper 3 evidence / eval metodolojisi için.
    /// Her karar explainable: packet_id, decision, threshold, candidates (explanation dahil),
    /// redirects, negative_assertions. Manuel JSON (serde_json runtime dep gerekmez).
    pub fn to_audit_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"packet_id\": \"{}\",\n", self.packet_id.0));
        s.push_str(&format!("  \"decision\": \"{:?}\",\n", self.decision));
        s.push_str(&format!(
            "  \"threshold_band\": \"{:?}\",\n",
            self.threshold_band
        ));
        s.push_str(&format!(
            "  \"requires_operator_review\": {},\n",
            self.requires_operator_review
        ));
        // candidates
        s.push_str("  \"candidates\": [");
        if self.candidates.is_empty() {
            s.push_str("],\n");
        } else {
            s.push('\n');
            for (i, c) in self.candidates.iter().enumerate() {
                s.push_str("    {\n");
                s.push_str(&format!("      \"kind\": \"{:?}\",\n", c.edge_kind));
                s.push_str(&format!("      \"target\": \"{}\",\n", c.target_node_id.0));
                s.push_str(&format!(
                    "      \"score\": {:.3},\n",
                    c.score.total_clamped()
                ));
                match &c.explanation {
                    Some(e) => s.push_str(&format!(
                        "      \"explanation\": \"{}\"\n",
                        escape_json(e.as_str())
                    )),
                    None => s.push_str("      \"explanation\": null\n"),
                }
                s.push_str("    }");
                if i + 1 < self.candidates.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str("  ],\n");
        }
        // redirects
        s.push_str(&format!(
            "  \"redirects\": {},\n",
            json_array(
                &self
                    .redirects
                    .iter()
                    .map(|r| {
                        format!(
                            "{{\"attempted\":\"{}\",\"existing\":\"{}\",\"reason\":\"{:?}\"}}",
                            escape_json(&r.attempted),
                            r.existing_node.0,
                            r.reason
                        )
                    })
                    .collect::<Vec<_>>()
            )
        ));
        // negative_assertions
        s.push_str(&format!(
            "  \"negative_assertions\": {}\n",
            json_array(
                &self
                    .negative_assertions
                    .iter()
                    .map(|n| format!("\"{}\"", escape_json(n)))
                    .collect::<Vec<_>>()
            )
        ));
        s.push('}');
        s
    }

    /// Markdown audit raporu — human-readable explainable karar.
    pub fn to_audit_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# AnchorPlan Audit — {}\n\n", self.packet_id.0));
        s.push_str(&format!("- **Decision:** `{:?}`\n", self.decision));
        s.push_str(&format!(
            "- **Threshold band:** `{:?}`\n",
            self.threshold_band
        ));
        s.push_str(&format!(
            "- **Requires operator review:** {}\n\n",
            self.requires_operator_review
        ));
        s.push_str(&format!("## Candidates ({})\n\n", self.candidates.len()));
        if self.candidates.is_empty() {
            s.push_str("_(none — unanchored)_\n\n");
        } else {
            for c in &self.candidates {
                s.push_str(&format!(
                    "- `{:?}` → `{}` (score {:.3})",
                    c.edge_kind,
                    c.target_node_id.0,
                    c.score.total_clamped()
                ));
                match &c.explanation {
                    Some(e) => s.push_str(&format!("\n  - {}", e.as_str())),
                    None => s.push_str("\n  - _(no explanation)_"),
                }
                s.push('\n');
            }
            s.push('\n');
        }
        if !self.redirects.is_empty() {
            s.push_str(&format!("## Redirects ({})\n\n", self.redirects.len()));
            for r in &self.redirects {
                s.push_str(&format!(
                    "- `{}` → `{}` ({:?})\n",
                    r.attempted, r.existing_node.0, r.reason
                ));
            }
            s.push('\n');
        }
        if !self.negative_assertions.is_empty() {
            s.push_str(&format!(
                "## Negative assertions ({})\n\n",
                self.negative_assertions.len()
            ));
            for n in &self.negative_assertions {
                s.push_str(&format!("- {}\n", n));
            }
        }
        s
    }
}

/// JSON string escape helper (manuel, serde_json runtime dep yok).
fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// JSON array builder (manuel).
fn json_array(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[\n{}\n  ]",
            items
                .iter()
                .map(|i| format!("    {}", i))
                .collect::<Vec<_>>()
                .join(",\n")
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CanonicalRedirect — INV-C8 canon gate sonucu (başarılı)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-C8: CreateNode öncesi canon gate mevcut node'a redirect etti.
/// Bu hata değil — "ödeme" → mevcut `Concept:Payment`'a bağlanma başarısı.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalRedirect {
    /// Yeni node olarak denenmiş terim ("Payments", "ödeme").
    pub attempted: String,
    /// Match edilen mevcut node.
    pub existing_node: ConceptNodeId,
    pub reason: CanonicalRedirectReason,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CanonicalRedirectReason {
    ExactCanonicalMatch,
    GlossaryAliasMatch,
    EditDistanceLe2 { distance: u32 },
}

// ═══════════════════════════════════════════════════════════════════════════════
// PositionSnapshot + PositionVector — §4.2
// ═══════════════════════════════════════════════════════════════════════════════

/// Family'ye göre eksen seti taşıyan position vektörü (INV-C2).
/// PositionSnapshot — INV-C2: `family` ayrı field DEĞİL, vector'dan türetilir.
///
/// # INV-C2 (Faz 2 — type-level, snapshot seviyesi)
/// `family` field kaldırıldı; [`PositionVector::family()`] vector'dan türetilir.
/// Bu, `family: PositionFamily::PhysicalCode` + `vector: PositionVector::Evidence(...)`
/// mismatch'ini imkansız kılar. Field'lar `pub(crate)` — TCB içi (store) erişir,
/// external crate literal construct edemez (serde hariç — deserialize için `#[serde(default)]`
/// veya custom deserialize gerekirse Faz 3'te eklenir).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PositionSnapshot {
    pub id: PositionSnapshotId,
    pub node_id: ConceptNodeId,
    pub vector: PositionVector,
    pub confidence: f64,
    /// Faz 1: u64 epoch (chrono yok).
    pub measured_at: u64,
}

impl PositionSnapshot {
    /// INV-C2: family vector'dan türetilir — mismatch imkansız.
    pub fn family(&self) -> PositionFamily {
        self.vector.family()
    }
}

/// Position eksenleri — Faz 2: family-parametric (INV-C2 type-level separation).
///
/// # INV-C2 yapısal garanti
/// Üç family'nin her biri **ayrı concrete type** (`PhysicalCodeVector`,
/// `ConceptualIntentVector`, `EvidenceVector`). Compiler karıştırmayı reddeder:
/// `PhysicalCodeVector` ile `EvidenceVector` farklı tipler, birbirine atanamaz.
/// `PositionVector` enum wrapper — `PositionSnapshot.vector` bunu taşır (serde-dostu).
///
/// Family-specific constructor'lar axis count'u position-by-position doğrular
/// (array değil, named params → yanlış eksen sayısı compile error).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "family")]
pub enum PositionVector {
    /// §4.1 PhysicalCode (Paper 1): coupling/cohesion/instability/entropy/witness_depth.
    PhysicalCode(PhysicalCodeVector),
    /// §4.1 ConceptualIntent (Paper 3): abstraction/vision_alignment/implementation/
    /// confidence/risk/code_alignment.
    ConceptualIntent(ConceptualIntentVector),
    /// §4.1 Evidence (Paper 1+3): confidence/coverage/recency/stability/source_reliability.
    Evidence(EvidenceVector),
}

impl PositionVector {
    /// INV-C2: family vector'dan türetilir — `PositionSnapshot` ayrı `family` field
    /// taşımaz, mismatch imkansız. Bu metod family/vector tutarlılığını garanti eder.
    pub fn family(&self) -> PositionFamily {
        match self {
            Self::PhysicalCode(_) => PositionFamily::PhysicalCode,
            Self::ConceptualIntent(_) => PositionFamily::ConceptualIntent,
            Self::Evidence(_) => PositionFamily::Evidence,
        }
    }
}

/// PhysicalCode position vector (Paper 1, 5 eksen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhysicalCodeVector {
    pub coupling: f64,
    pub cohesion: f64,
    pub instability: f64,
    pub entropy: f64,
    pub witness_depth: f64,
}

impl PhysicalCodeVector {
    pub fn new(
        coupling: f64,
        cohesion: f64,
        instability: f64,
        entropy: f64,
        witness_depth: f64,
    ) -> Self {
        Self {
            coupling,
            cohesion,
            instability,
            entropy,
            witness_depth,
        }
    }
}

/// ConceptualIntent position vector (Paper 3, 6 eksen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptualIntentVector {
    pub abstraction: f64,
    pub vision_alignment: f64,
    pub implementation: f64,
    pub confidence: f64,
    pub risk: f64,
    pub code_alignment: f64,
}

impl ConceptualIntentVector {
    pub fn new(
        abstraction: f64,
        vision_alignment: f64,
        implementation: f64,
        confidence: f64,
        risk: f64,
        code_alignment: f64,
    ) -> Self {
        Self {
            abstraction,
            vision_alignment,
            implementation,
            confidence,
            risk,
            code_alignment,
        }
    }
}

/// Evidence position vector (Paper 1+3, 5 eksen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceVector {
    pub confidence: f64,
    pub coverage: f64,
    pub recency: f64,
    pub stability: f64,
    pub source_reliability: f64,
}

impl EvidenceVector {
    pub fn new(
        confidence: f64,
        coverage: f64,
        recency: f64,
        stability: f64,
        source_reliability: f64,
    ) -> Self {
        Self {
            confidence,
            coverage,
            recency,
            stability,
            source_reliability,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptGraph — in-memory graph (space.rs desenine sadık)
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept anchoring graph'ı. `Space`'ten ayrı (`EdgeKind` 8 varyant,
/// `ConceptEdgeKind` 15). `HashMap<id, node>` + `Vec<edge>`.
///
/// # INV-C3 (Faz 2 — type-level kapsülleme)
/// `nodes`/`edges` field'ları **private**. Harici mutasyon yok — sadece
/// [`crate::anchoring::store::InMemoryAnchorStore`] (TCB içi) `pub(crate)`
/// metodlarla erişir. External crate `graph.nodes.get_mut(...).decision_status = Accepted`
/// yazamaz (compile error). Bu, INV-C3 "candidate isolation"ın yapısal garantisidir:
/// `Accepted` status sadece [`crate::anchoring::store::InMemoryAnchorStore::promote_to_accepted`]
/// (`OperatorAcceptance` ile) üzerinden yazılabilir.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConceptGraph {
    nodes: std::collections::HashMap<ConceptNodeId, ConceptNode>,
    edges: Vec<ConceptEdge>,
}

impl ConceptGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Node ekle (TCB içi — store/graph seed). INV-C3: harici crate erişemez.
    pub(crate) fn insert_node(&mut self, node: ConceptNode) -> &mut Self {
        self.nodes.insert(node.id.clone(), node);
        self
    }

    /// Edge ekle (TCB içi).
    pub(crate) fn insert_edge(&mut self, edge: ConceptEdge) -> &mut Self {
        self.edges.push(edge);
        self
    }

    pub fn node(&self, id: &ConceptNodeId) -> Option<&ConceptNode> {
        self.nodes.get(id)
    }

    /// Tüm node'lara read-only iterator (INV-C3: mutasyon yok).
    pub fn nodes_iter(&self) -> impl Iterator<Item = &ConceptNode> {
        self.nodes.values()
    }

    /// Tüm edge'lere read-only iterator.
    pub fn edges(&self) -> impl Iterator<Item = &ConceptEdge> {
        self.edges.iter()
    }

    /// INV-C8: canonical exact match.
    pub fn find_concept_by_canonical(&self, canonical: &str) -> Vec<&ConceptNode> {
        self.nodes
            .values()
            .filter(|n| n.canonical.eq_ignore_ascii_case(canonical))
            .collect()
    }

    /// INV-C8: canonical veya alias match.
    pub fn find_concepts_by_canonical_or_alias(&self, term: &str) -> Vec<&ConceptNode> {
        let lower = term.to_lowercase();
        self.nodes
            .values()
            .filter(|n| {
                n.canonical.to_lowercase() == lower
                    || n.aliases.iter().any(|a| a.to_lowercase() == lower)
            })
            .collect()
    }

    /// Node'u mutable al (TCB içi — store promote/apply_plan için).
    pub(crate) fn node_mut(&mut self, id: &ConceptNodeId) -> Option<&mut ConceptNode> {
        self.nodes.get_mut(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GraphSeed — runtime boundary (fixture-bağımsız)
// ═══════════════════════════════════════════════════════════════════════════════

/// Store seed'i. FixtureGiven (test-only) buraya dönüştürülür — store.rs
/// FixtureGiven bilmez (runtime/test boundary).
///
/// # Faz 5a — candidate bucket'ları (Patch 6)
/// PR33a önce yalnızca 3 bucket vardı (concepts/decisions/code_entities); candidate
/// node'ları (RuleCandidate/TaskCandidate/RiskCandidate) seed edilemiyordu. 3 yeni
/// bucket eklendi. Backward-compat: GraphSeed `Default` derive'a sahip; yeni field'lar
/// `Vec::default()` (boş) ile başlar — eski kod `GraphSeed { concepts, decisions,
/// code_entities }` ile çalışmaya devam eder (PartialEq hariç, ama seed literal
/// construct zaten pub(crate)/test-only). `all_nodes()` deterministik sıra: concepts
/// → decisions → code_entities → rule_candidates → task_candidates → risk_candidates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphSeed {
    pub concepts: Vec<ConceptNode>,
    pub decisions: Vec<ConceptNode>,
    pub code_entities: Vec<ConceptNode>,
    /// Faz 5a — RuleCandidate seed'leri.
    pub rule_candidates: Vec<ConceptNode>,
    /// Faz 5a — TaskCandidate seed'leri.
    pub task_candidates: Vec<ConceptNode>,
    /// Faz 5a — RiskCandidate seed'leri.
    pub risk_candidates: Vec<ConceptNode>,
}

impl GraphSeed {
    pub fn is_empty(&self) -> bool {
        self.concepts.is_empty()
            && self.decisions.is_empty()
            && self.code_entities.is_empty()
            && self.rule_candidates.is_empty()
            && self.task_candidates.is_empty()
            && self.risk_candidates.is_empty()
    }

    /// Tüm seed node'larını tek iteratörde (deterministik sıra — Patch 6).
    pub fn all_nodes(&self) -> impl Iterator<Item = &ConceptNode> {
        self.concepts
            .iter()
            .chain(self.decisions.iter())
            .chain(self.code_entities.iter())
            .chain(self.rule_candidates.iter())
            .chain(self.task_candidates.iter())
            .chain(self.risk_candidates.iter())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 3 — Persistence boundary tipleri (serde boundary, INV-C3/C8 koruması)
// ═══════════════════════════════════════════════════════════════════════════════

/// `ConceptGraph` snapshot — trusted restore için (Faz 3, INV-C3 persistence boundary).
///
/// # INV-C3 boundary
/// [`ConceptGraph`] private field'lara sahip ve `Deserialize` yok (serde bypass
/// edip Accepted node reconstruct etmesin diye). Bu snapshot **trusted restore
/// path** için — operator-belirlenmiş Accepted node'lar dahil tüm graph durumu.
/// [`InMemoryAnchorStore::restore_trusted_snapshot`] üzerinden yüklenir.
///
/// **Normal mutation DEĞİL** — bu deserialize/restore, yeni promotion değil.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptGraphSnapshot {
    pub nodes: Vec<ConceptNode>,
    pub edges: Vec<ConceptEdge>,
    /// Paper 1/2 `SNAPSHOT_FORMAT_VERSION` pattern mirror — schema migration hazırlığı.
    pub schema_version: u32,
}

impl ConceptGraphSnapshot {
    /// Faz 3 schema version. `restore_trusted_snapshot` bu değeri kontrol eder —
    /// mismatch → `StoreError::InvalidSnapshotVersion` (trusted restore boundary).
    pub const SCHEMA_VERSION: u32 = 1;
}

/// `AnchorPlan` audit record — DB'den okunan, **apply edilemez** kayıt (Faz 3, INV-C8).
///
/// # INV-C8 boundary
/// [`AnchorPlan`] `Serialize` var ama `Deserialize` **YOK** — "AnchorPlan almak =
/// canon gate'ten geçmiş olmak" garantisi serde ile delinmesin diye. Bu audit tipi
/// DB read için; apply edilemez (uygulama yapılamaz), sadece inspect/audit.
///
/// `AnchorPlan::to_audit_json()` audit write için; bu tip structured DB read için.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedAnchorPlanAudit {
    pub packet_id: String,
    pub decision: String,
    /// §8.2 threshold band (Strong/Tentative/Weak/Unanchored) — kararın skor bandı.
    pub threshold_band: String,
    pub candidates: Vec<PersistedAnchorCandidateAudit>,
    pub redirects: Vec<PersistedRedirectAudit>,
    pub negative_assertions: Vec<String>,
    pub requires_operator_review: bool,
    pub schema_version: u32,
}

/// Persisted candidate audit (apply edilemez).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedAnchorCandidateAudit {
    pub edge_kind: String,
    pub target: String,
    pub score: f64,
    pub explanation: Option<String>,
}

/// Persisted redirect audit.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedRedirectAudit {
    pub attempted: String,
    pub existing_node: String,
    pub reason: String,
}

impl PersistedAnchorPlanAudit {
    /// Faz 3 schema version (Paper 1/2 SNAPSHOT_FORMAT_VERSION pattern).
    pub const SCHEMA_VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_empty_explanation_rejects_empty() {
        // INV-C7: boş string type-level engelli
        assert!(NonEmptyExplanation::new("").is_err());
        assert!(NonEmptyExplanation::new("   ").is_err());
        assert!(NonEmptyExplanation::new("\t\n").is_err());
    }

    #[test]
    fn non_empty_explanation_accepts_non_empty() {
        let e = NonEmptyExplanation::new("risk derived").unwrap();
        assert_eq!(e.as_str(), "risk derived");
        assert_eq!(e.to_string(), "risk derived");
        assert_eq!(e.as_ref(), "risk derived");
    }

    #[test]
    fn non_empty_explanation_serde_roundtrip() {
        let e = NonEmptyExplanation::new("explanation text").unwrap();
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, "\"explanation text\"");
        let back: NonEmptyExplanation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn non_empty_explanation_serde_rejects_empty() {
        // Boş string deserialize'ta reject (EmptyExplanation → serde error)
        let result: Result<NonEmptyExplanation, _> = serde_json::from_str("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn anchor_plan_only_constructible_via_from_gate() {
        // INV-C8: AnchorPlan field'ları pub(crate) → bu test (aynı crate) erişebilir,
        // ama external crate (tests/) literal construct edemez. Burada from_gate çalışır.
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:test".into()),
            vec![],
            crate::anchoring::AnchorDecisionKind::MarkUnanchored,
            crate::anchoring::ThresholdBand::Unanchored,
            false,
            vec![],
            vec![],
        );
        assert_eq!(
            plan.decision(),
            crate::anchoring::AnchorDecisionKind::MarkUnanchored
        );
        assert!(plan.candidates().is_empty());
    }

    #[test]
    fn scalar_similarity_range_check() {
        // INV-C1: [0,1] range-check
        assert!(ScalarSimilarity::new(0.0).is_ok());
        assert!(ScalarSimilarity::new(0.5).is_ok());
        assert!(ScalarSimilarity::new(1.0).is_ok());
        assert!(ScalarSimilarity::new(-0.1).is_err());
        assert!(ScalarSimilarity::new(1.1).is_err());
        assert!(ScalarSimilarity::new(f64::NAN).is_err());
    }

    #[test]
    fn scalar_similarity_zero_one_get() {
        assert_eq!(ScalarSimilarity::zero().get(), 0.0);
        assert_eq!(ScalarSimilarity::one().get(), 1.0);
        let s = ScalarSimilarity::new(0.7).unwrap();
        assert_eq!(s.get(), 0.7);
    }

    #[test]
    fn scalar_similarity_serde_roundtrip() {
        let s = ScalarSimilarity::new(0.42).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "0.42");
        let back: ScalarSimilarity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn scalar_similarity_serde_rejects_out_of_range() {
        // Hardening PR: Deserialize new() üzerinden range-check yapar.
        // serde_json::from_str("2.0") / "-1.0" / "NaN" reject — constructor bypass
        // edilemez (EvidenceStrength R1 ile aynı serde hijyeni).
        assert!(serde_json::from_str::<ScalarSimilarity>("2.0").is_err());
        assert!(serde_json::from_str::<ScalarSimilarity>("-1.0").is_err());
        assert!(serde_json::from_str::<ScalarSimilarity>("\"NaN\"").is_err());
    }

    #[test]
    fn anchor_score_breakdown_uses_scalar_similarity() {
        // INV-C1: breakdown semantic_similarity ScalarSimilarity (vector değil)
        let mut b = AnchorScoreBreakdown::zeroed();
        b.semantic_similarity = ScalarSimilarity::one();
        assert_eq!(b.semantic_similarity.get(), 1.0);
        // raw_total: 0.25 * 1.0 = 0.25 (sadece semantic, diğerleri 0)
        assert!((b.raw_total() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn position_vector_families_are_distinct_types() {
        // INV-C2: üç family ayrı concrete type → compiler karıştırmayı reddeder
        let phys = PhysicalCodeVector::new(0.8, 0.6, 0.4, 0.3, 0.2);
        let intent = ConceptualIntentVector::new(0.5, 0.7, 0.3, 0.6, 0.4, 0.8);
        let evidence = EvidenceVector::new(0.9, 0.85, 0.7, 0.6, 0.95);

        // Farklı tipler — typed accessor'lar
        assert!((phys.coupling - 0.8).abs() < 1e-9);
        assert!((intent.abstraction - 0.5).abs() < 1e-9);
        assert!((evidence.source_reliability - 0.95).abs() < 1e-9);

        // PositionVector enum wrap
        let pv_phys = PositionVector::PhysicalCode(phys);
        let pv_intent = PositionVector::ConceptualIntent(intent);
        let pv_evidence = PositionVector::Evidence(evidence);
        assert!(matches!(pv_phys, PositionVector::PhysicalCode(_)));
        assert!(matches!(pv_intent, PositionVector::ConceptualIntent(_)));
        assert!(matches!(pv_evidence, PositionVector::Evidence(_)));
    }

    #[test]
    fn position_vector_serde_roundtrip() {
        let pv = PositionVector::PhysicalCode(PhysicalCodeVector::new(0.8, 0.6, 0.4, 0.3, 0.2));
        let json = serde_json::to_string(&pv).unwrap();
        let back: PositionVector = serde_json::from_str(&json).unwrap();
        assert_eq!(pv, back);
    }

    #[test]
    fn anchor_plan_audit_json_contains_decision_and_explanations() {
        // INV-C7 audit: her karar explainable
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:audit".into()),
            vec![AnchorCandidate::from_scored(
                ExtractedAnchorCandidate::new(
                    ConceptPacketId("pkt:audit".into()),
                    ConceptNodeId("Concept:Payment".into()),
                    crate::anchoring::ConceptEdgeKind::Mentions,
                    None,
                ),
                AnchorScoreBreakdown::zeroed(),
            )],
            crate::anchoring::AnchorDecisionKind::TentativeLink,
            crate::anchoring::ThresholdBand::Tentative,
            true,
            vec!["SUPERSEDES yasak".into()],
            vec![],
        );
        let json = plan.to_audit_json();
        assert!(json.contains("\"packet_id\": \"pkt:audit\""));
        assert!(json.contains("\"decision\": \"TentativeLink\""));
        assert!(json.contains("\"SUPERSEDES yasak\""));
        // explanation null (düşük-stake Mentions)
        assert!(json.contains("\"explanation\": null"));
    }

    #[test]
    fn anchor_plan_audit_markdown_readable() {
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:md".into()),
            vec![],
            crate::anchoring::AnchorDecisionKind::MarkUnanchored,
            crate::anchoring::ThresholdBand::Unanchored,
            false,
            vec![],
            vec![],
        );
        let md = plan.to_audit_markdown();
        assert!(md.contains("# AnchorPlan Audit — pkt:md"));
        assert!(md.contains("**Decision:** `MarkUnanchored`"));
        assert!(md.contains("_(none — unanchored)_"));
    }

    #[test]
    fn anchor_plan_audit_json_is_valid_json() {
        // Manuel JSON üretimi malformed olabilir — serde_json ile parse-validity doğrula.
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:valid".into()),
            vec![AnchorCandidate::from_scored(
                ExtractedAnchorCandidate::new(
                    ConceptPacketId("pkt:valid".into()),
                    ConceptNodeId("Concept:Payment".into()),
                    crate::anchoring::ConceptEdgeKind::Mentions,
                    None,
                ),
                AnchorScoreBreakdown::zeroed(),
            )],
            crate::anchoring::AnchorDecisionKind::TentativeLink,
            crate::anchoring::ThresholdBand::Tentative,
            true,
            vec!["SUPERSEDES yasak".into()],
            vec![],
        );
        let json = plan.to_audit_json();
        // Geçerli JSON olması lazım (serde_json parse edebilmeli)
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("audit JSON geçerli JSON olmalı");
        assert_eq!(parsed["packet_id"], "pkt:valid");
        assert_eq!(parsed["decision"], "TentativeLink");
        assert!(parsed["candidates"].is_array());
    }
}

#[test]
fn concept_graph_snapshot_serde_roundtrip() {
    // Faz 3: ConceptGraphSnapshot (trusted restore) serde
    let node = ConceptNode {
        id: ConceptNodeId("Concept:Payment".into()),
        canonical: "Payment".into(),
        aliases: vec!["ödeme".into()],
        node_kind: ConceptNodeKind::Concept,
        decision_status: DecisionStatus::Accepted,
        position_family: crate::anchoring::PositionFamily::ConceptualIntent,
    };
    let snapshot = ConceptGraphSnapshot {
        nodes: vec![node],
        edges: vec![],
        schema_version: 1,
    };
    let json = serde_json::to_string(&snapshot).unwrap();
    let back: ConceptGraphSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(snapshot, back);
}

#[test]
fn persisted_anchor_plan_audit_serde_roundtrip() {
    // Faz 3: PersistedAnchorPlanAudit (DB read, apply edilemez) serde
    let audit = PersistedAnchorPlanAudit {
        packet_id: "pkt:audit".into(),
        decision: "TentativeLink".into(),
        threshold_band: "Tentative".into(),
        candidates: vec![PersistedAnchorCandidateAudit {
            edge_kind: "DerivesRisk".into(),
            target: "RiskCandidate:X".into(),
            score: 0.7,
            explanation: Some("risk derived".into()),
        }],
        redirects: vec![PersistedRedirectAudit {
            attempted: "ödeme".into(),
            existing_node: "Concept:Payment".into(),
            reason: "GlossaryAliasMatch".into(),
        }],
        negative_assertions: vec!["SUPERSEDES yasak".into()],
        requires_operator_review: true,
        schema_version: PersistedAnchorPlanAudit::SCHEMA_VERSION,
    };
    let json = serde_json::to_string(&audit).unwrap();
    let back: PersistedAnchorPlanAudit = serde_json::from_str(&json).unwrap();
    assert_eq!(audit, back);
}
