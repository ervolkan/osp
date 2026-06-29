//! Analysis contract tipleri — Faz 3.1.
//!
//! Tüm adapter/SCIP/LCOM4 işinin yazılacağı temel. `MetricValue` provenance,
//! `AnalysisResult` output, `AnalysisConfig` config + `SemanticCoverage` quality.
//!
//! **MetricValue canonical kaynak:** `osp_core::coords` (Faz 3.1.1 migration).
//! Bu modül re-export eder — duplicate tanım yok (drift risk eliminated).

use std::collections::HashMap;
use std::path::PathBuf;

use osp_core::space::{NodeId, Space};

// Re-export: MetricValue/MetricSource/MetricValueError canonical kaynak osp_core::coords.
// Downstream kod `crate::contract::MetricValue` path'inde çalışmaya devam eder (backward compat).
pub use osp_core::coords::{MetricSource, MetricValue, MetricValueError};

// ═══════════════════════════════════════════════════════════════════════════════
// SemanticCoverage — SCIP index quality
// ═══════════════════════════════════════════════════════════════════════════════

/// SCIP index kalitesi — partial/stale tespiti.
#[derive(Debug, Clone)]
pub struct SemanticCoverage {
    pub files_total: usize,
    pub files_with_scip: usize,
    pub classes_total: usize,
    pub classes_with_field_access: usize,
    /// `files_with_scip / files_total` ∈ [0,1].
    pub coverage_ratio: f64,
    /// SCIP index'in üretildiği commit hash.
    pub index_commit: Option<String>,
    /// Repo'nun güncel HEAD commit hash.
    pub repo_head: String,
    /// `index_commit ≠ repo_head` → stale.
    pub stale: bool,
}

