//! LCOM4 cohesion computation (Faz 3.7).
//!
//! Bir class'ın method-field erişim grafindan connected components çıkarır.
//! LCOM4=1 → kohezif; LCOM4≥2 → fragmented.
//!
//! **Algorithm (§4.1):**
//! 1. Bipartite graph: methods ↔ fields, edge = method accesses field
//! 2. Connected components bul (union-find)
//! 3. LCOM4(C) = component count
//!
//! **Edge-case rules (§4.5):**
//! - 0 veya 1 method → LCOM4 = 1
//! - Field yok → LCOM4 = 1 (confidence düşük)
//! - Inherited methods → hariç (sadece kendi method'ları)
//! - Constructor field access → dahil
//! - Static method instance field → ayrı component

use super::index::{ClassSemanticInfo, SemanticIndex};
use crate::contract::MetricValue;

/// Bir class için LCOM4 sonucu.
#[derive(Debug, Clone)]
pub struct Lcom4Result {
    /// Connected component count (LCOM4 değeri).
    pub lcom4: usize,
    /// Method sayısı.
    pub method_count: usize,
    /// Field sayısı.
    pub field_count: usize,
    /// Field-access ilişki sayısı (bipartite edge count).
    pub access_count: usize,
}

impl Lcom4Result {
    /// `cohesion = 1 / max(lcom4, 1)` ∈ (0, 1].
    /// LCOM4=1 → cohesion=1.0 (tam kohezif).
    /// LCOM4=4 → cohesion=0.25.
    pub fn cohesion(&self) -> f64 {
        1.0 / self.lcom4.max(1) as f64
    }
}

/// Bir class için LCOM4 hesapla.
///
/// Bipartite graph (methods ∪ fields) üzerinde connected components bulur.
/// Union-Find (Disjoint Set Union) ile O(n·α(n)).
pub fn compute_lcom4(class: &ClassSemanticInfo) -> Lcom4Result {
    let methods = &class.methods;
    let fields = &class.fields;
    let accesses = &class.field_access;

    // §4.5 edge-case: 0 veya 1 method → LCOM4 = 1
    if methods.len() <= 1 {
        return Lcom4Result {
            lcom4: 1,
            method_count: methods.len(),
            field_count: fields.len(),
            access_count: accesses.len(),
        };
    }

    // §4.5 edge-case: field yok → LCOM4 = 1 (confidence düşük)
    if fields.is_empty() {
        return Lcom4Result {
            lcom4: 1,
            method_count: methods.len(),
            field_count: 0,
            access_count: 0,
        };
    }

    // Build union-find over methods ∪ fields
    // Map: method name → index [0..M), field name → index [M..M+F)
    let m = methods.len();
    let f = fields.len();
    let total = m + f;

    let mut method_idx: HashMap<&str, usize> = HashMap::new();
    for (i, method) in methods.iter().enumerate() {
        method_idx.insert(method.as_str(), i);
    }
    let mut field_idx: HashMap<&str, usize> = HashMap::new();
    for (i, field) in fields.iter().enumerate() {
        field_idx.insert(field.as_str(), m + i);
    }

    let mut dsu = Dsu::new(total);

    // For each field_access: union(method, field)
    let mut access_count = 0;
    for access in accesses {
        if let (Some(&mi), Some(&fi)) = (
            method_idx.get(access.method.as_str()),
            field_idx.get(access.field.as_str()),
        ) {
            dsu.union(mi, fi);
            access_count += 1;
        }
    }

    // §4.5: Static method instance field → ayrı component
    // (Bu durumda access yok → zaten ayrı kalır, doğal olarak)

    // Methods that don't access any field → isolated → separate component
    // (already isolated in DSU since no union was done for them)

    // Count connected components that contain at least one method
    let mut component_roots: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for i in 0..m {
        component_roots.insert(dsu.find(i));
    }

    let lcom4 = component_roots.len();

    Lcom4Result {
        lcom4,
        method_count: methods.len(),
        field_count: fields.len(),
        access_count,
    }
}

