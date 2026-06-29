//! Ontolojik primitifler — `Node`, `Edge`, `Space` (OSP-formalism.md §1).

use std::collections::HashMap;

use crate::coords::Position;

/// Kararlı düğüm tanımlayıcı.
///
/// Faz 1: `u64` (sequential). Faz 2+: içerik-adresli hash (özgünlük + immutability).
pub type NodeId = u64;

/// Meta-ontoloji düğüm türleri (OSP-formalism.md §1.2).
///
/// Üst-ontoloji: yazılım-süreç modelini (Feature/Bug/Rule) + epistemolojik rolleri
/// (Agent/Intent/Claim/Witness) birleştirir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    /// Kaynak dosya / paket (Faz 0 spike seviyesi). Default tür.
    #[default]
    Module,
    /// Domain kavramı (DDD aggregate root gibi) — Faz 1+ AST + isim-analizi.
    Concept,
    /// Kullanıcı-görünür yetenek — Big Bang tetikleyicisi.
    Feature,
    /// Hata / negatif-uzay işareti (θ > θ_eşik'nin somut karşılığı).
    Bug,
    /// Mimari/domain kuralı — kütleçekim kaynağı (`G`'yi besler).
    Rule,
    /// AI-agent (LLM sürücüsü) — `Intent` üretir, `Claim` üretir.
    Agent,
    /// Agent'a verilen görev — **`t_f` (Gelecek) katmanında yaşar** (potansiyel gradyan;
    /// agent-prompt-semantics.md §0 ontolojik harita, OSP-formalism.md §1.2 + §3.1).
    Intent,
    /// Agent'ın ürettiği iş (PR) — `t_m`'de Belief → `t_c`'de Knowledge (witness sonrası).
    Claim,
    /// Onay/red veren kimlik — `W` operatörüne girdi.
    Witness,
}

/// Dosya-rolü sınıflandırması (`NodeKind`'ten ayrı bir eksen).
///
/// `NodeKind` formal ontolojidir (Module/Concept/Feature/.../Witness) ve bir node'un
/// uzaydaki *semantik rolünü* tanımlar. `NodeClassification` ise bir source-module'un
/// *dosya rolünü* tanımlar — bu dosya production mı, test mi, migration mı?
///
/// Bu ayrım mimari yorum için kritiktir: bir test dosyasının `instability = 1.0`
/// (incoming=0) olması doğaldır ve "risk" değildir; bir domain-core dosyasında aynı
/// değer ciddi bir alarm üretir. UI bu bilgiyle context-aware vision uyarıları gösterir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum NodeClassification {
    /// Domain/business logic (varsayılan). Yüksek instability burada ciddi alarm.
    #[default]
    Production,
    /// `tests/`, `test_*.py`, `*_test.go`, `*.test.ts` — incoming=0 doğal.
    Test,
    /// `conftest.py`, `/fixtures/`, `/factories/`, `/__mocks__/` — test altyapısı.
    Fixture,
    /// `/migrations/`, `alembic/`, `flyway` — schema değişikliği, genelde snapshot dışı.
    Migration,
    /// `settings.py`, `config.rs`, `.env`, `go.mod` — runtime/derleme konfigürasyonu.
    Config,
    /// `/scripts/`, `manage.py`, `build.rs`, `Makefile` — otomasyon/tooling.
    Script,
    /// `.generated.`, `.pb.go`, `/gen/`, `/build/`, `.min.js` — üretilmiş kod.
    Generated,
    /// `/docs/`, `*.md` (node olması nadir).
    Documentation,
    /// Yukarıdakine uymayan — eski snapshot'lar veya sınıflandırılamayan dosyalar.
    Unknown,
}

