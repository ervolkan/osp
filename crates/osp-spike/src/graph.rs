//! Faz 0.3 — Tree-sitter bağımlılık grafı çıkarıcı.
//!
//! Bir repodaki `.py/.js/.jsx/.ts/.tsx` dosyalarını AST çöüzmler, import
//! deyimlerinden dosyalar-arası `Imports` kenarları çıkarır.
//!
//! **Spike seviyesi tasarım kararları:**
//! - Granularity: dosya-bazlı (her dosya = 1 `Module` düğümü). Sınıf/fonksiyon
//!   düğümleri Faz 3'te (tam analyzer) gelecek.
//! - Import çözümleme: yaklaşık, global suffix-match. Self-edge'ler elem.
//! - Hata toleransı: tek dosya parse edilemezse log+skip, spike durmaz.
//! - Dışsal paket importları (`react`, `lodash`, stdlib) otomatik elem (iç dosyaya
//!   çözümlenemedikleri için kenar oluşmaz).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tree_sitter::{Language, Parser};

use crate::model::{DepGraph, Edge, EdgeKind, Node, NodeId, NodeKind};

/// Bir reponun bağımlılık grafını çıkarır.
pub fn extract(repo: &Path) -> Result<DepGraph> {
    let files = collect_files(repo)?;
    tracing::info!(files = files.len(), "kaynak dosya bulundu");

    let mut nodes: Vec<Node> = Vec::with_capacity(files.len());
    let mut path_to_id: std::collections::HashMap<PathBuf, NodeId> =
        std::collections::HashMap::new();
    let mut imports_per_file: Vec<Vec<String>> = Vec::with_capacity(files.len());

    for (i, file) in files.iter().enumerate() {
        let rel = file
            .strip_prefix(repo)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned();
        let (mass, imports) = match analyze_file(file) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(file = %rel, error = %e, "dosya atlandı");
                (0usize, Vec::new())
            }
        };
        nodes.push(Node {
            id: i as NodeId,
            kind: NodeKind::Module,
            path: rel,
            mass,
        });
        path_to_id.insert(file.clone(), i as NodeId);
        imports_per_file.push(imports);
    }

    // Kenar kurulumu: her dosyanın import'larını iç dosyalara çözümle.
    let mut edges: Vec<Edge> = Vec::new();
    for i in 0..files.len() {
        for imp in &imports_per_file[i] {
            if let Some(target_path) = resolve_import(imp, &files) {
                if let Some(&target_id) = path_to_id.get(&target_path) {
                    if i as NodeId != target_id {
                        // self-edge'i önle
                        edges.push(Edge {
                            from: i as NodeId,
                            to: target_id,
                            kind: EdgeKind::Imports,
                        });
                    }
                }
            }
        }
    }

    tracing::info!(nodes = nodes.len(), edges = edges.len(), "graf kuruldu");
    Ok(DepGraph { nodes, edges })
}