/// Bir SemanticIndex'teki tüm class'lar için LCOM4 hesapla.
///
/// Returns: (class_name, Lcom4Result) listesi.
pub fn compute_all_lcom4(index: &SemanticIndex) -> Vec<(String, Lcom4Result)> {
    index
        .classes
        .iter()
        .map(|c| (c.name.clone(), compute_lcom4(c)))
        .collect()
}

/// Bir modülün (dosyanın) cohesion değeri.
///
/// Weighted average of LCOM4 cohesion across classes:
/// `cohesion(F) = weighted_avg(1/LCOM4(Ci), weight = |Ci.methods|)`
///
/// Class yok → cohesion = 1.0 (convention: fonksiyon-only modül kohezif).
pub fn module_cohesion(classes: &[Lcom4Result]) -> MetricValue {
    if classes.is_empty() {
        return MetricValue::heuristic(1.0, 0.5); // convention + low confidence
    }

    let total_methods: usize = classes.iter().map(|c| c.method_count.max(1)).sum();
    let weighted_sum: f64 = classes
        .iter()
        .map(|c| c.cohesion() * c.method_count.max(1) as f64)
        .sum();

    let cohesion = weighted_sum / total_methods as f64;
    MetricValue::scip(cohesion, 1.0, false) // fresh SCIP, full coverage of these classes
}

// ═══════════════════════════════════════════════════════════════════════════════
// Union-Find (Disjoint Set Union)
// ═══════════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;

