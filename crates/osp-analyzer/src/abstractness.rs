//! Abstractness computation — Martin's `A = Na / Nc` (Faz 3.5).
//!
//! Tier 1 (tree-sitter keyword check): her dilin adapter'ından gelen
//! `ClassDef.is_abstract` alanını kullanır. SCIP gerektirmez.
//!
//! `A` gerçek değer → `D = |A + I − 1|` anlamlı olur (Faz 1-2'de placeholder 0.5 idi).

use crate::contract::{ClassDef, MetricValue};

/// Per-module abstractness hesabı.
///
/// Bir dosyadaki tüm ClassDef'lerden:
/// - `Na` = `is_abstract == true` olan tip sayısı
/// - `Nc` = toplam tip sayısı
/// - `A = Na / Nc` ∈ [0, 1]
///
/// Hiç tip yoksa → `A = 0.0` (concrete-only convention).
#[derive(Debug, Clone)]
pub struct ModuleAbstractness {
    pub abstract_count: usize,
    pub total_count: usize,
    pub ratio: f64,
}

impl ModuleAbstractness {
    pub fn from_class_defs(defs: &[ClassDef]) -> Self {
        let total = defs.len();
        let abstract_count = defs.iter().filter(|d| d.is_abstract).count();
        let ratio = if total > 0 {
            abstract_count as f64 / total as f64
        } else {
            0.0 // no types → convention: A=0 (fully concrete)
        };
        Self {
            abstract_count,
            total_count: total,
            ratio,
        }
    }
}

/// Repo-level abstractness — tüm modüllerin toplamından.
///
/// `A_repo = ΣNa / ΣNc` (naive average değil, weighted by count).
#[derive(Debug, Clone)]
pub struct RepoAbstractness {
    pub total_abstract: usize,
    pub total_concrete: usize,
    pub ratio: f64,
}

impl RepoAbstractness {
    /// Tüm modüllerin ClassDef'lerini topla → repo-level A.
    pub fn from_all_modules(modules: &[ModuleAbstractness]) -> Self {
        let total_abstract: usize = modules.iter().map(|m| m.abstract_count).sum();
        let total_count: usize = modules.iter().map(|m| m.total_count).sum();
        let ratio = if total_count > 0 {
            total_abstract as f64 / total_count as f64
        } else {
            0.5 // no types at all → neutral placeholder (reviewer fallback)
        };
        Self {
            total_abstract,
            total_concrete: total_count - total_abstract,
            ratio,
        }
    }

    /// `MetricValue` ile provenance taşı (Tier 1 tree-sitter).
    /// `coverage` = tip içeren modül oranı (`modules_with_types / total_modules`).
    pub fn to_metric_value(&self, coverage: f64) -> MetricValue {
        MetricValue::tree_sitter(self.ratio, coverage)
    }
}

