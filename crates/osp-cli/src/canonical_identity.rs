// Test + CLI integration tamamlanana kadar dead-code.
#![allow(dead_code)]

//! Canonical code identity — repository-relative path identity model.
//!
//! Analysis bridge'in fiziksel modül kimlik katmanı. İki ayrı kavram:
//! - `display_path`: repo-relative, orijinal case korunmuş, forward-slash (canonical = gözlemlenen yazım)
//! - `identity_key`: case-folded collision key (ConceptNodeId materyali)
//!
//! # Identity-durum sözleşmesi (F-yeni)
//! `ConceptNodeId` (identity_key'den) = kalıcı entity kimliği. `canonical` (display_path) = gözlemlenen
//! mevcut repository spelling. `NodeDigest` = canonical dahil mevcut temsil/freshness özeti. Case-only
//! rename (AsciiCaseInsensitive) → aynı NodeId, farklı canonical, farklı digest = INV-C12 muhafazakâr
//! (StaleBasis doğru). Supersession değil representation refresh.
//!
//! # Lexical normalizasyon (P1)
//! Sıra: boş/NUL reddet → `\`→`/` → absolute/drive/UNC reddet → segment → `..` reddet →
//! son segment kontrol (trailing `/`/`.`→DirectoryLikePath) → `.`/boş iç segment kaldır →
//! birleştir → boş reddet → display dondur → identity_key case-fold.

// analysis_bridge.rs + commands/graph.rs integration tamamlanana kadar dead-code.

use thiserror::Error;

/// Path case politikası — host OS'den türetilmez (cross-platform determinism).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCasePolicy {
    /// Case-sensitive equality — display_path == identity_key.
    CaseSensitive,
    /// ASCII case-fold (to_ascii_lowercase). Unicode filesystem equivalence ayrı tightening.
    AsciiCaseInsensitive,
}

impl PathCasePolicy {
    /// identity_key üret — policy'ye göre case transform.
    fn fold(self, display_path: &str) -> String {
        match self {
            Self::CaseSensitive => display_path.to_string(),
            Self::AsciiCaseInsensitive => display_path.to_ascii_lowercase(),
        }
    }

    /// CLI/Display label (BridgeRunReport Display için — analysis_bridge.rs).
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::CaseSensitive => "case-sensitive",
            Self::AsciiCaseInsensitive => "ascii-insensitive",
        }
    }
}

/// Canonical code identity — iki ayrı string (display vs identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalCodeIdentity {
    display_path: String,
    identity_key: String,
}

impl CanonicalCodeIdentity {
    /// Lexical normalizasyon + identity key üretimi.
    pub fn new(path: &str, policy: PathCasePolicy) -> Result<Self, CanonicalIdentityError> {
        // (1) Boş/NUL input reddet.
        if path.is_empty() {
            return Err(CanonicalIdentityError::Empty);
        }
        if path.contains('\0') {
            return Err(CanonicalIdentityError::ContainsNul);
        }

        // (2) `\` → `/` (Windows backslash normalize).
        let normalized = path.replace('\\', "/");

        // (3) Absolute biçimler reddet (host OS'den bağımsız — Linux CI'da da C:/ reddet).
        reject_absolute_forms(&normalized)?;

        // (4) Segmentlere ayır.
        let raw_segments: Vec<&str> = normalized.split('/').collect();

        // (5)(6) `..` reddet; son segment directory-intent kontrolü; `.`/boş iç segment kaldır.
        let mut clean_segments: Vec<String> = Vec::new();
        for (i, seg) in raw_segments.iter().enumerate() {
            let is_last = i == raw_segments.len() - 1;
            match *seg {
                ".." => {
                    return Err(CanonicalIdentityError::ParentTraversal(normalized.clone()));
                }
                "." | "" => {
                    // Son segment "." veya boş → directory-intent (trailing `/` veya `/.`).
                    if is_last && i > 0 {
                        return Err(CanonicalIdentityError::DirectoryLikePath(normalized.clone()));
                    }
                    // İç segment "."/boş → kaldır (normalize).
                    continue;
                }
                _ => {
                    clean_segments.push((*seg).to_string());
                }
            }
        }

        // (7) Birleştir.
        let display_path = clean_segments.join("/");

        // (8) Normalize sonrası boş path reddet.
        if display_path.is_empty() {
            return Err(CanonicalIdentityError::EmptyAfterNormalization);
        }

        // (9)(10) display_path dondur (orijinal case); identity_key policy'ye göre case-fold.
        let identity_key = policy.fold(&display_path);

        Ok(Self {
            display_path,
            identity_key,
        })
    }