struct Dsu {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]); // path compression
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let px = self.find(x);
        let py = self.find(y);
        if px == py {
            return;
        }
        // Union by rank
        if self.rank[px] < self.rank[py] {
            self.parent[px] = py;
        } else if self.rank[px] > self.rank[py] {
            self.parent[py] = px;
        } else {
            self.parent[py] = px;
            self.rank[px] += 1;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler — known class structures (§4.1 example)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn class(
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
                .map(|(m, f)| crate::scip::FieldAccess {
                    method: m.to_string(),
                    field: f.to_string(),
                })
                .collect(),
        }
    }

    // --- §4.1 example: cohesive class (LCOM4=1) ---

    #[test]
    fn cohesive_class_validate_connects_groups() {
        // User { name, email; getName(), getEmail(), validate() }
        // validate accesses both name+email → connects → 1 component
        let c = class(
            "User",
            &["getName", "getEmail", "validate"],
            &["name", "email"],
            &[
                ("getName", "name"),
                ("getEmail", "email"),
                ("validate", "name"),
                ("validate", "email"), // ← connects both groups
            ],
        );
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 1, "validate connects → cohesive");
        assert!((result.cohesion() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fragmented_class_without_validate() {
        // Same class but without validate → 2 disconnected groups
        let c = class(
            "User",
            &["getName", "getEmail"],
            &["name", "email"],
            &[
                ("getName", "name"),
                ("getEmail", "email"),
                // no validate → no cross-connection
            ],
        );
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 2, "no bridge → 2 components");
        assert!((result.cohesion() - 0.5).abs() < 1e-9);
    }

    // --- Edge cases (§4.5) ---

    #[test]
    fn zero_methods_lcom4_one() {
        let c = class("Empty", &[], &["field1"], &[]);
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 1);
    }

    #[test]
    fn one_method_lcom4_one() {
        let c = class("Single", &["only"], &["f1", "f2"], &[("only", "f1")]);
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 1);
    }

    #[test]
    fn no_fields_lcom4_one() {
        let c = class("NoFields", &["m1", "m2", "m3"], &[], &[]);
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 1);
    }

    #[test]
    fn static_method_isolated() {
        // static_method doesn't access any instance field → separate component
        let c = class(
            "Mixed",
            &["instanceMethod", "staticMethod"],
            &["instanceField"],
            &[("instanceMethod", "instanceField")],
            // staticMethod has no access → isolated
        );
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 2, "static method isolated → 2 components");
    }

    #[test]
    fn constructor_connects() {
        // __init__ accesses all fields → connects everything
        let c = class(
            "Init",
            &["__init__", "getA", "getB"],
            &["a", "b"],
            &[
                ("__init__", "a"),
                ("__init__", "b"), // constructor bridges
                ("getA", "a"),
                ("getB", "b"),
            ],
        );
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 1, "constructor connects → cohesive");
    }

    // --- Complex scenario: 4 components ---

    #[test]
    fn four_components() {
        // 4 method-field pairs, no cross-connection → LCOM4=4
        let c = class(
            "Fragmented",
            &["m1", "m2", "m3", "m4"],
            &["f1", "f2", "f3", "f4"],
            &[("m1", "f1"), ("m2", "f2"), ("m3", "f3"), ("m4", "f4")],
        );
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 4);
        assert!((result.cohesion() - 0.25).abs() < 1e-9);
    }

    // --- Module cohesion aggregation ---

    #[test]
    fn module_cohesion_weighted_average() {
        // Class A: LCOM4=1 (3 methods, cohesion=1.0)
        // Class B: LCOM4=2 (1 method, cohesion=0.5)
        // Weighted: (1.0*3 + 0.5*1) / 4 = 0.875
        let classes = vec![
            Lcom4Result {
                lcom4: 1,
                method_count: 3,
                field_count: 2,
                access_count: 3,
            },
            Lcom4Result {
                lcom4: 2,
                method_count: 1,
                field_count: 2,
                access_count: 1,
            },
        ];
        let mv = module_cohesion(&classes);
        assert!(
            (mv.value - 0.875).abs() < 1e-9,
            "weighted cohesion = {}",
            mv.value
        );
        assert_eq!(mv.source, crate::contract::MetricSource::Scip);
    }

    #[test]
    fn module_no_classes_convention() {
        let mv = module_cohesion(&[]);
        assert!((mv.value - 1.0).abs() < 1e-9);
        assert_eq!(mv.source, crate::contract::MetricSource::Heuristic);
    }

    // --- compute_all_lcom4 ---

    #[test]
    fn compute_all_for_multiple_classes() {
        let index = SemanticIndex {
            classes: vec![
                class("A", &["m1"], &["f1"], &[("m1", "f1")]),
                class(
                    "B",
                    &["m1", "m2"],
                    &["f1", "f2"],
                    &[("m1", "f1"), ("m2", "f2")],
                ),
            ],
            files_indexed: 2,
            files_total: 2,
            ..Default::default()
        };
        let results = compute_all_lcom4(&index);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1.lcom4, 1); // A: 1 method → LCOM4=1
        assert_eq!(results[1].1.lcom4, 2); // B: 2 disconnected → LCOM4=2
    }

    // --- Real-world approximation: Python class ---

    #[test]
    fn python_django_model_like() {
        // Typical Django Model: fields + save() + clean() + __str__()
        // save() accesses multiple fields → connects
        let c = class(
            "Article",
            &["__str__", "save", "clean", "get_absolute_url"],
            &["title", "body", "published_at", "slug"],
            &[
                ("__str__", "title"),
                ("save", "title"),
                ("save", "body"),         // save bridges title+body
                ("save", "published_at"), // + published_at
                ("clean", "title"),
                ("clean", "body"), // clean also bridges
                // get_absolute_url accesses slug
                ("get_absolute_url", "slug"),
                // save also accesses slug (typical Django pattern)
                ("save", "slug"), // save connects ALL fields
            ],
        );
        let result = compute_lcom4(&c);
        assert_eq!(
            result.lcom4, 1,
            "save() connects all → Django models are cohesive"
        );
        assert!((result.cohesion() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn god_class_fragmented() {
        // God class: many unrelated method-field groups
        let c = class(
            "GodClass",
            &["handleUsers", "handleOrders", "handleReports", "handleAuth"],
            &["userDb", "orderDb", "reportQueue", "authToken"],
            &[
                ("handleUsers", "userDb"),
                ("handleOrders", "orderDb"),
                ("handleReports", "reportQueue"),
                ("handleAuth", "authToken"),
                // No cross-access → 4 completely separate responsibilities
            ],
        );
        let result = compute_lcom4(&c);
        assert_eq!(result.lcom4, 4, "God class → 4 fragmented responsibilities");
        assert!((result.cohesion() - 0.25).abs() < 1e-9, "very low cohesion");
    }
}
