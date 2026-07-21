//! INV-T9 #70 Commit 3 — BLAKE3 canonical encoding framing primitives (neutral layer).
//!
//! Düşük seviyeli framing primitive'leri (u64/u32/u8/f64/bytes/tag encoding) tek yerde
//! paylaşılır. Domain-specific encoder'lar (`encode_canonical_node`, `encode_canonical_edge_*`,
//! `encode_effective_predicate_*`, `encode_space_view_id` vb.) ilgili domain modülünde kalır
//! — yalnızca byte-level framing burada yaşar.
//!
//! **Bağımlılık yönü (P1-2 v4):** `authorization → canonical_encoding`,
//! `measurement → canonical_encoding`. Tersine YOK. `CanonicalEncodingError` crate-private;
//! authorization ve measurement kendi public error tiplerine stable mapping yapar.

/// **Neutral encoding error (P1-3 v3 / P1-2 v4):** byte-level framing hataları.
/// Authorization `AuthorizationBasisDigestError` ve measurement `MeasurementDigestError`
/// bu tipi stable `From` impl'leri ile sarar — primitive error dış API'ye sızmaz.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum CanonicalEncodingError {
    #[error("non-finite float (NaN or ±Infinity) rejected")]
    NonFiniteRejected,
    /// Encoding primitive'leri tarafından üretilir (örn `encode_bytes` length-prefix
    /// checked u64 conversion'da). Mevcut call site'larda üretilemiyor çünkü `usize → u64`
    /// infallible, ama varyant korunur — future-proof encoding eklemeleri için stable error
    /// surface (authorization `CanonicalDigestError::LengthOverflow` ile paralel).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "stable error surface — future encoding eklemeleri için korunur"
        )
    )]
    #[error("canonical length overflow in {field}")]
    LengthOverflow { field: &'static str },
}

pub(crate) fn encode_u64(hasher: &mut blake3::Hasher, val: u64, _field: &str) {
    hasher.update(&val.to_le_bytes());
}

pub(crate) fn encode_u32(hasher: &mut blake3::Hasher, val: u32, _field: &str) {
    hasher.update(&val.to_le_bytes());
}

pub(crate) fn encode_u8(hasher: &mut blake3::Hasher, val: u8, _field: &str) {
    hasher.update(&[val]);
}

/// Length-prefix (u64 LE) + raw bytes. `AuthorizationBasisDigest` ve digest zincirindeki
/// tüm variable-length section'lar için ortak convention.
pub(crate) fn encode_bytes(
    hasher: &mut blake3::Hasher,
    bytes: &[u8],
) -> Result<(), CanonicalEncodingError> {
    encode_u64(hasher, bytes.len() as u64, "len");
    hasher.update(bytes);
    Ok(())
}

/// **Step 6 P0:** Canonical f64 → 8 byte (tek primitive). Non-finite reject (NaN + ±Infinity),
/// -0.0 → 0.0 normalize, little-endian to_bits.
///
/// `encode_f64` (hasher) + `push_f64` (buffer) + `encode_optional_f64` hep bu kaynağı
/// kullanır — çift canonicalization yok. Preimage testleri doğrudan bu fonksiyonu çağırır.
pub(crate) fn canonical_f64_bytes(val: f64) -> Result<[u8; 8], CanonicalEncodingError> {
    if !val.is_finite() {
        return Err(CanonicalEncodingError::NonFiniteRejected);
    }
    // -0.0 → 0.0 normalize (to_bits farklı: -0.0 = 0x8000000000000000, 0.0 = 0x0).
    let normalized = if val == 0.0 { 0.0f64 } else { val };
    Ok(normalized.to_bits().to_le_bytes())
}

/// f64 canonical encoding — non-finite reject (NaN + ±Infinity), -0.0 → 0.0, little-endian to_bits.
///
/// **reviewer P0-2a:** yalnız NaN değil, ±Infinity de reddedilir.
///
/// **Step 6 P0:** `canonical_f64_bytes` üzerinden (tek kaynak).
pub(crate) fn encode_f64(
    hasher: &mut blake3::Hasher,
    val: f64,
    _field: &str,
) -> Result<(), CanonicalEncodingError> {
    hasher.update(&canonical_f64_bytes(val)?);
    Ok(())
}

