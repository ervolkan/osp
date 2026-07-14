//! Architecture guard — EI3-a: resolution API evidence isolation.
//!
//! Paper 3 v1.4 evidence-identity layer (§3.5, Table EI row EI3-a).
//!
//! ## Normative claim (narrow)
//!
//! The evaluated public resolution API signatures do not accept or expose the listed
//! evidence-source or evidence-mutation capability types. This is an API-shape policy
//! verified by a static architecture guard — it is not a type-level impossibility
//! (violation can be written, the guard catches it before merge).
//!
//! Enforcement class: **ARCH-GUARD** (API-shape policy + static AST-based source-scan test).
//!
//! ## What this guard checks (AST-based, via `syn`)
//!
//! The public resolution surface across `src/anchoring/{review,store,resolved_implementation}.rs`:
//! - **Struct fields** of structs whose name contains a resolution-API fragment
//!   (`ResolutionApplication`, `PresentedResolutionBasis`, `ResolutionRecord`, ...)
//! - **Trait method signatures** whose name contains a resolution-API fragment
//!   (`apply_resolution`, `resolution_target_for_identity`, ...) — trait metotları
//!   public-by-definition, `pub fn` olmasalar bile taranır
//! - **`impl` method signatures** that are `pub fn`/`pub(crate) fn` and either carry a
//!   resolution-API fragment in their name or mention a resolution type in their signature
//!
//! Forbidden tokens (capability surface — generic `<P: CodeEvidenceProvider>` atlatma önlemi
//! için tüm ilgili type isimleri listede):
//! - `CodeEvidenceProvider` (mutable provider capability trait — `&mut dyn` veya generic bound)
//! - `ResolvedCodeEvidenceProvider` (PR F adapter — lookup+source compose)
//! - `CodeEvidenceSource` (key-facing evidence source trait)
//! - `ObservedCodeEvidence` (evidence object)
//! - `InMemoryCodeEvidenceSource` (source implementation)
//!
//! ## Red-kanıt test
//!
//! `resolution_api_guard_catches_forbidden_evidence_in_synthetic_module`: sentetik bir Rust
//! modülü içine forbidden token taşıyan struct field + trait method + impl accessor koyar ve
//! guard'ın bunları yakaladığını (panik ile) doğrular. Bu, guard'ın gerçekten tarama
//! yaptığının kanıtıdır; test yeşil olsa bile tarama çalışmıyorsa red-kanıt kırılır.

use std::path::Path;
use syn::visit::Visit;
use quote::ToTokens;

/// Forbidden evidence-capability type names that must not appear in the public resolution
/// API signature surface.
const FORBIDDEN_TOKENS: &[&str] = &[
    "CodeEvidenceProvider",
    "ResolvedCodeEvidenceProvider",
    "CodeEvidenceSource",
    "ObservedCodeEvidence",
    "InMemoryCodeEvidenceSource",
];

/// Public resolution API surface item name fragments — the public types/functions we isolate.
const RESOLUTION_API_FRAGMENTS: &[&str] = &[
    "apply_resolution",
    "resolution_target_for_identity",
    "ResolutionApplication",
    "PresentedResolutionBasis",
    "ResolutionRecord",
    "CodeEntityResolutionSession",
    "ResolutionBasisView",
    "ResolutionOutcome",
];

/// `syn::Type` → string (token bazlı; `ToTokens::to_token_stream` üzerinden).
fn type_to_tokens_string(ty: &syn::Type) -> String {
    ty.to_token_stream().to_string().replace(' ', "")
}

/// Bir tip imzasında forbidden token geçiyor mu? Bulunan token listesini döndürür.
fn type_mentions_forbidden(ty: &syn::Type) -> Vec<&'static str> {
    let s = type_to_tokens_string(ty);
    FORBIDDEN_TOKENS
        .iter()
        .copied()
        .filter(|tok| s.contains(tok))
        .collect()
}

/// Bir isim resolution-API yüzeyine mi ait?
fn is_resolution_item(ident: &str) -> bool {
    RESOLUTION_API_FRAGMENTS.iter().any(|frag| ident.contains(frag))
}

/// AST ziyaretçisi: forbidden token içeren public resolution API yüzeylerini toplar.
struct ViolationCollector {
    violations: Vec<String>,
    /// Şu an bir resolution-API type'ının impl bloğu içinde miyiz?
    /// (`impl ResolutionApplication` gibi — tüm impl metotları resolution bağlamında.)
    in_resolution_impl: bool,
}

impl ViolationCollector {
    fn new() -> Self {
        Self {
            violations: Vec::new(),
            in_resolution_impl: false,
        }
    }
}

