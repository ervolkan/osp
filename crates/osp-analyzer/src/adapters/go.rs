//! Go adapter — tree-sitter-go.
//!
//! Import patterns: `import "fmt"`, `import "github.com/gin-gonic/gin"`
//! Abstract patterns: `interface X`

use std::path::Path;

use super::shared;
use crate::contract::{ClassDef, ImportKind, ImportStatement, ResolvedImport};
use crate::language::{LanguageAdapter, RepoContext};

pub struct GoAdapter;

impl LanguageAdapter for GoAdapter {
    fn name(&self) -> &str {
        "go"
    }

    fn extensions(&self) -> &[&str] {
        &[".go"]
    }

    fn extract_imports(&self, source: &str) -> Vec<ImportStatement> {
        let tree = match shared::parse_root(source, tree_sitter_go::LANGUAGE.into()) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let paths =
            shared::walk_imports(tree.root_node(), source.as_bytes(), &["import_declaration"]);
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
        // Go stdlib: single-word paths with no '.' and no '/' (fmt, os, net, http).
        let path = &import.path;
        let is_stdlib = !path.contains('.') && !path.contains('/');
        if is_stdlib {
            return Some(ResolvedImport {
                kind: ImportKind::StandardLibrary,
                target_path: None,
            });
        }
        // Internal package? Check against go.mod module path.
        if let Some(module_path) = &repo.go_module_path {
            // Zero-alloc prefix test: import == module OR import starts with "module/".
            let is_internal = path == module_path.as_str()
                || path
                    .strip_prefix(module_path.as_str())
                    .is_some_and(|rest| rest.starts_with('/'));
            if is_internal {
                if let Some(target) = repo.go_package_index.resolve(path, module_path) {
                    return Some(ResolvedImport {
                        kind: ImportKind::Internal,
                        target_path: Some(target.clone()),
                    });
                }
                // Internal module path but no files found (empty/ excluded pkg) → Unknown.
                return Some(ResolvedImport {
                    kind: ImportKind::Unknown,
                    target_path: None,
                });
            }
        }
        // Otherwise → external module (github.com/...).
        Some(ResolvedImport {
            kind: ImportKind::External,
            target_path: None,
        })
    }

    fn extract_class_defs(&self, source: &str) -> Vec<ClassDef> {
        let tree = match shared::parse_root(source, tree_sitter_go::LANGUAGE.into()) {
            Some(t) => t,
            None => return Vec::new(),
        };
        // Go: interface = abstract, struct = concrete
        // type_declaration nodes contain struct_type or interface_type
        shared::walk_class_defs(tree.root_node(), source, "", &["interface"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::RepoContext;
    use std::path::PathBuf;

    #[test]
    fn go_imports_extracted() {
        let src = r#"
package main
import "fmt"
import "github.com/gin-gonic/gin"
"#;
        let adapter = GoAdapter;
        let imports = adapter.extract_imports(src);
        assert!(imports.iter().any(|i| i.path == "fmt"), "{:?}", imports);
        assert!(
            imports.iter().any(|i| i.path.contains("gin")),
            "{:?}",
            imports
        );
    }

    #[test]
    fn go_interface_is_abstract() {
        let src = "package main\ntype Animal interface { Speak() }\ntype Dog struct {}\n";
        let adapter = GoAdapter;
        let defs = adapter.extract_class_defs(src);
        assert!(
            defs.iter().any(|d| d.is_abstract),
            "interface should be abstract"
        );
        assert!(
            defs.iter().any(|d| !d.is_abstract),
            "struct should be concrete"
        );
    }

    #[test]
    fn go_resolve_stdlib() {
        let repo = RepoContext::new(PathBuf::from("/repo"), vec![]);
        let adapter = GoAdapter;
        let import = ImportStatement {
            path: "fmt".into(),
            source_location: 0,
            ..Default::default()
        };
        let resolved = adapter
            .resolve_import(&import, Path::new("/repo/main.go"), &repo)
            .unwrap();
        assert_eq!(resolved.kind, ImportKind::StandardLibrary);
    }

    #[test]
    fn go_resolve_external() {
        let repo = RepoContext::new(PathBuf::from("/repo"), vec![]);
        let adapter = GoAdapter;
        let import = ImportStatement {
            path: "github.com/gin-gonic/gin".into(),
            source_location: 0,
            ..Default::default()
        };
        let resolved = adapter
            .resolve_import(&import, Path::new("/repo/main.go"), &repo)
            .unwrap();
        assert_eq!(resolved.kind, ImportKind::External);
    }

    #[test]
    fn go_resolve_internal_package_via_module_path() {
        // Build a temp repo with go.mod + two packages.
        let dir = std::env::temp_dir().join(format!(
            "osp-go-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("pkg/util")).unwrap();
        std::fs::write(
            dir.join("go.mod"),
            "module github.com/example/foo\n\ngo 1.21\n",
        )
        .unwrap();
        std::fs::write(dir.join("main.go"), "package main\n").unwrap();
        std::fs::write(dir.join("pkg/util/util.go"), "package util\n").unwrap();

        let repo = RepoContext::new(
            dir.clone(),
            vec![dir.join("main.go"), dir.join("pkg/util/util.go")],
        );
        assert_eq!(
            repo.go_module_path.as_deref(),
            Some("github.com/example/foo")
        );

        let adapter = GoAdapter;
        let import = ImportStatement {
            path: "github.com/example/foo/pkg/util".into(),
            source_location: 0,
            ..Default::default()
        };
        let resolved = adapter
            .resolve_import(&import, dir.join("main.go").as_path(), &repo)
            .unwrap();
        assert_eq!(resolved.kind, ImportKind::Internal);
        assert_eq!(
            resolved.target_path.as_deref(),
            Some(dir.join("pkg/util/util.go").as_path())
        );
    }

    #[test]
    fn go_stdlib_single_word_detected() {
        let repo = RepoContext::new(PathBuf::from("/repo"), vec![]);
        let adapter = GoAdapter;
        for p in &["fmt", "os", "strings"] {
            let import = ImportStatement {
                path: (*p).to_string(),
                source_location: 0,
                ..Default::default()
            };
            let resolved = adapter
                .resolve_import(&import, Path::new("/repo/main.go"), &repo)
                .unwrap();
            assert_eq!(resolved.kind, ImportKind::StandardLibrary, "{}", p);
        }
    }
}
