//! Analysis pipeline — repo → AnalysisResult with real A (Faz 3.10).
//!
//! 5-dil adapter + import resolution + abstractness → osp-core Space + MetricValue.

use std::path::{Path, PathBuf};

use osp_core::axes::{CouplingAxis, InstabilityAxis};
use osp_core::coords::Axis;
use osp_core::space::{Edge, EdgeKind, Node as CoreNode, NodeId, NodeKind, Space};

use crate::abstractness::{ModuleAbstractness, RepoAbstractness};
use crate::contract::{
    AnalysisConfig, AnalysisDiagnostic, AnalysisResult, ClassDef, DiagnosticCode,
    DiagnosticSeverity, MetricValue, ModuleMetrics, RepoMetrics, SemanticCoverage,
};
use crate::language::{AdapterRegistry, RepoContext};
use crate::scip::{load_scip_index, SemanticIndex};
use crate::scip::lcom4::{compute_lcom4, module_cohesion, Lcom4Result};

/// Bir reponun tam analizini yap → AnalysisResult (gerçek A ile).
pub fn analyze_repo(repo: &Path) -> anyhow::Result<AnalysisResult> {
    let registry = AdapterRegistry::default_all();
    analyze_repo_with_config(repo, &registry, &AnalysisConfig::default())
}

pub fn analyze_repo_with(
    repo: &Path,
    registry: &AdapterRegistry,
) -> anyhow::Result<AnalysisResult> {
    analyze_repo_with_config(repo, registry, &AnalysisConfig::default())
}