    /// Repo-relative, orijinal case, forward-slash canonical (gözlemlenen yazım).
    pub fn display_path(&self) -> &str {
        &self.display_path
    }

    /// Case-folded collision key (ConceptNodeId materyali).
    pub fn identity_key(&self) -> &str {
        &self.identity_key
    }

    /// Ownership ile parçalara ayır.
    pub fn into_parts(self) -> (String, String) {
        (self.display_path, self.identity_key)
    }
}

/// Absolute/UNC/Windows-drive biçimlerini reddet (host OS'den bağımsız).
fn reject_absolute_forms(normalized: &str) -> Result<(), CanonicalIdentityError> {
    // Unix absolute: `/...`
    if normalized.starts_with('/') {
        return Err(CanonicalIdentityError::AbsolutePath(normalized.to_string()));
    }
    // UNC: `//server/share/...` veya `//?/C:/...` (verbatim) — başında `//`.
    if normalized.starts_with("//") {
        return Err(CanonicalIdentityError::AbsolutePath(normalized.to_string()));
    }
    // Windows drive: `^[A-Za-z]:` — hem `C:/...` (absolute) hem `C:src` (drive-relative).
    // Drive-relative de absolute değildir ama drive context bağımlı → reddet (dürüst adlandırma).
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(CanonicalIdentityError::WindowsDrivePrefixed(normalized.to_string()));
    }
    Ok(())
}

