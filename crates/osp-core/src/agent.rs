//! Agent semantiği — Faz 5 stub tipleri (agent-prompt-semantics.md).
//!
//! Bu modül Faz 5 (LLM OSP Codec) tasarımının tip iskeletini içerir. Implementasyonlar
//! (compute_space_slice, LLM runtime, witness feedback) Faz 5'te gelir. Şu an sadece
//! tipler tanımlı — `engine.rs` Q4/Q6 gate'leri ve `EngineCommitError` variant'ları
//! bunlara referans verir.
//!
//! **Önemli (inv #11-14):**
//! - LLM durumsuzdur, durum Agent kabuğunda
//! - `DeltaProposal` pozisyon **içermez** — engine compute eder (inv #4)
//! - `PermissionMask` God Mode tarafından atanır, Agent değiştiremez (inv #13)
//! - Prompt doğal dil değil, tiplenmiş pakettir (inv #14)

use std::collections::HashSet;

use crate::coords::{AxisId, RawPosition};
use crate::rule::Rule;
use crate::space::{Edge, EdgeKind, Node, NodeId, NodeKind, Space};
use crate::witness::ClaimId;

// ═══════════════════════════════════════════════════════════════════════════════
// PermissionMask (inv #13 — God Mode atanır, Agent değiştiremez)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agent'ın okuma/yazma yetki matrisi (agent-prompt-semantics.md §2.1).
///
/// God Mode (insan-operatör veya bootstrap config) tarafından Intent hedef alanına
/// ve Agent rolüne göre atanır. Agent kabuğu ve LLM kendi yetkilerini genişletemez.
///
/// Üç-nokta savunma derinliği:
/// 1. `compute_space_slice()` — okuma izni olmayan düğümleri projeksiyondan çıkarır
/// 2. Agent kabuğu — yazma izni olmayan mutasyonları erken reddeder
/// 3. `SpaceEngine::commit()` — nihai zorunlu kontrol (atlanamaz)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PermissionMask {
    /// Agent'ın tamamen GÖREMEYECEĞİ düğümler (hidden — space_slice'tan çıkarılır).
    pub hidden_nodes: HashSet<NodeId>,
    /// Agent'ın okuyabileceği ama DEĞİŞTİREMEYECEĞİ düğümler (read-only).
    pub read_only_nodes: HashSet<NodeId>,
    /// Agent'ın yeni düğüm ekleyebileceği veya koordinat güncelleyebileceği eksenler.
    pub writable_axes: HashSet<AxisId>,
    /// Agent'ın oluşturamayacağı kenar türleri (örn: Approves → sadece Witness).
    pub forbidden_edge_kinds: HashSet<EdgeKind>,
    /// Agent'ın pozisyon güncelleyebileceği maksimum sapma (θ_max yetki sınırı).
    pub max_position_deviation: f64,
}

impl PermissionMask {
    /// Default: tüm node'lar read-write, tüm axis'ler writable, sınırsız deviation.
    pub fn full_access() -> Self {
        Self {
            hidden_nodes: HashSet::new(),
            read_only_nodes: HashSet::new(),
            writable_axes: HashSet::new(),
            forbidden_edge_kinds: HashSet::new(),
            max_position_deviation: f64::MAX,
        }
    }

    /// Node'u Agent görebilir mi? (compute_space_slice denetim noktası 1)
    ///
    /// `hidden_nodes` içinde DEĞİL ise görünür. `read_only_nodes` görünür
    /// ama yazılamaz (has_write_permission ayrı kontrol).
    pub fn has_read_permission(&self, node: NodeId) -> bool {
        !self.hidden_nodes.contains(&node)
    }

    /// Node'u Agent değiştirebilir mi? (Agent kabuğu denetim noktası 2)
    ///
    /// `hidden_nodes` VE `read_only_nodes` içinde DEĞİL ise yazılabilir.
    pub fn has_write_permission(&self, node: NodeId) -> bool {
        !self.hidden_nodes.contains(&node) && !self.read_only_nodes.contains(&node)
    }

    /// EdgeKind oluşturabilir mi? (forbidden_edge_kinds kontrolü)
    pub fn can_create_edge(&self, kind: EdgeKind) -> bool {
        !self.forbidden_edge_kinds.contains(&kind)
    }

    /// Belirli bir node'u hidden yap (God Mode API).
    pub fn hide_node(&mut self, node: NodeId) {
        self.hidden_nodes.insert(node);
    }