/// Path-rule-based classifier — dosya yolundan `NodeClassification` çıkarır.
///
/// Üretim varsayılan (Production); sonra en spesifik desen kazanır. Python/Rust/Go/
/// TypeScript/JavaScript path convention'larını kapsar. Path separator olarak `/`
/// bekler (pipeline zaten normalize ediyor).
pub fn classify_path(path: &str) -> NodeClassification {
    // Path'i lower-case + forward-slash normalize et (cross-platform dayanıklılık).
    let p = path.replace('\\', "/");
    let lower = p.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);

    // Generated (en spesifik — önce kontrol et, çünkü test_*.generated da olabilir)
    if base.contains(".generated.")
        || base.ends_with(".pb.go")
        || base.ends_with(".pb.rs")
        || base.ends_with(".min.js")
        || lower.contains("/gen/")
        || lower.contains("/build/")
        || lower.contains("/dist/")
        || lower.contains("/target/")
        || lower.contains("/vendor/")
        || lower.starts_with("dist/")
        || lower.starts_with("gen/")
        || lower.starts_with("build/")
    {
        return NodeClassification::Generated;
    }

    // Fixture (test altyapısı — test'lerden önce, daha spesifik)
    if base == "conftest.py"
        || lower.contains("/fixtures/")
        || lower.contains("/factories/")
        || lower.contains("/__mocks__/")
        || base.starts_with("fixture_")
        || base.starts_with("test_helper")
    {
        return NodeClassification::Fixture;
    }

    // Test (en yaygın convention'lar)
    if lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains("/__tests__/")
        || lower.contains("/spec/")
        || lower.starts_with("tests/")
        || lower.starts_with("test/")
        || lower.starts_with("__tests__/")
        || lower.starts_with("spec/")
        || base.starts_with("test_")
        || base.starts_with("test-")
        || base.ends_with("_test.go")
        || base.ends_with("_test.rs")
        || base.ends_with(".test.ts")
        || base.ends_with(".test.tsx")
        || base.ends_with(".test.js")
        || base.ends_with(".spec.ts")
        || base.ends_with(".spec.js")
        || base.ends_with("_test.py")
    {
        return NodeClassification::Test;
    }

    // Migration
    if lower.contains("/migrations/")
        || lower.contains("/alembic/")
        || lower.contains("/flyway/")
        || lower.contains("/migrate/")
        || lower.contains("migrations/")
        || lower.starts_with("migrations/")
        || lower.starts_with("alembic/")
        || lower.starts_with("db/migrate/")
    {
        return NodeClassification::Migration;
    }

    // Config
    if base == "settings.py"
        || base == "config.rs"
        || base == "config.go"
        || base == "config.ts"
        || base == "go.mod"
        || base == "go.sum"
        || base == "cargo.toml"
        || base == "package.json"
        || base == "tsconfig.json"
        || base == ".env"
        || base.ends_with(".toml")
        && (base == "config.toml" || base.contains("config"))
    {
        return NodeClassification::Config;
    }

    // Script
    if lower.contains("/scripts/")
        || base == "manage.py"
        || base == "build.rs"
        || base == "makefile"
        || base.ends_with(".sh")
        || base.ends_with(".ps1")
    {
        return NodeClassification::Script;
    }

    // Documentation
    if lower.contains("/docs/") || base.ends_with(".md") || base.ends_with(".rst") {
        return NodeClassification::Documentation;
    }

    NodeClassification::Production
}

// ═══════════════════════════════════════════════════════════════════════════════
// NodeRole — mimari rol (classification + metric shape'tan çıkarılır)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir node'un mimari rolü — `NodeClassification`'tan daha ince taneli.
///
/// Classification dosya *rolünü* söyler (test/production/migration); Role ise
/// mimari *şeklini* söyler (type surface, core, adapter, utility). Aynı
/// classification içinde farklı roller olabilir: bir `Production` dosyası
/// TypeSurface (.d.ts), Core (domain), Adapter (integration) veya Utility
/// (leaf helper) olabilir.
///
/// Role-aware vision her role için ayrı hedef vector kullanır — örn. TypeSurface
/// için coupling düşük beklenir (declaration = runtime deps yok), Core için
/// instability düşük beklenir (stable foundation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum NodeRole {
    /// Declaration/type surface — .d.ts, types/, interfaces (coupling low, cohesion high).
    TypeSurface,
    /// Domain core — business logic, stable foundation (instability low, cohesion high).
    Core,
    /// Integration boundary — adapters, API clients (instability high olabilir).
    Adapter,
    /// Leaf utility — helpers, low coupling, low incoming.
    Utility,
    /// Runtime implementation — default Production behavior.
    #[default]
    Runtime,
    /// Test/fixture/migration/generated — classification'tan inherit, advisory vision.
    Support,
}

