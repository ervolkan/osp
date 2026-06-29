//! SCIP index loader — .scip protobuf → SemanticIndex (Faz 3.6).
//!
//! `scip` crate (SourceGraph official) ile .scip dosyasını parse eder.
//! SemanticIndex → LCOM4 cohesion hesabı için kullanılır.
//!
//! SCIP occurrence format:
//! - `symbol`: "python pkg.mod ClassName#method(" veya "python pkg.mod ClassName.field"
//! - `range`: [line, col, end_col] veya [line, col, end_line, end_col]
//! - `symbol_roles`: bitfield (bit 0 = Definition, bit 1 = Import)

use std::collections::HashMap;
use std::path::Path;

use protobuf::Message;

use super::index::{ClassSemanticInfo, FieldAccess, SemanticIndex};

/// SCIP SymbolInformation.Kind'dan çıkarsanan sembol kategorisi (LCOM4 için).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InferredKind {
    Class,
    Method,
    Field,
    Other,
}

impl InferredKind {
    /// SCIP proto Kind değerinden kategori çıkar (kind != 0 durumu).
    fn from_kind_value(val: i32) -> Self {
        match val {
            // Class-like
            7 | 49 | 53 | 56 | 75 | 33 => Self::Class,
            // Method-like
            26 | 9 | 66 | 68 | 69 | 70 | 71 | 74 | 76 | 80 => Self::Method,
            // Field-like
            15 | 41 | 77 | 79 | 81 | 18 | 45 => Self::Field,
            _ => Self::Other,
        }
    }
}

/// Sembol string'inden descriptor suffix çıkarımı (kind=0 fallback).
///
/// SCIP sembol formatı (descriptor path — son space'ten sonraki segment):
/// - `...ClassName#` → Type (class) — `#` suffix
/// - `...fieldName.` → Term (field) — `.` suffix (ama `).` değil)
/// - `...methodName().` → Method — `().` suffix
/// - `...(paramName)` → Parameter — `)` suffix (içerikli parantez)
/// - `...path/` → Package/path — `/` suffix
fn infer_kind_from_symbol(symbol: &str) -> InferredKind {
    // Son space'ten sonraki segment = descriptor path
    let last = symbol.rsplit(' ').next().unwrap_or(symbol);

    if last.ends_with('#') {
        InferredKind::Class
    } else if last.ends_with(").") {
        // `methodName().` → Method (empty parens + dot separator)
        InferredKind::Method
    } else if last.ends_with(')') {
        // `(paramName)` → Parameter (içerikli parantez) — LCOM4 için "Other"
        InferredKind::Other
    } else if last.ends_with('.') {
        // `fieldName.` → Term (field)
        InferredKind::Field
    } else {
        InferredKind::Other
    }
}

/// SCIP index'ten SemanticIndex kur.
///
/// .scip dosyasını parse eder → class/method/field/field-access çıkarır.
/// LCOM4 hesabı için gereken tüm veriyi toplar. `classes_by_file` map'i
/// pipeline'ın per-module cohesion hesabı için dosya → class ilişkisini saklar.
pub fn load_scip_index(scip_path: &Path) -> anyhow::Result<SemanticIndex> {
    let bytes = std::fs::read(scip_path)?;
    parse_scip_bytes(&bytes)
}