/// Canonical identity normalizasyon hatası.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CanonicalIdentityError {
    #[error("empty path input")]
    Empty,
    #[error("path contains NUL byte")]
    ContainsNul,
    #[error("absolute path rejected (Unix / or UNC //): {0}")]
    AbsolutePath(String),
    #[error("Windows drive-prefixed path rejected (C:/ or C:src drive-relative): {0}")]
    WindowsDrivePrefixed(String),
    #[error("parent traversal (..) rejected: {0}")]
    ParentTraversal(String),
    #[error("directory-like path rejected (trailing / or /.): {0}")]
    DirectoryLikePath(String),
    #[error("path empty after normalization")]
    EmptyAfterNormalization,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(path: &str, policy: PathCasePolicy) -> Result<(String, String), CanonicalIdentityError> {
        CanonicalCodeIdentity::new(path, policy).map(|i| i.into_parts())
    }

    // ── Mutlu yol normalizasyon ──────────────────────────────────────────────

    #[test]
    fn happy_path_simple_relative() {
        let (display, key) = id("src/payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/payment.rs");
        assert_eq!(key, "src/payment.rs");
    }

    #[test]
    fn backslash_to_forward_slash() {
        let (display, _) = id("src\\payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/payment.rs");
    }

    #[test]
    fn dot_segment_normalized() {
        let (display, _) = id("src/./payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/payment.rs");
    }

    #[test]
    fn duplicate_slash_normalized() {
        let (display, _) = id("src//payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/payment.rs");
    }

    #[test]
    fn leading_dot_slash_normalized() {
        let (display, _) = id("./src/payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/payment.rs");
    }

    // ── Case policy ──────────────────────────────────────────────────────────

    #[test]
    fn case_sensitive_preserves_display() {
        let (display, key) = id("src/Payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/Payment.rs");
        assert_eq!(key, "src/Payment.rs"); // CaseSensitive: key == display
    }

    #[test]
    fn ascii_insensitive_folds_key_preserves_display() {
        let (display, key) = id("src/Payment.rs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        assert_eq!(display, "src/Payment.rs"); // display korunur
        assert_eq!(key, "src/payment.rs"); // key case-folded
    }

    #[test]
    fn case_only_difference_different_identity_sensitive() {
        // Case-sensitive: iki ayrı identity.
        let a = CanonicalCodeIdentity::new("src/Payment.cs", PathCasePolicy::CaseSensitive).unwrap();
        let b = CanonicalCodeIdentity::new("src/payment.cs", PathCasePolicy::CaseSensitive).unwrap();
        assert_ne!(a.identity_key(), b.identity_key());
    }

    #[test]
    fn case_only_difference_same_identity_insensitive() {
        // AsciiCaseInsensitive: aynı identity_key (collision detection için).
        let a = CanonicalCodeIdentity::new("src/Payment.cs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let b = CanonicalCodeIdentity::new("src/payment.cs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        assert_eq!(a.identity_key(), b.identity_key());
        assert_ne!(a.display_path(), b.display_path()); // display farklı
    }

    // ── Bit-equivalent determinism ───────────────────────────────────────────

    #[test]
    fn same_input_same_policy_platform_independent() {
        let a = CanonicalCodeIdentity::new("src/Payment/Service.cs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let b = CanonicalCodeIdentity::new("src/Payment/Service.cs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        assert_eq!(a, b);
    }

    // ── Absolute/UNC/drive reddet ────────────────────────────────────────────

    #[test]
    fn reject_unix_absolute() {
        assert!(matches!(
            id("/src/a.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::AbsolutePath(_))
        ));
    }

    #[test]
    fn reject_unc_double_slash() {
        assert!(matches!(
            id("//server/share/a.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::AbsolutePath(_))
        ));
    }

    #[test]
    fn reject_verbatim_unc() {
        // \\?\C:\src\a.rs → normalize //?/C:/src/a.rs → AbsolutePath (//) VEYA WindowsDrivePrefixed.
        let r = id("\\\\?\\C:\\src\\a.rs", PathCasePolicy::CaseSensitive);
        assert!(r.is_err());
    }

    #[test]
    fn reject_windows_drive_absolute() {
        assert!(matches!(
            id("C:/src/a.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::WindowsDrivePrefixed(_))
        ));
    }

    #[test]
    fn reject_windows_drive_absolute_backslash() {
        assert!(matches!(
            id("C:\\src\\a.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::WindowsDrivePrefixed(_))
        ));
    }

    #[test]
    fn reject_windows_drive_relative() {
        // C:src — drive-relative (absolute değil ama drive context bağımlı).
        assert!(matches!(
            id("C:src/a.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::WindowsDrivePrefixed(_))
        ));
    }

    // ── Parent traversal ─────────────────────────────────────────────────────

    #[test]
    fn reject_parent_traversal_unix() {
        assert!(matches!(
            id("src/../outside.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::ParentTraversal(_))
        ));
    }

    #[test]
    fn reject_parent_traversal_windows_backslash() {
        // Backslash slash'a çevrildikten sonra segment kontrol — `..` yakalanır.
        assert!(matches!(
            id("src\\..\\outside.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::ParentTraversal(_))
        ));
    }

    // ── Directory-like path ──────────────────────────────────────────────────

    #[test]
    fn reject_trailing_slash() {
        assert!(matches!(
            id("src/payment/", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::DirectoryLikePath(_))
        ));
    }

    #[test]
    fn reject_trailing_double_slash() {
        assert!(matches!(
            id("src/payment//", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::DirectoryLikePath(_))
        ));
    }

    #[test]
    fn reject_trailing_dot() {
        // src/payment/. — son segment "." → directory-intent.
        assert!(matches!(
            id("src/payment/.", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::DirectoryLikePath(_))
        ));
    }

    #[test]
    fn reject_trailing_dot_slash() {
        assert!(matches!(
            id("src/payment/./", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::DirectoryLikePath(_))
        ));
    }

    #[test]
    fn accept_internal_dot_segment() {
        // İç segment "." normalize edilir — directory-intent değil.
        let (display, _) = id("src/./payment.rs", PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(display, "src/payment.rs");
    }

    // ── Empty/NUL ────────────────────────────────────────────────────────────

    #[test]
    fn reject_empty() {
        assert!(matches!(
            id("", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::Empty)
        ));
    }

    #[test]
    fn reject_nul_byte() {
        assert!(matches!(
            id("src\0/payment.rs", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::ContainsNul)
        ));
    }

    #[test]
    fn reject_only_dot_segments() {
        // "././." → son segment "." → DirectoryLikePath (directory-intent).
        assert!(matches!(
            id("././.", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::DirectoryLikePath(_))
        ));
    }

    #[test]
    fn reject_only_slashes() {
        // "///" → `/` ile başlıyor → AbsolutePath.
        assert!(matches!(
            id("///", PathCasePolicy::CaseSensitive),
            Err(CanonicalIdentityError::AbsolutePath(_))
        ));
    }
}