/// **Step 6 P0:** Option\<f64\> → Vec\<u8\> (shared byte helper). Presence tag:
/// `None → [0]`, `Some(v) → [1] || canonical_f64_bytes(v)`. Tag olmadan aynı byte dizisini
/// üreten context çiftleri imkânsız (reviewer P0-1 encoding collision fix).
/// Preimage testleri doğrudan bu fonksiyonu çağırır.
pub(crate) fn encode_optional_f64_to_vec(
    value: Option<f64>,
) -> Result<Vec<u8>, CanonicalEncodingError> {
    let mut bytes = Vec::with_capacity(9);
    match value {
        None => push_u8(&mut bytes, 0),
        Some(v) => {
            push_u8(&mut bytes, 1);
            bytes.extend_from_slice(&canonical_f64_bytes(v)?);
        }
    }
    Ok(bytes)
}

/// Option\<f64\> canonical encoding — **reviewer P0-1 (encoding collision fix).**
///
/// Presence tag: `None → [0]`, `Some(v) → [1] || canonical_f64(v)`. Tag olmadan aynı
/// byte dizisini üreten context çiftleri artık imkânsız.
///
/// **Step 6 P0:** `encode_optional_f64_to_vec` üzerinden (tek kaynak).
pub(crate) fn encode_optional_f64(
    hasher: &mut blake3::Hasher,
    value: Option<f64>,
    _field: &str,
) -> Result<(), CanonicalEncodingError> {
    hasher.update(&encode_optional_f64_to_vec(value)?);
    Ok(())
}

/// Canonical numeric tag trait — newtype'ların `as_u8()` üzerinden encode edilmesi.
///
/// `encode_u8` ve `push_u8` generic hale gelir; newtype'lar otomatik desteklenir.
/// Ham `u8` de desteklenir (scope_tag gibi plain numeric alanlar için).
pub(crate) trait CanonicalTag {
    fn tag_u8(&self) -> u8;
}

impl CanonicalTag for u8 {
    fn tag_u8(&self) -> u8 {
        *self
    }
}

pub(crate) fn encode_tag<T: CanonicalTag>(hasher: &mut blake3::Hasher, val: T, field: &str) {
    encode_u8(hasher, val.tag_u8(), field);
}

/// **INV-T9 #70 Commit 4b Faz 3 (reviewer v4 P2-1):** Neutral per-axis measurement encoder.
///
/// `value` (canonical float) + `source_tag` (stable `CanonicalMetricSourceTag`) + `axis_discrim`
/// (axis discriminator byte) encode eder. Auth-layer tiplerini (`CanonicalAxisMeasurement`)
/// tanımaz — üst katmanlar kendi tiplerini bu bileşenlere dönüştürür. Bu sayede cycle:
/// `authorization → canonical_encoding` VE `measurement → canonical_encoding` korunur,
/// tersine bağımlılık yok.
///
/// **Axis discriminator (reviewer v4 P2-1):** axis sırası structural olarak sabittir
/// (coupling→cohesion→instability→entropy→witness_depth), ama explicit discriminator
/// defense-in-depth olarak encoding'e eklenir. Mapping stable ve pinlidir:
/// `coupling=0, cohesion=1, instability=2, entropy=3, witness_depth=4`.
///
/// **Source tag (reviewer v4 P2-2):** `CanonicalMetricSourceTag` stable mapping kullanır
/// (`canonical_tag_newtype!` macro — enum discriminant DEĞİL). Varyant sırası değişse bile
/// tag byte'ı sabit kalır.
///
/// **Kullanım:** `MeasurementDigest` (Faz 3 yeni commitment) bu primitifi kullanır.
/// Mevcut `AuthorizationBasisDigest` v1 byte contract'ı korunur — ayrı encoding.
pub(crate) fn encode_axis_components(
    hasher: &mut blake3::Hasher,
    value: f64,
    source_tag: crate::canonical_tags::CanonicalMetricSourceTag,
    axis_discrim: u8,
) -> Result<(), CanonicalEncodingError> {
    encode_u8(hasher, axis_discrim, "axis_discrim");
    encode_f64(hasher, value, "axis_value")?;
    encode_tag(hasher, source_tag, "axis_source");
    Ok(())
}

/// Stable axis discriminator bytes (reviewer v4 P2-1 — pinli mapping).
pub(crate) const AXIS_DISCRIM_COUPLING: u8 = 0;
pub(crate) const AXIS_DISCRIM_COHESION: u8 = 1;
pub(crate) const AXIS_DISCRIM_INSTABILITY: u8 = 2;
pub(crate) const AXIS_DISCRIM_ENTROPY: u8 = 3;
pub(crate) const AXIS_DISCRIM_WITNESS_DEPTH: u8 = 4;

