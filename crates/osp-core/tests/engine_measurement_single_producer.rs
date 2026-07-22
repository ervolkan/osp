//! Architecture guard — INV-T9 #70 Commit 4b Faz 3: `EngineMeasurement` single-producer.
//!
//! **Reviewer v6 P1-2/P1-4 + v7 P1-2:** `EngineMeasurement::new` `pub(crate)` — osp-core
//! içindeki tüm modüllerin çağırmasına izin verir. Type-level DEĞİL; source-level invariant.
//! Verifier `measurement.after()` değerine güveniyor (yeniden ölçmüyor) — producer-origin
//! verifier güvenlik sınırı.
//!
//! **Kanıt seviyesi (reviewer v6 #4):** AST tabanlı production source-structure
//! regression guard. Kesin/type-level kanıt DEĞİL. Faz 9/10 strengthening ile güçlendir.
//!
//! **Reviewer v7 P1-2 (3 bypass kapandı):**
//! - P1-2a: `cfg(test)` substring kontrolü → **exact cfg ayrımı**. `cfg(not(test))`
//!   production kodunu dışlamaz; `cfg(any(test, ...))` belirsiz → taramaya devam (safe).
//! - P1-2b: read/parse error sessizce yutuluyordu → **fail-closed** (Err → test fail).
//! - P1-2c: yalnız `ExprCall` aranıyor, struct literal bypass var → **ExprStruct detection**
//!   eklendi. Production'da `EngineMeasurement { ... }` literal count == 0 olmalı.

use quote::ToTokens;
use std::path::PathBuf;
use syn::visit::{visit_item, Visit};
use syn::{Item, ItemFn};

const TARGET_TYPE: &str = "EngineMeasurement";
const TARGET_METHOD: &str = "new";
const EXPECTED_CALLER: &str = "measure_task_delta";

/// **P1-2a (reviewer v7):** cfg attribute exact test ayrımı.
/// `#[cfg(test)]` → test (dışla). `#[cfg(not(test))]` → production (TARAMAYA DEVAM ET).
/// `#[cfg(any(test, feature))]` → belirsiz, TARAMAYA DEVAM ET (safe default).
/// `#[cfg_attr(test, ...)]` → test bağlamında attr uygulanır → dışla.
fn is_exact_cfg_test(attr: &syn::Attribute) -> bool {
    let syn::Meta::List(meta_list) = &attr.meta else {
        return false;
    };
    let path_str = meta_list.path.to_token_stream().to_string();
    if path_str == "cfg_attr" {
        let tokens = meta_list.tokens.to_token_stream().to_string();
        let trimmed = tokens.trim();
        return trimmed == "test" || trimmed.starts_with("test,");
    }
    if path_str != "cfg" {
        return false;
    }
    let tokens = meta_list.tokens.to_token_stream().to_string();
    tokens.trim() == "test"
}

fn has_exact_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(is_exact_cfg_test)
}

fn is_test_module(ident: &str) -> bool {
    let lower = ident.to_lowercase();
    lower == "tests" || lower == "test" || lower.starts_with("tests_") || lower.ends_with("_tests")
}

fn is_target_call(expr: &syn::Expr) -> bool {
    let tokens = expr.to_token_stream().to_string().replace(' ', "");
    tokens.contains(&format!("{TARGET_TYPE}::{TARGET_METHOD}"))
        || tokens.contains(&format!("::{TARGET_TYPE}::{TARGET_METHOD}"))
        || tokens.contains(&format!("measurement::{TARGET_TYPE}::{TARGET_METHOD}"))
}

/// **P1-2c (reviewer v7):** struct literal bypass detection.
fn is_target_struct_literal(expr_struct: &syn::ExprStruct) -> bool {
    let path_str = expr_struct
        .path
        .to_token_stream()
        .to_string()
        .replace(' ', "");
    path_str.ends_with(TARGET_TYPE)
        || path_str.ends_with(&format!("::{TARGET_TYPE}"))
        || path_str.ends_with(&format!("measurement::{TARGET_TYPE}"))
}

#[derive(Debug, Clone)]
struct ConstructSite {
    file: String,
    enclosing_fn: String,
    kind: ConstructKind,
}

#[derive(Debug, Clone, PartialEq)]
enum ConstructKind {
    Call,
    StructLiteral,
}

/// **P1-2b (reviewer v7):** fail-closed scan error.
#[derive(Debug)]
struct ScanError {
    file: String,
    detail: String,
}

struct ConstructCollector {
    constructs: Vec<ConstructSite>,
    fn_stack: Vec<String>,
    test_depth: u32,
    current_file: String,
}