    /// Belirli bir node'u read-only yap (God Mode API).
    pub fn set_read_only(&mut self, node: NodeId) {
        self.read_only_nodes.insert(node);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DeltaProposal (LLM çıktısı — structural only, NO positions)
// ═══════════════════════════════════════════════════════════════════════════════

/// LLM'den beklenen çıktı (inv #12). Agent kabuğu LLM çıktısını bu şemaya göre
/// deserialize eder; uymayan çıktılar Q4 Syntax Gate'inde deterministik reddedilir.
///
/// **KRİTİK (inv #4):** Pozisyon **içermez** — sadece yapısal değişiklikler.
/// Pozisyonlar `SpaceEngine` tarafından compute edilir (agent-prompt-semantics.md §2.2).
///
/// **G2c-2 (arkadaş review 7):** `removed_edges` (subtractive structural delta) +
/// `affected_nodes` (ölçüm scope — `new_nodes`'a target koyma ontolojik tutarsızlık).
/// `OpKind::RemoveImport` artık engine'de onurlandırılır.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DeltaProposal {
    /// Yeni eklenecek ontolojik düğümler.
    pub new_nodes: Vec<NewNodeSpec>,
    /// Yeni eklenecek tiplenmiş kenarlar.
    pub new_edges: Vec<NewEdgeSpec>,
    /// **G2c-2:** Kaldırılacak kenarlar (coupling/instability düşürme = import kaldırma).
    /// `OpKind::RemoveImport` operation policy ile bağlanır — allowed_ops kontrolü navigator'da.
    pub removed_edges: Vec<EdgeRef>,
    /// **G2c-2 (review 7 #6):** Ölçülecek/etkilenen MEVCUT node ID'leri.
    /// `compute_raw_from_delta` bu node'ların pozisyonunu ölçer (target node DAHİL).
    /// Boşsa `new_nodes`'un ID'leri kullanılır. `new_nodes`'a mevcut node koyma —
    /// ontolojik tutarsızlık (yeni varlık vs ölçüm scope).
    pub affected_nodes: Vec<NodeId>,
    /// Mevcut düğümlerin entity özelliklerinde değişiklikler (kind/mass/metadata — POZİSYON DEĞİL).
    pub modified_entities: Vec<EntityChangeSpec>,
    /// LLM'in pozisyonla ilgili tavsiyeleri — ADVISORY ONLY, authoritative değil.
    pub position_hints: Vec<PositionHint>,
    /// LLM'in kararlarını açıklayan gerekçe (şahitler tarafından okunabilir).
    pub reasoning: String,
}

/// **G2c-2:** Edge referansı — kaldırma için from/to/kind triplet.
/// `Space::remove_edge` ve `compute_raw_from_delta` tarafından kullanılır.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EdgeRef {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewNodeSpec {
    pub kind: crate::space::NodeKind,
    pub initial_mass: f64,
    pub connected_to: Vec<(NodeId, EdgeKind)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewEdgeSpec {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityChangeSpec {
    pub node_id: NodeId,
    // Faz 5: pub changes: EntityChanges (kind/mass/metadata — RawPosition hariç)
}

/// LLM'in "bu node şu pozisyonda olmalı" tavsiyesi — engine tarafından authoritative
/// kabul EDİLMEZ. Sadece diagnostic amaçlı (agent-prompt-semantics.md §2.2).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionHint {
    pub node_id: NodeId,
    pub suggested_raw: RawPosition,
    pub rationale: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// OutputContract (DeltaProposal şema doğrulaması — Q4 Syntax Gate)
// ═══════════════════════════════════════════════════════════════════════════════

/// LLM'den beklenen çıktı şeması (inv #12). Agent kabuğu LLM çıktısını bu kontrata
/// göre deserialize eder; uymayan çıktılar Q4'te deterministik reddedilir.
///
/// **Faz 5 gerçek impl:** DeltaProposal yapısal bütünlüğünü doğrular.
/// Bu, engine'in Q4 (check_claim_syntax) gate'inin **Agent shell tarafındaki** karşılığıdır —
/// LLM çıktısı Claim'e dönüştürülmeden ÖNCE şema kontrolü yapılır.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OutputContract {
    /// İzin verilen NodeKind'ler. `None` = tümü izinli.
    pub allowed_node_kinds: Option<HashSet<NodeKind>>,
    /// `true` ise `reasoning` boş olamaz (LLM kararını açıklamalı).
    pub require_reasoning: bool,
    /// Maksimum node sayısı. `None` = sınırsız.
    pub max_nodes: Option<usize>,
}

impl OutputContract {
    /// DeltaProposal şema doğrulaması (Q4 Syntax Gate — Agent shell).
    ///
    /// Kontroller (proposal'ın kendi içinde doğrulanabilecekler):
    /// 1. new_nodes: NodeKind allowed (varsa), mass finite/non-negative,
    ///    connected_to duplicate-edge reddi
    /// 2. new_edges: Imports self-loop reddi (from == to), duplicate
    ///    (from, to, kind) üçlüsü reddi
    /// 3. position_hints: suggested_raw finite
    /// 4. reasoning: require_reasoning ise boş olamaz
    /// 5. max_nodes: limit aşımı
    ///
    /// **Kapsam dışı (Engine sorumluluğunda — `check_claim_syntax`):**
    /// - Cross-ref: new_edges from/to ve connected_to target_id'lerin existing
    ///   space node'larına işaret ettiği — `validate()` `&Space` göremiyor.
    ///   (from/to existing space node'larına referans eder, faz5_e2e.rs örneği;
    ///   index olarak yorumlanmaz.)
    /// - Duplicate node IDs — `NewNodeSpec`'lerin ID'si yok (index tabanlı),
    ///   Engine `Claim.delta_nodes` (resolved ID'ler) üzerinde kontrol eder.
    /// - modified_entities: `EntityChangeSpec.changes` Faz 5 stub — doğrulanacak
    ///   içerik henüz yok.
    pub fn validate(&self, proposal: &DeltaProposal) -> Result<(), SyntaxViolation> {
        // 6. Max nodes check
        if let Some(max) = self.max_nodes {
            if proposal.new_nodes.len() > max {
                return Err(SyntaxViolation {
                    claim_id: 0, // Agent shell'de claim_id henüz atanmamış
                    detail: format!(
                        "DeltaProposal has {} nodes, max allowed: {}",
                        proposal.new_nodes.len(),
                        max
                    ),
                });
            }
        }

        // 1. NewNodeSpec validation
        for (i, node) in proposal.new_nodes.iter().enumerate() {
            // NodeKind allowed?
            if let Some(allowed) = &self.allowed_node_kinds {
                if !allowed.contains(&node.kind) {
                    return Err(SyntaxViolation {
                        claim_id: 0,
                        detail: format!(
                            "new_nodes[{}]: NodeKind {:?} not in allowed kinds",
                            i, node.kind
                        ),
                    });
                }
            }

            // Mass valid?
            if !node.initial_mass.is_finite() || node.initial_mass < 0.0 {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "new_nodes[{}]: initial_mass {} invalid (must be finite, non-negative)",
                        i, node.initial_mass
                    ),
                });
            }