/// Per-package abstractness breakdown (reviewer 1 #6 — rapor için).
pub fn abstractness_by_package(packages: &[(String, &[ClassDef])]) -> Vec<(String, f64)> {
    packages
        .iter()
        .map(|(name, defs)| {
            let abs = ModuleAbstractness::from_class_defs(defs);
            (name.clone(), abs.ratio)
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn class_def(name: &str, is_abstract: bool) -> ClassDef {
        ClassDef {
            name: name.into(),
            is_abstract,
            methods: vec![],
            source_location: 0,
        }
    }

    // --- ModuleAbstractness ---

    #[test]
    fn module_all_concrete() {
        let defs = vec![class_def("Foo", false), class_def("Bar", false)];
        let abs = ModuleAbstractness::from_class_defs(&defs);
        assert_eq!(abs.abstract_count, 0);
        assert_eq!(abs.total_count, 2);
        assert!((abs.ratio - 0.0).abs() < 1e-9);
    }

    #[test]
    fn module_all_abstract() {
        let defs = vec![class_def("IFoo", true), class_def("IBar", true)];
        let abs = ModuleAbstractness::from_class_defs(&defs);
        assert_eq!(abs.abstract_count, 2);
        assert!((abs.ratio - 1.0).abs() < 1e-9);
    }

    #[test]
    fn module_half_abstract() {
        let defs = vec![class_def("Animal", true), class_def("Dog", false)];
        let abs = ModuleAbstractness::from_class_defs(&defs);
        assert_eq!(abs.abstract_count, 1);
        assert_eq!(abs.total_count, 2);
        assert!((abs.ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn module_no_types() {
        let defs: Vec<ClassDef> = vec![];
        let abs = ModuleAbstractness::from_class_defs(&defs);
        assert_eq!(abs.total_count, 0);
        assert!((abs.ratio - 0.0).abs() < 1e-9); // convention: no types → A=0
    }

    // --- RepoAbstractness ---

    #[test]
    fn repo_weighted_by_count() {
        // Module A: 3 abstract, 1 concrete (A=0.75)
        // Module B: 0 abstract, 5 concrete (A=0.0)
        // Repo: 3/9 = 0.333 (NOT average of 0.75 and 0.0)
        let modules = vec![
            ModuleAbstractness {
                abstract_count: 3,
                total_count: 4,
                ratio: 0.75,
            },
            ModuleAbstractness {
                abstract_count: 0,
                total_count: 5,
                ratio: 0.0,
            },
        ];
        let repo = RepoAbstractness::from_all_modules(&modules);
        assert_eq!(repo.total_abstract, 3);
        assert_eq!(repo.total_concrete, 6);
        assert!(
            (repo.ratio - 3.0 / 9.0).abs() < 1e-9,
            "repo ratio = {}",
            repo.ratio
        );
    }

    #[test]
    fn repo_no_types_anywhere() {
        let modules = vec![ModuleAbstractness {
            abstract_count: 0,
            total_count: 0,
            ratio: 0.0,
        }];
        let repo = RepoAbstractness::from_all_modules(&modules);
        assert!((repo.ratio - 0.5).abs() < 1e-9, "no types → neutral 0.5");
    }

    #[test]
    fn repo_to_metric_value_carries_source() {
        let repo = RepoAbstractness {
            total_abstract: 4,
            total_concrete: 6,
            ratio: 0.4,
        };
        let mv = repo.to_metric_value(0.9);
        assert_eq!(mv.source, crate::contract::MetricSource::TreeSitter);
        assert!((mv.value - 0.4).abs() < 1e-9);
        assert!((mv.confidence - 0.75 * 0.9).abs() < 1e-9); // 0.75 * coverage
        assert!((mv.coverage - 0.9).abs() < 1e-9);
    }

    // --- Faz 0 spike repos with real-like class distributions ---

    #[test]
    fn faz0_click_like_distribution() {
        // click: mostly concrete, few ABC-based classes
        let defs = vec![
            class_def("BaseCommand", true), // ABC
            class_def("Command", false),    // concrete
            class_def("Group", false),
            class_def("Parameter", false),
            class_def("Option", false),
        ];
        let abs = ModuleAbstractness::from_class_defs(&defs);
        assert!(
            (abs.ratio - 0.2).abs() < 1e-9,
            "1/5 abstract = {}",
            abs.ratio
        );
    }

    #[test]
    fn faz0_rust_like_distribution() {
        // Typical Rust repo: traits (abstract) + structs (concrete)
        let defs = vec![
            class_def("Serializer", true),   // trait
            class_def("Deserializer", true), // trait
            class_def("Config", false),      // struct
            class_def("Engine", false),      // struct
            class_def("Error", false),       // enum (concrete)
            class_def("Result", false),      // enum
        ];
        let abs = ModuleAbstractness::from_class_defs(&defs);
        assert!((abs.ratio - 2.0 / 6.0).abs() < 1e-9, "2/6 = {}", abs.ratio);
    }

    // --- D = |A + I - 1| becomes meaningful ---

    #[test]
    fn d_meaningful_with_real_abstractness() {
        // Before Faz 3.5: A=0.5 placeholder → D = |0.5 + I - 1|
        // After Faz 3.5: A=0.33 real → D = |0.33 + I - 1| → different!

        let a_real: f64 = 0.33;
        let a_placeholder: f64 = 0.5;
        let i_module: f64 = 0.7;

        let d_real = (a_real + i_module - 1.0).abs();
        let d_placeholder = (a_placeholder + i_module - 1.0).abs();

        assert!((d_real - 0.03).abs() < 1e-9, "D real = {}", d_real); // near main-seq
        assert!(
            (d_placeholder - 0.2).abs() < 1e-9,
            "D placeholder = {}",
            d_placeholder
        );

        // Real A shows this module IS near main-sequence (good architecture)
        // Placeholder A showed it as further off (misleading)
        assert!(d_real < d_placeholder, "real D should be more accurate");
    }
}
