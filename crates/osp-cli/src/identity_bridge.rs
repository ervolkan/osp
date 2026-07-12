// PR E2 — CLI canonical identity → core code identity köprüsü.
#![allow(dead_code)]

//! CLI canonical identity → core code identity köprüsü (PR E2).
//!
//! Tek mapping boundary: CLI [`CanonicalCodeIdentity`] (lexical normalize edilmiş display_path +
//! identity_key) → core [`CodeIdentityKey`] (scheme + canonicalize edilmiş key).
//!
//! # Duplication bilinçli (PR E core sahiplenir)
//! CLI [`PathCasePolicy`] ↔ core [`CodePathCasePolicy`] aynı varyantlar ama ayrı tipler. Bu modül
//! `From` impl + dönüştürme sağlar; CLI enum korunur (PR D anti-corruption prensibi).
//!
//! # AnalysisIdentityContext (tur 3 P2-1)
//! Scheme + policy tek propagation value altında gruplanır; accidental parameter divergence
//! azaltır. **Nihai koruma runtime [`IdentityBridgeError::CanonicalizationDrift`] check'tir**
//! (type-level engel iddiası DARALTILDI — `CanonicalCodeIdentity::new(path, policy)` ayrı policy
//! alır, bu yüzden mismatch type-level imkânsız DEĞİL; runtime fail-closed yakalanır).
//!
//! # Canonicalize çift-katmanı (zararsız, deterministic)
//! CLI `CanonicalCodeIdentity::new` zaten `AsciiCaseInsensitive → to_ascii_lowercase()` uygular.
//! Core `CodeIdentityKey::new` tekrar `canonicalize` eder. İkinci canonicalize idempotent —
//! zaten lowercase string tekrar lowercase'e geçer. Çift validation empty/control-check için
//! defense-in-depth; behavior değişmez.

use osp_core::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};

use crate::analysis_bridge::AnalysisIdentityScheme;
use crate::canonical_identity::{CanonicalCodeIdentity, CanonicalIdentityError, PathCasePolicy};

/// Scheme + policy tek context value (tur 3 P2-1).
///
/// Private fields + constructor → accidental parameter divergence azaltır. Nihai koruma runtime
/// drift check'tir (type-level engel iddiası DARALTILDI — `CanonicalCodeIdentity::new` ayrı policy
/// alır, bu yüzden bu context'i oluşturan caller policy'yi yanlış geçirebilir; drift check
/// yakalar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisIdentityContext {
    scheme: AnalysisIdentityScheme,
    path_case_policy: PathCasePolicy,
}

impl AnalysisIdentityContext {
    pub fn new(scheme: AnalysisIdentityScheme, path_case_policy: PathCasePolicy) -> Self {
        Self {
            scheme,
            path_case_policy,
        }
    }

    pub fn scheme(&self) -> AnalysisIdentityScheme {
        self.scheme
    }

    pub fn path_case_policy(&self) -> PathCasePolicy {
        self.path_case_policy
    }

    /// Context'in policy'si ile canonical identity üret (tek policy kaynağı — tur 3 P2-1).
    pub fn canonical_identity(
        self,
        path: &str,
    ) -> Result<CanonicalCodeIdentity, CanonicalIdentityError> {
        CanonicalCodeIdentity::new(path, self.path_case_policy)
    }
}

impl From<PathCasePolicy> for CodePathCasePolicy {
    fn from(policy: PathCasePolicy) -> Self {
        match policy {
            PathCasePolicy::CaseSensitive => CodePathCasePolicy::CaseSensitive,
            PathCasePolicy::AsciiCaseInsensitive => CodePathCasePolicy::AsciiCaseInsensitive,
        }
    }
}

/// Tur 2 P1-C — mapping drift (caller policy mismatch) typed error.
///
/// Tur 3 P1-1A: `Eq` derive ÇIKARILDI (core `CodeIdentityKeyError` Eq değil → derive hatası).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IdentityBridgeError {
    #[error("identity key empty/control rejected by core")]
    CoreValidation(#[from] osp_core::anchoring::identity::CodeIdentityKeyError),
    #[error(
        "canonicalization drift: cli key {cli_key:?} != core key {core_key:?} \
         (caller policy mismatch — use AnalysisIdentityContext)"
    )]
    CanonicalizationDrift {
        cli_key: String,
        core_key: String,
    },
}

