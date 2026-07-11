//! Physical code identity model (PR E).
//!
//! Node identity ≠ physical code identity. İki ayrı ontolojik node
//! ([`CodeEntityCandidate`](crate::anchoring::types::ConceptNodeKind::CodeEntityCandidate) +
//! [`CodeEntity`](crate::anchoring::types::ConceptNodeKind::CodeEntity)) aynı fiziksel kod varlığına
//! gönderme yapar. [`CodeIdentityKey`] bu fiziksel identity'yi temsil eder; evidence (PR F) bu key
//! üzerinden çözülür, node ID'ye kopyalanmaz.
//!
//! # Case policy canonicalization (tur 3 P2-A)
//! [`CodeIdentityKey::new`] constructor'ı scheme içindeki case policy'yi uygular:
//! `AsciiCaseInsensitive` → `to_ascii_lowercase()`. Path structural normalization (slash/root)
//! YAPMAZ — aldığı key'in zaten upstream producer (CLI `CanonicalCodeIdentity`) tarafından
//! normalization'dan geçtiğini varsayar.
//!
//! # Entity ID derivation (tur 3 P2-B — algoritma sabit)
//! [`derive_resolved_code_entity_id`] deterministic FNV-1a türetmedir. "Collision imkansız"
//! DEĞİL; scheme/policy domain-separated encoding'e katılır, aynı key deterministik olarak aynı
//! ID'yi üretir. Hash collision ihtimali store-level material/key comparison ile fail-closed
//! yakalanır ([`EntityIdentityCollision`]). Algoritma version-tagged; Debug format/serde metni
//! hash input'u OLAMAZ (refactor ile değişebilir).

use crate::anchoring::types::ConceptNodeId;

// ═══════════════════════════════════════════════════════════════════════════════
// CodePathCasePolicy
// ═══════════════════════════════════════════════════════════════════════════════

/// Path case normalization policy (core canonical identity).
///
/// Scheme'in parçasıdır (tur 2 P1-B) — iki farklı case policy farklı identity domain üretir.
/// `CodeIdentityKey` equality'si scheme'i (dolayısıyla case policy'yi) kapsar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CodePathCasePolicy {
    /// Key case duyarlı (olduğu gibi).
    CaseSensitive,
    /// Key `to_ascii_lowercase()` canonicalize edilir (iki farklı yazım aynı key).
    AsciiCaseInsensitive,
}

