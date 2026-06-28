//! TypeScript adapter — tree-sitter-typescript.
//!
//! Import patterns: `import x from "./y"`, `export * from "./y"`
//! Abstract patterns: `abstract class X`, `interface X`

use std::path::Path;

use crate::contract::{ClassDef, ImportStatement, ImportKind, ResolvedImport};
use crate::language::{LanguageAdapter, RepoContext};
use super::shared;

pub struct TypeScriptAdapter;

impl LanguageAdapter for TypeScriptAdapter {
    fn name(&self) -> &str {
        "typescript"
    }

    fn extensions(&self) -> &[&str] {
        &[".ts", ".tsx"]
    }

    fn extract_imports(&self, source: &str) -> Vec<ImportStatement> {
        let tree = match shared::parse_root(
            source,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        ) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let paths = shared::walk_imports(
            tree.root_node(),
            source.as_bytes(),
            &["import_statement"],
        );
        paths
            .into_iter()
            .enumerate()
            .map(|(i, path)| ImportStatement { path, source_location: i })
            .collect()
    }

    fn resolve_import(
        &self,
        import: &ImportStatement,
        _from_file: &Path,
        repo: &RepoContext,
    ) -> Option<ResolvedImport> {
        // Relative import (./ or ../) → try internal (Faz 3.9.1: HashMap)
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
        // Non-relative → external package
        Some(ResolvedImport {
            kind: ImportKind::External,
            target_path: None,
        })
    }

    fn extract_class_defs(&self, source: &str) -> Vec<ClassDef> {
        let tree = match shared::parse_root(
            source,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        ) {
            Some(t) => t,
            None => return Vec::new(),
        };
        shared::walk_class_defs(
            tree.root_node(),
            source,
            "class_declaration",
            &["abstract class", "interface "],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::RepoContext;
    use std::path::PathBuf;

    #[test]
    fn ts_imports_extracted() {
        let src = "import { x } from './foo';\nimport y from '../bar';\nimport z from 'react';\n";
        let adapter = TypeScriptAdapter;
        let imports = adapter.extract_imports(src);
        assert!(imports.iter().any(|i| i.path.contains("foo")), "{:?}", imports);
        assert!(imports.iter().any(|i| i.path == "react"));
    }

    #[test]
    fn ts_abstract_class_detected() {
        let src = "abstract class Animal { speak(): void {} }\nclass Dog extends Animal { bark() {} }\n";
        let adapter = TypeScriptAdapter;
        let defs = adapter.extract_class_defs(src);
        assert!(defs.iter().any(|d| d.is_abstract), "Animal abstract");
        assert!(defs.iter().any(|d| !d.is_abstract), "Dog concrete");
    }

    #[test]
    fn ts_interface_counted_as_abstract() {
        // Regression: değerlendirme Svelte'de Abstractness=0.0 gördü. Kök neden
        // walk_class_defs'in interface_declaration node kind'ini tanımamasıydı.
        // Interface'ler Martin tanımına göre abstract sayılır (abstract contract).
        let src = "interface Animal { speak(): void; }\nclass Dog implements Animal { speak() {} }\n";
        let adapter = TypeScriptAdapter;
        let defs = adapter.extract_class_defs(src);
        assert!(defs.iter().any(|d| d.is_abstract), "Animal interface should be abstract");
        assert!(defs.iter().any(|d| !d.is_abstract), "Dog class should be concrete");
        // 2 def beklenir (interface + class), ikisi de sayılmalı
        assert_eq!(defs.len(), 2, "interface + class both counted");
    }

    #[test]
    fn ts_type_alias_counted_as_abstract() {
        // type alias da abstract surface — declaration dosyalarında yoğun.
        let src = "type Status = 'active' | 'inactive';\nclass Service { status: Status; }\n";
        let adapter = TypeScriptAdapter;
        let defs = adapter.extract_class_defs(src);
        assert!(defs.iter().any(|d| d.is_abstract), "type alias should be abstract");
    }

    #[test]
    fn ts_resolve_relative_internal() {
        let repo = RepoContext::new(
            PathBuf::from("/repo"),
            vec![PathBuf::from("/repo/src/util.ts")],
        );
        let adapter = TypeScriptAdapter;
        let import = ImportStatement { path: "./util".into(), source_location: 0 };
        let resolved = adapter.resolve_import(&import, Path::new("/repo/src/main.ts"), &repo).unwrap();
        assert_eq!(resolved.kind, ImportKind::Internal);
    }

    #[test]
    fn ts_resolve_external_package() {
        let repo = RepoContext::new(PathBuf::from("/repo"), vec![]);
        let adapter = TypeScriptAdapter;
        let import = ImportStatement { path: "react".into(), source_location: 0 };
        let resolved = adapter.resolve_import(&import, Path::new("/repo/src/main.ts"), &repo).unwrap();
        assert_eq!(resolved.kind, ImportKind::External);
    }
}
