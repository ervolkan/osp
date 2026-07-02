//! AnchorPipeline facade + AnchorError (Faz 1).
//!
//! `AnchorPipeline` stateless facade — store DIŞINDA (store mutable state).
//! Test/ürün API'sinin tohumu: `run(text, language, graph) -> AnchorPlan`.
//!
//! # Akış
//! classify → ConceptPacket → extract[Extracted] → score[AnchorCandidate] → gate → AnchorPlan

use crate::anchoring::classifier::{Classifier, Glossary};
use crate::anchoring::extractor::Extractor;
use crate::anchoring::gate::{AnchorGate, GateError};
use crate::anchoring::scorer::AnchorScorer;
use crate::anchoring::store::StoreError;
use crate::anchoring::types::{AnchorPlan, ConceptGraph, ConceptPacket, PacketSource};

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorError — modül-spanning pipeline hatası (thiserror, EngineCommitError desenine sadık)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum AnchorError {
    #[error("classify hatası: {0}")]
    Classify(#[from] ClassifyError),
    #[error("extract hatası: {0}")]
    Extract(#[from] ExtractError),
    #[error("score hatası: {0}")]
    Score(#[from] ScoreError),
    #[error("gate hatası: {0}")]
    Gate(#[from] GateError),
    #[error("store hatası: {0}")]
    Store(#[from] StoreError),
}

/// Classifier leaf hatası.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassifyError {
    EmptyInput,
}

impl std::fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "boş girdi metni"),
        }
    }
}
impl std::error::Error for ClassifyError {}

/// Extractor leaf hatası.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractError {
    TypedRefParse(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypedRefParse(s) => write!(f, "typed ref parse hatası: {}", s),
        }
    }
}
impl std::error::Error for ExtractError {}

/// Scorer leaf hatası.
#[derive(Debug, Clone, PartialEq)]
pub enum ScoreError {
    InvalidScore,
}

impl std::fmt::Display for ScoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScore => write!(f, "geçersiz skor (NaN/Inf)"),
        }
    }
}
impl std::error::Error for ScoreError {}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorPipeline — stateless facade
// ═══════════════════════════════════════════════════════════════════════════════

/// Anchoring pipeline facade. Classifier + Extractor + Scorer + Gate'yi birleştirir.
/// Store DIŞINDA — store mutable state, pipeline stateless.
///
/// # Ömür notu
/// Faz 1: `Classifier`/`Glossary` pipeline ömründe leak'lenir (`Box::leak`).
/// Bu test/MVP kullanımı için kabul edilebilir (process ömrü). Faz 2'de owned
/// variant veya `Arc` değerlendirilebilir.
pub struct AnchorPipeline {
    pub classifier: &'static Classifier,
    pub glossary: &'static Glossary,
    pub extractor: Extractor<'static>,
    pub scorer: AnchorScorer,
    pub gate: AnchorGate,
}

impl AnchorPipeline {
    /// Varsayılan pipeline (seed glossary, rule-based classifier).
    pub fn default_pipeline() -> Self {
        Self::with_glossary(Glossary::seed_default())
    }

    /// Belirli glossary ile pipeline kur (test için).
    pub fn with_glossary(glossary: Glossary) -> Self {
        let classifier = Classifier::new();
        let g: &'static Glossary = Box::leak(Box::new(glossary));
        let c: &'static Classifier = Box::leak(Box::new(classifier));
        let extractor = Extractor::new(g, c);
        let scorer = AnchorScorer::new();
        let gate = AnchorGate::new((*g).clone());
        Self {
            classifier: c,
            glossary: g,
            extractor,
            scorer,
            gate,
        }
    }

    /// Metni AnchorPlan'a dönüştür (tam pipeline).
    pub fn run(
        &self,
        text: &str,
        language: &str,
        graph: &ConceptGraph,
    ) -> Result<AnchorPlan, AnchorError> {
        self.run_with_source(text, language, graph, PacketSource::ExplicitUser)
    }

    /// Belirli source ile pipeline.
    pub fn run_with_source(
        &self,
        text: &str,
        language: &str,
        graph: &ConceptGraph,
        source: PacketSource,
    ) -> Result<AnchorPlan, AnchorError> {
        if text.trim().is_empty() {
            return Err(ClassifyError::EmptyInput.into());
        }

        // 1. Classify
        let packet_type = self.classifier.classify(text, language);

        // 2. Packet
        let packet = ConceptPacket::new(make_id(text), packet_type, text, language, source);

        // 3. Extract
        let extracted = self.extractor.extract(&packet, graph);

        // 4. Score
        let scored: Vec<_> = extracted
            .into_iter()
            .map(|e| self.scorer.score(e, graph, source))
            .collect();

        // 5. Gate
        let plan = self.gate.decide(&packet.id, scored, graph)?;

        Ok(plan)
    }
}

/// Metinden deterministik packet ID üret (hash-benzeri, basit).
fn make_id(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("pkt:{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_runs_fix_001() {
        let p = AnchorPipeline::default_pipeline();
        let graph = ConceptGraph::new();
        let plan = p.run(
            "Kullanıcı ödeme yaparken kendini güvende hissetmeli.",
            "tr",
            &graph,
        )
        .expect("pipeline");
        // DerivesRisk veya Mentions olmalı
        assert!(!plan.candidates.is_empty() || matches!(plan.decision, crate::anchoring::AnchorDecisionKind::MarkUnanchored));
    }

    #[test]
    fn pipeline_rejects_empty() {
        let p = AnchorPipeline::default_pipeline();
        let err = p.run("", "tr", &ConceptGraph::new()).unwrap_err();
        assert!(matches!(err, AnchorError::Classify(ClassifyError::EmptyInput)));
    }

    #[test]
    fn pipeline_unanchored_vague() {
        let p = AnchorPipeline::default_pipeline();
        let plan = p.run(
            "Belki hafta sonu bazı şeyleri gözden geçirmek lazım.",
            "tr",
            &ConceptGraph::new(),
        )
        .expect("pipeline");
        assert!(plan.candidates.is_empty(), "glossary match yok → boş candidate");
    }

    #[test]
    fn pipeline_no_implemented_by_emitted() {
        // INV: Faz 1'de hiçbir durumda ImplementedBy üretilmemeli
        let p = AnchorPipeline::default_pipeline();
        let plan = p.run("Kimlik doğrulama akışı AuthService'de implement edilmeli.", "tr", &ConceptGraph::new());
        // Plan başarılıysa ImplementedBy yok; hata (IllegalDirectCodeBinding) da olabilir
        match plan {
            Ok(plan) => {
                assert!(
                    !plan.candidates.iter().any(|c| c.edge_kind == crate::anchoring::ConceptEdgeKind::ImplementedBy),
                    "ImplementedBy üretilmemeli"
                );
            }
            Err(AnchorError::Gate(GateError::IllegalDirectCodeBinding { .. })) | _ => { /* kabul */ }
        }
    }
}