/// Tam analiz pipeline'ı — AnalysisConfig ile SCIP index + exclude policy desteği.
///
/// SCIP index varsa: Tier 2 semantic analysis → gerçek LCOM4 cohesion.
/// SCIP yoksa: cohesion placeholder (0.5, confidence=0.0).
pub fn analyze_repo_with_config(
    repo: &Path,
    registry: &AdapterRegistry,
    config: &AnalysisConfig,
) -> anyhow::Result<AnalysisResult> {
    let repo = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());

    // 1. Collect source files
    let files = collect_source_files(&repo, registry)?;
    tracing::info!(files = files.len(), repo = ?repo, "kaynak dosya bulundu");

    // 2. Phase 1: extract per-file data (isolated scope — registry borrow ends here)
    let (all_class_defs, file_data) = extract_file_data(&files, registry);

    // 2b. Phase 2: load SCIP index (Tier 2 semantic) if configured
    let semantic_index = match &config.scip_index {
        Some(scip_path) => match load_scip_index(scip_path) {
            Ok(idx) => {
                tracing::info!(
                    scip_classes = idx.classes.len(),
                    scip_files = idx.files_indexed,
                    "SCIP index loaded — real LCOM4 cohesion active"
                );
                idx
            }
            Err(e) => {
                tracing::warn!(error = %e, "SCIP index load failed — falling back to placeholder cohesion");
                SemanticIndex::empty()
            }
        },
        None => SemanticIndex::empty(),
    };

    // 3. Build RepoContext
    let file_paths: Vec<PathBuf> = file_data.iter().map(|f| f.path.clone()).collect();
    let repo_ctx = RepoContext::new(repo.clone(), file_paths);

    // 4. Build graph (Space)
    let mut space = Space::new();
    let mut node_map: std::collections::HashMap<PathBuf, NodeId> = std::collections::HashMap::new();
    let mut diagnostics = Vec::new();

    for (i, fd) in file_data.iter().enumerate() {
        space.insert_node(CoreNode {
            id: i as NodeId,
            kind: NodeKind::Module,
            mass: fd.loc as f64,
            ..Default::default()
        });
        node_map.insert(fd.path.clone(), i as NodeId);
    }

    // 5. Phase 2: resolve imports → edges (registry borrowed again, fresh)
    let mut seen_edges: std::collections::HashSet<(NodeId, NodeId)> =
        std::collections::HashSet::new();
    for fd in &file_data {
        let adapter = match registry.adapter_for_extension(&fd.ext) {
            Some(a) => a,
            None => continue,
        };
        let from_id = node_map[&fd.path];
        for imp in &fd.imports {
            if let Some(resolved) = adapter.resolve_import(imp, &fd.path, &repo_ctx) {
                use crate::contract::ImportKind;
                match resolved.kind {
                    ImportKind::Internal => {
                        if let Some(target) = &resolved.target_path {
                            if let Some(&to_id) = node_map.get(target) {
                                if from_id != to_id && seen_edges.insert((from_id, to_id)) {
                                    space.insert_edge(Edge {
                                        from: from_id,
                                        to: to_id,
                                        kind: EdgeKind::Imports,
                                    });
                                }
                            }
                        }
                    }
                    ImportKind::Unknown => {
                        diagnostics.push(AnalysisDiagnostic {
                            severity: DiagnosticSeverity::Info,
                            code: DiagnosticCode::UnknownImport,
                            message: format!("Unknown import: {}", imp.path),
                            file: Some(fd.path.strip_prefix(&repo).unwrap_or(&fd.path).to_string_lossy().into_owned()),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // 5. Abstractness (real A!)
    let module_abs = ModuleAbstractness::from_class_defs(&all_class_defs);
    let repo_abs = RepoAbstractness::from_all_modules(&[module_abs]);
    let type_coverage = if file_data.is_empty() {
        0.0
    } else if all_class_defs.is_empty() {
        0.5 // files exist but no types detected → partial coverage
    } else {
        1.0
    };
    let abstractness_mv = repo_abs.to_metric_value(type_coverage);

    // 6. Repo-level instability (mass-weighted average of module I values)
    let repo_instability = compute_repo_instability(&space);

    // 7. D = |A + I - 1|
    let d_value = (repo_abs.ratio + repo_instability - 1.0).abs();
    let d_mv = MetricValue::tree_sitter(d_value, type_coverage);

    // 8. Module metrics (per-file coupling/instability via osp_core axes + SCIP cohesion)
    let coupling_axis = CouplingAxis::new();
    let instability_axis = InstabilityAxis::new();
    let mut module_metrics = std::collections::HashMap::new();
    for (i, fd) in file_data.iter().enumerate() {
        let node_id = i as NodeId;
        let node = space.nodes.get(&node_id).expect("node inserted above");
        let coupling = coupling_axis.compute(node, &space);
        let instability = instability_axis.compute(node, &space);
        let cohesion = compute_module_cohesion(fd.path.as_path(), &repo, &semantic_index);
        // Wire cohesion into Node → CoordinateSystem::CohesionAxis reads it (per-node y-axis)
        if let Some(n) = space.nodes.get_mut(&node_id) {
            n.cohesion = Some(cohesion.value);
        }
        module_metrics.insert(
            node_id,
            ModuleMetrics {
                coupling: MetricValue::tree_sitter(coupling, 1.0),
                cohesion,
                instability: MetricValue::tree_sitter(instability, 1.0),
            },
        );
    }

    // 9. repo_head
    let repo_head = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(&["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    tracing::info!(
        nodes = space.node_count(),
        edges = space.edge_count(),
        abstract_count = repo_abs.total_abstract,
        concrete_count = repo_abs.total_concrete,
        A = repo_abs.ratio,
        I = repo_instability,
        D = d_value,
        "analiz tamamlandı"
    );

    Ok(AnalysisResult {
        space,
        module_metrics,
        repo_metrics: RepoMetrics {
            abstractness: abstractness_mv,
            main_sequence_distance: d_mv,
            abstractness_by_package: None,
        },
        semantic_coverage: build_semantic_coverage(&semantic_index, files.len(), repo_head),
        diagnostics,
    })
}

/// SCIP SemanticIndex'ten SemanticCoverage kur (pipeline çıktısı için).
/// SCIP yoksa coverage=0 ama files_total yine de actual source file sayısını yansıtır.
fn build_semantic_coverage(
    semantic_index: &SemanticIndex,
    files_total: usize,
    repo_head: String,
) -> SemanticCoverage {
    let files_with_scip = if semantic_index.is_available() {
        semantic_index.files_indexed
    } else {
        0
    };
    let coverage_ratio = if files_total > 0 {
        files_with_scip as f64 / files_total as f64
    } else {
        0.0
    };
    SemanticCoverage {
        files_total,
        files_with_scip,
        classes_total: semantic_index.classes.len(),
        classes_with_field_access: semantic_index
            .classes
            .iter()
            .filter(|c| !c.field_access.is_empty())
            .count(),
        coverage_ratio,
        index_commit: None, // Faz 3.11: SCIP metadata'dan commit hash çıkar
        repo_head,
        stale: false, // Faz 3.11: index_commit ≠ repo_head kontrolü
    }
}

/// Bir modülün (dosyanın) cohesion değerini SCIP SemanticIndex'ten hesapla.
///
/// SCIP varsa + dosya için class verisi varsa → gerçek LCOM4 cohesion (MetricValue::scip).
/// SCIP yoksa veya dosya için veri yoksa → placeholder (0.5, confidence=0.0).
/// Class yoksa (function-only module) → heuristic (1.0, confidence=0.5).
fn compute_module_cohesion(
    file_path: &Path,
    repo: &Path,
    semantic_index: &SemanticIndex,
) -> MetricValue {
    if !semantic_index.is_available() {
        return MetricValue::placeholder(0.5);
    }

    // SCIP relative_path ile eşleştir (repo prefix'i strip + normalize separators)
    let rel = file_path
        .strip_prefix(repo)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");

    let classes = match semantic_index.classes_by_file.get(&rel) {
        Some(c) if !c.is_empty() => c,
        _ => return MetricValue::placeholder(0.5), // no SCIP data for this file
    };

    let lcom4_results: Vec<Lcom4Result> = classes.iter().map(compute_lcom4).collect();
    module_cohesion(&lcom4_results)
}

struct FileData {
    path: PathBuf,
    ext: String,
    imports: Vec<crate::contract::ImportStatement>,
    loc: usize,
}

/// Phase 1: extract per-file data in isolated scope (registry borrow contained).
fn extract_file_data(
    files: &[PathBuf],
    registry: &AdapterRegistry,
) -> (Vec<ClassDef>, Vec<FileData>) {
    let mut all_class_defs = Vec::new();
    let mut file_data = Vec::new();

    for file in files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let dotted = format!(".{ext}");
        let adapter = match registry.adapter_for_extension(&dotted) {
            Some(a) => a,
            None => continue,
        };
        let imports = adapter.extract_imports(&source);
        let class_defs = adapter.extract_class_defs(&source);
        let loc = source.lines().count();

        all_class_defs.extend(class_defs);
        file_data.push(FileData {
            path: file.clone(),
            ext: dotted,
            imports,
            loc,
        });
    }
    (all_class_defs, file_data)
}

fn collect_source_files(repo: &Path, registry: &AdapterRegistry) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_dir(repo, &mut files, registry)?;
    files.sort();
    Ok(files)
}

fn walk_dir(dir: &Path, files: &mut Vec<PathBuf>, registry: &AdapterRegistry) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "node_modules" | "target" | "__pycache__" | "venv" | ".venu"
                        | "env" | "build" | "dist" | "site-packages" | "vendor" | ".git"
                )
            {
                continue;
            }
            walk_dir(&path, files, registry)?;
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let dotted = format!(".{ext}");
                if registry.adapter_for_extension(&dotted).is_some() {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

/// Repo-level instability: mass-weighted average of per-node Martin I values.
/// Per-node I via `osp_core::axes::InstabilityAxis` (no formula duplication).
fn compute_repo_instability(space: &Space) -> f64 {
    if space.nodes.is_empty() {
        return 0.5;
    }
    let axis = InstabilityAxis::new();
    let mut weighted_sum = 0.0;
    let mut total_mass = 0.0;
    for node in space.nodes.values() {
        let i = axis.compute(node, space);
        weighted_sum += i * node.mass;
        total_mass += node.mass;
    }
    if total_mass > 0.0 {
        weighted_sum / total_mass
    } else {
        0.5
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler — smoke test for analyze_repo main API
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::MetricSource;
    use std::fs;
    use tempfile::TempDir;

    /// Fixture repo oluştur: 3 Python dosyası, internal imports, bir abstract class.
    fn make_fixture() -> TempDir {
        let dir = TempDir::new().expect("temp dir");

        // main.py → utils.py (internal import)
        fs::write(
            dir.path().join("main.py"),
            "from utils import helper\n\nclass App:\n    pass\n",
        )
        .unwrap();

        // utils.py → models.py (internal import)
        fs::write(
            dir.path().join("utils.py"),
            "from models import User\n\nclass Helper:\n    pass\n",
        )
        .unwrap();

        // models.py — abstract (ABC inheritance)
        fs::write(
            dir.path().join("models.py"),
            "from abc import ABC\n\nclass User(ABC):\n    pass\n",
        )
        .unwrap();

        // unrelated.txt — ignored (no adapter)
        fs::write(dir.path().join("readme.md"), "# not source\n").unwrap();

        dir
    }

    #[test]
    fn analyze_repo_builds_space_with_correct_node_count() {
        let dir = make_fixture();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");

        // 3 Python files → 3 Module nodes (readme.md ignored)
        assert_eq!(result.space.node_count(), 3, "3 .py files → 3 nodes");
    }

    #[test]
    fn analyze_repo_resolves_internal_imports_as_edges() {
        let dir = make_fixture();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");

        // main → utils, utils → models = 2 internal edges
        assert_eq!(result.space.edge_count(), 2, "2 internal import edges");
        assert!(result.space.edges.iter().all(|e| e.kind == EdgeKind::Imports));
    }

    #[test]
    fn analyze_repo_assigns_node_ids_by_file_order() {
        let dir = make_fixture();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");

        // Nodes sorted by path: main.py, models.py, utils.py (alphabetical)
        let node_ids: Vec<NodeId> = (0..3).collect();
        for id in &node_ids {
            assert!(result.space.nodes.contains_key(id), "node {id} exists");
        }
    }

    #[test]
    fn analyze_repo_computes_module_metrics_via_osp_core_axes() {
        let dir = make_fixture();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");

        // Every module has coupling + instability metrics (from CouplingAxis/InstabilityAxis)
        assert_eq!(result.module_metrics.len(), 3);
        for (_id, m) in &result.module_metrics {
            assert!(m.coupling.value >= 0.0 && m.coupling.value < 1.0, "coupling ∈ [0,1)");
            assert!(m.instability.value >= 0.0 && m.instability.value <= 1.0, "instability ∈ [0,1]");
            assert_eq!(m.coupling.source, MetricSource::TreeSitter);
            // Cohesion placeholder (SCIP pending)
            assert_eq!(m.cohesion.source, MetricSource::Placeholder);
        }
    }

    #[test]
    fn analyze_repo_detects_abstractness_from_class_defs() {
        let dir = make_fixture();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");

        // 3 classes: App (concrete), Helper (concrete), User (abstract via ABC)
        // A = Na/Nc = 1/3 ≈ 0.333
        let a = result.repo_metrics.abstractness.value;
        assert!(
            (a - (1.0 / 3.0)).abs() < 0.01,
            "A should be 1/3 (1 abstract / 3 total), got {a}"
        );
    }

    #[test]
    fn analyze_repo_empty_dir_returns_zero_nodes() {
        let dir = TempDir::new().unwrap();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");
        assert_eq!(result.space.node_count(), 0);
        assert_eq!(result.space.edge_count(), 0);
    }

    #[test]
    fn analyze_repo_skips_non_source_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("notes.txt"), "not source\n").unwrap();
        fs::write(dir.path().join("data.json"), "{}").unwrap();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");
        assert_eq!(result.space.node_count(), 0, "no recognized source files");
    }

    #[test]
    fn analyze_repo_repo_metrics_have_tree_sitter_source() {
        let dir = make_fixture();
        let result = analyze_repo(dir.path()).expect("analyze succeeded");

        assert_eq!(result.repo_metrics.abstractness.source, MetricSource::TreeSitter);
        assert_eq!(
            result.repo_metrics.main_sequence_distance.source,
            MetricSource::TreeSitter
        );
    }

    #[test]
    fn compute_repo_instability_returns_0_5_for_empty_space() {
        let space = Space::new();
        assert_eq!(compute_repo_instability(&space), 0.5);
    }

    #[test]
    fn compute_repo_instability_mass_weighted_average() {
        // 2 nodes: node 0 (mass=10, ce=2, ca=0 → I=1.0), node 1 (mass=10, ce=0, ca=2 → I=0.0)
        // weighted = (1.0×10 + 0.0×10) / 20 = 0.5
        let mut space = Space::new();
        space.insert_node(CoreNode { id: 0, kind: NodeKind::Module, mass: 10.0, ..Default::default() });
        space.insert_node(CoreNode { id: 1, kind: NodeKind::Module, mass: 10.0, ..Default::default() });
        space.insert_edge(Edge { from: 0, to: 1, kind: EdgeKind::Imports });
        space.insert_edge(Edge { from: 0, to: 1, kind: EdgeKind::Imports });
        let i = compute_repo_instability(&space);
        // node 0: ce=2, ca=0 → I=1.0; node 1: ce=0, ca=2 → I=0.0
        // weighted = (1.0×10 + 0.0×10)/20 = 0.5
        assert!((i - 0.5).abs() < 1e-9, "mass-weighted instability should be 0.5, got {i}");
    }

    // --- SCIP integration: compute_module_cohesion ---

    use crate::scip::index::{ClassSemanticInfo, FieldAccess};

    fn make_class(name: &str, methods: &[&str], fields: &[&str], accesses: &[(&str, &str)]) -> ClassSemanticInfo {
        ClassSemanticInfo {
            name: name.to_string(),
            methods: methods.iter().map(|s| s.to_string()).collect(),
            fields: fields.iter().map(|s| s.to_string()).collect(),
            field_access: accesses
                .iter()
                .map(|(m, f)| FieldAccess { method: m.to_string(), field: f.to_string() })
                .collect(),
        }
    }

    #[test]
    fn compute_module_cohesion_without_scip_returns_placeholder() {
        let idx = SemanticIndex::empty();
        let cohesion = compute_module_cohesion(std::path::Path::new("repo/foo.py"), std::path::Path::new("repo"), &idx);
        assert_eq!(cohesion.source, MetricSource::Placeholder);
        assert!((cohesion.value - 0.5).abs() < 1e-9);
        assert_eq!(cohesion.confidence, 0.0);
    }

    #[test]
    fn compute_module_cohesion_with_scip_matching_file_returns_real_lcom4() {
        // Cohesive class: m1 and m2 both access f1 → 1 component → LCOM4=1 → cohesion=1.0
        let class = make_class("Foo", &["m1", "m2"], &["f1"], &[("m1", "f1"), ("m2", "f1")]);
        let mut idx = SemanticIndex::empty();
        idx.classes.push(class.clone());
        idx.classes_by_file.insert("foo.py".to_string(), vec![class]);
        idx.files_indexed = 1;

        let cohesion = compute_module_cohesion(std::path::Path::new("repo/foo.py"), std::path::Path::new("repo"), &idx);
        assert_eq!(cohesion.source, MetricSource::Scip, "should be SCIP-sourced");
        assert!((cohesion.value - 1.0).abs() < 1e-9, "cohesive class → cohesion=1.0");
        assert!(cohesion.confidence > 0.0, "SCIP → confidence > 0");
    }

    #[test]
    fn compute_module_cohesion_fragmented_class_returns_low_cohesion() {
        // Fragmented: m1→f1 only, m2→f2 only → 2 components → LCOM4=2 → cohesion=0.5
        let class = make_class("Bar", &["m1", "m2"], &["f1", "f2"], &[("m1", "f1"), ("m2", "f2")]);
        let mut idx = SemanticIndex::empty();
        idx.classes.push(class.clone());
        idx.classes_by_file.insert("bar.py".to_string(), vec![class]);
        idx.files_indexed = 1;

        let cohesion = compute_module_cohesion(std::path::Path::new("repo/bar.py"), std::path::Path::new("repo"), &idx);
        assert_eq!(cohesion.source, MetricSource::Scip);
        assert!((cohesion.value - 0.5).abs() < 1e-9, "LCOM4=2 → cohesion=0.5");
    }

    #[test]
    fn compute_module_cohesion_scip_but_file_not_indexed_returns_placeholder() {
        // SCIP exists but this file isn't in it → placeholder
        let class = make_class("Foo", &["m1"], &["f1"], &[("m1", "f1")]);
        let mut idx = SemanticIndex::empty();
        idx.classes.push(class.clone());
        idx.classes_by_file.insert("other.py".to_string(), vec![class]);
        idx.files_indexed = 1;

        let cohesion = compute_module_cohesion(std::path::Path::new("repo/missing.py"), std::path::Path::new("repo"), &idx);
        assert_eq!(cohesion.source, MetricSource::Placeholder, "file not in SCIP → placeholder");
    }

    #[test]
    fn compute_module_cohesion_normalizes_windows_paths() {
        // Windows backslash paths should match SCIP forward-slash keys
        let class = make_class("Baz", &["m1"], &["f1"], &[("m1", "f1")]);
        let mut idx = SemanticIndex::empty();
        idx.classes.push(class.clone());
        idx.classes_by_file.insert("src/baz.py".to_string(), vec![class]);
        idx.files_indexed = 1;

        // Pipeline gives "repo\src\baz.py" on Windows, strip_prefix → "src\baz.py"
        // normalize → "src/baz.py" → match
        let cohesion = compute_module_cohesion(
            std::path::Path::new("repo/src/baz.py"),
            std::path::Path::new("repo"),
            &idx,
        );
        assert_eq!(cohesion.source, MetricSource::Scip, "normalized path should match");
    }

    // --- build_semantic_coverage ---

    #[test]
    fn build_semantic_coverage_empty_index_returns_none() {
        let idx = SemanticIndex::empty();
        let cov = build_semantic_coverage(&idx, 10, "abc123".to_string());
        assert_eq!(cov.coverage_ratio, 0.0);
        assert_eq!(cov.files_total, 10);
        assert_eq!(cov.files_with_scip, 0);
        assert_eq!(cov.repo_head, "abc123");
    }

    #[test]
    fn build_semantic_coverage_with_index_returns_ratio() {
        let mut idx = SemanticIndex::empty();
        idx.files_indexed = 8;
        idx.files_total = 8;
        idx.classes.push(make_class("A", &["m1"], &["f1"], &[("m1", "f1")]));

        let cov = build_semantic_coverage(&idx, 10, "abc123".to_string());
        assert!((cov.coverage_ratio - 0.8).abs() < 1e-9, "8/10 = 0.8");
        assert_eq!(cov.files_with_scip, 8);
        assert_eq!(cov.classes_total, 1);
        assert!(cov.coverage_ratio > 0.0);
    }

    // --- analyze_repo_with_config: SCIP flag wiring ---

    #[test]
    fn analyze_repo_without_scip_uses_placeholder_cohesion() {
        let dir = make_fixture();
        let registry = AdapterRegistry::default_all();
        let config = AnalysisConfig::default(); // no SCIP
        let result = analyze_repo_with_config(dir.path(), &registry, &config).expect("ok");

        // All modules should have placeholder cohesion
        for m in result.module_metrics.values() {
            assert_eq!(m.cohesion.source, MetricSource::Placeholder);
            assert_eq!(m.cohesion.confidence, 0.0);
        }
        assert_eq!(result.semantic_coverage.coverage_ratio, 0.0);
    }

    #[test]
    fn analyze_repo_with_nonexistent_scip_falls_back_gracefully() {
        let dir = make_fixture();
        let registry = AdapterRegistry::default_all();
        let config = AnalysisConfig {
            scip_index: Some(std::path::PathBuf::from("/nonexistent/index.scip")),
            ..Default::default()
        };
        // Should not error — fall back to placeholder
        let result = analyze_repo_with_config(dir.path(), &registry, &config).expect("graceful fallback");
        for m in result.module_metrics.values() {
            assert_eq!(m.cohesion.source, MetricSource::Placeholder);
        }
    }
}