/// Bir node'un mimari rolünü classification + path + metric shape'tan çıkar.
///
/// Heuristic kurallar (en spesifikten en genele):
/// - `.d.ts`, `types/`, `declarations/`, `interfaces/` → TypeSurface
/// - Support classification (Test/Fixture/Migration/Generated/Config/Script) → Support
/// - High mass + high incoming + low instability → Core (foundation)
/// - High outgoing + low incoming → Adapter (integration boundary)
/// - Low mass + low coupling → Utility (leaf helper)
/// - gerisi → Runtime (default Production)
///
/// `metrics` opsiyonel — yoksa sadece path/classification'a dayanır.
pub fn infer_role(
    path: &str,
    classification: NodeClassification,
    metrics: Option<RoleMetrics>,
) -> NodeRole {
    use NodeClassification as C;
    use NodeRole as R;

    let p = path.replace('\\', "/");
    let lower = p.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);

    // Support — test/fixture/migration/generated/config/script ÖNCE inherit edilir.
    //
    // Önemli sıralama kararı: bir dosya hem Support classification'a (örn. Test) hem de
    // TypeSurface path pattern'ine (örn. `tests/types/*.ts`, `.d.ts`) sahip olabilir.
    // Bu durumda Support kazanır — çünkü gate açısından bir test dosyasının tip tanımı
    // içermesi onu "production type surface" yapmaz; advisory vision'a (Support) tabi
    // olmalı, TypeSurface relaxation'ına değil. Önceki sıralama TypeSurface'i öne
    // aldığı için `tests/types/*` dosyaları yanlışça TypeSurface'a düşüyordu (svelte
    // diagnostic'inde 8 node — review'deki "TypeSurface 8 reject" şikayetinin kaynağı).
    // Epistemolojik kural: dosya *rolü* (test/migration) mimari *şeklinden* (type surface)
    // önce gelir; bir test dosyası production kurallarıyla değerlendirilmemelidir.
    match classification {
        C::Test | C::Fixture | C::Migration | C::Generated | C::Config | C::Script => {
            return R::Support;
        }
        _ => {}
    }

    // TypeSurface — en spesifik (declaration dosyaları). Yalnız Production dosyalar için
    // (Support yukarıda elendi). `.d.ts`, `types/`, `declarations/`, `interfaces/`.
    if base.ends_with(".d.ts")
        || lower.contains("/types/")
        || lower.contains("/declarations/")
        || lower.contains("/interfaces/")
        || lower.contains("/typings/")
    {
        return R::TypeSurface;
    }

    // Metric shape'e dayalı rol çıkarımı (varsa)
    if let Some(m) = metrics {
        // Core: high incoming (başka modüller buna bağlı) + stable (low instability)
        if m.incoming >= 5.0 && m.instability <= 0.3 {
            return R::Core;
        }
        // Adapter: high outgoing (çok şey import ediyor) + low incoming
        if m.outgoing >= 3.0 && m.incoming <= 1.0 {
            return R::Adapter;
        }
        // Utility: small + low coupling
        if m.mass <= 100.0 && m.outgoing <= 1.0 {
            return R::Utility;
        }
    }

    R::Runtime
}

/// Role çıkarımı için gerekli metric özeti (coupling/incoming/outgoing/mass).
/// infer_role'e opsiyonel olarak verilir — yoksa sadece path/classification'a bakar.
#[derive(Debug, Clone, Default)]
pub struct RoleMetrics {
    pub mass: f64,
    pub outgoing: f64,
    pub incoming: f64,
    pub instability: f64,
}