// ──────────────────────────────────────────────────────────────────────────────
// Vec\<u8\> canonical encoding helpers (predicate sort için)
// ──────────────────────────────────────────────────────────────────────────────

pub(crate) fn push_u8(buf: &mut Vec<u8>, val: u8) {
    buf.push(val);
}

pub(crate) fn push_tag<T: CanonicalTag>(buf: &mut Vec<u8>, val: T) {
    push_u8(buf, val.tag_u8());
}

pub(crate) fn push_u64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// **Step 6 P0:** buffer'a canonical f64 yazar — `canonical_f64_bytes` üzerinden (tek kaynak).
pub(crate) fn push_f64(buf: &mut Vec<u8>, val: f64) -> Result<(), CanonicalEncodingError> {
    buf.extend_from_slice(&canonical_f64_bytes(val)?);
    Ok(())
}

pub(crate) fn push_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    push_u64(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// `encode_u64` little-endian byte'ları hasher'a yazar. Preimage: val.to_le_bytes().
    #[test]
    fn encode_u64_writes_le_bytes() {
        let mut hasher = blake3::Hasher::new();
        encode_u64(&mut hasher, 0x0102030405060708, "test");
        // verify by using a derived hash and re-deriving the same digest from raw bytes
        let h1 = *hasher.finalize().as_bytes();

        let mut hasher2 = blake3::Hasher::new();
        hasher2.update(&0x0102030405060708u64.to_le_bytes());
        let h2 = *hasher2.finalize().as_bytes();

        assert_eq!(h1, h2, "encode_u64 must write little-endian bytes");
    }

    /// `encode_bytes` length-prefix (u64 LE) + raw bytes yazar. Preimage:
    /// `[len_u64_le] || [bytes]`.
    #[test]
    fn encode_bytes_length_prefix_u64_le() {
        let payload = b"hello";
        let mut hasher = blake3::Hasher::new();
        encode_bytes(&mut hasher, payload).unwrap();
        let h1 = *hasher.finalize().as_bytes();

        let mut hasher2 = blake3::Hasher::new();
        hasher2.update(&(payload.len() as u64).to_le_bytes());
        hasher2.update(payload);
        let h2 = *hasher2.finalize().as_bytes();

        assert_eq!(
            h1, h2,
            "encode_bytes must write u64 LE length prefix + raw bytes"
        );
    }

    /// `canonical_f64_bytes` NaN ve ±Infinity reddeder (reviewer P0-2a).
    #[test]
    fn canonical_f64_bytes_rejects_non_finite() {
        assert!(matches!(
            canonical_f64_bytes(f64::NAN),
            Err(CanonicalEncodingError::NonFiniteRejected)
        ));
        assert!(matches!(
            canonical_f64_bytes(f64::INFINITY),
            Err(CanonicalEncodingError::NonFiniteRejected)
        ));
        assert!(matches!(
            canonical_f64_bytes(f64::NEG_INFINITY),
            Err(CanonicalEncodingError::NonFiniteRejected)
        ));
    }

    /// `canonical_f64_bytes` -0.0 → 0.0 normalize eder (to_bits farklı).
    #[test]
    fn canonical_f64_bytes_normalizes_neg_zero() {
        let pos_zero = canonical_f64_bytes(0.0).unwrap();
        let neg_zero = canonical_f64_bytes(-0.0).unwrap();
        assert_eq!(pos_zero, neg_zero, "-0.0 must normalize to 0.0");
        assert_eq!(pos_zero, [0u8; 8]);
    }

    /// `encode_optional_f64_to_vec` presence tag: None=[0], Some(v)=[1]+canonical_f64(v).
    #[test]
    fn encode_optional_f64_presence_tag() {
        let none_bytes = encode_optional_f64_to_vec(None).unwrap();
        assert_eq!(none_bytes, vec![0], "None must encode as [0]");

        let some_bytes = encode_optional_f64_to_vec(Some(1.5)).unwrap();
        assert_eq!(some_bytes.len(), 9, "Some(v) must be [tag] + 8 bytes");
        assert_eq!(some_bytes[0], 1, "Some(v) tag must be 1");
        let mut expected_tail = [0u8; 8];
        expected_tail.copy_from_slice(&canonical_f64_bytes(1.5).unwrap());
        assert_eq!(
            &some_bytes[1..],
            &expected_tail,
            "payload must be canonical_f64(v)"
        );
    }
}
