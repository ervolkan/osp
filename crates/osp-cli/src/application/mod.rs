//! Application layer — review domain service + store repository.
//!
//! Subcommand adapter'ları ve interactive wizard aynı application service'i çağırır
//! → iki farklı davranış oluşmaz (Review 3#3).
//!
//! Bağımlılık yönü:
//! ```text
//! subcommand adapter ──┐
//!                      ├→ application service
//! interactive adapter ─┘
//! ```

pub mod repository;
pub mod review;

// Command adapter'lar ve interactive wizard bu tipleri kullanır.
pub(crate) use review::{ReviewApplicationService, ReviewReadOutput};