/// Tipleşmiş kenar türleri (OSP-formalism.md §1.3).
///
/// Klasik yazılım grafindan (yalnız `DependsOn`) farklı olarak OSP'de epistemolojik
/// ilişkileri (`Witnesses`, `Approves`, `Violates`) de modeller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum EdgeKind {
    /// `from` modülü `to` modülünü import ediyor (modül-düzeyi bağımlılık).
    #[default]
    Imports,
    /// `from` fonksiyonu `to` fonksiyonunu çağırıyor (fonksiyon-düzeyi).
    Calls,
    /// Genel bağımlılık (mimari, runtime).
    DependsOn,
    /// Mereolojik parça ilişkisi (`from` ∈ `to` aggregate).
    PartOf,
    /// Kalıtım / generalizasyon (is-a, subsumption).
    DerivesFrom,
    /// `from` witness'ı `to` claim'ini şahitlik ediyor.
    Witnesses,
    /// `from` witness'ı `to` claim'ini onaylıyor (`Witnesses` + verdict=Approve).
    Approves,
    /// `from` düğümü `to` kuralını ihlal ediyor (negatif-uzay sinyali).
    Violates,
}

/// Kavramsal uzay düğümü (OSP-formalism.md §1.1).
///
/// `Default`: `id=0, kind=Module, mass=0.0, position=[]` — builder/test kolaylığı için;
/// **`id` gerçek değerle override edilmeli** (insert sırasında).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Düğüm kütlesi `∈ ℝ⁺`. Faz 0: LOC. Faz 1: `α·LOC + β·AST + γ·in_degree`.
    pub mass: f64,
    /// `(x, y, z, w, v, u, ...)` koordinat — `CoordinateSystem::position_of` ile hesaplanır.
    pub position: Position,
    /// Analyzer-tarafından set edilen LCOM4 cohesion `∈ [0, 1]` (Faz 3.6+).
    /// `None` = ölçülmemiş → CohesionAxis fallback (0.5) kullanır.
    /// CouplingAxis/InstabilityAxis graf'ten compute ettiği için burada yok —
    /// sadece cohesion external (SCIP) veri gerektirir.
    #[serde(default)]
    pub cohesion: Option<f64>,
    /// Dosya-rolü sınıflandırması (test/production/migration/...). Context-aware
    /// mimari yorum için: örn. test dosyasında yüksek instability normaldir.
    /// Eski snapshot'lar `Unknown` default ile deserialize olur.
    #[serde(default)]
    pub classification: NodeClassification,
    /// Mimari rol (TypeSurface/Core/Adapter/Utility/Runtime/Support) —
    /// classification + metric shape'tan çıkarılır. Role-aware vision için.
    /// Eski snapshot'lar `Runtime` default ile deserialize olur.
    #[serde(default)]
    pub role: NodeRole,
}

/// Tipleşmiş yönlü kenar (OSP-formalism.md §1.3).
///
/// **Self-loop semantiği:** `from == to` bazı türler için anlamlı (`Calls` — rekürsiyon),
/// bazıları için değil (`Imports` — modül kendini import edemez; `Witnesses` — self-witness
/// reddi). Tür-bazlı self-loop validasyonu Faz 1.2/1.3 graf kurulumında eklenecek.
///
/// **Type-only import ayrımı:** `is_type_only` bayrağı TS `import type {Foo}` gibi
/// runtime dependency üretmeyen import'ları işaretler. CouplingAxis/InstabilityAxis
/// bu edge'leri *value-only* degree metotlarıyla (`out_degree_value`/`in_degree_value`)
/// dışlar — type-only import runtime coupling değildir. `#[serde(default)]` eski
/// snapshot'larla backward-compat sağlar (Node.classification pattern'i).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    /// TS `import type` / `import {type X}` ile üretilen type-only import mu?
    /// `true` → runtime dependency değil, coupling/instability'den hariç.
    /// Backward-compat: eski snapshot'larda yok → `false` (value import varsay).
    #[serde(default)]
    pub is_type_only: bool,
}

/// Kütleçekim vektörü — `Rule`'lardan gelen kısıt ağırlıkları (`ℝᵏ`).
///
/// Örn: `Rule = "Feature'lar Test olmadan var olamaz"` ihlali → ilgili düğümün
/// `gravity` değerini düşürür → negatif-uzay sinyali.
pub type GravityVector = Vec<f64>;