/// SCIP bytes → SemanticIndex (test için ayrılmış).
pub fn parse_scip_bytes(bytes: &[u8]) -> anyhow::Result<SemanticIndex> {
    let index = scip::types::Index::parse_from_bytes(bytes)?;

    let mut classes: Vec<ClassSemanticInfo> = Vec::new();
    let mut classes_by_file: HashMap<String, Vec<ClassSemanticInfo>> = HashMap::new();
    let mut files_indexed = 0;

    for doc in &index.documents {
        if doc.relative_path.is_empty() {
            continue;
        }
        files_indexed += 1;
        // Normalize: SCIP Windows index'ler backslash kullanır, pipeline forward-slash arar.
        // Tüm path separator'ları forward-slash'a normalize et (cross-platform matching).
        let rel_path = doc.relative_path.replace('\\', "/");

        // Collect symbol definitions + occurrences for this document
        let mut class_defs: Vec<(String, String)> = Vec::new(); // (symbol, display_name)
        let mut method_defs: Vec<(String, String, Vec<i32>)> = Vec::new(); // (symbol, name, range)
        let mut field_defs: Vec<(String, String)> = Vec::new(); // (symbol, display_name)

        // SymbolInformation: definitions
        for sym_info in &doc.symbols {
            let symbol = &sym_info.symbol;
            let display = if sym_info.display_name.is_empty() {
                extract_last_segment(symbol)
            } else {
                sym_info.display_name.clone()
            };

            let kind_val = sym_info.kind.value();
            // SCIP SymbolInformation.Kind enum values (scip.proto — SourceGraph):
            // Class-like (LCOM4 subjects — concrete types with method bodies):
            //   7=Class, 49=Struct, 53=Trait, 56=TypeClass, 75=SingletonClass, 33=Object
            // Method-like:
            //   26=Method, 9=Constructor, 66=AbstractMethod, 68=ProtocolMethod,
            //   69=PureVirtualMethod, 70=TraitMethod, 71=TypeClassMethod,
            //   74=MethodAlias, 76=SingletonMethod, 80=StaticMethod
            // Field-like:
            //   15=Field, 41=Property, 77=StaticDataMember, 79=StaticField,
            //   81=StaticProperty, 18=Getter, 45=Setter
            // Ref: scip 0.5 proto — SymbolInformation.Kind enum
            //
            // FALLBACK: bazı indexer'lar (scip-typescript) kind=0 (UnspecifiedKind) bırakır.
            // Bu durumda sembol string'inden descriptor suffix çıkarımı yap.
            let inferred = if kind_val == 0 {
                infer_kind_from_symbol(symbol)
            } else {
                InferredKind::from_kind_value(kind_val)
            };
            match inferred {
                InferredKind::Class => {
                    class_defs.push((symbol.clone(), display));
                }
                InferredKind::Method => {
                    method_defs.push((symbol.clone(), display, vec![]));
                }
                InferredKind::Field => {
                    field_defs.push((symbol.clone(), display));
                }
                InferredKind::Other => {}
            }
        }

        // Occurrences: method definition range'lerini topla (sadece Method sembolleri)
        for occ in &doc.occurrences {
            let is_definition = occ.symbol_roles & 1 != 0;
            if !is_definition {
                continue;
            }
            let symbol = &occ.symbol;
            // Sadece method sembolleri topla — class/field/parameter değil
            if infer_kind_from_symbol(symbol) != InferredKind::Method {
                continue;
            }
            let name = extract_last_segment(symbol);
            let range = occ.range.clone();
            if let Some(m) = method_defs.iter_mut().find(|(s, _, _)| s == symbol) {
                m.2 = range;
            } else {
                method_defs.push((symbol.clone(), name, range));
            }
        }

        // For each class: find its methods, fields, and field-accesses
        for (class_symbol, class_name) in &class_defs {
            // SCIP sembol desenleri dil-bazlı farklılık gösterir:
            // - Python/TS/Rust-trait: `...ClassName#member` → starts_with(class_symbol)
            // - Rust impl-block: `...path/impl#[ClassName][Trait]method().` → ayrı desen
            //   (struct method'ları trait method'larından farklı namespace'te)
            // `symbol_belongs_to_class` her iki deseni de ele alır.
            let class_methods: Vec<(String, Vec<i32>)> = method_defs
                .iter()
                .filter(|(s, _, _)| symbol_belongs_to_class(s, class_symbol))
                .map(|(_, name, range)| (name.clone(), range.clone()))
                .collect();

            let class_fields: Vec<String> = field_defs
                .iter()
                .filter(|(s, _)| symbol_belongs_to_class(s, class_symbol))
                .map(|(_, name)| name.clone())
                .collect();

            // Field access: for each method, find field references within its body.
            //
            // SCIP method definition range sadece method İMZASINI kapsar (örn [3,2,9] =
            // "addItem" identifier'ı line 3 col 2-9). Method BODY'nin range'i verilmez.
            // Çözüm: method'ları definition line'a göre sırala, her method'un body'si
            // kendisinden sonraki method'un definition line'ına kadar uzanır.
            let mut field_accesses = Vec::new();

            // Method'ları start line'a göre sırala
            let mut methods_by_line: Vec<(String, Vec<i32>)> = class_methods
                .iter()
                .map(|(name, range)| (name.clone(), range.clone()))
                .collect();
            methods_by_line.sort_by_key(|(_, range)| range.first().copied().unwrap_or(0));

            for (idx, (method_name, method_range)) in methods_by_line.iter().enumerate() {
                let method_start = method_range.first().copied().unwrap_or(0);
                // Body: bu method'dan sonraki method'a kadar (son method → dosya sonu)
                let method_end = if idx + 1 < methods_by_line.len() {
                    methods_by_line[idx + 1]
                        .1
                        .first()
                        .copied()
                        .unwrap_or(i32::MAX)
                        - 1
                } else {
                    i32::MAX // last method: body extends to end of file
                };

                for occ in &doc.occurrences {
                    let occ_symbol = &occ.symbol;
                    let suffix = occ_symbol.rsplit(' ').next().unwrap_or(occ_symbol);

                    // Yalnızca gerçek field occurrence'ları say — type/method/param/impl-block
                    // referanslarını ele. Sıralama önemli: daha spesifik desenler önce.
                    //
                    // Atlananlar:
                    //   `Type#` (# ile biten) → type/class referansı (örn `Error::new`)
                    //   `...()` veya `...method().` → method (def veya çağrı)
                    //   `(param)` → parameter referansı
                    //   `impl#[Type][Trait]` (impl-block type) → impl referansı
                    // Kalan: `Type#field.` → field access (definition veya read).
                    if suffix.ends_with('#') {
                        continue;
                    }
                    if suffix.ends_with("().") {
                        continue;
                    }
                    if suffix.ends_with(')') && !suffix.ends_with("()") {
                        continue;
                    }
                    // impl-block type sembolü: `...impl#[Type][Trait]` (method() değil)
                    // Method çağrısı zaten yukarıda `().` ile elendi; burada type-ref kalır.
                    if suffix.contains("impl#[") && !suffix.ends_with("().") {
                        continue;
                    }

                    // Sadece bu class'ın field'larına erişimleri say
                    // (definition = self.x = ... write, reference = self.x read — ikisi de access)
                    // Hem `ClassName#field.` (desen 1) hem `impl#[ClassName]...field` (desen 2)
                    // desteklenir — trait'te tanımlı field yoktur ama struct'ta her ikisi olabilir.
                    if !symbol_belongs_to_class(occ_symbol, class_symbol) {
                        continue;
                    }
                    let occ_line = occ.range.first().copied().unwrap_or(-1);
                    if occ_line >= method_start && occ_line <= method_end {
                        let field_name = extract_last_segment(occ_symbol);
                        field_accesses.push(FieldAccess {
                            method: method_name.clone(),
                            field: field_name,
                        });
                    }
                }
            }

            if !class_methods.is_empty() || !class_fields.is_empty() {
                let info = ClassSemanticInfo {
                    name: class_name.clone(),
                    methods: class_methods.iter().map(|(n, _)| n.clone()).collect(),
                    fields: class_fields,
                    field_access: field_accesses,
                };
                classes.push(info.clone());
                classes_by_file
                    .entry(rel_path.clone())
                    .or_default()
                    .push(info);
            }
        }
    }

    Ok(SemanticIndex {
        classes,
        classes_by_file,
        files_indexed,
        files_total: files_indexed,
    })
}