impl SemanticCoverage {
    /// SCIP yok — coverage=0, stale=false. `repo_head` zorunlu parametre.
    pub fn none(repo_head: String) -> Self {
        Self {
            files_total: 0,
            files_with_scip: 0,
            classes_total: 0,
            classes_with_field_access: 0,
            coverage_ratio: 0.0,
            index_commit: None,
            repo_head,
            stale: false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Diagnostics
// ═══════════════════════════════════════════════════════════════════════════════

/// Analysis diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// Structured diagnostic code (raporlama + test için).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    UnknownImport,
    ScipIndexStale,
    ParseFailed,
    PlaceholderMetric,
    GeneratedExcluded,
    CoverageLow,
}

/// Tek diagnostic mesajı.
#[derive(Debug, Clone)]
pub struct AnalysisDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub message: String,
    pub file: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ModuleMetrics + RepoMetrics
// ═══════════════════════════════════════════════════════════════════════════════

/// Per-module (dosya) metrik paketi.
#[derive(Debug, Clone)]
pub struct ModuleMetrics {
    pub coupling: MetricValue,     // x
    pub cohesion: MetricValue,     // y (SCIP ise gerçek LCOM4; yoksa Placeholder)
    pub instability: MetricValue,  // z (Martin I saf)
}

/// Repo-level metrik paketi.
#[derive(Debug, Clone)]
pub struct RepoMetrics {
    pub abstractness: MetricValue,              // A — Tier 1 keyword check
    pub main_sequence_distance: MetricValue,    // D = |A + I − 1|
    /// Package-level breakdown (opsiyonel — rapor için).
    pub abstractness_by_package: Option<HashMap<String, f64>>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnalysisResult — output contract
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir node'a (source file) ait SCIP semantic özeti.
///
/// Inspector'ın "class-level ownership" görünürlüğü için: bu dosyada kaç class
/// var, kaç method/field içeriyorlar. SCIP yoksa tüm alanlar 0; bu durumda
/// inspector "no SCIP data for this file" gösterir.
#[derive(Debug, Clone, Default)]
pub struct NodeSemanticSummary {
    /// Bu dosyada tanımlı class sayısı.
    pub class_count: usize,
    /// Tüm class'lardaki toplam method sayısı.
    pub method_count: usize,
    /// Tüm class'lardaki toplam field sayısı.
    pub field_count: usize,
    /// En yüksek LCOM4 component sayısı (en düşük cohesion'lı class).
    pub max_lcom4: u32,
}

/// Full analysis pipeline çıktısı. Analyzer → Engine arayüzü.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Graph + positions (osp-core Space).
    pub space: Space,
    /// Per-module metrik paketleri.
    pub module_metrics: HashMap<NodeId, ModuleMetrics>,
    /// Node ID → source file relative path eşlemesi (Inspector için).
    pub node_paths: HashMap<NodeId, String>,
    /// Node ID → SCIP semantic özeti (class/method/field sayıları).
    /// SCIP yoksa boş → inspector "no SCIP data" gösterir.
    pub node_semantics: HashMap<NodeId, NodeSemanticSummary>,
    /// Repo-level metrikler (A, D).
    pub repo_metrics: RepoMetrics,
    /// SCIP index kalitesi (coverage, stale).
    pub semantic_coverage: SemanticCoverage,
    /// Diagnostic mesajları.
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnalysisConfig + UnknownImportPolicy
// ═══════════════════════════════════════════════════════════════════════════════

/// Unknown import'lar için politika.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownImportPolicy {
    /// Edge YOK, diagnostic üret (default — coupling şişmez).
    DiagnosticOnly,
    /// Sessizce atla.
    Skip,
    /// Internal edge gibi say (riskli — coupling şişebilir).
    TreatAsInternal,
}

impl Default for UnknownImportPolicy {
    fn default() -> Self {
        Self::DiagnosticOnly
    }
}

/// Analyzer konfigürasyonu.
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// SCIP index dosyası (opsiyonel — yoksa Tier 2 skip).
    pub scip_index: Option<PathBuf>,
    /// Unknown import politikası.
    pub unknown_import_policy: UnknownImportPolicy,
    /// `generated/`, `.gen.rs` gibi dizinleri/dosyaları hariç tut.
    pub exclude_generated: bool,
    /// `vendor/`, `node_modules/` hariç tut.
    pub exclude_vendor: bool,
    /// `*_test.*`, `test_*` hariç tut (default false — test'ler mimari için faydalı).
    pub exclude_tests: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            scip_index: None,
            unknown_import_policy: UnknownImportPolicy::default(),
            exclude_generated: true,
            exclude_vendor: true,
            exclude_tests: false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Import tipleri (Language Adapter System §3.1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Import deyimi (syntactic — tree-sitter çıktısı).
#[derive(Debug, Clone, Default)]
pub struct ImportStatement {
    /// "foo.bar" (Python) / "./foo" (JS) / "crate::foo" (Rust)
    pub path: String,
    pub source_location: usize,
    /// TS `import type {Foo}` / `import {type Foo}` ile üretilen type-only import mu?
    /// `true` → runtime dependency değil, coupling/instability'den hariç (Edge::is_type_only).
    /// Sadece TypeScript adapter'ı `true` set eder; JS/Python/Rust/Go her zaman `false`
    /// (bu dillerde type-only import kavramı yok).
    pub is_type_only: bool,
}

/// Import çözümleme sonucu — internal/external/stdlib ayrımı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// Repo içindeki dosyaya → edge oluştur.
    Internal,
    /// Üçüncü-parti paket → edge YOK.
    External,
    /// Standart kütüphane → edge YOK.
    StandardLibrary,
    /// Çözümlenemedi → AnalysisConfig.unknown_import_policy'ye göre action.
    Unknown,
}

/// Çözümlenmiş import.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    pub kind: ImportKind,
    /// Internal ise çözümlenen dosya yolu.
    pub target_path: Option<PathBuf>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ClassDef — abstractness + LCOM4 için
// ═══════════════════════════════════════════════════════════════════════════════

/// Class/function tanımı (tree-sitter çıktısı).
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    /// `interface`/`abstract class`/`trait`/`Protocol` → true.
    /// Rust: `trait X` = true; `impl X for Y` = false (concrete).
    pub is_abstract: bool,
    /// Method isimleri (LCOM4 için).
    pub methods: Vec<String>,
    pub source_location: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // --- MetricValue constructors ---

    #[test]
    fn placeholder_has_zero_confidence() {
        let m = MetricValue::placeholder(0.5);
        assert_eq!(m.source, MetricSource::Placeholder);
        assert!((m.confidence - 0.0).abs() < 1e-9);
        assert!((m.coverage - 0.0).abs() < 1e-9);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn tree_sitter_confidence_scales_with_coverage() {
        let full = MetricValue::tree_sitter(0.8, 1.0);
        assert!((full.confidence - 0.75).abs() < 1e-9);
        let half = MetricValue::tree_sitter(0.8, 0.5);
        assert!((half.confidence - 0.375).abs() < 1e-9);
    }

    #[test]
    fn scip_confidence_includes_stale_penalty() {
        let fresh = MetricValue::scip(0.8, 0.9, false);
        assert!((fresh.confidence - 0.95 * 0.9).abs() < 1e-9);
        let stale = MetricValue::scip(0.8, 0.9, true);
        assert!((stale.confidence - 0.95 * 0.9 * 0.5).abs() < 1e-9);
    }

    #[test]
    fn heuristic_custom_confidence() {
        let h = MetricValue::heuristic(0.6, 0.55);
        assert_eq!(h.source, MetricSource::Heuristic);
        assert!((h.confidence - 0.55).abs() < 1e-9);
    }

    // --- MetricValue finite invariant (§12 #7) ---

    #[test]
    fn validate_rejects_nan() {
        let m = MetricValue {
            value: f64::NAN,
            source: MetricSource::TreeSitter,
            confidence: 0.5,
            coverage: 1.0,
        };
        assert!(matches!(
            m.validate(),
            Err(MetricValueError::NonFiniteValue)
        ));
    }

    #[test]
    fn validate_rejects_confidence_above_one() {
        let m = MetricValue {
            value: 0.5,
            source: MetricSource::TreeSitter,
            confidence: 1.5,
            coverage: 1.0,
        };
        assert!(matches!(
            m.validate(),
            Err(MetricValueError::ConfidenceOutOfRange(1.5))
        ));
    }

    #[test]
    fn validate_rejects_negative_coverage() {
        let m = MetricValue {
            value: 0.5,
            source: MetricSource::TreeSitter,
            confidence: 0.5,
            coverage: -0.1,
        };
        assert!(matches!(
            m.validate(),
            Err(MetricValueError::CoverageOutOfRange(_))
        ));
    }

    // --- SemanticCoverage ---

    #[test]
    fn none_has_zero_coverage_with_repo_head() {
        let cov = SemanticCoverage::none("abc123".into());
        assert_eq!(cov.files_total, 0);
        assert!((cov.coverage_ratio - 0.0).abs() < 1e-9);
        assert!(!cov.stale);
        assert_eq!(cov.repo_head, "abc123");
    }

    // --- UnknownImportPolicy default ---

    #[test]
    fn default_policy_is_diagnostic_only() {
        assert_eq!(
            UnknownImportPolicy::default(),
            UnknownImportPolicy::DiagnosticOnly
        );
    }

    // --- AnalysisConfig default ---

    #[test]
    fn default_config_excludes_generated_and_vendor() {
        let config = AnalysisConfig::default();
        assert!(config.exclude_generated);
        assert!(config.exclude_vendor);
        assert!(!config.exclude_tests); // test'ler dahil
        assert_eq!(
            config.unknown_import_policy,
            UnknownImportPolicy::DiagnosticOnly
        );
        assert!(config.scip_index.is_none());
    }

    // --- MetricSource Display ---

    #[test]
    fn metric_source_display() {
        assert_eq!(MetricSource::TreeSitter.to_string(), "tree-sitter");
        assert_eq!(MetricSource::Scip.to_string(), "scip");
        assert_eq!(MetricSource::Placeholder.to_string(), "placeholder");
        assert_eq!(MetricSource::Heuristic.to_string(), "heuristic");
    }
}