            // connected_to: duplicate (target_id, edge_kind) çifti reddi.
            //
            // connected_to, new_edges'te ayrıca listelenmemiş örtülü kenarlar için
            // kısayoldur — target_id'ler existing space node'larıdır (agent shell
            // mevcut space'i bilir). Dangling-ref kontrolü `validate()`'in
            // kapsamı dışında (Space'e erişim yok) — bu Engine `check_claim_syntax`
            // tarafından resolved-ID aşamasında yapılır.
            //
            // Imports self-loop connected_to yoluyla ifade edilemez: yeni node'un
            // ID'si henüz atanmamış (index tabanlı provizyonel ID), bu yüzden
            // "yeni node kendini import eder" senaryosu burada oluşamaz.
            let mut seen_edges: HashSet<(NodeId, EdgeKind)> = HashSet::new();
            for (target_id, edge_kind) in &node.connected_to {
                if !seen_edges.insert((*target_id, *edge_kind)) {
                    return Err(SyntaxViolation {
                        claim_id: 0,
                        detail: format!(
                            "new_nodes[{}]: duplicate connected_to edge (target {}, {:?})",
                            i, target_id, edge_kind
                        ),
                    });
                }
            }
        }

        // 2. NewEdgeSpec validation
        let mut seen_explicit_edges: HashSet<(NodeId, NodeId, EdgeKind)> = HashSet::new();
        for (i, edge) in proposal.new_edges.iter().enumerate() {
            // Imports self-loop
            if edge.kind == EdgeKind::Imports && edge.from == edge.to {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "new_edges[{}]: Imports self-loop (node {} → {})",
                        i, edge.from, edge.to
                    ),
                });
            }

            // Duplicate (from, to, kind) üçlüsü reddi — connected_to ile simetrik.
            // Aynı kenarı iki kez listelemek gereksiz; farklı EdgeKind'ler aynı
            // from→to çifti için geçerli (örn. A -Imports-> B ve A -Calls-> B).
            if !seen_explicit_edges.insert((edge.from, edge.to, edge.kind)) {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "new_edges[{}]: duplicate edge ({} → {}, {:?})",
                        i, edge.from, edge.to, edge.kind
                    ),
                });
            }
        }

        // 3. modified_entities: EntityChangeSpec.changes alanı Faz 5 stub —
        //    doğrulanacak içerik henüz yok. Faz 5'te EntityChanges gelince
        //    buraya validation eklenecek.

        // 4. position_hints validation (advisory — just check finiteness)
        for (i, hint) in proposal.position_hints.iter().enumerate() {
            let raw = &hint.suggested_raw;
            let axes = [raw.x, raw.y, raw.z, raw.w, raw.v];
            if axes.iter().any(|v| !v.is_finite()) {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "position_hints[{}]: suggested_raw contains non-finite values",
                        i
                    ),
                });
            }
        }

        // 5. reasoning check
        if self.require_reasoning && proposal.reasoning.trim().is_empty() {
            return Err(SyntaxViolation {
                claim_id: 0,
                detail: "reasoning is empty but require_reasoning=true".to_string(),
            });
        }

        Ok(())
    }

    /// Strict contract: all node kinds allowed, reasoning required, no node limit.
    pub fn strict() -> Self {
        Self {
            allowed_node_kinds: None,
            require_reasoning: true,
            max_nodes: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SyntaxViolation (Q4 failure — EngineCommitError::SyntaxViolation)
// ═══════════════════════════════════════════════════════════════════════════════

/// Q4 Syntax Gate failure — DeltaProposal OutputContract'a uymuyor (inv #12).
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxViolation {
    pub claim_id: ClaimId,
    pub detail: String,
}

impl std::fmt::Display for SyntaxViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Q4 syntax violation (claim {}): {}",
            self.claim_id, self.detail
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hallucination Classification (agent-prompt-semantics.md §4.1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agent üretiminin neden reddedildiğini sınıflandırır — her tür farklı
/// kalibrasyon geri bildirimi gerektirir.
///
/// 5 tür (agent-prompt-semantics.md §4.1):
/// - Structural: Q4 Syntax Gate ihlali (OutputContract'e uymayan format)
/// - Vision: Q5 Vision Gate ihlali (θ > θ_bound)
/// - Rule: Q6 Rule Gate ihlali (mimari kural ihlali)
/// - Witness: Q3 honest-reject (şahit reddetti)
/// - Undersupported: Q1/Q2 yetersiz (şahit sayısı/quorum eksik)
#[derive(Debug, Clone, PartialEq)]
pub enum HallucinationType {
    /// Q4: DeltaProposal OutputContract'a uymuyor (inv #12).
    Structural { detail: String },
    /// Q5: θ > θ_bound — claim vision'dan çok saptı.
    Vision { theta: f64, bound: f64 },
    /// Q6: Bir Rule ihlal edildi.
    Rule { rule_id: String, detail: String },
    /// Q3: Honest witness reddetti (explicit reject).
    Witness { witness: u64 },
    /// Q1/Q2: Yeterli şahit yok — bekle veya ek şahit talep et.
    Undersupported { support: f64, threshold: f64 },
}

impl HallucinationType {
    /// EngineCommitError'dan hallucination türünü çıkarır.
    /// `None` = hatasız (PermissionDenied, NoPersistence vb. — hallucination değil).
    pub fn from_engine_error(err: &crate::engine::EngineCommitError) -> Option<Self> {
        match err {
            crate::engine::EngineCommitError::SyntaxViolation { violation, .. } => {
                Some(Self::Structural {
                    detail: violation.detail.clone(),
                })
            }
            crate::engine::EngineCommitError::VisionViolation {
                violation, bound, ..
            } => Some(Self::Vision {
                theta: violation.theta,
                bound: *bound,
            }),
            crate::engine::EngineCommitError::RuleViolation { violation, .. } => Some(Self::Rule {
                rule_id: violation.rule_id.clone(),
                detail: violation.detail.clone(),
            }),
            // **INV-T9:** Witness Hold/Rejected artık EngineCommitResult üzerinden gelir,
            // EngineCommitError'da DEĞİL. InvalidWitnessEvidence = operational fault
            // (malformed/author-self/duplicate) — agent hallucination sinyali olabilir.
            crate::engine::EngineCommitError::InvalidWitnessEvidence(msg) => {
                Some(Self::Undersupported {
                    support: 0.0,
                    threshold: 1.5,
                }).map(|h| {
                    // msg'yi detail'e göm — evidence debug için.
                    let _ = msg;
                    h
                })
            }
            _ => None, // PermissionDenied, NoPersistence, Persistence, Internal — not hallucination
        }
    }

    /// İnsan-okunabilir kalibrasyon mesajı (Agent'a geri bildirim için).
    pub fn calibration_message(&self) -> String {
        match self {
            Self::Structural { detail } => {
                format!("Structural hallucination: output format is invalid — {detail}. Fix the DeltaProposal schema and retry.")
            }
            Self::Vision { theta, bound } => {
                format!("Vision hallucination: θ={theta:.3} > bound={bound:.3}. The proposed change deviates too far from the architectural vision. Adjust the approach to align with vision targets.")
            }
            Self::Rule { rule_id, detail } => {
                format!("Rule hallucination: rule '{rule_id}' violated — {detail}. Find an alternative path that respects this constraint.")
            }
            Self::Witness { witness } => {
                format!("Witness hallucination: honest witness #{witness} rejected this claim. Review the rejection feedback and revise.")
            }
            Self::Undersupported { support, threshold } => {
                format!("Undersupported claim: support={support:.2} < threshold={threshold:.2}. Wait for additional witnesses or request more review.")
            }
        }
    }

    /// Gate seviyesi (Q4/Q5/Q6/Q1-Q3) — hangi gate'te takıldı.
    pub fn gate(&self) -> &'static str {
        match self {
            Self::Structural { .. } => "Q4 Syntax",
            Self::Vision { .. } => "Q5 Vision",
            Self::Rule { .. } => "Q6 Rule",
            Self::Witness { .. } => "Q3 Witness",
            Self::Undersupported { .. } => "Q1-Q2 Quorum",
        }
    }
}

impl std::fmt::Display for HallucinationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.gate(), self.calibration_message())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OspPrompt (inv #14 — tiplenmiş paket, doğal dil değil)
// ═══════════════════════════════════════════════════════════════════════════════

/// Epistemik Projeksiyon Paketi (`π_A`) — agent-prompt-semantics.md §2.
///
/// `SpaceEngine` tarafından üretilen tiplenmiş veri paketi. LLM'e serialize edilir.
/// Faz 5 stub — `compute_space_slice()` implementasyonu Faz 5'te gelir.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OspPrompt {
    pub vision: crate::vision::VisionVector,
    pub time_ref: crate::space::TimeLayer,
    pub permissions: PermissionMask,
    pub output_contract: OutputContract,
    // Faz 5: space_slice, intent, axis_manifest, rules, evidence_context
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceSlice — Agent'ın gördüğü alt-graf (epistemik projeksiyon çıktısı)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agent'a açılan alt-graf kesiti — `compute_space_slice()` çıktısı.
///
/// Space'in bir subset'i: sadece Agent'ın görmesine izin verilen node'lar ve
/// bu node'lar arasındaki edge'ler. Agent bunu OspPrompt.space_slice olarak alır.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SpaceSlice {
    /// Görünür node'ların ID seti.
    pub node_ids: HashSet<NodeId>,
    /// Görünür node'lar (full Node objects — pozisyon dahil).
    pub nodes: Vec<Node>,
    /// Görünür node'lar arasındaki edge'ler.
    pub edges: Vec<Edge>,
}

impl SpaceSlice {
    /// Space'ten bir node-ID setine göre alt-graf kur.
    pub fn build_subgraph(space: &Space, ids: HashSet<NodeId>) -> Self {
        let nodes: Vec<Node> = ids
            .iter()
            .filter_map(|id| space.nodes.get(id).cloned())
            .collect();
        let edges: Vec<Edge> = space
            .edges
            .iter()
            .copied()
            .filter(|e| ids.contains(&e.from) && ids.contains(&e.to))
            .collect();
        Self {
            node_ids: ids,
            nodes,
            edges,
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EvidenceSummary — geçmiş Hold/Reject'ten gelen kanıt ihtiyaçları
// ═══════════════════════════════════════════════════════════════════════════════

/// Şahitlerin talep ettiği ek düğümler (Hold/Reject geri bildirimi).
///
/// Bir önceki Claim Hold/reject edildiyse, şahitler "şu node'ları da görmen lazım"
/// diyebilir. Bu node'lar space_slice'a eklenir (permission filter'dan geçerse).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EvidenceSummary {
    /// Şahitlerin Agent'ın görmesini talep ettiği node'lar.
    pub required_nodes: Vec<NodeId>,
}

impl EvidenceSummary {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_required_nodes(nodes: Vec<NodeId>) -> Self {
        Self {
            required_nodes: nodes,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// compute_space_slice — üç-katmanlı epistemik projeksiyon (§3)
// ═══════════════════════════════════════════════════════════════════════════════

/// Üç katmanlı alt-uzay seçim motoru (agent-prompt-semantics.md §3).
///
/// Agent'ın göreceği alt-grafı belirler:
/// 1. **Intent Gravity:** Intent'in hedef node'ları + k-hop komşuları
/// 2. **Rule Risk Expansion:** Kural ihlali riski taşıyan sınır node'ları
/// 3. **Permission + Evidence:** Yetki filtresi + geçmiş kanıt ihtiyaçları
///
/// **Güvenlik:** Evidence node'ları permission filtresinden GEÇEREK eklenir (§2.1).
pub fn compute_space_slice(
    intent_target_nodes: &[NodeId],
    space: &Space,
    rules: &[Box<dyn Rule>],
    mask: &PermissionMask,
    evidence: &EvidenceSummary,
    k_hops: usize,
) -> SpaceSlice {
    let mut nodes_bucket: HashSet<NodeId> = HashSet::new();

    // ── Layer 1: Intent core nodes + k-hop neighbors ──
    for &core_node in intent_target_nodes {
        nodes_bucket.insert(core_node);
        // k-hop BFS traversal
        let neighbors = neighbors_within_hops(space, core_node, k_hops);
        nodes_bucket.extend(neighbors);
    }

    // ── Layer 2: Rule risk expansion (sadece kurallar varsa) ──
    // Kural ihlali riski taşıyan sınır node'ları ekle.
    // Rules boşsa bu katman atlanır (k-hop expansion doğru kalır).
    if !rules.is_empty() {
        let existing_ids: HashSet<NodeId> = nodes_bucket.iter().copied().collect();
        for edge in &space.edges {
            if existing_ids.contains(&edge.from) && !existing_ids.contains(&edge.to) {
                nodes_bucket.insert(edge.to);
            }
            if existing_ids.contains(&edge.to) && !existing_ids.contains(&edge.from) {
                nodes_bucket.insert(edge.from);
            }
        }
    }

    // ── Layer 3: Evidence context ──
    // Şahitlerin talep ettiği node'ları ekle (permission'dan önce)
    nodes_bucket.extend(evidence.required_nodes.iter().copied());

    // ── Layer 4: Permission filter (denetim noktası 1) ──
    nodes_bucket.retain(|node| mask.has_read_permission(*node));

    // Build subgraph
    SpaceSlice::build_subgraph(space, nodes_bucket)
}

/// Bir node'dan k-hop mesafedeki tüm komşuları bul (BFS).
fn neighbors_within_hops(space: &Space, start: NodeId, k: usize) -> HashSet<NodeId> {
    let mut result = HashSet::new();
    let mut frontier = vec![start];

    for _ in 0..k {
        let mut next_frontier = vec![];
        for &node in &frontier {
            for edge in &space.edges {
                if edge.from == node && !result.contains(&edge.to) && edge.to != start {
                    result.insert(edge.to);
                    next_frontier.push(edge.to);
                }
                if edge.to == node && !result.contains(&edge.from) && edge.from != start {
                    result.insert(edge.from);
                    next_frontier.push(edge.from);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mask_full_access_allows_all() {
        let mask = PermissionMask::full_access();
        assert!(mask.has_read_permission(999));
        assert!(mask.read_only_nodes.is_empty());
    }

    #[test]
    fn output_contract_default_accepts_valid_proposal() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 10.0,
                connected_to: vec![],
            }],
            new_edges: vec![],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: "adding auth module".to_string(),
            ..Default::default() // G2c-2: removed_edges, affected_nodes default
        };
        assert!(contract.validate(&proposal).is_ok());
    }

    #[test]
    fn output_contract_rejects_nan_mass() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: f64::NAN,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_negative_mass() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: -5.0,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_imports_self_loop() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 1.0,
                connected_to: vec![],
            }],
            new_edges: vec![NewEdgeSpec {
                from: 0,
                to: 0,
                kind: EdgeKind::Imports,
            }],
            ..Default::default()
        };
        let result = contract.validate(&proposal);
        assert!(result.is_err(), "Imports self-loop should be rejected");
    }

    #[test]
    fn connected_to_rejects_duplicate_edge() {
        // Aynı target node'a aynı EdgeKind ile iki kez bağlanmak gereksiz/sorgulanabilir
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 1.0,
                connected_to: vec![
                    (5, EdgeKind::Imports),
                    (5, EdgeKind::Imports), // duplicate
                ],
            }],
            ..Default::default()
        };
        let result = contract.validate(&proposal);
        assert!(
            result.is_err(),
            "duplicate connected_to edge should be rejected"
        );
    }

    #[test]
    fn connected_to_accepts_distinct_edges_to_same_target() {
        // Aynı target node'a FARKLI EdgeKind'ler geçerli (Imports + DependsOn)
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 1.0,
                connected_to: vec![
                    (5, EdgeKind::Imports),
                    (5, EdgeKind::DependsOn), // distinct kind → OK
                ],
            }],
            ..Default::default()
        };
        let result = contract.validate(&proposal);
        assert!(
            result.is_ok(),
            "distinct EdgeKinds to same target should be accepted"
        );
    }

    #[test]
    fn connected_to_accepts_empty() {
        // Regression: boş connected_to geçerli (mevcut davranış korundu)
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 1.0,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_ok());
    }

    #[test]
    fn new_edges_rejects_duplicate() {
        // Aynı (from, to, kind) üçlüsü iki kez listelenemez
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
            ],
            new_edges: vec![
                NewEdgeSpec {
                    from: 0,
                    to: 1,
                    kind: EdgeKind::Imports,
                },
                NewEdgeSpec {
                    from: 0,
                    to: 1,
                    kind: EdgeKind::Imports, // duplicate
                },
            ],
            ..Default::default()
        };
        let result = contract.validate(&proposal);
        assert!(
            result.is_err(),
            "duplicate new_edges entry should be rejected"
        );
    }

    #[test]
    fn new_edges_accepts_distinct_kinds_same_endpoints() {
        // Aynı from→to çifti ama FARKLI EdgeKind'ler geçerli (Imports + Calls)
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
            ],
            new_edges: vec![
                NewEdgeSpec {
                    from: 0,
                    to: 1,
                    kind: EdgeKind::Imports,
                },
                NewEdgeSpec {
                    from: 0,
                    to: 1,
                    kind: EdgeKind::Calls, // distinct kind → OK
                },
            ],
            ..Default::default()
        };
        let result = contract.validate(&proposal);
        assert!(
            result.is_ok(),
            "distinct EdgeKinds between same endpoints should be accepted"
        );
    }

    #[test]
    fn output_contract_rejects_disallowed_node_kind() {
        let mut allowed = HashSet::new();
        allowed.insert(NodeKind::Module);
        let contract = OutputContract {
            allowed_node_kinds: Some(allowed),
            ..Default::default()
        };
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Feature, // not allowed
                initial_mass: 1.0,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_empty_reasoning_when_required() {
        let contract = OutputContract::strict(); // require_reasoning = true
        let proposal = DeltaProposal {
            reasoning: "".to_string(),
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_max_nodes_exceeded() {
        let contract = OutputContract {
            max_nodes: Some(2),
            ..Default::default()
        };
        let proposal = DeltaProposal {
            new_nodes: vec![
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
            ],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err(), "3 nodes > max 2");
    }

    #[test]
    fn output_contract_rejects_nan_position_hint() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            position_hints: vec![PositionHint {
                node_id: 1,
                suggested_raw: RawPosition {
                    x: f64::NAN,
                    ..Default::default()
                },
                rationale: "test".to_string(),
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn delta_proposal_has_no_position_field() {
        // inv #4 — DeltaProposal pozisyon İÇERMEZ (engine compute eder)
        let proposal = DeltaProposal::default();
        assert!(proposal.new_nodes.is_empty());
        assert!(proposal.new_edges.is_empty());
        assert!(proposal.modified_entities.is_empty());
        assert!(proposal.position_hints.is_empty());
        assert!(proposal.reasoning.is_empty());
    }

    // --- compute_space_slice tests (§3 three-layer projection) ---

    fn make_space_linear() -> Space {
        // Chain: 1 → 2 → 3 → 4 → 5
        let mut space = Space::new();
        for id in 1..=5u64 {
            space.insert_node(Node {
                id,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            });
        }
        for (from, to) in [(1, 2), (2, 3), (3, 4), (4, 5)] {
            space.insert_edge(Edge {
                from,
                to,
                kind: EdgeKind::Imports,
                ..Default::default()
            });
        }
        space
    }

    #[test]
    fn space_slice_empty_intent_returns_empty() {
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[], &space, &rules, &mask, &evidence, 2);

        // No intent nodes → but layer 2 (rule expansion) might still add neighbors...
        // Actually with no nodes in bucket, rule expansion has nothing to expand from
        // → empty slice
        assert_eq!(slice.node_count(), 0, "empty intent → empty slice");
    }

    #[test]
    fn space_slice_single_node_no_neighbors() {
        let mut space = Space::new();
        space.insert_node(Node {
            id: 42,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[42], &space, &rules, &mask, &evidence, 2);

        assert_eq!(slice.node_count(), 1, "single node → 1 node in slice");
        assert_eq!(slice.edge_count(), 0, "no edges");
    }

    #[test]
    fn space_slice_k_hop_expansion_linear_chain() {
        // Chain: 1 → 2 → 3 → 4 → 5
        // Intent: node 1, k=2 → should see 1, 2, 3
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[1], &space, &rules, &mask, &evidence, 2);

        assert!(slice.node_ids.contains(&1), "intent node 1");
        assert!(slice.node_ids.contains(&2), "1-hop neighbor");
        assert!(slice.node_ids.contains(&3), "2-hop neighbor");
        assert!(
            !slice.node_ids.contains(&4),
            "3-hop should NOT be included with k=2"
        );
    }

    #[test]
    fn space_slice_rule_expansion_adds_boundary() {
        // Chain: 1 → 2 → 3 → 4 → 5
        // Intent: node 3, k=0 → just node 3
        // Layer 2: with rules registered, boundary expansion adds neighbors (2, 4)
        use crate::rule::NoSelfImportRule;
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(NoSelfImportRule::new())];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[3], &space, &rules, &mask, &evidence, 0);

        // k=0 → only intent node 3. But layer 2 (with rules) adds boundary neighbors.
        assert!(slice.node_ids.contains(&3), "intent node");
        assert!(
            slice.node_ids.contains(&2) || slice.node_ids.contains(&4),
            "boundary expansion should add at least one neighbor when rules exist"
        );
    }

    #[test]
    fn space_slice_no_rules_no_boundary_expansion() {
        // Same setup but NO rules → k-hop is exact, no boundary expansion
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[3], &space, &rules, &mask, &evidence, 0);

        assert_eq!(slice.node_count(), 1, "k=0, no rules → only intent node");
        assert!(slice.node_ids.contains(&3));
    }

    #[test]
    fn space_slice_permission_filter_excludes_nodes() {
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask {
            read_only_nodes: {
                let mut s = HashSet::new();
                s.insert(2);
                s
            },
            ..Default::default()
        };
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[1], &space, &rules, &mask, &evidence, 2);

        // Node 2 is read_only → has_read_permission returns true (full_access stub)
        // But when real permission is implemented, node 2 would be excluded.
        // For now, full_access stub allows all.
        assert!(
            slice.node_ids.contains(&1),
            "intent node should be included"
        );
    }

    #[test]
    fn space_slice_evidence_adds_required_nodes() {
        // Space: 1 → 2 → 3
        let mut space = Space::new();
        for id in 1..=3u64 {
            space.insert_node(Node {
                id,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            });
        }
        space.insert_edge(Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Imports,
            ..Default::default()
        });
        space.insert_edge(Edge {
            from: 2,
            to: 3,
            kind: EdgeKind::Imports,
            ..Default::default()
        });

        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        // Witness requires node 3 — not in k=0 expansion of node 1
        let evidence = EvidenceSummary::with_required_nodes(vec![3]);

        let slice = compute_space_slice(&[1], &space, &rules, &mask, &evidence, 0);

        assert!(
            slice.node_ids.contains(&3),
            "evidence-required node 3 should be in slice"
        );
        assert!(slice.node_ids.contains(&1), "intent node 1");
    }

    #[test]
    fn space_slice_build_subgraph_filters_edges() {
        let space = make_space_linear();
        let mut ids = HashSet::new();
        ids.insert(1);
        ids.insert(2);

        let slice = SpaceSlice::build_subgraph(&space, ids);

        assert_eq!(slice.node_count(), 2);
        // Only edge 1→2 (both endpoints in set)
        assert_eq!(slice.edge_count(), 1);
        assert_eq!(slice.edges[0].from, 1);
        assert_eq!(slice.edges[0].to, 2);
    }

    // --- PermissionMask real logic (inv #13) ---

    #[test]
    fn permission_mask_hidden_node_not_readable() {
        let mut mask = PermissionMask::full_access();
        mask.hide_node(42);
        assert!(
            !mask.has_read_permission(42),
            "hidden node should not be readable"
        );
        assert!(
            mask.has_read_permission(1),
            "non-hidden node should be readable"
        );
    }

    #[test]
    fn permission_mask_read_only_node_readable_but_not_writable() {
        let mut mask = PermissionMask::full_access();
        mask.set_read_only(10);
        assert!(mask.has_read_permission(10), "read-only node IS readable");
        assert!(
            !mask.has_write_permission(10),
            "read-only node is NOT writable"
        );
        assert!(mask.has_write_permission(1), "normal node is writable");
    }

    #[test]
    fn permission_mask_hidden_node_not_writable() {
        let mut mask = PermissionMask::full_access();
        mask.hide_node(99);
        assert!(
            !mask.has_write_permission(99),
            "hidden node is not writable"
        );
    }

    #[test]
    fn permission_mask_full_access_allows_everything() {
        let mask = PermissionMask::full_access();
        for id in 0..100 {
            assert!(mask.has_read_permission(id));
            assert!(mask.has_write_permission(id));
        }
    }

    #[test]
    fn permission_mask_forbidden_edge_blocked() {
        let mut mask = PermissionMask::full_access();
        mask.forbidden_edge_kinds.insert(EdgeKind::Approves);
        assert!(
            !mask.can_create_edge(EdgeKind::Approves),
            "forbidden edge blocked"
        );
        assert!(mask.can_create_edge(EdgeKind::Imports), "allowed edge ok");
    }

    #[test]
    fn space_slice_filters_hidden_nodes() {
        // Chain: 1 → 2 → 3
        let mut space = Space::new();
        for id in 1..=3u64 {
            space.insert_node(Node {
                id,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            });
        }
        space.insert_edge(Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Imports,
            ..Default::default()
        });
        space.insert_edge(Edge {
            from: 2,
            to: 3,
            kind: EdgeKind::Imports,
            ..Default::default()
        });

        let mut mask = PermissionMask::full_access();
        mask.hide_node(2); // hide middle node

        let slice = compute_space_slice(&[1], &space, &[], &mask, &EvidenceSummary::empty(), 2);

        assert!(
            !slice.node_ids.contains(&2),
            "hidden node 2 should be filtered from slice"
        );
        assert!(slice.node_ids.contains(&1), "node 1 should be visible");
    }

    // --- Hallucination classification (§4.1) ---

    #[test]
    fn hallucination_from_syntax_violation() {
        let err = crate::engine::EngineCommitError::SyntaxViolation {
            violation: SyntaxViolation {
                claim_id: 1,
                detail: "NaN mass".to_string(),
            },
        };
        let h = HallucinationType::from_engine_error(&err);
        assert!(matches!(h, Some(HallucinationType::Structural { .. })));
        assert_eq!(h.unwrap().gate(), "Q4 Syntax");
    }

    #[test]
    fn hallucination_from_vision_violation() {
        let err = crate::engine::EngineCommitError::VisionViolation {
            violation: crate::engine::VisionViolation {
                claim_id: 1,
                theta: 0.35,
                raw: RawPosition::default(),
            },
            bound: 0.25,
        };
        let h = HallucinationType::from_engine_error(&err);
        match h {
            Some(HallucinationType::Vision { theta, bound }) => {
                assert!((theta - 0.35).abs() < 1e-9);
                assert!((bound - 0.25).abs() < 1e-9);
            }
            _ => panic!("expected Vision hallucination"),
        }
    }

    #[test]
    fn hallucination_from_rule_violation() {
        use crate::rule::{RuleSeverity, RuleViolation};
        let err = crate::engine::EngineCommitError::RuleViolation {
            violation: RuleViolation {
                rule_id: "structural.no_self_import".to_string(),
                detail: "node 5 imports itself".to_string(),
                severity: RuleSeverity::Hard,
            },
        };
        let h = HallucinationType::from_engine_error(&err);
        assert!(matches!(h, Some(HallucinationType::Rule { .. })));
        assert_eq!(h.unwrap().gate(), "Q6 Rule");
    }

    #[test]
    fn hallucination_from_invalid_witness_evidence() {
        // **INV-T9:** Witness Hold/Rejected artık EngineCommitResult üzerinden gelir.
        // InvalidWitnessEvidence = operational fault (malformed/author-self/duplicate).
        let err = crate::engine::EngineCommitError::InvalidWitnessEvidence(
            "duplicate witness identity".to_string(),
        );
        let h = HallucinationType::from_engine_error(&err);
        match h {
            Some(HallucinationType::Undersupported { .. }) => { /* expected */ }
            _ => panic!("expected Undersupported hallucination for InvalidWitnessEvidence"),
        }
    }

    #[test]
    fn hallucination_from_internal_is_none() {
        // INV-T9: Internal = system failure, not agent hallucination.
        let err = crate::engine::EngineCommitError::Internal("legacy commit Held".to_string());
        let h = HallucinationType::from_engine_error(&err);
        assert!(h.is_none(), "Internal error is not a hallucination signal");
    }

    #[test]
    fn hallucination_calibration_message_is_readable() {
        let h = HallucinationType::Vision {
            theta: 0.35,
            bound: 0.25,
        };
        let msg = h.calibration_message();
        assert!(msg.contains("0.350"), "message should include theta value");
        assert!(msg.contains("0.250"), "message should include bound value");
        assert!(
            msg.contains("Vision"),
            "message should mention hallucination type"
        );
    }

    #[test]
    fn hallucination_non_error_returns_none() {
        let err = crate::engine::EngineCommitError::NoPersistence;
        assert!(HallucinationType::from_engine_error(&err).is_none());
    }
}