/// Zaman katmanı (OSP-formalism.md §3.1).
///
/// OSP'de zaman kronolojik sayaç değil **epistemolojik durum**'dur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TimeLayer {
    /// Miş'li (öznel, `t_m`) — agent'ın izole lokal uzayı: taslak iddialar, feature branch.
    Misli,
    /// Şimdiki (nesnel, `t_c`) — onaylanmış, kütleçekimli gerçek uzay: main branch.
    #[default]
    Simdiki,
    /// Gelecek (potansiyel, `t_f`) — henüz-doğmamış niyetler: issue'lar, roadmap.
    Gelecek,
}

/// Kavramsal uzay `S = (V, E, G, t_state)` (OSP-formalism.md §1.4).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Space {
    /// Düğüm kümesi `V`.
    pub nodes: HashMap<NodeId, Node>,
    /// Kenar kümesi `E ⊆ V × V × EdgeKind`.
    pub edges: Vec<Edge>,
    /// Kütleçekim `G: NodeId → ℝᵏ`. Faz 1: stored (manuel set). Faz 2: `Rule`'lerden computed.
    // TODO(Faz 2): `compute_gravity()` / `apply_rule()` — Rule düğümleri eklendiğinde gravity
    //              vektörleri otomatik güncellenmeli. Şu an pasif (manuel HashMap set).
    pub gravity: HashMap<NodeId, GravityVector>,
    /// Zaman katmanı `t_state`.
    pub time_layer: TimeLayer,
}

impl Space {
    /// Yeni boş uzay (default: `Simdiki` katman).
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            gravity: HashMap::new(),
            time_layer: TimeLayer::Simdiki,
        }
    }

    /// Düğüm ekle (id çakışmasında eskiyi overwrite).
    pub fn insert_node(&mut self, node: Node) -> &mut Self {
        self.nodes.insert(node.id, node);
        self
    }

    /// Kenar ekle.
    pub fn insert_edge(&mut self, edge: Edge) -> &mut Self {
        self.edges.push(edge);
        self
    }

    /// Düğüm sayısı `|V|`.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Kenar sayısı `|E|`.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Belirli türdeki kenar sayısı (ör. `Imports` için coupling hesabında).
    pub fn edge_count_of(&self, kind: EdgeKind) -> usize {
        self.edges.iter().filter(|e| e.kind == kind).count()
    }

    /// Bir düğümün in-degree'i (belirli tür için; `Witnesses`/`Approves` şahitlik derinliği).
    pub fn in_degree(&self, id: NodeId, kind: EdgeKind) -> usize {
        self.edges
            .iter()
            .filter(|e| e.to == id && e.kind == kind)
            .count()
    }

    /// Bir düğümün out-degree'i (belirli tür için).
    pub fn out_degree(&self, id: NodeId, kind: EdgeKind) -> usize {
        self.edges
            .iter()
            .filter(|e| e.from == id && e.kind == kind)
            .count()
    }

    /// Value-only out-degree — type-only import'lar (`is_type_only: true`) hariç.
    /// CouplingAxis/InstabilityAxis bu metodu kullanır: type-only import runtime
    /// dependency değildir, coupling'i artırmamalı. Diğer tüm `out_degree`
    /// çağrıcıları (agent slice, role refinement) etkilenmez — onlar tüm edge'leri sayar.
    pub fn out_degree_value(&self, id: NodeId, kind: EdgeKind) -> usize {
        self.edges
            .iter()
            .filter(|e| e.from == id && e.kind == kind && !e.is_type_only)
            .count()
    }

    /// Value-only in-degree — type-only import'lar hariç (InstabilityAxis Ca için).
    pub fn in_degree_value(&self, id: NodeId, kind: EdgeKind) -> usize {
        self.edges
            .iter()
            .filter(|e| e.to == id && e.kind == kind && !e.is_type_only)
            .count()
    }
}