/// SCIP symbol string'inden son identifier segmenti çıkar.
///
/// SCIP descriptor suffix'leri: `#` (Type), `.` (Term/field), `()` (Method),
/// `(name)` (Parameter), `/` (Package/path).
///
/// **Rust impl-block deseni:** rust-analyzer, `impl` bloklarındaki method'ları
/// `...path/impl#[Type][Trait]method().` olarak yazar. `[Type][Trait]` segmentleri
/// method adından önce gelir. Bu segmentler atlanır, son `]` sonrası alınır.
fn extract_last_segment(symbol: &str) -> String {
    let last_segment = symbol.rsplit(' ').next().unwrap_or(symbol);

    // Parameter: `(identifier)` — içeride basit bir identifier varsa
    if last_segment.ends_with(')') {
        if let Some(open) = last_segment.rfind('(') {
            let inside = &last_segment[open + 1..last_segment.len() - 1];
            if !inside.is_empty()
                && !inside.contains('.')
                && !inside.contains('(')
                && !inside.contains(')')
                && !inside.contains('#')
            {
                return inside.to_string();
            }
        }
    }

    // Trailing descriptor suffix'leri strip et
    let stripped = last_segment
        .trim_end_matches('#') // Type suffix
        .trim_end_matches("().") // Method suffix
        .trim_end_matches('.') // Term suffix
        .trim_end_matches('/'); // Package suffix

    // Rust impl-block: `path/impl#[Type][Trait]method` deseni. `[...]` köşeli
    // parantez gruplarını atla, son `]` sonrasındaki identifier'ı al.
    // (Trait adı yoksa `impl#[Type]method`; her ikisi de olabilir.)
    if let Some(impl_pos) = stripped.find("impl#") {
        let after_impl = &stripped[impl_pos + "impl#".len()..];
        // `[...]` gruplarını atla — eşleşen `]` bul
        let mut rest = after_impl;
        while rest.starts_with('[') {
            if let Some(close) = rest.find(']') {
                rest = &rest[close + 1..];
            } else {
                break; // unmatched bracket — leave as-is
            }
        }
        // `rest` artık `method` (method adı) veya boş (impl-block type sembolü)
        if !rest.is_empty() && !rest.starts_with('[') {
            return strip_generics(rest);
        }
        // rest boşsa → bu bir impl-block *type* sembolü (`impl#[Type][Trait]`);
        // type adını döndürmek için ilk köşeli parantezin içeriğini al.
        if let Some(first_close) = after_impl.find(']') {
            let type_name = &after_impl[1..first_close];
            return strip_generics(type_name);
        }
    }

    // Son `#` veya `/` delimiter'ından sonraki kısım = identifier
    if let Some(pos) = stripped.rfind(|c: char| c == '#' || c == '/') {
        stripped[pos + 1..].to_string()
    } else {
        stripped.to_string()
    }
}