/// CLI `CanonicalCodeIdentity` → core `CodeIdentityKey`.
///
/// # Pre-conditions (CLI sağlar — context üzerinden)
/// - `identity_key` lexical normalize edilmiş (`CanonicalCodeIdentity::new` path structural validation).
/// - `path_case_policy` identity üretimiyle AYNI context value (accidental divergence azaltır).
///
/// # Post-conditions (core sağlar)
/// - `CodeIdentityKey::new` empty/control-check + scheme canonicalize.
/// - `scheme = AnalysisPathV1 { case_policy }` (CLI `AnalysisIdentityScheme::PathV1` tek varyant).
///
/// # Tur 3 P1-C — CanonicalizationDrift
/// Core canonicalize sonrası `canonical_key() != identity.identity_key()` ise drift hatası.
/// CaseSensitive policy'de her zaman pass (lowercase yapılmaz); AsciiCaseInsensitive'de identity
/// zaten lowercase üretildiği için idempotent pass. Mismatch yalnız caller bug'ında (policy
/// propagation error — farklı policy ile identity üretilip context'e başka policy verilirse).
pub fn to_core_identity_key(
    identity: &CanonicalCodeIdentity,
    ctx: AnalysisIdentityContext,
) -> Result<CodeIdentityKey, IdentityBridgeError> {
    let core_scheme = match ctx.scheme {
        AnalysisIdentityScheme::PathV1 => CodeIdentityScheme::AnalysisPathV1 {
            case_policy: ctx.path_case_policy.into(),
        },
    };
    let cli_key = identity.identity_key();
    let core_key = CodeIdentityKey::new(core_scheme, cli_key)?;
    if core_key.canonical_key() != cli_key {
        return Err(IdentityBridgeError::CanonicalizationDrift {
            cli_key: cli_key.to_owned(),
            core_key: core_key.canonical_key().to_owned(),
        });
    }
    Ok(core_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(
        scheme: AnalysisIdentityScheme,
        policy: PathCasePolicy,
    ) -> AnalysisIdentityContext {
        AnalysisIdentityContext::new(scheme, policy)
    }

    // ── Mutlu yol + canonicalize idempotent ──────────────────────────────────

    #[test]
    fn to_core_identity_key_case_sensitive_passthrough() {
        let identity = CanonicalCodeIdentity::new("src/auth.rs", PathCasePolicy::CaseSensitive).unwrap();
        let key = to_core_identity_key(
            &identity,
            ctx(AnalysisIdentityScheme::PathV1, PathCasePolicy::CaseSensitive),
        )
        .unwrap();
        assert_eq!(key.canonical_key(), "src/auth.rs");
        // Drift check pass (CaseSensitive lowercase yapmaz).
        assert_eq!(key.canonical_key(), identity.identity_key());
    }

    #[test]
    fn to_core_identity_key_ascii_case_insensitive_lowercase_idempotent() {
        // CLI AsciiCaseInsensitive zaten lowercase üretti → core tekrar lowercase idempotent.
        let identity =
            CanonicalCodeIdentity::new("src/Auth.rs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        assert_eq!(identity.identity_key(), "src/auth.rs"); // CLI lowercase
        let key = to_core_identity_key(
            &identity,
            ctx(AnalysisIdentityScheme::PathV1, PathCasePolicy::AsciiCaseInsensitive),
        )
        .unwrap();
        assert_eq!(key.canonical_key(), "src/auth.rs");
        // Drift check pass (idempotent).
        assert_eq!(key.canonical_key(), identity.identity_key());
    }

    #[test]
    fn to_core_identity_key_control_char_rejects() {
        // CLI constructor NUL reject eder ama diğer control char'lar geçebilir; core reject.
        // \u{0001} (SOH) — control char ama NUL değil.
        let identity = CanonicalCodeIdentity::new("src/\u{0001}auth.rs", PathCasePolicy::CaseSensitive);
        // CLI constructor geçirebilir veya reddedebilir; her durumda core validation downstream.
        match identity {
            Ok(id) => {
                let err = to_core_identity_key(
                    &id,
                    ctx(AnalysisIdentityScheme::PathV1, PathCasePolicy::CaseSensitive),
                )
                .unwrap_err();
                assert!(matches!(
                    err,
                    IdentityBridgeError::CoreValidation(
                        osp_core::anchoring::identity::CodeIdentityKeyError::ControlCharacter
                    )
                ));
            }
            Err(_) => {
                // CLI constructor reject etti — empty test core'a bırakılır (tur 3 P1-D pattern).
            }
        }
    }

    // ── Enum mapping (exhaustive) ────────────────────────────────────────────

    #[test]
    fn path_case_policy_to_core_case_sensitive() {
        let policy: CodePathCasePolicy = PathCasePolicy::CaseSensitive.into();
        assert_eq!(policy, CodePathCasePolicy::CaseSensitive);
    }

    #[test]
    fn path_case_policy_to_core_ascii_case_insensitive() {
        let policy: CodePathCasePolicy = PathCasePolicy::AsciiCaseInsensitive.into();
        assert_eq!(policy, CodePathCasePolicy::AsciiCaseInsensitive);
    }

    #[test]
    fn analysis_scheme_path_v1_maps_to_core_analysis_path_v1_with_case_policy() {
        let identity = CanonicalCodeIdentity::new("src/x.rs", PathCasePolicy::CaseSensitive).unwrap();
        let key = to_core_identity_key(
            &identity,
            ctx(AnalysisIdentityScheme::PathV1, PathCasePolicy::CaseSensitive),
        )
        .unwrap();
        match key.scheme() {
            CodeIdentityScheme::AnalysisPathV1 { case_policy } => {
                assert_eq!(*case_policy, CodePathCasePolicy::CaseSensitive);
            }
        }
    }

    // ── Determinism ──────────────────────────────────────────────────────────

    #[test]
    fn to_core_identity_key_deterministic_round_trip() {
        let identity = CanonicalCodeIdentity::new("src/payment.rs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let context = ctx(AnalysisIdentityScheme::PathV1, PathCasePolicy::AsciiCaseInsensitive);
        let k1 = to_core_identity_key(&identity, context).unwrap();
        let k2 = to_core_identity_key(&identity, context).unwrap();
        assert_eq!(k1, k2);
        assert_eq!(k1.canonical_key(), k2.canonical_key());
        // Entity ID deterministic (core derive_resolved_code_entity_id).
        assert_eq!(k1.derive_entity_id(), k2.derive_entity_id());
        assert!(
            k1.derive_entity_id().0.starts_with("CodeEntity:"),
            "derived ID CodeEntity: prefix taşımalı"
        );
    }

    // ── Tur 3 P1-D — drift test (empty yerine; CLI constructor empty engeller) ──

    #[test]
    fn to_core_identity_key_rejects_policy_canonicalization_drift() {
        // Senaryo: identity AsciiCaseInsensitive ile üretilmiş (lowercase key), ama context
        // CaseSensitive verilmiş. Core CaseSensitive canonicalize yapmaz → canonical_key == cli_key.
        // Bu durumda drift YOK (CaseSensitive lowercase yapmaz). Bu yüzden gerçek drift testi için
        // ters senaryo: identity CaseSensitive (mixed case korunur), context AsciiCaseInsensitive.
        //
        // Ama CanonicalCodeIdentity CaseSensitive'de identity_key == display_path (case korunur).
        // Core AsciiCaseInsensitive lowercase yapar → drift!
        //
        // Not: CanonicalCodeIdentity private fields; sadece public constructor ile üretilebilir.
        // CaseSensitive policy ile "Src/Auth.rs" → identity_key = "Src/Auth.rs" (case korunur).
        // Core AsciiCaseInsensitive → canonical_key = "src/auth.rs" → drift.
        let identity = CanonicalCodeIdentity::new("Src/Auth.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(identity.identity_key(), "Src/Auth.rs"); // CaseSensitive: case korunur
        let err = to_core_identity_key(
            &identity,
            ctx(AnalysisIdentityScheme::PathV1, PathCasePolicy::AsciiCaseInsensitive),
        )
        .unwrap_err();
        match err {
            IdentityBridgeError::CanonicalizationDrift {
                cli_key,
                core_key,
            } => {
                assert_eq!(cli_key, "Src/Auth.rs");
                assert_eq!(core_key, "src/auth.rs");
            }
            other => panic!("expected CanonicalizationDrift, got {other:?}"),
        }
    }
}