impl CodePathCasePolicy {
    /// Case policy'yi key'e uygula (tur 3 P2-A canonicalization).
    fn canonicalize(self, key: &str) -> String {
        match self {
            Self::CaseSensitive => key.to_string(),
            Self::AsciiCaseInsensitive => key.to_ascii_lowercase(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeIdentityScheme
// ═══════════════════════════════════════════════════════════════════════════════

/// Physical code identity scheme (tur 2 P1-B — case-policy scheme'in parçası).
///
/// Kendi smart-constructor invariant'ı yok; derive `serde::Deserialize` uygundur
/// ([`CodeIdentityKey`] custom deserializer DTO'dan scheme'i deserialize eder).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "variant", content = "params")]
pub enum CodeIdentityScheme {
    /// Analysis path-based identity (CLI `CanonicalCodeIdentity` + path case policy).
    AnalysisPathV1 {
        case_policy: CodePathCasePolicy,
    },
}

impl CodeIdentityScheme {
    /// Case policy discriminant (entity ID derivation input + doc).
    fn case_policy(&self) -> CodePathCasePolicy {
        match self {
            Self::AnalysisPathV1 { case_policy } => *case_policy,
        }
    }

    /// Scheme discriminant string (entity ID derivation input — sabit, refactor-safe).
    fn discriminant(&self) -> &'static str {
        match self {
            Self::AnalysisPathV1 { .. } => "AnalysisPathV1",
        }
    }

    /// Case-policy discriminant string (entity ID derivation input — sabit).
    fn case_policy_discriminant(&self) -> &'static str {
        match self.case_policy() {
            CodePathCasePolicy::CaseSensitive => "CaseSensitive",
            CodePathCasePolicy::AsciiCaseInsensitive => "AsciiCaseInsensitive",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CodeIdentityKey
// ═══════════════════════════════════════════════════════════════════════════════

/// Physical code identity key — node ID ≠ physical code identity.
///
/// İki ayrı ontolojik node (`CodeEntityCandidate` + `CodeEntity`) aynı fiziksel kod varlığına
/// gönderme yapar. Evidence (PR F) bu key üzerinden çözülür; node ID'ye kopyalanmaz.
///
/// # Canonicalization (tur 3 P2-A)
/// Constructor scheme içindeki case policy'yi uygular. Path structural normalization YAPMAZ.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct CodeIdentityKey {
    scheme: CodeIdentityScheme,
    key: String, // canonicalized
}

impl CodeIdentityKey {
    /// Smart constructor — boş/whitespace/NUL/control reject + case policy canonicalization.
    ///
    /// Path structural normalization (slash/root) YAPMAZ — aldığı key'in zaten upstream producer
    /// (CLI `CanonicalCodeIdentity`) tarafından normalization'dan geçtiğini varsayar; yalnız
    /// scheme içindeki case policy'yi uygular.
    pub fn new(
        scheme: CodeIdentityScheme,
        key: impl Into<String>,
    ) -> Result<Self, CodeIdentityKeyError> {
        let raw_key = key.into();
        if raw_key.trim().is_empty() {
            return Err(CodeIdentityKeyError::Empty);
        }
        if raw_key.chars().any(|c| c == '\0' || c.is_control()) {
            return Err(CodeIdentityKeyError::ControlCharacter);
        }
        let canonicalized = scheme.case_policy().canonicalize(&raw_key);
        Ok(Self {
            scheme,
            key: canonicalized,
        })
    }

    /// Identity scheme.
    pub fn scheme(&self) -> &CodeIdentityScheme {
        &self.scheme
    }

    /// Canonical key (entity material için — tur 3 P2-C).
    pub fn canonical_key(&self) -> &str {
        &self.key
    }

    /// Deterministic entity ID derivation (tur 2 nokta 4 + tur 3 P2-B algoritma).
    pub fn derive_entity_id(&self) -> ConceptNodeId {
        derive_resolved_code_entity_id(self)
    }
}

/// Custom Deserialize — `new()` üzerinden (tur 2 P2-A; derive bypass YOK).
/// DTO'dan scheme + key deserialize eder, constructor canonicalization + validation uygular.
impl<'de> serde::Deserialize<'de> for CodeIdentityKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct CodeIdentityKeyDto {
            scheme: CodeIdentityScheme,
            key: String,
        }
        let dto = CodeIdentityKeyDto::deserialize(deserializer)?;
        CodeIdentityKey::new(dto.scheme, dto.key).map_err(serde::de::Error::custom)
    }
}

/// `CodeIdentityKey` constructor hatası.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CodeIdentityKeyError {
    #[error("identity key boş/whitespace olamaz")]
    Empty,
    #[error("identity key NUL/control character içeremez")]
    ControlCharacter,
}

// ═══════════════════════════════════════════════════════════════════════════════
// derive_resolved_code_entity_id — deterministic CodeIdentityKey → CodeEntity ID
// ═══════════════════════════════════════════════════════════════════════════════

/// Domain tag (tur 3 P2-B — algoritma version-tagged).
const CODE_ENTITY_DERIVATION_DOMAIN_TAG: &str = "osp:code-entity:v1";

/// Deterministic `CodeIdentityKey → CodeEntity` ID derivation (tur 3 P2-B — algoritma sabit).
///
/// # Algoritma (FNV-1a-v1, version-tagged)
/// Input encoding (length-prefixed):
///   - domain tag `"osp:code-entity:v1"`
///   - scheme discriminant (`"AnalysisPathV1"`)
///   - case-policy discriminant (`"CaseSensitive"` | `"AsciiCaseInsensitive"`)
///   - canonical key bytes
/// Output: `CodeEntity:<16-hex>`
///
/// # Sözleşme (tur 3 P2-B — "collision imkansız" DEĞİL)
/// Scheme/policy domain-separated encoding'e katılır; aynı key deterministik olarak aynı ID'yi
/// üretir. Hash collision ihtimali store-level material/key comparison ile fail-closed yakalanır
/// (`EntityIdentityCollision`). Debug format/enum serde metni hash input'u OLAMAZ.
pub fn derive_resolved_code_entity_id(key: &CodeIdentityKey) -> ConceptNodeId {
    let scheme = key.scheme();
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    let mut feed = |bytes: &[u8]| {
        // length-prefix (refactor-safe; boyut değişikliği ayırt edilir)
        let len = bytes.len() as u64;
        for b in len.to_le_bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3); // FNV prime
        }
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    feed(CODE_ENTITY_DERIVATION_DOMAIN_TAG.as_bytes());
    feed(scheme.discriminant().as_bytes());
    feed(scheme.case_policy_discriminant().as_bytes());
    feed(key.canonical_key().as_bytes());
    if hash == 0 {
        hash = 1;
    }
    ConceptNodeId(format!("CodeEntity:{:016x}", hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_v1_insensitive() -> CodeIdentityScheme {
        CodeIdentityScheme::AnalysisPathV1 {
            case_policy: CodePathCasePolicy::AsciiCaseInsensitive,
        }
    }

    fn path_v1_sensitive() -> CodeIdentityScheme {
        CodeIdentityScheme::AnalysisPathV1 {
            case_policy: CodePathCasePolicy::CaseSensitive,
        }
    }

    #[test]
    fn code_identity_key_valid_construction() {
        let key = CodeIdentityKey::new(path_v1_insensitive(), "src/auth.rs").unwrap();
        assert_eq!(key.canonical_key(), "src/auth.rs");
    }

    #[test]
    fn code_identity_key_empty_reject() {
        assert_eq!(
            CodeIdentityKey::new(path_v1_insensitive(), "").unwrap_err(),
            CodeIdentityKeyError::Empty
        );
        assert_eq!(
            CodeIdentityKey::new(path_v1_insensitive(), "   ").unwrap_err(),
            CodeIdentityKeyError::Empty
        );
    }

    #[test]
    fn code_identity_key_control_character_reject() {
        assert_eq!(
            CodeIdentityKey::new(path_v1_insensitive(), "src\0auth").unwrap_err(),
            CodeIdentityKeyError::ControlCharacter
        );
    }

    #[test]
    fn code_identity_scheme_case_policy_participates_in_equality() {
        // Farklı case policy → farklı scheme → farklı key.
        let k1 = CodeIdentityKey::new(path_v1_insensitive(), "src/auth.rs").unwrap();
        let k2 = CodeIdentityKey::new(path_v1_sensitive(), "src/auth.rs").unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn code_identity_key_case_insensitive_canonicalizes() {
        // tur 3 P2-A: AsciiCaseInsensitive iki farklı yazım → aynı canonical key.
        let k1 = CodeIdentityKey::new(path_v1_insensitive(), "Src/Auth.rs").unwrap();
        let k2 = CodeIdentityKey::new(path_v1_insensitive(), "src/auth.rs").unwrap();
        assert_eq!(k1, k2, "AsciiCaseInsensitive canonicalize → eşitlik");
        assert_eq!(k1.canonical_key(), "src/auth.rs");
    }

    #[test]
    fn code_identity_key_case_sensitive_preserves() {
        let k1 = CodeIdentityKey::new(path_v1_sensitive(), "Src/Auth.rs").unwrap();
        let k2 = CodeIdentityKey::new(path_v1_sensitive(), "src/auth.rs").unwrap();
        assert_ne!(k1, k2, "CaseSensitive → farklı yazım farklı key");
        assert_eq!(k1.canonical_key(), "Src/Auth.rs");
    }

    #[test]
    fn code_identity_key_custom_deserialize_validates() {
        let json = r#"{"scheme":{"variant":"AnalysisPathV1","params":{"case_policy":"AsciiCaseInsensitive"}},"key":"  "}"#;
        let result: Result<CodeIdentityKey, _> = serde_json::from_str(json);
        assert!(result.is_err(), "empty key deserialize reject (new() üzerinden)");
    }

    #[test]
    fn code_identity_key_serde_roundtrip() {
        let key = CodeIdentityKey::new(path_v1_insensitive(), "src/auth.rs").unwrap();
        let json = serde_json::to_string(&key).unwrap();
        let back: CodeIdentityKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn derive_entity_id_deterministic_same_key_same_id() {
        let k1 = CodeIdentityKey::new(path_v1_insensitive(), "src/auth.rs").unwrap();
        let k2 = CodeIdentityKey::new(path_v1_insensitive(), "Src/Auth.rs").unwrap(); // canonicalize → eşit
        assert_eq!(k1.derive_entity_id(), k2.derive_entity_id());
        let id = k1.derive_entity_id();
        assert!(
            id.0.starts_with("CodeEntity:"),
            "derived ID CodeEntity: prefix taşımalı"
        );
    }

    #[test]
    fn derive_entity_id_different_scheme_different_domain() {
        let k1 = CodeIdentityKey::new(path_v1_insensitive(), "src/auth.rs").unwrap();
        let k2 = CodeIdentityKey::new(path_v1_sensitive(), "src/auth.rs").unwrap();
        assert_ne!(
            k1.derive_entity_id(),
            k2.derive_entity_id(),
            "farklı scheme/policy → farklı ID domain"
        );
    }
}