/// Angle-bracket generic parametrelerini strip et: `Type<E>` → `Type`.
/// rust-analyzer backtick içine alınmış generic tipleri kullanır: `` `Type<E>` ``
/// → backtick + generic strip → `Type`.
fn strip_generics(name: &str) -> String {
    let unbacked = name.trim_matches('`');
    if let Some(lt) = unbacked.find('<') {
        unbacked[..lt].to_string()
    } else {
        unbacked.to_string()
    }
}

/// Bir sembolün belirli bir class'a (type) ait olup olmadığını kontrol et.
///
/// İki SCIP deseni desteklenir:
/// 1. **Python/TS/Rust-trait:** `...ClassName#member.` veya `...ClassName#method().`
///    → `symbol.starts_with(class_symbol)` (class_symbol `#` ile biter).
/// 2. **Rust impl-block:** `...path/impl#[ClassName][Trait]method().`
///    → `[ClassName]` segmenti class adıyla eşleşmeli (generic + backtick strip sonrası).
///
/// `class_symbol`'den class adı çıkarılır, sonra sembolün deseni kontrol edilir.
/// Bu, struct field'ları için (desen 1) ve impl method'ları için (desen 2) doğru
/// matching sağlar — iki desen asla karışmaz.
fn symbol_belongs_to_class(symbol: &str, class_symbol: &str) -> bool {
    // Desen 1: doğrudan prefix (Python/TS/trait methodları + struct field'ları)
    if symbol.starts_with(class_symbol) {
        return true;
    }

    // Desen 2: Rust impl-block — class adını çıkar, `impl#[ClassName]` desenini kontrol et
    // class_symbol: `...path/ClassName#` → class adı = son `/` ile `#` arası
    let class_name = match class_symbol.rfind('/') {
        Some(slash) => &class_symbol[slash + 1..class_symbol.len().saturating_sub(1)], // strip trailing '#'
        None => &class_symbol[..class_symbol.len().saturating_sub(1)],
    };
    if class_name.is_empty() {
        return false;
    }

    // Sembolde `impl#[` ara; hemen sonrasındaki `[...]` type adını karşılaştır.
    // Hem `impl#[ClassName]` hem backtick'li `` impl#[`ClassName<E>`] `` desenleri.
    let needle_plain = format!("impl#[{class_name}]");
    let needle_backtick = format!("impl#[`{class_name}<");
    symbol.contains(&needle_plain) || symbol.contains(&needle_backtick)
}

