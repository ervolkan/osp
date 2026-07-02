//! # OSP-Core
//!
//! **Ontological Space Protocol** çekirdek tipleri.
//! [`docs/OSP-formalism.md`] §1 (primitifler) ve §2 (koordinat sistemi)'nin Rust gerçeklemesi.
//!
//! ## Modüller (Faz 1.1)
//! - [`space`] — ontolojik primitifler: `Node`, `NodeKind`, `Edge`, `EdgeKind`, `Space`
//! - [`coords`] — pluggable `Axis` trait + `CoordinateSystem`
//!
//! ## Planlanan (Faz 1.2–1.7)
//! - `time` — `TimeLayer` durum-geçiş makinesi (§3)
//! - `witness` — workflow-agnostik şahitlik operatörü `W` (§4, Q1-Q3)
//! - `vision` — `VisionVector` + sapma `θ` (§5)
//! - `bigbang` — `apply_delta()` + gravity (§6, mutation-only — infallible)

pub mod agent;
pub mod anchoring;
pub mod axes;
pub mod bigbang;
pub mod coords;
pub mod engine;
pub mod navigator;
pub mod persistence;
pub mod rule;
pub mod space;
pub mod time;
pub mod trajectory;
pub mod vision;
pub mod vision_config;
pub mod witness;
