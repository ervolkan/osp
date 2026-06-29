//! SCIP semantic indexer + LCOM4 cohesion computation (Faz 3.6-3.7).
//!
//! **Two-layer design:**
//! - `SemanticIndex` — soyut veri yapısı (SCIP'ten bağımsız, test edilebilir)
//! - `lcom4` — LCOM4 algoritması (bipartite graph → connected components)
//! - SCIP loader (Faz 3.6+) — .scip dosyasını parse → SemanticIndex
//!
//! Bu tasarım LCOM4'ü SCIP olmadan da test etmemizi sağlar (synthetic data ile).

pub mod index;
pub mod lcom4;
pub mod loader;

pub use index::{ClassSemanticInfo, FieldAccess, SemanticIndex};
pub use loader::load_scip_index;
