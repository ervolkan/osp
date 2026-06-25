//! # OSP Analyzer
//!
//! Production code→space mapper. Two-tier analysis:
//! - **Tier 1** (always): tree-sitter syntactic (imports, class defs, abstractness)
//! - **Tier 2** (optional): SCIP semantic (LCOM4 cohesion, field access)
//!
//! **Faz 3.1:** Contract tipleri — `MetricValue`, `AnalysisResult`, `SemanticCoverage`,
//! `AnalysisConfig`, `LanguageAdapter` trait. Adapter implementasyonları Faz 3.2+.

pub mod abstractness;
pub mod adapters;
pub mod contract;
pub mod language;
pub mod pipeline;
pub mod scip;

pub use contract::AnalysisResult;
pub use pipeline::analyze_repo;

// Faz 3.2+: pub mod adapters;
// Faz 3.3+: pub mod scip;
// Faz 3.5+: pub mod pipeline;
// Faz 3.9:  pub mod scale;
