//! Python adapter — tree-sitter-python.
//!
//! Import patterns: `import x`, `from x import y`
//! Abstract patterns: `class X(ABC):`, `class X(Protocol):`

use std::path::Path;

use crate::contract::{ClassDef, ImportStatement, ResolvedImport};
use crate::language::{LanguageAdapter, RepoContext};
use super::shared;

pub struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn name(&self) -> &str {
        "python"
    }

    fn extensions(&self) -> &[&str] {
        &[".py"]
    }

    fn extract_imports(&self, source: &str) -> Vec<ImportStatement> {
        let tree = match shared::parse_root(
            source,
            tree_sitter_python::LANGUAGE.into(),
        ) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let paths = shared::walk_imports(
            tree.root_node(),
            source.as_bytes(),
            &["import_statement", "import_from_statement"],
        );
        paths
            .into_iter()
            .enumerate()
            .map(|(i, path)| ImportStatement {
                path,
                source_location: i, // approximate
                is_type_only: false, // Python'da type-only import kavramı yok (TYPE_CHECKING idiom future work)
            })
            .collect()
    }

    fn resolve_import(
        &self,
        import: &ImportStatement,
        _from_file: &Path,
        repo: &RepoContext,
    ) -> Option<ResolvedImport> {
        // Try internal resolution (Faz 3.9.1: HashMap O(1) lookup)
        if let Some(target) = repo.resolver.resolve(&import.path).cloned() {
            return Some(ResolvedImport {
                kind: crate::contract::ImportKind::Internal,
                target_path: Some(target),
            });
        }
        // External (could be stdlib or third-party — we don't distinguish here)
        // Faz 3.3: stdlib detection (known module list)
        Some(ResolvedImport {
            kind: crate::contract::ImportKind::External,
            target_path: None,
        })
    }

    fn extract_class_defs(&self, source: &str) -> Vec<ClassDef> {
        let tree = match shared::parse_root(
            source,
            tree_sitter_python::LANGUAGE.into(),
        ) {
            Some(t) => t,
            None => return Vec::new(),
        };
        shared::walk_class_defs(
            tree.root_node(),
            source,
            "class_definition",
            &["ABC", "Protocol", "ABCMeta"], // Python abstract patterns
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::ImportKind;
    use crate::language::RepoContext;
    use std::path::PathBuf;

    #[test]
    fn python_imports_extracted() {
        let src = "import os\nimport foo.bar\nfrom baz.qux import x\n";
        let adapter = PythonAdapter;
        let imports = adapter.extract_imports(src);
        assert!(imports.iter().any(|i| i.path == "foo.bar"), "imports: {:?}", imports);
        assert!(imports.iter().any(|i| i.path == "baz.qux"));
    }

    #[test]
    fn python_class_defs_with_abc() {
        let src = r#"
from abc import ABC
class Animal(ABC):
    def speak(self): pass
class Dog(Animal):
    def bark(self): pass
"#;
        let adapter = PythonAdapter;
        let defs = adapter.extract_class_defs(src);
        assert_eq!(defs.len(), 2);
        assert!(defs[0].is_abstract, "Animal(ABC) abstract");
        assert!(!defs[1].is_abstract, "Dog concrete");
        assert!(defs[0].methods.contains(&"speak".to_string()));
        assert!(defs[1].methods.contains(&"bark".to_string()));
    }

    #[test]
    fn python_resolve_internal() {
        let repo = RepoContext::new(
            PathBuf::from("/repo"),
            vec![PathBuf::from("/repo/foo/bar.py")],
        );
        let adapter = PythonAdapter;
        let import = ImportStatement {
            path: "foo.bar".into(),
            source_location: 0,
            ..Default::default()
        };
        let resolved = adapter
            .resolve_import(&import, Path::new("/repo/main.py"), &repo)
            .unwrap();
        assert_eq!(resolved.kind, ImportKind::Internal);
    }

    #[test]
    fn python_resolve_external() {
        let repo = RepoContext::new(PathBuf::from("/repo"), vec![]);
        let adapter = PythonAdapter;
        let import = ImportStatement {
            path: "external_pkg".into(),
            source_location: 0,
            ..Default::default()
        };
        let resolved = adapter
            .resolve_import(&import, Path::new("/repo/main.py"), &repo)
            .unwrap();
        assert_eq!(resolved.kind, ImportKind::External);
    }
}
