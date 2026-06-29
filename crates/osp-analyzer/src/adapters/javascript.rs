//! JavaScript adapter — tree-sitter-javascript.
//!
//! Import patterns: `import x from "./y"`, `require("y")`, `export * from`
//! Abstract patterns: (JS has no abstract — A=0 always)

use std::path::Path;

use super::shared;
use crate::contract::{ClassDef, ImportKind, ImportStatement, ResolvedImport};
use crate::language::{LanguageAdapter, RepoContext};

pub struct JavaScriptAdapter;

impl LanguageAdapter for JavaScriptAdapter {
    fn name(&self) -> &str {
        "javascript"
    }

    fn extensions(&self) -> &[&str] {
        &[".js", ".jsx"]
    }

    fn extract_imports(&self, source: &str) -> Vec<ImportStatement> {
        let tree = match shared::parse_root(source, tree_sitter_javascript::LANGUAGE.into()) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let paths =
            shared::walk_imports(tree.root_node(), source.as_bytes(), &["import_statement"]);
        paths
            .into_iter()
            .enumerate()
            .map(|(i, path)| ImportStatement {
                path,
                source_location: i,
                is_type_only: false,
            })
            .collect()
    }

    fn resolve_import(
        &self,
        import: &ImportStatement,
        _from_file: &Path,
        repo: &RepoContext,
    ) -> Option<ResolvedImport> {
        // Relative → try internal (Faz 3.9.1: HashMap)
        if import.path.starts_with('.') || import.path.starts_with('/') {
            if let Some(target) = repo.resolver.resolve(&import.path).cloned() {
                return Some(ResolvedImport {
                    kind: ImportKind::Internal,
                    target_path: Some(target),
                });
            }
            return Some(ResolvedImport {
                kind: ImportKind::Unknown,
                target_path: None,
            });
        }
        // Non-relative → external
        Some(ResolvedImport {
            kind: ImportKind::External,
            target_path: None,
        })
    }

    fn extract_class_defs(&self, source: &str) -> Vec<ClassDef> {
        let tree = match shared::parse_root(source, tree_sitter_javascript::LANGUAGE.into()) {
            Some(t) => t,
            None => return Vec::new(),
        };
        // JS has no abstract keyword — all classes are concrete (is_abstract=false)
        shared::walk_class_defs(
            tree.root_node(),
            source,
            "class_declaration",
            &["__NEVER_MATCH__"], // JS has no abstract → always false
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::RepoContext;
    use std::path::PathBuf;

    #[test]
    fn js_imports_extracted() {
        let src = "import x from './foo';\nimport y from 'express';\n";
        let adapter = JavaScriptAdapter;
        let imports = adapter.extract_imports(src);
        assert!(
            imports.iter().any(|i| i.path.contains("foo")),
            "{:?}",
            imports
        );
        assert!(imports.iter().any(|i| i.path == "express"));
    }

    #[test]
    fn js_classes_all_concrete() {
        let src = "class Foo { bar() {} }\n";
        let adapter = JavaScriptAdapter;
        let defs = adapter.extract_class_defs(src);
        assert_eq!(defs.len(), 1);
        assert!(!defs[0].is_abstract, "JS has no abstract");
        assert!(defs[0].methods.contains(&"bar".to_string()));
    }

    #[test]
    fn js_resolve_relative() {
        let repo = RepoContext::new(
            PathBuf::from("/repo"),
            vec![PathBuf::from("/repo/lib/util.js")],
        );
        let adapter = JavaScriptAdapter;
        let import = ImportStatement {
            path: "./util".into(),
            source_location: 0,
            ..Default::default()
        };
        let resolved = adapter
            .resolve_import(&import, Path::new("/repo/main.js"), &repo)
            .unwrap();
        assert_eq!(resolved.kind, ImportKind::Internal);
    }
}