impl ConstructCollector {
    fn new(file: &str) -> Self {
        Self {
            constructs: Vec::new(),
            fn_stack: Vec::new(),
            test_depth: 0,
            current_file: file.to_string(),
        }
    }
}

impl<'ast> Visit<'ast> for ConstructCollector {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Mod(item_mod) => {
                let is_exact_cfg = has_exact_cfg_test(&item_mod.attrs);
                let is_test_mod_name = is_test_module(&item_mod.ident.to_string());
                if is_exact_cfg || is_test_mod_name {
                    self.test_depth += 1;
                }
                visit_item(self, item);
                if is_exact_cfg || is_test_mod_name {
                    self.test_depth = self.test_depth.saturating_sub(1);
                }
            }
            Item::Fn(item_fn) => {
                if has_exact_cfg_test(&item_fn.attrs) {
                    return;
                }
                let fn_name = item_fn.sig.ident.to_string();
                self.fn_stack.push(fn_name);
                syn::visit::visit_item_fn(self, item_fn);
                self.fn_stack.pop();
            }
            Item::Impl(item_impl) => {
                if has_exact_cfg_test(&item_impl.attrs) {
                    return;
                }
                syn::visit::visit_item_impl(self, item_impl);
            }
            _ => {
                visit_item(self, item);
            }
        }
    }

    fn visit_expr_call(&mut self, expr_call: &'ast syn::ExprCall) {
        if self.test_depth == 0 && is_target_call(&syn::Expr::Call(expr_call.clone())) {
            let enclosing = self.fn_stack.last().cloned().unwrap_or_default();
            self.constructs.push(ConstructSite {
                file: self.current_file.clone(),
                enclosing_fn: enclosing,
                kind: ConstructKind::Call,
            });
        }
        syn::visit::visit_expr_call(self, expr_call);
    }

    fn visit_expr_struct(&mut self, expr_struct: &'ast syn::ExprStruct) {
        if self.test_depth == 0 && is_target_struct_literal(expr_struct) {
            let enclosing = self.fn_stack.last().cloned().unwrap_or_default();
            self.constructs.push(ConstructSite {
                file: self.current_file.clone(),
                enclosing_fn: enclosing,
                kind: ConstructKind::StructLiteral,
            });
        }
        syn::visit::visit_expr_struct(self, expr_struct);
    }

    fn visit_impl_item_fn(&mut self, item_fn: &'ast syn::ImplItemFn) {
        if has_exact_cfg_test(&item_fn.attrs) {
            return;
        }
        let fn_name = item_fn.sig.ident.to_string();
        self.fn_stack.push(fn_name);
        syn::visit::visit_impl_item_fn(self, item_fn);
        self.fn_stack.pop();
    }
}

/// **P1-2b (reviewer v7):** fail-closed scan. Read/parse error → Err (test fail).
fn collect_constructs_in_file(
    path: &PathBuf,
    collector: &mut ConstructCollector,
) -> Result<(), ScanError> {
    let source = std::fs::read_to_string(path).map_err(|e| ScanError {
        file: path.to_string_lossy().to_string(),
        detail: format!("read failed: {e}"),
    })?;
    let file = syn::parse_file(&source).map_err(|e| ScanError {
        file: path.to_string_lossy().to_string(),
        detail: format!("parse failed: {e}"),
    })?;
    collector.current_file = path.to_string_lossy().to_string();
    syn::visit::visit_file(collector, &file);
    Ok(())
}

fn collect_all_constructs() -> Result<Vec<ConstructSite>, ScanError> {
    let mut collector = ConstructCollector::new("");
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    walk_rs(&src_dir, &mut collector)?;
    Ok(collector.constructs)
}

fn walk_rs(dir: &PathBuf, collector: &mut ConstructCollector) -> Result<(), ScanError> {
    let entries = std::fs::read_dir(dir).map_err(|e| ScanError {
        file: dir.to_string_lossy().to_string(),
        detail: format!("read_dir failed: {e}"),
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| ScanError {
            file: dir.to_string_lossy().to_string(),
            detail: format!("entry failed: {e}"),
        })?;
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, collector)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            collect_constructs_in_file(&path, collector)?;
        }
    }
    Ok(())
}