/// Bir `Signature` üzerinde (parametreler + dönüş tipi) forbidden token ara.
fn check_fn_signature(
    collector: &mut ViolationCollector,
    name: &str,
    sig: &syn::Signature,
    context: &str,
) {
    for arg in &sig.inputs {
        if let syn::FnArg::Typed(pat_type) = arg {
            for tok in type_mentions_forbidden(&pat_type.ty) {
                collector.violations.push(format!(
                    "{context}: fn `{name}` parametre tipi forbidden token `{tok}` taşıyor: `{}`",
                    type_to_tokens_string(&pat_type.ty)
                ));
            }
        }
    }
    if let syn::ReturnType::Type(_, ret_ty) = &sig.output {
        for tok in type_mentions_forbidden(ret_ty) {
            collector.violations.push(format!(
                "{context}: fn `{name}` dönüş tipi forbidden token `{tok}` taşıyor: `{}`",
                type_to_tokens_string(ret_ty)
            ));
        }
    }
}

/// Bir struct field'ı üzerinde forbidden token ara.
fn check_struct_field(
    collector: &mut ViolationCollector,
    struct_name: &str,
    field: &syn::Field,
) {
    for tok in type_mentions_forbidden(&field.ty) {
        collector.violations.push(format!(
            "struct `{struct_name}` field forbidden token `{tok}` taşıyor: `{}`",
            type_to_tokens_string(&field.ty)
        ));
    }
}

impl<'ast> Visit<'ast> for ViolationCollector {
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        if !is_resolution_item(&name) {
            return;
        }
        for field in &node.fields {
            check_struct_field(self, &name, field);
        }
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        // Trait metotları public-by-definition. Yalnızca resolution-API fragment'ı içeren
        // metot isimlerini tara (AnchorStore trait'i apply_resolution içerir ama kendisi
        // farklı isim; metot bazında filtre).
        for item in &node.items {
            if let syn::TraitItem::Fn(method) = item {
                let method_name = method.sig.ident.to_string();
                if is_resolution_item(&method_name) {
                    check_fn_signature(
                        self,
                        &method_name,
                        &method.sig,
                        &format!("trait `{}` method", node.ident),
                    );
                }
            }
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // impl bloğunun self tipi resolution-API type'ı ise (örn. `impl ResolutionApplication`),
        // tüm impl metotları resolution bağlamında kabul edilir — scoping isim bazlı değil,
        // impl context bazlı olur. Bu, `ResolutionApplication::evidence_source()` gibi
        // accessor'ların yakalanmasını sağlar.
        let self_is_resolution = node.self_ty.to_token_stream().to_string().replace(' ', "");
        let in_resolution_impl = is_resolution_item(&self_is_resolution);

        let old_in_impl = self.in_resolution_impl;
        self.in_resolution_impl = in_resolution_impl;
        syn::visit::visit_item_impl(self, node);
        self.in_resolution_impl = old_in_impl;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        // impl bloklarındaki public metotlar (pub veya pub(crate)).
        let is_pub = !matches!(node.vis, syn::Visibility::Inherited);
        if !is_pub {
            return;
        }
        let method_name = node.sig.ident.to_string();
        let name_is_resolution = is_resolution_item(&method_name);
        // İsim resolution-API değilse, imzada resolution tipi var mı bak.
        let mut mentions_resolution_in_sig = false;
        for arg in &node.sig.inputs {
            if let syn::FnArg::Typed(pt) = arg {
                let s = type_to_tokens_string(&pt.ty);
                if s.contains("Resolution") || s.contains("resolution") {
                    mentions_resolution_in_sig = true;
                }
            }
        }
        if let syn::ReturnType::Type(_, rt) = &node.sig.output {
            let s = type_to_tokens_string(rt);
            if s.contains("Resolution") || s.contains("resolution") {
                mentions_resolution_in_sig = true;
            }
        }
        // Bir impl metodu resolution bağlamında sayılırsa:
        //   (a) ismi resolution-API fragment'ı içeriyorsa, veya
        //   (b) imzasında resolution tipi varsa, veya
        //   (c) impl bloğunun self tipi resolution-API type'ı ise (örn. impl ResolutionApplication).
        if name_is_resolution || mentions_resolution_in_sig || self.in_resolution_impl {
            check_fn_signature(self, &method_name, &node.sig, "impl method");
        }
    }
}

/// Üç hedef dosyayı parse et ve violation topla. (violations, parsed_filenames) döndürür.
fn collect_violations_from_resolution_surface(
    files: &[&Path],
) -> (Vec<String>, Vec<String>) {
    let mut all_violations = Vec::new();
    let mut parsed_files = Vec::new();
    for path in files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                all_violations.push(format!("dosya okunamadı {}: {e}", path.display()));
                continue;
            }
        };
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(e) => {
                all_violations.push(format!("syn parse hatası {}: {e}", path.display()));
                continue;
            }
        };
        parsed_files.push(path.file_name().unwrap_or_default().to_string_lossy().into_owned());
        let mut collector = ViolationCollector::new();
        collector.visit_file(&file);
        all_violations.extend(collector.violations);
    }
    (all_violations, parsed_files)
}

