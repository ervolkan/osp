//! Architecture guard — INV-T9 #70 Commit 4b Faz 3: `EngineMeasurement` single-producer.
//!
//! **Reviewer v6 P1-2/P1-4:** `EngineMeasurement::new` `pub(crate)` — osp-core içindeki
//! tüm modüllerin çağırmasına izin verir. Bu type-level bir invariant DEĞİL; source-level.
//! Verifier `measurement.after()` değerine güveniyor (yeniden ölçmüyor, hash'liyor) —
//! bu yüzden producer-origin verifier güvenlik sınırının parçası.
//!
//! **Kanıt seviyesi (reviewer v6 #4):** AST tabanlı production source-structure
//! regression guard. Kesin/type-level kanıt DEĞİL — semantik Rust name-resolution
//! yapmaz (alias `use EngineMeasurement as EM` veya macro teorik olarak kaçabilir).
//! Faz 9/10 type-level strengthening veya constructor visibility daraltma ile
//! güçlendirilebilir.
//!
//! ## Bu guard ne kontrol eder
//!
//! - `src/**/*.rs` (production non-test code) içinde `EngineMeasurement::new(...)` call
//!   expression'ları
//! - `#[cfg(test)]` item'ları ve `mod tests` modülleri DIŞLANIR
//! - Macro expanding call'lar tam yakalanamayabilir (sınırlama doc'ta)
//! - Production call count == 1 olmalı
//! - Enclosing function `measure_task_delta` olmalı

use std::path::PathBuf;
use syn::visit::{visit_item, Visit};
use syn::{Item, ItemFn};

/// Target type + method — `EngineMeasurement::new`.
const TARGET_TYPE: &str = "EngineMeasurement";
const TARGET_METHOD: &str = "new";
const EXPECTED_CALLER: &str = "measure_task_delta";

/// Bir dosya `#[cfg(test)]` attribute'lu mu? (basit kontrol — attribute listesinde
/// `cfg(test)` veya `test` varyantı). Macro/gelişmiş cfg çözümlemesi yapılmaz.
fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if let syn::Meta::List(meta_list) = &attr.meta {
            let path_str = meta_list.path.to_token_stream().to_string();
            if path_str == "cfg" {
                let tokens = meta_list.tokens.to_token_stream().to_string();
                if tokens.contains("test") {
                    return true;
                }
            }
        }
        false
    })
}

use quote::ToTokens;

/// Test modülü mü? (mod adi `tests`, `test`, `tests_*` veya `*_tests`).
fn is_test_module(ident: &str) -> bool {
    let lower = ident.to_lowercase();
    lower == "tests" || lower == "test" || lower.starts_with("tests_") || lower.ends_with("_tests")
}

/// Bir call expression `EngineMeasurement::new(...)` mi?
/// `ExprCall` → `Expr` → ... path'i takip eder. Basit kontrol: token stream'de
/// `EngineMeasurement :: new` pattern'i veya `EngineMeasurement :: < :: new`.
fn is_target_call(expr: &syn::Expr) -> bool {
    let tokens = expr.to_token_stream().to_string().replace(' ', "");
    // `EngineMeasurement::new` veya aliased path'ler (örn crate::measurement::...::new).
    // Sadece doğrudan `EngineMeasurement::new` veya `::EngineMeasurement::new` yakalanır.
    tokens.contains(&format!("{TARGET_TYPE}::{TARGET_METHOD}"))
        || tokens.contains(&format!("::{TARGET_TYPE}::{TARGET_METHOD}"))
        || tokens.contains(&format!("measurement::{TARGET_TYPE}::{TARGET_METHOD}"))
}

/// Production caller info — enclosing function name.
#[derive(Debug, Clone)]
struct CallSite {
    file: String,
    enclosing_fn: String,
}

/// AST visitor — `EngineMeasurement::new` call'larını toplar (test modülleri dışında).
struct CallCollector {
    calls: Vec<CallSite>,
    /// Şu anki enclosing function stack (nested fn için).
    fn_stack: Vec<String>,
    /// Test modülü/cfg(test) derinliği — >0 ise call'ları sayma.
    test_depth: u32,
    current_file: String,
}

impl CallCollector {
    fn new(file: &str) -> Self {
        Self {
            calls: Vec::new(),
            fn_stack: Vec::new(),
            test_depth: 0,
            current_file: file.to_string(),
        }
    }
}

