//! Shared tree-sitter helpers — AST walk patterns reusable across adapters.

use std::path::Path;

use tree_sitter::{Language, Node, Parser};

use crate::contract::ClassDef;

/// Parse source with given language → root Node. Returns None on parse failure.
pub fn parse_root(source: &str, language: Language) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Walk AST, collect import path strings from nodes matching `import_node_kinds`.
/// For Python: `["import_statement", "import_from_statement"]`.
/// For JS/TS: `["import_statement"]`.
pub fn walk_imports(root: Node, source_bytes: &[u8], import_node_kinds: &[&str]) -> Vec<String> {
    let mut imports = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if import_node_kinds.contains(&n.kind()) {
            extract_import_path(&n, source_bytes, &mut imports);
        }
        // Push children right-to-left → processed left-to-right (DFS order fix)
        for i in (0..n.child_count()).rev() {
            if let Some(c) = n.child(i) {
                stack.push(c);
            }
        }
    }
    imports
}

/// Python: `import_from_statement` → `module_name` field; `import_statement` → `dotted_name` children.
/// JS/TS: `import_statement` → `source` field (string literal, strip quotes).
fn extract_import_path(node: &Node, source: &[u8], imports: &mut Vec<String>) {
    let kind = node.kind();
    if kind == "import_from_statement" {
        // Python: `from foo.bar import x` → module_name = "foo.bar"
        if let Some(module) = node.child_by_field_name("module_name") {
            if let Ok(text) = module.utf8_text(source) {
                imports.push(text.trim().to_string());
            }
        }
    } else if kind == "import_statement" {
        // Check if this is Python (dotted_name children) or JS/TS (source field)
        if let Some(src) = node.child_by_field_name("source") {
            // JS/TS: source is a string literal → strip quotes
            if let Ok(text) = src.utf8_text(source) {
                let stripped = text
                    .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                    .trim()
                    .to_string();
                if !stripped.is_empty() {
                    imports.push(stripped);
                }
            }
        } else {
            // Python: `import a.b, c.d` → each dotted_name is a separate import
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.kind() == "dotted_name" {
                        if let Ok(text) = c.utf8_text(source) {
                            imports.push(text.trim().to_string());
                        }
                    }
                }
            }
        }
    } else if kind == "use_declaration" {
        // Rust: `use crate::foo::Bar;` → expand grouped imports recursively.
        // `use foo::{a, b}` → "foo::a", "foo::b". `use foo::bar::{A, B}}` → both.
        if let Ok(text) = node.utf8_text(source) {
            let body = text
                .trim_start_matches("use")
                .trim()
                .trim_end_matches(';')
                .trim();
            let mut expanded = Vec::new();
            expand_rust_use_group("", body, &mut expanded);
            for path in expanded {
                let p = path.trim().trim_end_matches("::").to_string();
                if !p.is_empty() && p != "self" && p != "crate" && p != "super" {
                    imports.push(p);
                }
            }
        }
    } else if kind == "import_declaration" {
        // Go: walk for interpreted_string_literal children
        let mut stk = vec![*node];
        while let Some(n) = stk.pop() {
            if n.kind() == "interpreted_string_literal" {
                if let Ok(text) = n.utf8_text(source) {
                    let path = text.trim_matches('"').to_string();
                    if !path.is_empty() {
                        imports.push(path);
                    }
                }
            }
            for i in 0..n.child_count() {
                if let Some(c) = n.child(i) {
                    stk.push(c);
                }
            }
        }
    }
}