impl Default for Space {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mod_node(id: NodeId, mass: f64) -> Node {
        Node {
            id,
            mass,
            ..Default::default()
        }
    }

    #[test]
    fn space_starts_empty_in_simdiki() {
        let s = Space::new();
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.edge_count(), 0);
        assert_eq!(s.time_layer, TimeLayer::Simdiki);
    }

    #[test]
    fn insert_nodes_and_edges() {
        let mut s = Space::new();
        s.insert_node(mod_node(1, 100.0));
        s.insert_node(mod_node(2, 50.0));
        s.insert_edge(Edge { from: 1, to: 2, kind: EdgeKind::Imports, ..Default::default() });
        s.insert_edge(Edge { from: 2, to: 1, kind: EdgeKind::Calls, ..Default::default() });
        assert_eq!(s.node_count(), 2);
        assert_eq!(s.edge_count(), 2);
        assert_eq!(s.edge_count_of(EdgeKind::Imports), 1);
        assert_eq!(s.edge_count_of(EdgeKind::Calls), 1);
    }

    #[test]
    fn in_out_degree_by_kind() {
        let mut s = Space::new();
        s.insert_node(mod_node(1, 1.0));
        s.insert_node(mod_node(2, 1.0));
        s.insert_node(mod_node(3, 1.0));
        // 1 → 2 (Imports), 3 → 2 (Imports), 1 → 3 (Calls)
        s.insert_edge(Edge { from: 1, to: 2, kind: EdgeKind::Imports, ..Default::default() });
        s.insert_edge(Edge { from: 3, to: 2, kind: EdgeKind::Imports, ..Default::default() });
        s.insert_edge(Edge { from: 1, to: 3, kind: EdgeKind::Calls, ..Default::default() });

        assert_eq!(s.in_degree(2, EdgeKind::Imports), 2);
        assert_eq!(s.out_degree(1, EdgeKind::Imports), 1);
        assert_eq!(s.in_degree(3, EdgeKind::Calls), 1);
        assert_eq!(s.in_degree(2, EdgeKind::Calls), 0); // farklı tür
    }

    #[test]
    fn node_kind_and_edge_kind_distinct() {
        assert_ne!(NodeKind::Module, NodeKind::Concept);
        assert_ne!(NodeKind::Feature, NodeKind::Bug);
        assert_ne!(NodeKind::Agent, NodeKind::Witness);
        assert_ne!(EdgeKind::Imports, EdgeKind::Calls);
        assert_ne!(EdgeKind::Witnesses, EdgeKind::Approves);
        assert_ne!(EdgeKind::PartOf, EdgeKind::DerivesFrom);
    }

    #[test]
    fn time_layer_default_is_simdiki() {
        assert_eq!(TimeLayer::default(), TimeLayer::Simdiki);
    }

    #[test]
    fn builder_chain_returns_self() {
        let mut s = Space::new();
        s.insert_node(mod_node(1, 1.0))
            .insert_node(mod_node(2, 1.0))
            .insert_edge(Edge {
                from: 1,
                to: 2,
                kind: EdgeKind::Calls,
                ..Default::default()
            });
        assert_eq!(s.node_count(), 2);
        assert_eq!(s.edge_count(), 1);
    }

    #[test]
    fn insert_node_overwrites_on_id_clash() {
        let mut s = Space::new();
        s.insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 100.0,
            ..Default::default()
        });
        s.insert_node(Node {
            id: 1,
            kind: NodeKind::Concept,
            mass: 50.0,
            ..Default::default()
        });
        assert_eq!(s.node_count(), 1, "aynı id overwrite etmeli, eklememeli");
        let n = s.nodes.get(&1).expect("id=1 mevcut olmalı");
        assert_eq!(n.kind, NodeKind::Concept, "overwrite son değeri tutmalı");
        assert!((n.mass - 50.0).abs() < 1e-9);
    }

    #[test]
    fn node_default_is_module_zero() {
        let n = Node::default();
        assert_eq!(n.id, 0);
        assert_eq!(n.kind, NodeKind::Module);
        assert!((n.mass).abs() < 1e-9);
        // Faz 1.4: position artık Position struct (raw + derived), ikisi de default
        assert_eq!(n.position.raw, crate::coords::RawPosition::default());
        assert_eq!(n.position.derived, crate::coords::DerivedPosition::default());
    }

    // ─── classify_path tests ───

    #[test]
    fn classify_path_test_files() {
        // Python conventions
        assert_eq!(classify_path("tests/test_foo.py"), NodeClassification::Test);
        assert_eq!(classify_path("app/test_models.py"), NodeClassification::Test);
        assert_eq!(classify_path("src/foo/test_bar.py"), NodeClassification::Test);
        // Go
        assert_eq!(classify_path("handler/handler_test.go"), NodeClassification::Test);
        // Rust
        assert_eq!(classify_path("src/lib_test.rs"), NodeClassification::Test);
        // TypeScript/JavaScript
        assert_eq!(classify_path("src/foo.test.ts"), NodeClassification::Test);
        assert_eq!(classify_path("src/bar.spec.js"), NodeClassification::Test);
        // __tests__ directory
        assert_eq!(classify_path("__tests__/unit.ts"), NodeClassification::Test);
    }

    #[test]
    fn classify_path_fixtures_and_config() {
        assert_eq!(classify_path("tests/conftest.py"), NodeClassification::Fixture);
        assert_eq!(classify_path("tests/fixtures/user.py"), NodeClassification::Fixture);
        assert_eq!(classify_path("src/__mocks__/db.ts"), NodeClassification::Fixture);
        // Config
        assert_eq!(classify_path("myapp/settings.py"), NodeClassification::Config);
        assert_eq!(classify_path("go.mod"), NodeClassification::Config);
        assert_eq!(classify_path("Cargo.toml"), NodeClassification::Config);
    }

    #[test]
    fn classify_path_migrations_scripts_generated() {
        assert_eq!(classify_path("app/migrations/0001_initial.py"), NodeClassification::Migration);
        assert_eq!(classify_path("alembic/versions/abc.py"), NodeClassification::Migration);
        assert_eq!(classify_path("db/migrate/001_create.rb"), NodeClassification::Migration);
        // Scripts
        assert_eq!(classify_path("scripts/deploy.sh"), NodeClassification::Script);
        assert_eq!(classify_path("manage.py"), NodeClassification::Script);
        // Generated
        assert_eq!(classify_path("api/foo.pb.go"), NodeClassification::Generated);
        assert_eq!(classify_path("dist/bundle.min.js"), NodeClassification::Generated);
        assert_eq!(classify_path("proto/foo.generated.rs"), NodeClassification::Generated);
    }

    #[test]
    fn classify_path_production_default() {
        // Normal source files → Production (default)
        assert_eq!(classify_path("src/models/user.py"), NodeClassification::Production);
        assert_eq!(classify_path("handler/handler.go"), NodeClassification::Production);
        assert_eq!(classify_path("src/lib.rs"), NodeClassification::Production);
        assert_eq!(classify_path("app/services/auth.ts"), NodeClassification::Production);
    }

    #[test]
    fn classify_path_cross_platform_separators() {
        // Windows backslash separators should be normalized (pipeline already
        // normalizes, but classify_path should be defensive).
        assert_eq!(
            classify_path("tests\\test_foo.py"),
            NodeClassification::Test
        );
        assert_eq!(
            classify_path("app\\migrations\\0001.py"),
            NodeClassification::Migration
        );
    }

    #[test]
    fn node_default_classification_is_production() {
        // Backward-compat: yeni Node'lar Production default alır.
        // (Eski snapshot'lar Unknown değil — serde default = Production.)
        let n = Node::default();
        assert_eq!(n.classification, NodeClassification::Production);
    }

    #[test]
    fn node_classification_serde_backward_compat() {
        // Eski snapshot (classification alanı YOK) deserialize → default.
        // "Unknown" enum değeri manuel set için kullanılabilir ama serde default
        // Production'dır (#[default]).
        // Gerçek Position serialize edip classification'ı çıkararak simüle et
        // (elle yazmak Position struct tüm alanlarına bağımlı olur).
        let mut full = Node::default();
        full.id = 1;
        let mut json_val: serde_json::Value =
            serde_json::to_value(&full).expect("serialize");
        // classification alanını çıkar → eski snapshot formatı
        json_val
            .as_object_mut()
            .expect("node is object")
            .remove("classification");
        let old_json = serde_json::to_string(&json_val).expect("re-serialize");
        assert!(
            !old_json.contains("classification"),
            "test setup: classification removed"
        );

        let n: Node = serde_json::from_str(&old_json).expect("deserialize old node");
        assert_eq!(
            n.classification,
            NodeClassification::Production,
            "missing classification field should default to Production"
        );
    }

    // ── infer_role regression test'leri ──────────────────────────────────────
    // Bu testler role/classification sıralama kararlarını kilitler. Özellikle
    // `support_beats_typesurface_for_test_files` bir bug'ın regression guard'ıdır:
    // eski sıralama `tests/types/*.ts` dosyalarını TypeSurface'a düşürüyordu (svelte
    // diagnostic'inde 8 node — "TypeSurface 8 reject" şikayetinin kaynağı).

    #[test]
    fn infer_role_support_beats_typesurface_for_test_files() {
        // Bir dosya hem Test classification'a hem TypeSurface path pattern'ine sahip
        // olabilir (örn. `packages/svelte/tests/types/actions.ts`). Bu durumda Support
        // kazanmalı — production type-surface relaxation'ı değil, advisory vision.
        use super::{infer_role, NodeClassification as C, NodeRole as R};
        let role = infer_role("packages/svelte/tests/types/actions.ts", C::Test, None);
        assert_eq!(
            role,
            R::Support,
            "test files under tests/types/ must be Support, not TypeSurface"
        );

        // Aynı desen `.d.ts` için de: bir `.d.ts` dosyası Test klasöründeyse Support.
        let role = infer_role("tests/mocks/types.d.ts", C::Test, None);
        assert_eq!(role, R::Support, ".d.ts under tests/ must be Support");
    }

    #[test]
    fn infer_role_typesurface_for_production_declaration_files() {
        // Production `.d.ts` / `types/` dosyaları TypeSurface olmalı (relaxation aktif).
        use super::{infer_role, NodeClassification as C, NodeRole as R};
        assert_eq!(
            infer_role("packages/svelte/elements.d.ts", C::Production, None),
            R::TypeSurface
        );
        assert_eq!(
            infer_role("src/types/index.ts", C::Production, None),
            R::TypeSurface
        );
        assert_eq!(
            infer_role("src/declarations/foo.ts", C::Production, None),
            R::TypeSurface
        );
    }

    #[test]
    fn infer_role_runtime_default_for_production() {
        // Production + metrics yok + path pattern yok → Runtime (default).
        use super::{infer_role, NodeClassification as C, NodeRole as R};
        assert_eq!(
            infer_role("src/index.js", C::Production, None),
            R::Runtime
        );
    }

    #[test]
    fn infer_role_metric_aware_refinement() {
        // Metric shape rolleri: Core (high incoming + stable), Adapter (high outgoing +
        // low incoming), Utility (small + low coupling).
        use super::{infer_role, NodeClassification as C, NodeRole as R, RoleMetrics};
        let core = RoleMetrics {
            incoming: 8.0,
            instability: 0.2,
            ..Default::default()
        };
        assert_eq!(
            infer_role("src/core/domain.js", C::Production, Some(core)),
            R::Core
        );

        let adapter = RoleMetrics {
            outgoing: 5.0,
            incoming: 0.0,
            ..Default::default()
        };
        assert_eq!(
            infer_role("src/adapters/api.js", C::Production, Some(adapter)),
            R::Adapter
        );

        let utility = RoleMetrics {
            mass: 40.0,
            outgoing: 1.0,
            ..Default::default()
        };
        assert_eq!(
            infer_role("src/utils/helper.js", C::Production, Some(utility)),
            R::Utility
        );
    }
}