impl<'ast> Visit<'ast> for CallCollector {
    /// Item ziyaret — cfg(test) ve test modüllerini atla, fn stack'i yönet.
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Mod(item_mod) => {
                let was_test = self.test_depth > 0;
                let is_cfg_test = has_cfg_test(&item_mod.attrs);
                let is_test_mod_name = is_test_module(&item_mod.ident.to_string());
                if is_cfg_test || is_test_mod_name {
                    self.test_depth += 1;
                }
                visit_item(self, item);
                if is_cfg_test || is_test_mod_name {
                    self.test_depth = self.test_depth.saturating_sub(1);
                }
                let _ = was_test;
            }
            Item::Fn(item_fn) => {
                // cfg(test) fn → skip
                if has_cfg_test(&item_fn.attrs) {
                    return;
                }
                let fn_name = item_fn.sig.ident.to_string();
                self.fn_stack.push(fn_name);
                // Body içindeki call'ları topla (visit_item içinde gezilir).
                syn::visit::visit_item_fn(self, item_fn);
                self.fn_stack.pop();
            }
            Item::Impl(item_impl) => {
                if has_cfg_test(&item_impl.attrs) {
                    return;
                }
                // impl bloğu içindeki metotları gez — ImplItem::Fn'ler için fn_stack yönet.
                syn::visit::visit_item_impl(self, item_impl);
            }
            _ => {
                // cfg(test) attribute'lu diğer item'ları atla
                let attrs: Vec<&syn::Attribute> = match item {
                    Item::Const(c) => c.attrs.iter().collect(),
                    Item::Static(s) => s.attrs.iter().collect(),
                    Item::Struct(s) => s.attrs.iter().collect(),
                    Item::Enum(e) => e.attrs.iter().collect(),
                    Item::Union(u) => u.attrs.iter().collect(),
                    _ => vec![],
                };
                if attrs.iter().any(|a| {
                    let meta_str = a.to_token_stream().to_string();
                    meta_str.contains("cfg(test") || meta_str.contains("cfg(test)")
                }) {
                    return;
                }
                visit_item(self, item);
            }
        }
    }

    /// Call expression — hedef mi?
    fn visit_expr_call(&mut self, expr_call: &'ast syn::ExprCall) {
        if self.test_depth == 0 {
            if is_target_call(&syn::Expr::Call(expr_call.clone())) {
                let enclosing = self.fn_stack.last().cloned().unwrap_or_default();
                self.calls.push(CallSite {
                    file: self.current_file.clone(),
                    enclosing_fn: enclosing,
                });
            }
        }
        // Nested call'lar için devam et.
        syn::visit::visit_expr_call(self, expr_call);
    }

    /// Impl metodu — fn_stack'e push (enclosing function tracking).
    fn visit_impl_item_fn(&mut self, item_fn: &'ast syn::ImplItemFn) {
        if has_cfg_test(&item_fn.attrs) {
            return;
        }
        let fn_name = item_fn.sig.ident.to_string();
        self.fn_stack.push(fn_name);
        syn::visit::visit_impl_item_fn(self, item_fn);
        self.fn_stack.pop();
    }
}

/// Bir .rs dosyasını parse et ve call'ları topla.
fn collect_calls_in_file(path: &PathBuf, collector: &mut CallCollector) -> Result<(), String> {
    let source =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let file = syn::parse_file(&source).map_err(|e| format!("parse {}: {}", path.display(), e))?;
    let file_name = path.to_string_lossy().to_string();
    collector.current_file = file_name;
    syn::visit::visit_file(collector, &file);
    Ok(())
}

/// `src/` altındaki tüm .rs dosyalarını recursive tara.
fn collect_all_calls() -> Vec<CallSite> {
    let mut collector = CallCollector::new("");
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    walk_rs(&src_dir, &mut collector);
    collector.calls
}

fn walk_rs(dir: &PathBuf, collector: &mut CallCollector) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, collector);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let _ = collect_calls_in_file(&path, collector);
        }
    }
}

#[test]
fn engine_measurement_new_has_single_production_issuer() {
    // **Reviewer v6 P1-2/P1-4:** Production non-test code'da `EngineMeasurement::new`
    // call count == 1, enclosing function == `measure_task_delta`.
    //
    // Bu test source-structure regression guard — type-level kanıt DEĞİL. Sınırlar:
    // - alias (`use EngineMeasurement as EM`) teorik olarak kaçabilir
    // - macro expanding call'lar tam yakalanamayabilir
    // - Faz 9/10 type-level strengthening için ayrı suite.
    let calls = collect_all_calls();

    // Sadece production call'ları say (test_depth==0 zaten filtreli).
    let production_calls: Vec<&CallSite> = calls.iter().collect();

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

/// **Red-kanıt test:** sentetik bir source parçasında ikinci bir call eklendiğinde
/// guard'ın bunu yakaladığını doğrular. Guard tarama yapıyorsa bu test yeşil; yoksa
/// (sentetik call eklenince count 2 olur) assertion kırılır.
#[test]
fn guard_detects_additional_production_call_in_synthetic_source() {
    let synthetic = r#"
        fn evil_producer() {
            let _ = crate::measurement::EngineMeasurement::new(
                crate::measurement::MeasurementBaseline::Available(m),
                after,
                ctx,
                req,
            );
        }
    "#;
    let file = syn::parse_file(synthetic).unwrap();
    let mut collector = CallCollector::new("synthetic.rs");
    syn::visit::visit_file(&mut collector, &file);
    assert_eq!(
        collector.calls.len(),
        1,
        "guard must detect synthetic EngineMeasurement::new call"
    );
    assert_eq!(collector.calls[0].enclosing_fn, "evil_producer");
}

/// `ItemFn` ziyaretçisi — syn::visit modülünce sağlanır ama bizim override visit_item
/// fn stack yönetimi için visit_item_fn çağrıyor. Bu re-export sadece docs için.
#[allow(dead_code)]
fn _ensure_item_fn_visit_linked(_: &ItemFn) {}
