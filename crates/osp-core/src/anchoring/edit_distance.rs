//! Levenshtein edit distance — INV-C8 lexical dedup 3. katmanı.
//!
//! Bağımlılık YOK (strsim crate ekleme — OSP TCB felsefesi: minimal bağımlılık).
//! Sadece ≤2 threshold için optimize gerekmez; tam DP tablosu yeterli.

/// Levenshtein edit distance (ekle/sil/değiştir).
///
/// INV-C8: ≤2 match → `CanonicalRedirect(EditDistanceLe2)` (Faz 1-2 lexical dedup).
/// Faz 7'de embedding dedup (cosine ≥0.85) eklenecek.
pub fn levenshtein(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());

    if n == 0 {
        return m as u32;
    }
    if m == 0 {
        return n as u32;
    }

    // Tek satır DP — space O(min(n,m))
    let mut prev: Vec<u32> = (0..=m).map(|i| i as u32).collect();
    let mut curr: Vec<u32> = vec![0; m + 1];

    for i in 1..=n {
        curr[0] = i as u32;
        for j in 1..=m {
            let cost = if a[i - 1].eq_ignore_ascii_case(&b[j - 1]) {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1) // sil
                .min(curr[j - 1] + 1) // ekle
                .min(prev[j - 1] + cost); // değiştir
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[m]
}

/// ≤2 threshold helper (INV-C8 3. katman).
pub fn within_edit_distance_2(a: &str, b: &str) -> Option<u32> {
    let d = levenshtein(a, b);
    (d <= 2).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_zero() {
        assert_eq!(levenshtein("Payment", "Payment"), 0);
        assert_eq!(levenshtein("ödeme", "ödeme"), 0);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(levenshtein("Payment", "payment"), 0);
        assert_eq!(levenshtein("PAYMENT", "payment"), 0);
    }

    #[test]
    fn single_substitution() {
        assert_eq!(levenshtein("odeme", "ödeme"), 1);
        assert_eq!(levenshtein("Payment", "Peyment"), 1);
    }

    #[test]
    fn within_2_matches() {
        // INV-C8: "Payments" / "Payment" → distance 1
        assert_eq!(within_edit_distance_2("Payment", "Payments"), Some(1));
        // "Paymentx" → distance 1
        assert_eq!(within_edit_distance_2("Payment", "Paymentx"), Some(1));
    }

    #[test]
    fn beyond_2_no_match() {
        assert_eq!(within_edit_distance_2("Payment", "Checkout"), None);
        assert_eq!(within_edit_distance_2("a", "abcdefg"), None);
    }

    #[test]
    fn empty_string_distance() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn turkish_chars() {
        // Türkçe karakterler tek char — "güven" / "guven" distance 1
        assert_eq!(levenshtein("güven", "guven"), 1);
        assert_eq!(within_edit_distance_2("güven", "guven"), Some(1));
    }
}