/// Synthetic SCIP index oluştur (test için).
pub fn build_synthetic_index(classes: Vec<ClassSemanticInfo>) -> SemanticIndex {
    SemanticIndex {
        classes,
        files_indexed: 1,
        files_total: 1,
        ..Default::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn class_info(
        name: &str,
        methods: &[&str],
        fields: &[&str],
        accesses: &[(&str, &str)],
    ) -> ClassSemanticInfo {
        ClassSemanticInfo {
            name: name.into(),
            methods: methods.iter().map(|s| s.to_string()).collect(),
            fields: fields.iter().map(|s| s.to_string()).collect(),
            field_access: accesses
                .iter()
                .map(|(m, f)| FieldAccess {
                    method: m.to_string(),
                    field: f.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn extract_last_segment_symbol() {
        // Type (class) — `#` suffix
        assert_eq!(
            extract_last_segment("scip npm . . path/File.ts/ClassName#"),
            "ClassName"
        );
        // Method — `().` suffix
        assert_eq!(
            extract_last_segment("scip npm . . path/File.ts/ClassName#methodName()."),
            "methodName"
        );
        // Field — `.` suffix
        assert_eq!(
            extract_last_segment("scip npm . . path/File.ts/ClassName#fieldName."),
            "fieldName"
        );
        // Parameter — `(paramName)` suffix
        assert_eq!(
            extract_last_segment("scip npm . . path/File.ts/ClassName#methodName().(paramName)"),
            "paramName"
        );
        // Bare identifier (no descriptor suffix)
        assert_eq!(
            extract_last_segment("python pkg.mod ClassName"),
            "ClassName"
        );
    }

    #[test]
    fn build_synthetic_and_compute_lcom4() {
        // Build synthetic index with known class structure
        let index = build_synthetic_index(vec![
            class_info(
                "Cohesive",
                &["a", "b", "c"],
                &["x", "y"],
                &[
                    ("a", "x"),
                    ("b", "y"),
                    ("c", "x"),
                    ("c", "y"), // c bridges → LCOM4=1
                ],
            ),
            class_info(
                "Fragmented",
                &["a", "b"],
                &["x", "y"],
                &[
                    ("a", "x"),
                    ("b", "y"), // no bridge → LCOM4=2
                ],
            ),
        ]);

        assert!(index.is_available());
        assert_eq!(index.classes.len(), 2);

        let results = crate::scip::lcom4::compute_all_lcom4(&index);
        assert_eq!(results[0].1.lcom4, 1, "Cohesive → LCOM4=1");
        assert_eq!(results[1].1.lcom4, 2, "Fragmented → LCOM4=2");
    }

    #[test]
    fn parse_real_scip_bytes_if_available() {
        // This test only runs if a real .scip file is available
        let scip_path = std::path::Path::new("test.scip");
        if !scip_path.exists() {
            eprintln!("Skipping real SCIP test (no test.scip file)");
            return;
        }

        let result = load_scip_index(scip_path);
        assert!(result.is_ok(), "SCIP parse should succeed");
        let index = result.unwrap();
        assert!(index.files_indexed > 0, "Should have indexed files");
    }

    /// End-to-end: synthetic SCIP → SemanticIndex → LCOM4 → MetricValue
    #[test]
    fn end_to_end_synthetic_lcom4_pipeline() {
        // Simulate a real Python class with field access
        let index = build_synthetic_index(vec![class_info(
            "Article",
            &["__init__", "save", "get_summary"],
            &["title", "body", "tags"],
            &[
                ("__init__", "title"),
                ("__init__", "body"),
                ("__init__", "tags"), // __init__ accesses all → bridges
                ("save", "title"),
                ("save", "body"),
                ("get_summary", "body"),
            ],
        )]);

        let results = crate::scip::lcom4::compute_all_lcom4(&index);
        let lcom4_results: Vec<_> = results.iter().map(|(_, r)| r.clone()).collect();
        let cohesion_mv = crate::scip::lcom4::module_cohesion(&lcom4_results);

        // Article is cohesive (__init__ bridges) → LCOM4=1 → cohesion=1.0
        assert_eq!(results[0].1.lcom4, 1);
        assert!((cohesion_mv.value - 1.0).abs() < 1e-9);
        assert_eq!(cohesion_mv.source, crate::contract::MetricSource::Scip);
    }

    // ───────────────────────────────────────────────────────────────────────
    // Rust impl#[Type] method-matching fix (#4) — scip-rust toolchain.
    // Kök neden: rust-analyzer struct method'larını `impl#[Type]...method().`
    // deseniyle yazar; prefix matching struct'lar için method'ları kaçırıyordu.
    // ───────────────────────────────────────────────────────────────────────

    #[test]
    fn extract_last_segment_rust_impl_block_method() {
        // rust-analyzer impl-block method: `path/impl#[Type][Trait]method().`
        // Method adı = `]` gruplarından sonraki segment
        assert_eq!(
            extract_last_segment("scip cargo crate 1.0 path/impl#[Error][Error]custom()."),
            "custom"
        );
        // Trait'siz impl: `impl#[Type]method().`
        assert_eq!(
            extract_last_segment("scip cargo crate 1.0 path/impl#[Config]new()."),
            "new"
        );
        // Backtick + generic: `` impl#[`Type<E>`]method(). `` → method adı
        assert_eq!(
            extract_last_segment("scip cargo crate 1.0 path/impl#[`Deserializer<E>`]new()."),
            "new"
        );
    }

    #[test]
    fn extract_last_segment_rust_impl_block_type() {
        // impl-block *type* sembolü: `impl#[Type][Trait]` (method değil) → type adı
        assert_eq!(
            extract_last_segment("scip cargo crate 1.0 path/impl#[Error][Error]"),
            "Error"
        );
        // Backtick + generic → generic parametre strip edilir
        assert_eq!(
            extract_last_segment("scip cargo crate 1.0 path/impl#[`Deserializer<E>`]"),
            "Deserializer"
        );
    }

    #[test]
    fn symbol_belongs_to_class_python_trait_pattern() {
        // Desen 1: `Type#member` (Python/TS/Rust-trait) — starts_with(class_symbol)
        let class = "scip npm . . path/File.ts/ClassName#";
        assert!(symbol_belongs_to_class(
            "scip npm . . path/File.ts/ClassName#method().",
            class
        ));
        assert!(symbol_belongs_to_class(
            "scip npm . . path/File.ts/ClassName#field.",
            class
        ));
    }

    #[test]
    fn symbol_belongs_to_class_rust_impl_block_pattern() {
        // Desen 2: Rust impl-block — `impl#[ClassName]...`
        let class = "scip cargo crate 1.0 path/ClassName#";
        assert!(symbol_belongs_to_class(
            "scip cargo crate 1.0 path/impl#[ClassName][Trait]method().",
            class
        ));
        // Trait'siz impl
        assert!(symbol_belongs_to_class(
            "scip cargo crate 1.0 path/impl#[ClassName]new().",
            class
        ));
    }

    #[test]
    fn symbol_belongs_to_class_rust_generic_impl() {
        // Backtick + generic: `` impl#[`ClassName<E>`]... ``
        let class = "scip cargo crate 1.0 path/ClassName#";
        assert!(symbol_belongs_to_class(
            "scip cargo crate 1.0 path/impl#[`ClassName<E>`][Trait]method().",
            class
        ));
    }

    #[test]
    fn symbol_belongs_to_class_rejects_different_class() {
        // Farklı class'ın method'u → false (negative case)
        let class = "scip cargo crate 1.0 path/ClassName#";
        assert!(!symbol_belongs_to_class(
            "scip cargo crate 1.0 path/impl#[OtherClass][Trait]method().",
            class
        ));
        // Sibling struct: prefix çakışması olmamalı
        let class_a = "scip cargo crate 1.0 path/ClassA#";
        assert!(!symbol_belongs_to_class(
            "scip cargo crate 1.0 path/impl#[ClassAB][Trait]method().",
            class_a
        ));
    }

    #[test]
    fn symbol_belongs_to_class_field_still_works_for_rust() {
        // Struct field: `Type#field.` (desen 1) — impl-block değişikliği field'ları bozmamalı
        let class = "scip cargo crate 1.0 path/Config#";
        assert!(symbol_belongs_to_class(
            "scip cargo crate 1.0 path/Config#host.",
            class
        ));
    }

    /// Regression: gerçek SCIP protobuf bytes üretip Rust impl-block deseniyle
    /// bir struct'ın hem method hem field gösterdiğini ve gerçek LCOM4 hesaplandığını doğrula.
    /// Öncesi (bug): method'lar kaçırılırdı → methods=0 → LCOM4=1 (placeholder).
    #[test]
    fn parse_scip_bytes_rust_impl_block_struct_gets_methods_and_fields() {
        // rust-analyzer SCIP formatında küçük bir index kur.
        // Struct `Counter { count: u32 }` + `impl Counter { new(), inc(), get() }`.
        let index = build_test_scip_index_rust_counter();
        let parsed = parse_scip_bytes(&index).expect("SCIP parse should succeed");

        // Counter class'ı bulun
        let counter = parsed
            .classes
            .iter()
            .find(|c| c.name == "Counter")
            .expect("Counter class should be extracted");

        // Öncesi: methods=0 (impl#[Counter] method'ları kaçırılırdı)
        // Sonrası (fix): methods içinde new/inc/get olmalı
        assert!(
            counter.methods.iter().any(|m| m == "inc"),
            "inc() method should be detected via impl#[Counter] pattern; got: {:?}",
            counter.methods
        );
        assert!(
            counter.methods.contains(&"new".to_string()),
            "new() method should be detected; got: {:?}",
            counter.methods
        );
        assert!(
            counter.fields.iter().any(|f| f == "count"),
            "count field should be detected; got: {:?}",
            counter.fields
        );
        // field_access dolu olmalı: inc() count'a erişiyor, get() count'a erişiyor
        assert!(
            !counter.field_access.is_empty(),
            "field_access should be non-empty after fix; got {} accesses",
            counter.field_access.len()
        );
        assert!(
            counter
                .field_access
                .iter()
                .any(|fa| fa.method == "inc" && fa.field == "count"),
            "inc() → count access should be detected; got: {:?}",
            counter.field_access
        );
    }

    /// Regression: Rust struct fix sonrası gerçek LCOM4 > 1 hesaplanabilmeli
    /// (öncesi tüm Rust class'ları LCOM4=1 placeholder'a düşerdi).
    #[test]
    fn parse_scip_bytes_rust_struct_real_lcom4_above_one() {
        // Fragmented struct: iki method iki ayrı field'a erişir, köprü yok → LCOM4=2
        let index = build_test_scip_index_rust_fragmented();
        let parsed = parse_scip_bytes(&index).expect("SCIP parse should succeed");

        let frag = parsed
            .classes
            .iter()
            .find(|c| c.name == "Fragmented")
            .expect("Fragmented class should be extracted");

        let result = crate::scip::lcom4::compute_lcom4(frag);
        assert_eq!(
            result.lcom4,
            2,
            "two disconnected method-field groups → LCOM4=2; got methods={:?} fields={:?} accesses={:?}",
            frag.methods,
            frag.fields,
            frag.field_access
        );
    }

    // ───────────────────────────────────────────────────────────────────────
    // Test yardımcıları: gerçek rust-analyzer SCIP formatında synthetic index üret
    // (protobuf serialization — scip crate'inin mesaj derleyicilerini kullanır)
    // ───────────────────────────────────────────────────────────────────────

    /// `Counter { count }` + `impl Counter { new(), inc(), get() }` için SCIP bytes.
    fn build_test_scip_index_rust_counter() -> Vec<u8> {
        use scip::types::Document;
        let mut doc = Document::new();
        doc.relative_path = "src/counter.rs".into();
        doc.language = "Rust".into();

        let base = "scip cargo counter 1.0 src/counter/";

        // Class definition: Counter# (kind=49 Struct)
        doc.symbols
            .push(symbol_info(&format!("{base}Counter#"), "Counter", 49));
        // Field: Counter#count. (kind=15 Field) — definition occurrence L5
        doc.symbols
            .push(symbol_info(&format!("{base}Counter#count."), "count", 15));
        doc.occurrences
            .push(occurrence_def(&format!("{base}Counter#count."), 5, 4, 9));
        // impl methods: impl#[Counter]new()., impl#[Counter]inc()., impl#[Counter]get().
        for (mname, line) in [("new", 8), ("inc", 12), ("get", 17)] {
            let sym = format!("{base}impl#[Counter]{mname}().");
            doc.symbols.push(symbol_info(&sym, mname, 80));
            doc.occurrences
                .push(occurrence_def(&sym, line, 4, line_method_name_len(mname)));
        }

        // Occurrences: inc() body (L13-15) count'a erişir, get() body (L18-19) count'a erişir
        // Field access = REF (non-definition) on Counter#count. within method body line range
        doc.occurrences
            .push(occurrence_ref(&format!("{base}Counter#count."), 13)); // inc body
        doc.occurrences
            .push(occurrence_ref(&format!("{base}Counter#count."), 18)); // get body

        serialize_index(vec![doc])
    }

    /// `Fragmented { a, b }` + `impl Fragmented { use_a(), use_b() }` — LCOM4=2.
    fn build_test_scip_index_rust_fragmented() -> Vec<u8> {
        use scip::types::Document;
        let mut doc = Document::new();
        doc.relative_path = "src/frag.rs".into();
        doc.language = "Rust".into();
        let base = "scip cargo frag 1.0 src/frag/";

        doc.symbols
            .push(symbol_info(&format!("{base}Fragmented#"), "Fragmented", 49));
        doc.symbols
            .push(symbol_info(&format!("{base}Fragmented#a."), "a", 15));
        doc.symbols
            .push(symbol_info(&format!("{base}Fragmented#b."), "b", 15));
        doc.occurrences
            .push(occurrence_def(&format!("{base}Fragmented#a."), 5, 4, 5));
        doc.occurrences
            .push(occurrence_def(&format!("{base}Fragmented#b."), 6, 4, 5));
        for (mname, line) in [("use_a", 9), ("use_b", 14)] {
            let sym = format!("{base}impl#[Fragmented]{mname}().");
            doc.symbols.push(symbol_info(&sym, mname, 80));
            doc.occurrences
                .push(occurrence_def(&sym, line, 4, line_method_name_len(mname)));
        }
        // use_a body (L10-12) → a ; use_b body (L15-16) → b
        doc.occurrences
            .push(occurrence_ref(&format!("{base}Fragmented#a."), 10));
        doc.occurrences
            .push(occurrence_ref(&format!("{base}Fragmented#b."), 15));
        serialize_index(vec![doc])
    }

    fn symbol_info(symbol: &str, display: &str, kind: i32) -> scip::types::SymbolInformation {
        use protobuf::{Enum, EnumOrUnknown};
        use scip::types::{symbol_information::Kind, SymbolInformation};
        let mut si = SymbolInformation::new();
        si.symbol = symbol.into();
        si.display_name = display.into();
        si.kind = EnumOrUnknown::from(Kind::from_i32(kind).unwrap_or(Kind::UnspecifiedKind));
        si.documentation = vec![];
        si.relationships = vec![];
        si
    }

    fn occurrence_def(
        symbol: &str,
        line: i32,
        col_start: i32,
        col_end: i32,
    ) -> scip::types::Occurrence {
        use scip::types::SyntaxKind;
        let mut occ = scip::types::Occurrence::new();
        occ.range = vec![line, col_start, col_end];
        occ.symbol = symbol.into();
        occ.symbol_roles = 1; // Definition
        occ.syntax_kind = protobuf::EnumOrUnknown::from(SyntaxKind::UnspecifiedSyntaxKind);
        occ.diagnostics = vec![];
        occ.enclosing_range = vec![];
        occ.override_documentation = vec![];
        occ
    }

    fn occurrence_ref(symbol: &str, line: i32) -> scip::types::Occurrence {
        let mut occ = occurrence_def(symbol, line, 0, 1);
        occ.symbol_roles = 0; // Reference (non-definition)
        occ
    }

    fn line_method_name_len(name: &str) -> i32 {
        // col_end = col_start(4) + name.len() — identifier span
        4 + name.len() as i32
    }

    fn serialize_index(documents: Vec<scip::types::Document>) -> Vec<u8> {
        use protobuf::Message;
        use scip::types::{Index, Metadata, ProtocolVersion, TextEncoding, ToolInfo};
        let mut tool = ToolInfo::new();
        tool.name = "test-rust-analyzer".into();
        tool.version = "0.0.0-test".into();
        let mut meta = Metadata::new();
        meta.tool_info = protobuf::MessageField::some(tool);
        meta.version = protobuf::EnumOrUnknown::from(ProtocolVersion::UnspecifiedProtocolVersion);
        meta.project_root = "/test".into();
        meta.text_document_encoding = protobuf::EnumOrUnknown::from(TextEncoding::UTF8);
        let mut idx = Index::new();
        idx.metadata = protobuf::MessageField::some(meta);
        idx.documents = documents;
        idx.external_symbols = vec![];
        idx.write_to_bytes().expect("serialize test index")
    }
}