/// Walk AST, collect class definitions.
/// Matches any node kind containing "class" (class_definition, class_declaration, etc.).
pub fn walk_class_defs(
    root: Node,
    source: &str,
    _class_node_kind: &str,  // ignored — uses contains("class") for robustness
    abstract_patterns: &[&str],
) -> Vec<ClassDef> {
    let source_bytes = source.as_bytes();
    let mut defs = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        // Match actual class definition nodes (not class_body, class_heritage etc.)
        let k = n.kind();
        let is_class_def = k == "class_definition"        // Python
            || k == "class_declaration"                     // JS/TS
            || k == "abstract_class_declaration"            // TS abstract
            || k == "interface_declaration"                 // TS/JS interface (abstract — Martin)
            || k == "type_alias_declaration"                // TS type alias (abstract surface)
            || k == "struct_item"                           // Rust concrete
            || k == "trait_item"                            // Rust abstract (trait)
            || k == "enum_item"                             // Rust concrete (enum)
            || k == "type_declaration";                     // Go
        // TS interface/type_alias abstract sayılır (Martin: interface = abstract contract).
        // extract_class_def abstract_patterns'e bakar ama biz bu node kind'lerini
        // doğrudan abstract işaretliyoruz — TS adapter'da pattern gerekmez.
        let force_abstract = k == "interface_declaration" || k == "type_alias_declaration";
        if is_class_def {
            if let Some(mut def) = extract_class_def(&n, source_bytes, abstract_patterns) {
                if force_abstract {
                    def.is_abstract = true;
                }
                defs.push(def);
            }
        }
        for i in (0..n.child_count()).rev() {
            if let Some(c) = n.child(i) {
                stack.push(c);
            }
        }
    }
    defs
}

fn extract_class_def(
    node: &Node,
    source: &[u8],
    abstract_patterns: &[&str],
) -> Option<ClassDef> {
    let full_text = node.utf8_text(source).ok()?.to_string();
    let is_abstract = abstract_patterns.iter().any(|&p| full_text.contains(p));

    // Robust name search: walk children for first identifier/type_identifier
    let name = find_first_identifier(node, source)?;

    // Robust method search: recursive walk for function_definition/method_definition
    let methods = find_methods(node, source);

    Some(ClassDef {
        name,
        is_abstract,
        methods,
        source_location: node.start_byte(),
    })
}

fn find_first_identifier(node: &Node, source: &[u8]) -> Option<String> {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "identifier"
            || n.kind() == "type_identifier"
            || n.kind() == "property_identifier"
        {
            return n.utf8_text(source).ok().map(|s| s.trim().to_string());
        }
        for i in (0..n.child_count()).rev() {
            if let Some(c) = n.child(i) {
                stack.push(c);
            }
        }
    }
    None
}

fn find_methods(node: &Node, source: &[u8]) -> Vec<String> {
    let mut methods = Vec::new();
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "function_definition" || n.kind() == "method_definition" {
            if let Some(name) = find_first_identifier(&n, source) {
                methods.push(name);
            }
        }
        for i in (0..n.child_count()).rev() {
            if let Some(c) = n.child(i) {
                stack.push(c);
            }
        }
    }
    methods
}

/// Strip JS/TS file extension from import path.
/// `./foo.js` → `./foo`, `./types.d.ts` → `./types`
pub fn strip_js_extension(s: &str) -> &str {
    for ext in [".d.ts", ".mjs", ".cjs", ".tsx", ".ts", ".jsx", ".js"] {
        if s.ends_with(ext) {
            return &s[..s.len() - ext.len()];
        }
    }
    s
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rust `use` expansion + resolution (crate::/super::/self:: aware)
// ═══════════════════════════════════════════════════════════════════════════════

/// Recursively expand a Rust `use` body into individual path strings.
///
/// - `"crate::foo::Bar"` → `["crate::foo::Bar"]`
/// - `"foo::{a, b}"` → `["foo::a", "foo::b"]`
/// - `"foo::bar::{A, B::{C, D}}"` → `["foo::bar::A", "foo::bar::B::C", "foo::bar::B::D"]`
/// - `"foo::bar as baz"` → `["foo::bar"]` (alias stripped)
fn expand_rust_use_group(prefix: &str, segment: &str, out: &mut Vec<String>) {
    let seg = segment.trim();
    if let Some(brace) = seg.find('{') {
        let pre = seg[..brace].trim().trim_end_matches("::").trim();
        let full_prefix = if prefix.is_empty() {
            pre.to_string()
        } else if pre.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}::{pre}")
        };
        let after = &seg[brace + 1..];
        if let Some(close) = matching_rust_brace(after) {
            let inner = &after[..close];
            for item in split_top_level_commas_rust(inner) {
                expand_rust_use_group(&full_prefix, item.trim(), out);
            }
        }
    } else {
        // No group — strip `as Alias`, emit prefix::segment (or segment alone).
        let path_part = seg.split_whitespace().next().unwrap_or(seg);
        if path_part.is_empty() {
            return;
        }
        let full = if prefix.is_empty() {
            path_part.to_string()
        } else {
            format!("{prefix}::{path_part}")
        };
        out.push(full);
    }
}