#[test]
fn engine_measurement_new_has_single_production_issuer() {
    // **P1-2 (reviewer v7):** fail-closed scan + struct literal bypass detection.
    let constructs = collect_all_constructs()
        .unwrap_or_else(|e| panic!("scan failed (fail-closed): {} — {}", e.file, e.detail));

    let production_calls: Vec<&ConstructSite> = constructs
        .iter()
        .filter(|c| c.kind == ConstructKind::Call)
        .collect();
    let production_literals: Vec<&ConstructSite> = constructs
        .iter()
        .filter(|c| c.kind == ConstructKind::StructLiteral)
        .collect();

    // **P1-2c:** struct literal bypass — production'da 0 olmalı.
    assert_eq!(
        production_literals.len(),
        0,
        "EngineMeasurement struct literal must not appear in production code, found {}: {:#?}",
        production_literals.len(),
        production_literals
    );

    // EngineMeasurement::new call count == 1.
    assert_eq!(
        production_calls.len(),
        1,
        "EngineMeasurement::new must have exactly 1 production call-site, found {}: {:#?}",
        production_calls.len(),
        production_calls
    );

    let only_call = production_calls[0];
    assert_eq!(
        only_call.enclosing_fn, EXPECTED_CALLER,
        "EngineMeasurement::new production call must be in `{EXPECTED_CALLER}`, found in `{}` ({})",
        only_call.enclosing_fn, only_call.file
    );
}

#[test]
fn guard_detects_additional_production_call_in_synthetic_source() {
    let synthetic = r#"
        fn evil_producer() {
            let _ = crate::measurement::EngineMeasurement::new(a, b, c, d);
        }
    "#;
    let file = syn::parse_file(synthetic).unwrap();
    let mut collector = ConstructCollector::new("synthetic.rs");
    syn::visit::visit_file(&mut collector, &file);
    let calls: Vec<&ConstructSite> = collector
        .constructs
        .iter()
        .filter(|c| c.kind == ConstructKind::Call)
        .collect();
    assert_eq!(calls.len(), 1, "guard must detect synthetic call");
    assert_eq!(calls[0].enclosing_fn, "evil_producer");
}

#[test]
fn guard_detects_struct_literal_bypass_in_synthetic_source() {
    // **P1-2c:** red-kanıt — struct literal detection çalışıyor mu?
    let synthetic = r#"
        fn evil_literal_producer() {
            let _ = crate::measurement::EngineMeasurement {
                before: b,
                after: a,
                context: c,
                request: r,
            };
        }
    "#;
    let file = syn::parse_file(synthetic).unwrap();
    let mut collector = ConstructCollector::new("synthetic.rs");
    syn::visit::visit_file(&mut collector, &file);
    let literals: Vec<&ConstructSite> = collector
        .constructs
        .iter()
        .filter(|c| c.kind == ConstructKind::StructLiteral)
        .collect();
    assert_eq!(
        literals.len(),
        1,
        "guard must detect synthetic EngineMeasurement struct literal"
    );
    assert_eq!(literals[0].enclosing_fn, "evil_literal_producer");
}

#[test]
fn guard_exact_cfg_not_test_does_not_exclude() {
    // **P1-2a:** red-kanıt — `#[cfg(not(test))]` production kodunu dışlamaz.
    let synthetic = r#"
        #[cfg(not(test))]
        fn production_under_not_test() {
            let _ = crate::measurement::EngineMeasurement::new(a, b, c, d);
        }
    "#;
    let file = syn::parse_file(synthetic).unwrap();
    let mut collector = ConstructCollector::new("synthetic.rs");
    syn::visit::visit_file(&mut collector, &file);
    let calls: Vec<&ConstructSite> = collector
        .constructs
        .iter()
        .filter(|c| c.kind == ConstructKind::Call)
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "cfg(not(test)) must NOT exclude production code — substring 'test' false-positive closed"
    );
    assert_eq!(calls[0].enclosing_fn, "production_under_not_test");
}

#[test]
fn guard_exact_cfg_test_excludes_real_cfg_test() {
    // **P1-2a:** `#[cfg(test)]` (exact) dışlanır.
    let synthetic = r#"
        #[cfg(test)]
        fn test_only_producer() {
            let _ = crate::measurement::EngineMeasurement::new(a, b, c, d);
        }
    "#;
    let file = syn::parse_file(synthetic).unwrap();
    let mut collector = ConstructCollector::new("synthetic.rs");
    syn::visit::visit_file(&mut collector, &file);
    let calls: Vec<&ConstructSite> = collector
        .constructs
        .iter()
        .filter(|c| c.kind == ConstructKind::Call)
        .collect();
    assert_eq!(
        calls.len(),
        0,
        "exact cfg(test) must exclude test-only code from scan"
    );
}

#[allow(dead_code)]
fn _ensure_item_fn_visit_linked(_: &ItemFn) {}