/// EI3-a: the public resolution API signatures do not accept or expose evidence-source
/// or evidence-mutation capability types.
#[test]
fn resolution_api_has_no_evidence_capability() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let anchoring_dir = manifest.join("src/anchoring");
    let targets = [
        anchoring_dir.join("review.rs"),
        anchoring_dir.join("store.rs"),
        anchoring_dir.join("resolved_implementation.rs"),
    ];
    let target_refs: Vec<&Path> = targets.iter().map(|p| p.as_path()).collect();

    let (violations, parsed) = collect_violations_from_resolution_surface(&target_refs);

    // Sanity: store.rs ve review.rs parse edilmeli (apply_resolution her ikisinde de var).
    assert!(
        parsed.iter().any(|f| f == "store.rs"),
        "EI3-a guard sanity: store.rs parse edilmedi, parse edilenler: {:?}",
        parsed
    );
    assert!(
        parsed.iter().any(|f| f == "review.rs"),
        "EI3-a guard sanity: review.rs parse edilmedi, parse edilenler: {:?}",
        parsed
    );

    if !violations.is_empty() {
        panic!(
            "EI3-a violation — resolution API signatures carry evidence-capability types \
             (expected: API-shape isolation, ARCH-GUARD):\n  - {}\n\n\
             Normative claim: the evaluated public resolution API signatures do not accept \
             or expose the listed evidence-source or evidence-mutation capability types. \
             If a signature legitimately needs an evidence capability, the claim must be \
             narrowed or the API redesigned; do not weaken this guard without updating \
             manuscript §3.5 Table EI row EI3-a.",
            violations.join("\n  - ")
        );
    }
}

/// Red-kanıt: guard gerçekten tarama yapıyor mu? Sentetik bir Rust kaynak parçasında
/// forbidden token taşıyan struct field + trait method + impl accessor koy ve guard'ın
/// bunları yakaladığını doğrula. Bu test kırılırsa guard pass-through (no-op) olmuş demektir.
#[test]
fn resolution_api_guard_catches_forbidden_evidence_in_synthetic_module() {
    let synthetic = r#"
//! Sentetik test modülü — guard'ın tarama yaptığını kanıtlar.

pub struct ResolutionApplication {
    pub candidate_id: String,
    // FORBIDDEN: evidence object resolution application içinde
    pub evidence: ObservedCodeEvidence,
}

pub trait AnchorStore {
    // FORBIDDEN: mutable evidence provider capability trait metodunda
    fn apply_resolution_with_evidence(
        &mut self,
        app: ResolutionApplication,
        provider: &mut dyn CodeEvidenceProvider,
    ) -> Result<ResolutionRecord, ()>;

    // Resolution-API fragment'ı içermeyen ama forbidden token taşıyan metot —
    // bu yakalanMAMALI (resolution context dışında), guard'ın scoping'i doğruysa.
    fn unrelated_method(&self, e: CodeEvidenceSource) -> bool {
        let _ = e;
        true
    }
}

impl ResolutionApplication {
    // FORBIDDEN: accessor forbidden token dönüyor
    pub fn evidence_source(&self) -> &dyn CodeEvidenceSource {
        unimplemented!()
    }
}
"#;

    let file = match syn::parse_file(synthetic) {
        Ok(f) => f,
        Err(e) => panic!("sentetik kaynak parse edilemedi: {e}"),
    };
    let mut collector = ViolationCollector::new();
    collector.visit_file(&file);

    // En az 3 violation beklenir:
    // 1. ResolutionApplication.evidence field (ObservedCodeEvidence)
    // 2. AnchorStore::apply_resolution_with_evidence parametresi (CodeEvidenceProvider)
    // 3. ResolutionApplication::evidence_source dönüş tipi (CodeEvidenceSource)
    // NOT: unrelated_method yakalanmamalı (resolution context dışında).
    assert!(
        collector.violations.len() >= 3,
        "guard en az 3 violation yakalamalıydı (struct field + trait method param + impl accessor), {} buldu:\n{}",
        collector.violations.len(),
        collector.violations.join("\n")
    );

    let joined = collector.violations.join("\n");
    assert!(joined.contains("ObservedCodeEvidence"), "ObservedCodeEvidence violation yakalanmadı");
    assert!(joined.contains("CodeEvidenceProvider"), "CodeEvidenceProvider violation yakalanmadı");
    assert!(joined.contains("CodeEvidenceSource"), "CodeEvidenceSource violation yakalanmadı");
    assert!(
        !joined.contains("unrelated_method"),
        "guard yanlışlıkla unrelated_method'u yakaladı — scoping hatası"
    );
}