/// Find the matching closing brace index in `s` (content after an opening `{`).
/// Handles nested braces. Returns None if unbalanced.
fn matching_rust_brace(s: &str) -> Option<usize> {
    let mut depth = 1u32;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split on top-level commas (depth-0 only). Used for `use` group expansion.
fn split_top_level_commas_rust(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Resolve a Rust `use` path to a source file via the HashMap resolver.
///
/// Strips `crate::` / `super::` / `self::` prefixes (iteratively), converts
/// `::` → `.`, then tries progressively shorter suffixes (dropping the trailing
/// type name, e.g. `crate::foo::Bar` → tries `foo.Bar`, then `foo`).
pub fn resolve_rust_use<'a>(
    path: &str,
    resolver: &'a ImportResolver,
) -> Option<&'a std::path::PathBuf> {
    let mut cleaned = path.trim();
    loop {
        let stripped = cleaned
            .strip_prefix("crate::")
            .or_else(|| cleaned.strip_prefix("super::"))
            .or_else(|| cleaned.strip_prefix("self::"));
        match stripped {
            Some(rest) => cleaned = rest.trim(),
            None => break,
        }
    }
    let dotted = cleaned.replace("::", ".");
    let parts: Vec<&str> = dotted.split('.').collect();
    if parts.is_empty() {
        return None;
    }
    // Try full path, then drop trailing segments (type name) until 1 segment left.
    for end in (1..=parts.len()).rev() {
        let candidate = parts[..end].join(".");
        if let Some(p) = resolver.resolve(&candidate) {
            return Some(p);
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// Go module-path detection + package resolution
// ═══════════════════════════════════════════════════════════════════════════════

/// Read `go.mod` from `repo_root` and return the `module` path.
pub fn detect_go_module_path(repo_root: &Path) -> Option<String> {
    let go_mod = repo_root.join("go.mod");
    let content = std::fs::read_to_string(&go_mod).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            let m = rest.trim();
            if !m.is_empty() {
                return Some(m.to_string());
            }
        }
    }
    None
}

/// Pre-indexed map of Go package directories → their `.go` files (sorted).
///
/// Built once from `all_files` relative to `repo_root`. Keys are
/// repo-root-relative package directories with forward-slash separators
/// (e.g. `"pkg/util"`), so sibling directories that share a path suffix
/// (e.g. `internal/pkg/util` vs `pkg/util`) map to distinct keys — no
/// collision. Enables O(1) per-import lookup instead of O(M×N) linear scan.
#[derive(Debug, Clone, Default)]
pub struct GoPackageIndex {
    map: std::collections::HashMap<String, Vec<std::path::PathBuf>>,
}

impl GoPackageIndex {
    /// Build the index from all source files. Only `.go` files are indexed;
    /// files outside `repo_root` are skipped.
    pub fn build(repo_root: &Path, all_files: &[std::path::PathBuf]) -> Self {
        let mut map: std::collections::HashMap<String, Vec<std::path::PathBuf>> =
            std::collections::HashMap::new();
        for f in all_files {
            if f.extension().and_then(|e| e.to_str()) != Some("go") {
                continue;
            }
            let rel = match f.strip_prefix(repo_root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let parent = match rel.parent() {
                Some(p) if !p.as_os_str().is_empty() => p,
                _ => continue,
            };
            let pkg_dir = parent.to_string_lossy().replace('\\', "/");
            map.entry(pkg_dir).or_default().push(f.clone());
        }
        for files in map.values_mut() {
            files.sort();
        }
        Self { map }
    }

    /// Number of indexed packages (diagnostic).
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Resolve a Go import path to a representative source file in the target package.
    ///
    /// `import_path` must already be confirmed internal (starts with `module_path`).
    /// Returns None if the target package directory contains no indexed `.go` files.
    pub fn resolve(
        &self,
        import_path: &str,
        module_path: &str,
    ) -> Option<&std::path::PathBuf> {
        // Strip module prefix + '/'. import_path uses forward slashes.
        let rest = import_path
            .strip_prefix(module_path)
            .map(|r| r.trim_start_matches('/'))?;
        if rest.is_empty() {
            return None;
        }
        let pkg_dir = rest.replace('\\', "/");
        let files = self.map.get(&pkg_dir)?;
        debug_assert!(!files.is_empty(), "indexed package dir has no files");
        let pkg_name = pkg_dir.rsplit('/').next().unwrap_or(&pkg_dir);
        // Priority 1: <pkg>/<pkgname>.go (conventional primary file).
        let primary = format!("/{pkg_name}.go");
        if let Some(p) = files.iter().find(|f| f.to_string_lossy().ends_with(&primary)) {
            return Some(p);
        }
        // Priority 2: first non-test .go file.
        if let Some(p) = files.iter().find(|f| !f.to_string_lossy().ends_with("_test.go")) {
            return Some(p);
        }
        // Priority 3: any file (only test files exist).
        files.first()
    }
}

/// HashMap-based import resolver — O(depth) build + O(1) lookup.
///
/// Faz 3.9.1 refactor: O(N×M) linear scan → O(N×depth + M) HashMap.
/// django 119s → <2s (60x speedup).
///
/// Her dosya için tüm dotted-path suffix'leri key olarak saklanır:
/// `repo.src.foo.bar` → keys: `bar`, `foo.bar`, `src.foo.bar`, `repo.src.foo.bar`
#[derive(Debug, Clone)]
pub struct ImportResolver {
    map: std::collections::HashMap<String, std::path::PathBuf>,
}impl ImportResolver {
    /// Tüm dosyalardan HashMap kur. O(N × avg_depth).
    pub fn build(all_files: &[std::path::PathBuf]) -> Self {
        let mut map = std::collections::HashMap::new();
        for f in all_files {
            let normalized = path_normalized_dotted(&f.to_string_lossy());
            let parts: Vec<&str> = normalized.split('.').collect();
            for i in 0..parts.len() {
                let key = parts[i..].join(".");
                map.entry(key).or_insert_with(|| f.clone());
            }
        }
        Self { map }
    }

    /// Import path → dosya yolu. O(1) lookup.
    pub fn resolve(&self, import_path: &str) -> Option<&std::path::PathBuf> {
        let cleaned = import_path
            .trim_start_matches("./")
            .trim_start_matches("../");
        let cleaned = strip_js_extension(cleaned);
        let normalized = cleaned.replace(['/', '\\'], ".");
        self.map.get(&normalized)
    }

    /// HashMap entry count (diagnostic).
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Eski linear resolver — geri uyumluluk için (deprecated, ImportResolver kullanın).
#[deprecated(note = "ImportResolver::build + resolve kullanın — O(1) lookup")]
pub fn try_resolve_internal(import_path: &str, all_files: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    ImportResolver::build(all_files).resolve(import_path).cloned()
}

fn path_normalized_dotted(s: &str) -> String {
    // Extract parent dirs + file stem, join with dots
    let path = std::path::Path::new(s);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let parents: Vec<&str> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect()
        })
        .unwrap_or_default();
    if parents.is_empty() {
        stem.to_string()
    } else {
        format!("{}.{}", parents.join("."), stem)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler — ImportResolver (O(1) HashMap lookup, 60x speedup claim)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    // --- ImportResolver::build + resolve roundtrip ---

    #[test]
    fn resolver_resolves_simple_filename() {
        let files = vec![pb("/repo/utils.py")];
        let resolver = ImportResolver::build(&files);
        // Import "utils" → /repo/utils.py
        assert_eq!(resolver.resolve("utils"), Some(&pb("/repo/utils.py")));
    }

    #[test]
    fn resolver_resolves_dotted_path() {
        let files = vec![pb("/repo/src/models/user.py")];
        let resolver = ImportResolver::build(&files);
        // All suffix keys: "user", "models.user", "src.models.user", "repo.src.models.user"
        assert_eq!(resolver.resolve("models.user"), Some(&pb("/repo/src/models/user.py")));
        assert_eq!(
            resolver.resolve("src.models.user"),
            Some(&pb("/repo/src/models/user.py"))
        );
    }

    #[test]
    fn resolver_resolves_path_with_separators() {
        // Import paths use / or \ — resolver normalizes to dots
        let files = vec![pb("/repo/src/foo/bar.py")];
        let resolver = ImportResolver::build(&files);
        assert_eq!(resolver.resolve("src/foo/bar"), Some(&pb("/repo/src/foo/bar.py")));
        assert_eq!(resolver.resolve("src\\foo\\bar"), Some(&pb("/repo/src/foo/bar.py")));
    }

    #[test]
    fn resolver_strips_relative_prefixes() {
        let files = vec![pb("/repo/utils.py")];
        let resolver = ImportResolver::build(&files);
        assert_eq!(resolver.resolve("./utils"), Some(&pb("/repo/utils.py")));
        assert_eq!(resolver.resolve("../utils"), Some(&pb("/repo/utils.py")));
    }

    #[test]
    fn resolver_strips_js_extensions() {
        let files = vec![pb("/repo/components/Button.tsx")];
        let resolver = ImportResolver::build(&files);
        // Import "./Button.tsx" → strip ext → "Button" → resolve
        assert_eq!(
            resolver.resolve("./Button.tsx"),
            Some(&pb("/repo/components/Button.tsx"))
        );
        assert_eq!(
            resolver.resolve("./Button.js"),
            Some(&pb("/repo/components/Button.tsx"))
        );
    }

    #[test]
    fn resolver_returns_none_for_unknown_import() {
        let files = vec![pb("/repo/utils.py")];
        let resolver = ImportResolver::build(&files);
        assert_eq!(resolver.resolve("nonexistent"), None);
        assert_eq!(resolver.resolve("foo.bar.baz"), None);
    }

    #[test]
    fn resolver_handles_multiple_files() {
        let files = vec![
            pb("/repo/main.py"),
            pb("/repo/utils.py"),
            pb("/repo/models/user.py"),
        ];
        let resolver = ImportResolver::build(&files);
        assert_eq!(resolver.resolve("main"), Some(&pb("/repo/main.py")));
        assert_eq!(resolver.resolve("utils"), Some(&pb("/repo/utils.py")));
        assert_eq!(resolver.resolve("user"), Some(&pb("/repo/models/user.py")));
        assert_eq!(resolver.resolve("models.user"), Some(&pb("/repo/models/user.py")));
    }

    #[test]
    fn resolver_len_reflects_all_suffix_keys() {
        // Relative path (cross-platform — no root prefix ambiguity)
        let files = vec![pb("repo/utils.py")];
        let resolver = ImportResolver::build(&files);
        // path_normalized_dotted("repo/utils.py") = "repo.utils"
        // suffixes: "utils", "repo.utils" → 2 keys
        assert_eq!(resolver.len(), 2);
    }

    // --- strip_js_extension ---

    #[test]
    fn strip_js_extension_handles_all_variants() {
        assert_eq!(strip_js_extension("foo.ts"), "foo");
        assert_eq!(strip_js_extension("foo.tsx"), "foo");
        assert_eq!(strip_js_extension("foo.js"), "foo");
        assert_eq!(strip_js_extension("foo.jsx"), "foo");
        assert_eq!(strip_js_extension("foo.mjs"), "foo");
        assert_eq!(strip_js_extension("foo.cjs"), "foo");
        assert_eq!(strip_js_extension("foo.d.ts"), "foo");
    }

    #[test]
    fn strip_js_extension_no_match_returns_original() {
        assert_eq!(strip_js_extension("foo.py"), "foo.py");
        assert_eq!(strip_js_extension("foo"), "foo");
        assert_eq!(strip_js_extension("./utils"), "./utils");
    }

    #[test]
    fn strip_js_extension_prefers_longest_match() {
        // ".d.ts" (5 chars) vs ".ts" (3 chars) — .d.ts should win
        assert_eq!(strip_js_extension("types.d.ts"), "types");
        // But plain .ts on a file named "foo.ts" still works
        assert_eq!(strip_js_extension("foo.ts"), "foo");
    }

    // --- path_normalized_dotted (helper) ---

    #[test]
    fn path_normalized_dotted_extracts_parents_and_stem() {
        // Relative paths (cross-platform — absolute paths include root prefix on Windows)
        assert_eq!(path_normalized_dotted("repo/src/models/user.py"), "repo.src.models.user");
        assert_eq!(path_normalized_dotted("user.py"), "user");
        assert_eq!(path_normalized_dotted("a/b/c.py"), "a.b.c");
    }

    // --- deprecated try_resolve_internal backward compat ---

    #[test]
    #[allow(deprecated)]
    fn deprecated_resolver_matches_new_resolver() {
        let files = vec![pb("/repo/utils.py"), pb("/repo/models/user.py")];
        // Old API
        let old_result = try_resolve_internal("utils", &files);
        // New API
        let resolver = ImportResolver::build(&files);
        let new_result = resolver.resolve("utils").cloned();

        assert_eq!(old_result, new_result);
        assert_eq!(old_result, Some(pb("/repo/utils.py")));
    }

    // --- resolve_rust_use (crate::/super::/self:: + trailing type drop) ---

    #[test]
    fn resolve_rust_use_strips_crate_prefix_and_drops_type_name() {
        // crate::models::User → drop "User" → resolve "models" → /repo/models.rs
        let files = vec![pb("/repo/models.rs"), pb("/repo/main.rs")];
        let resolver = ImportResolver::build(&files);
        assert_eq!(
            resolve_rust_use("crate::models::User", &resolver),
            Some(&pb("/repo/models.rs"))
        );
        // Bare module import (lowercase) → kept as-is.
        assert_eq!(
            resolve_rust_use("crate::models", &resolver),
            Some(&pb("/repo/models.rs"))
        );
    }

    #[test]
    fn resolve_rust_use_strips_super_and_self_prefixes() {
        let files = vec![pb("/repo/foo/bar.rs")];
        let resolver = ImportResolver::build(&files);
        assert_eq!(
            resolve_rust_use("super::bar", &resolver),
            Some(&pb("/repo/foo/bar.rs"))
        );
        assert_eq!(
            resolve_rust_use("self::bar", &resolver),
            Some(&pb("/repo/foo/bar.rs"))
        );
    }

    #[test]
    fn resolve_rust_use_returns_none_for_unresolvable() {
        let files = vec![pb("/repo/main.rs")];
        let resolver = ImportResolver::build(&files);
        assert_eq!(resolve_rust_use("crate::nonexistent::Thing", &resolver), None);
    }

    // --- expand_rust_use_group (grouped + nested imports) ---

    #[test]
    fn expand_rust_use_group_handles_plain_path() {
        let mut out = Vec::new();
        expand_rust_use_group("", "crate::foo::Bar", &mut out);
        assert_eq!(out, vec!["crate::foo::Bar"]);
    }

    #[test]
    fn expand_rust_use_group_handles_simple_group() {
        let mut out = Vec::new();
        expand_rust_use_group("", "foo::{a, b}", &mut out);
        assert_eq!(out, vec!["foo::a", "foo::b"]);
    }

    #[test]
    fn expand_rust_use_group_handles_nested_group() {
        let mut out = Vec::new();
        expand_rust_use_group("", "foo::bar::{A, B::{C, D}}", &mut out);
        assert_eq!(
            out,
            vec!["foo::bar::A", "foo::bar::B::C", "foo::bar::B::D"]
        );
    }

    #[test]
    fn expand_rust_use_group_strips_as_alias() {
        let mut out = Vec::new();
        expand_rust_use_group("", "foo::Bar as Baz", &mut out);
        assert_eq!(out, vec!["foo::Bar"]);
    }

    // --- detect_go_module_path + GoPackageIndex ---

    #[test]
    fn detect_go_module_path_reads_go_mod() {
        let dir = tempdir();
        std::fs::write(dir.join("go.mod"), "module github.com/example/foo\n\ngo 1.21\n").unwrap();
        assert_eq!(
            detect_go_module_path(&dir),
            Some("github.com/example/foo".to_string())
        );
    }

    #[test]
    fn detect_go_module_path_returns_none_without_go_mod() {
        let dir = tempdir();
        assert_eq!(detect_go_module_path(&dir), None);
    }

    #[test]
    fn go_package_index_prefers_primary_file() {
        // pkg/baz has baz.go (primary) + other.go + other_test.go
        let root = pb("/repo");
        let files = vec![
            pb("/repo/pkg/baz/baz.go"),
            pb("/repo/pkg/baz/util.go"),
            pb("/repo/pkg/baz/util_test.go"),
        ];
        let idx = GoPackageIndex::build(&root, &files);
        let target = idx.resolve("github.com/example/foo/pkg/baz", "github.com/example/foo");
        assert_eq!(target, Some(&pb("/repo/pkg/baz/baz.go")));
    }

    #[test]
    fn go_package_index_falls_back_to_first_non_test() {
        // No primary file (no baz.go) → first non-test file (sorted: util.go).
        let root = pb("/repo");
        let files = vec![pb("/repo/pkg/baz/util.go"), pb("/repo/pkg/baz/util_test.go")];
        let idx = GoPackageIndex::build(&root, &files);
        let target = idx.resolve("github.com/example/foo/pkg/baz", "github.com/example/foo");
        assert_eq!(target, Some(&pb("/repo/pkg/baz/util.go")));
    }

    #[test]
    fn go_package_index_returns_none_for_external() {
        let root = pb("/repo");
        let files = vec![pb("/repo/pkg/baz/baz.go")];
        let idx = GoPackageIndex::build(&root, &files);
        assert_eq!(
            idx.resolve("github.com/other/pkg", "github.com/example/foo"),
            None
        );
    }

    #[test]
    fn go_package_index_returns_none_for_empty_package() {
        let root = pb("/repo");
        let files = vec![pb("/repo/main.go")];
        let idx = GoPackageIndex::build(&root, &files);
        assert_eq!(
            idx.resolve("github.com/example/foo/pkg/baz", "github.com/example/foo"),
            None
        );
    }

    #[test]
    fn go_package_index_does_not_collide_on_sibling_suffix_dirs() {
        // Regression: unanchored ends_with matched both pkg/util and internal/pkg/util.
        // With repo-root-relative keys, they are distinct packages.
        let root = pb("/repo");
        let files = vec![
            pb("/repo/pkg/util/util.go"),
            pb("/repo/internal/pkg/util/internal_util.go"),
        ];
        let idx = GoPackageIndex::build(&root, &files);
        assert_eq!(idx.len(), 2, "two distinct package dirs indexed");
        // Import of pkg/util must NOT resolve into internal/pkg/util.
        let target = idx.resolve("github.com/example/foo/pkg/util", "github.com/example/foo");
        assert_eq!(target, Some(&pb("/repo/pkg/util/util.go")));
        let target2 =
            idx.resolve("github.com/example/foo/internal/pkg/util", "github.com/example/foo");
        assert_eq!(target2, Some(&pb("/repo/internal/pkg/util/internal_util.go")));
    }

    fn tempdir() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "osp-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