/// Repodaki kaynak dosyaları toplar. Gizli + build/bağımlılık dizinlarını atlar.
fn collect_files(repo: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk(repo, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let it = std::fs::read_dir(dir).with_context(|| format!("read_dir: {:?}", dir))?;
    for entry in it {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if path.is_dir() {
            if name_str.starts_with('.')
                || matches!(
                    name_str.as_ref(),
                    "node_modules"
                        | "target"
                        | "__pycache__"
                        | "venv"
                        | ".venv"
                        | "env"
                        | "build"
                        | "dist"
                        | "site-packages"
                        | "egg-info"
                )
            {
                continue;
            }
            walk(&path, files)?;
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext, "py" | "js" | "jsx" | "ts" | "tsx") {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

/// Bir dosyayı AST çöüzmler; LOC (`mass`) + import yolu listesi döner.
fn analyze_file(path: &Path) -> Result<(usize, Vec<String>)> {
    let source =
        std::fs::read_to_string(path).with_context(|| format!("dosya okunamadı: {:?}", path))?;
    let mass = source.lines().count().max(1);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let imports = match ext {
        "py" => extract_imports(
            &source,
            tree_sitter_python::LANGUAGE.into(),
            LangKind::Python,
        )
        .with_context(|| format!("python parse: {:?}", path))?,
        "js" | "jsx" => extract_imports(
            &source,
            tree_sitter_javascript::LANGUAGE.into(),
            LangKind::JsLike,
        )
        .with_context(|| format!("js parse: {:?}", path))?,
        "ts" | "tsx" => extract_imports(
            &source,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            LangKind::JsLike,
        )
        .with_context(|| format!("ts parse: {:?}", path))?,
        _ => Vec::new(),
    };
    Ok((mass, imports))
}

#[derive(Clone, Copy)]
enum LangKind {
    Python,
    JsLike,
}

/// AST yürüyüşü ile import deyimlerini toplar (manuel DFS, query API'sinden
/// daha sağlam sürüm-geçirgen).
fn extract_imports(source: &str, lang: Language, kind: LangKind) -> Result<Vec<String>> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang)
        .context("tree-sitter grammar yüklenemedi")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter parse None döndü")?;
    let root = tree.root_node();
    let bytes = source.as_bytes();

    let mut imports = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let node_kind = n.kind();
        if matches!(node_kind, "import_statement" | "import_from_statement") {
            match kind {
                LangKind::Python => {
                    if node_kind == "import_from_statement" {
                        if let Some(m) = n.child_by_field_name("module_name") {
                            if let Ok(t) = m.utf8_text(bytes) {
                                imports.push(t.trim().to_string());
                            }
                        }
                    } else {
                        // `import a.b, c.d` → her dotted_name çocuğu ayrı import.
                        for i in 0..n.child_count() {
                            if let Some(c) = n.child(i) {
                                if c.kind() == "dotted_name" {
                                    if let Ok(t) = c.utf8_text(bytes) {
                                        imports.push(t.trim().to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                LangKind::JsLike => {
                    if let Some(src) = n.child_by_field_name("source") {
                        if let Ok(t) = src.utf8_text(bytes) {
                            let stripped = t
                                .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                                .trim()
                                .to_string();
                            if !stripped.is_empty() {
                                imports.push(stripped);
                            }
                        }
                    }
                }
            }
        }
        for i in 0..n.child_count() {
            if let Some(c) = n.child(i) {
                stack.push(c);
            }
        }
    }
    Ok(imports)
}

/// JS/TS import yolundaki bilinen uzantıyı soyar. `./foo.js` → `./foo`.
/// Python dotted-name'lerine dokunmaz (onlarda uzantı yoktur). Uzun/özel
/// uzantılar önce denenir (`.d.ts` > `.ts`).
fn strip_js_extension(s: &str) -> &str {
    for ext in [".d.ts", ".mjs", ".cjs", ".tsx", ".ts", ".jsx", ".js"] {
        if s.ends_with(ext) {
            return &s[..s.len() - ext.len()];
        }
    }
    s
}

/// Import yolunu repodaki bir dosyaya çözümler (yaklaşık global suffix-match).
///
/// `pkg.mod` ↔ `.../pkg/mod.py`; `./util` ↔ `.../util.js`. Sıkı eşleştirme:
/// `full == normalized` veya `full` `.<normalized>` ile bitiyor.
fn resolve_import(imp: &str, all_files: &[PathBuf]) -> Option<PathBuf> {
    let cleaned = imp.trim_start_matches("./").trim_start_matches("../");
    // JS/TS import'ları sıklıkla uzantı içerir (`./foo.js`) ama kaynak dosya
    // farklı uzantılı (`foo.ts`) — date-fns spektrumunda tespit edildi.
    let cleaned = strip_js_extension(cleaned);
    let normalized = cleaned.replace(['/', '\\'], ".");
    if normalized.is_empty() {
        return None;
    }
    let suffix = format!(".{}", normalized);
    for f in all_files {
        let stem = match f.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let parents = f
            .parent()
            .map(|p| {
                p.components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default();
        let full = if parents.is_empty() {
            stem.to_string()
        } else {
            format!("{}.{}", parents, stem)
        };
        if full == normalized || full.ends_with(&suffix) {
            return Some(f.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_imports_extracted() {
        let src = "import os\nimport foo.bar\nfrom baz.qux import x\nimport a, b.c\n";
        let imps = extract_imports(src, tree_sitter_python::LANGUAGE.into(), LangKind::Python)
            .expect("python parse");
        // foo.bar ve baz.qux iç modüller; os/a/b.c de çıkarılır (resolve aşaması elem yapar)
        assert!(imps.contains(&"foo.bar".to_string()), "imps: {:?}", imps);
        assert!(imps.contains(&"baz.qux".to_string()), "imps: {:?}", imps);
    }

    #[test]
    fn js_imports_extracted() {
        let src = "import x from './foo';\nimport y from \"../bar\";\nimport z from 'react';\n";
        let imps = extract_imports(
            src,
            tree_sitter_javascript::LANGUAGE.into(),
            LangKind::JsLike,
        )
        .expect("js parse");
        assert!(imps.iter().any(|s| s.contains("foo")), "imps: {:?}", imps);
        assert!(imps.iter().any(|s| s.contains("bar")), "imps: {:?}", imps);
        assert!(imps.iter().any(|s| s == "react"), "imps: {:?}", imps);
    }

    #[test]
    fn ts_imports_extracted() {
        let src = "import { A } from './types';\nexport interface B {}\n";
        let imps = extract_imports(
            src,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            LangKind::JsLike,
        )
        .expect("ts parse");
        assert!(imps.iter().any(|s| s.contains("types")), "imps: {:?}", imps);
    }

    #[test]
    fn resolve_internal_python_dotted() {
        let files = vec![PathBuf::from("repo/pkg/mod.py")];
        let r = resolve_import("pkg.mod", &files);
        assert_eq!(r, Some(PathBuf::from("repo/pkg/mod.py")));
    }

    #[test]
    fn resolve_internal_js_relative() {
        let files = vec![PathBuf::from("repo/src/util.js")];
        let r = resolve_import("./util", &files);
        assert_eq!(r, Some(PathBuf::from("repo/src/util.js")));
    }

    #[test]
    fn resolve_rejects_short_name_false_positive() {
        // "util" ile biten ama farklı dosyalar (ör. visual.js) yanlış eşleşmemeli.
        let files = vec![PathBuf::from("repo/src/visual.js")];
        let r = resolve_import("./util", &files);
        assert_eq!(r, None);
    }

    #[test]
    fn resolve_skips_truly_external() {
        let files = vec![PathBuf::from("repo/app.py")];
        let r = resolve_import("some_unrelated_pkg", &files);
        assert_eq!(r, None);
    }

    #[test]
    fn strip_js_extension_works() {
        assert_eq!(strip_js_extension("./foo.js"), "./foo");
        assert_eq!(strip_js_extension("./foo.ts"), "./foo");
        assert_eq!(strip_js_extension("./types.d.ts"), "./types");
        assert_eq!(strip_js_extension("./foo.mjs"), "./foo");
        assert_eq!(strip_js_extension("pkg.mod"), "pkg.mod"); // python unaffected
    }

    #[test]
    fn resolve_js_import_with_extension_matches_ts_file() {
        // date-fns spektrumu: import '../fp/index.js' ama kaynak index.ts.
        let files = vec![PathBuf::from("repo/fp/index.ts")];
        let r = resolve_import("../fp/index.js", &files);
        assert_eq!(r, Some(PathBuf::from("repo/fp/index.ts")));
    }

    #[test]
    fn extract_on_nonexistent_repo_errors_cleanly() {
        let r = extract(Path::new("nonexistent/repo/xyz"));
        assert!(r.is_err());
    }
}
