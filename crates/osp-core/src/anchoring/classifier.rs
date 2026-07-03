//! Rule-based Classifier + Glossary (Faz 1, §11).
//!
//! **Disiplin:** deterministic heuristic. LLM/embedding YOK (§11). Türkçe/EN keyword
//! setleri. 7 seviyeli precedence (ilk match kazanır). Q2/Q6: manuel seed glossary;
//! Faz 6 Concept Synthesis zenginleştirir.
//!
//! # Precedence (değerlendirme netleştirmesi)
//! 1. AntiGoal → 2. Decision → 3. Assumption → 4. UserVision →
//! 5. Requirement → 6. Risk (sadece doğrudan) → 7. Default: Assumption
//!
//! # Coarse-grained not
//! Faz 1 classifier coarse-grained deterministic heuristic'tir. UserVision/Requirement
//! ayrımı fixture coverage kadar yapılır; ince ayrım Faz 2+ calibration'a bırakılır.

use crate::anchoring::ConceptPacketType;

// ═══════════════════════════════════════════════════════════════════════════════
// Glossary — runtime domain alias map
// ═══════════════════════════════════════════════════════════════════════════════

/// Türkçe/EN domain glossary. Alias → canonical mapping (Q6, Q9).
/// Faz 1: manuel seed; Faz 6: Concept Synthesis zenginleştirir.
#[derive(Debug, Clone, Default)]
pub struct Glossary {
    entries: Vec<GlossaryEntry>,
}

#[derive(Debug, Clone)]
pub struct GlossaryEntry {
    pub canonical: String,
    pub aliases: Vec<String>,
    pub language: String,
}

impl Glossary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed glossary — §12 Q6 önerisi (ödeme→Payment, güven→Trust).
    pub fn seed_default() -> Self {
        let mut g = Self::new();
        g.insert(GlossaryEntry {
            canonical: "Payment".into(),
            aliases: vec![
                "ödeme".into(),
                "payments".into(),
                "checkout".into(),
                "ödeme akışı".into(),
            ],
            language: "tr/en".into(),
        });
        g.insert(GlossaryEntry {
            canonical: "Trust".into(),
            aliases: vec![
                "güven".into(),
                "SecurityPerception".into(),
                "güvenlik algısı".into(),
            ],
            language: "tr/en".into(),
        });
        g.insert(GlossaryEntry {
            canonical: "User".into(),
            aliases: vec!["kullanıcı".into(), "müşteri".into(), "client".into()],
            language: "tr/en".into(),
        });
        g.insert(GlossaryEntry {
            canonical: "Authentication".into(),
            aliases: vec!["kimlik doğrulama".into(), "auth".into()],
            language: "tr/en".into(),
        });
        g.insert(GlossaryEntry {
            canonical: "Notification".into(),
            aliases: vec!["bildirim".into(), "notifications".into()],
            language: "tr/en".into(),
        });
        g
    }

    pub fn insert(&mut self, entry: GlossaryEntry) {
        self.entries.push(entry);
    }

    /// Terim → canonical (alias veya exact canonical match). Case-insensitive.
    pub fn canonical_for(&self, term: &str) -> Option<&str> {
        let lower = term.to_lowercase();
        self.entries.iter().find_map(|e| {
            if e.canonical.to_lowercase() == lower {
                return Some(e.canonical.as_str());
            }
            e.aliases
                .iter()
                .find(|a| a.to_lowercase() == lower)
                .map(|_| e.canonical.as_str())
        })
    }

    pub fn aliases_of(&self, canonical: &str) -> &[String] {
        let lower = canonical.to_lowercase();
        self.entries
            .iter()
            .find(|e| e.canonical.to_lowercase() == lower)
            .map(|e| e.aliases.as_slice())
            .unwrap_or(&[])
    }

    /// Terim glossary'de var mı (alias veya canonical).
    pub fn matches(&self, term: &str) -> bool {
        self.canonical_for(term).is_some()
    }

    pub fn entries(&self) -> &[GlossaryEntry] {
        &self.entries
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Classifier — 7 precedence
// ═══════════════════════════════════════════════════════════════════════════════

/// Rule-based concept packet classifier.
pub struct Classifier {
    /// Risk sinyali için ayrı keyword seti (packet type'tan bağımsız edge için).
    risk_markers: &'static [&'static str],
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Classifier {
    pub fn new() -> Self {
        Self {
            // Sadece doğrudan risk bildiren kelimeler (precedence #6).
            risk_markers: &[
                "risk",
                "tehlike",
                "danger",
                "açık",
                "vulnerability",
                "failure",
                "başarısızlık",
            ],
        }
    }

    /// Metni ConceptPacketType'a sınıflandır (7 precedence).
    pub fn classify(&self, text: &str, _language: &str) -> ConceptPacketType {
        let lower = text.to_lowercase();

        // 1. AntiGoal markers
        if matches_any(&lower, ANTIGOAL_MARKERS) {
            return ConceptPacketType::AntiGoal;
        }
        // 2. Decision markers
        if matches_any(&lower, DECISION_MARKERS) {
            return ConceptPacketType::Decision;
        }
        // 3. Assumption markers
        if matches_any(&lower, ASSUMPTION_MARKERS) {
            return ConceptPacketType::Assumption;
        }
        // 4. UserVision markers
        if matches_any(&lower, USER_VISION_MARKERS) {
            return ConceptPacketType::UserVision;
        }
        // 5. Requirement markers
        if matches_any(&lower, REQUIREMENT_MARKERS) {
            return ConceptPacketType::Requirement;
        }
        // 6. Risk (sadece doğrudan) — "güven" tek başına risk DEĞİL
        if matches_any(&lower, self.risk_markers) {
            return ConceptPacketType::Risk;
        }
        // 7. Default
        ConceptPacketType::Assumption
    }

    /// Risk sinyali var mı (packet type'dan bağımsız — extractor DerivesRisk için).
    pub fn has_risk_signal(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        // "güven" / "hissetmeli" gibi güven bağlamı → risk türetme sinyali
        matches_any(&lower, RISK_SIGNAL_MARKERS) || matches_any(&lower, self.risk_markers)
    }

    /// Rule/şart sinyali var mı (DerivesRule için).
    pub fn has_rule_signal(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        matches_any(&lower, RULE_MARKERS)
    }

    /// Task sinyali var mı (Faz 5a — DerivesTask için). Packet type'dan bağımsız.
    /// PR33a'da task signal + typed `TaskCandidate:<Name>` ref birlikte gerekir
    /// (extractor). Doğal dilden task adı türetme PR33a dışı.
    ///
    /// # Review patch (D2): `"task"` token-based eşleşme
    /// `"task"` substring aramak `TaskCandidate:Foo` typed ref'i yanlışlıkla task
    /// signal sayar (lowercase `taskcandidate` içinde `"task"` substring var). Bu
    /// yüzden `"task"` token olarak aranır (word boundary); TR marker'lar substring
    /// kalır (ekler için: "görevler", "yapılmalılar" vb.). Böylece typed ref tek
    /// başına DerivesTask üretmez — Patch 7 ikili koşul korunur.
    pub fn has_task_signal(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        // TR marker'lar substring (ek toleransı).
        if lower.contains("görev")
            || lower.contains("yapılmalı")
            || lower.contains("implement edilmeli")
            || lower.contains("geliştirilmeli")
        {
            return true;
        }
        // "task" token-based — `taskcandidate` içinde geçen "task" eşleşmemeli.
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|tok| tok == "task")
    }
}

fn matches_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| text.contains(m))
}

// Precedence keyword setleri (Türkçe + EN, lowercase)
const ANTIGOAL_MARKERS: &[&str] = &[
    "olmamalı",
    "kaçınılmalı",
    "anti-pattern",
    "yasak",
    "never",
    "avoid",
    "should not",
    "kaçın",
];

const DECISION_MARKERS: &[&str] = &[
    "karar verdik",
    "kararı",
    "kabul edilen karar",
    "adopted",
    "decided",
    "seçildi",
    "referans al",
    "referans alın",
];

const ASSUMPTION_MARKERS: &[&str] = &[
    "varsay",
    "assume",
    "kabul ediyoruz",
    "varsayılıyor",
    "ön kabul",
];

const USER_VISION_MARKERS: &[&str] = &[
    "kullanıcı",
    "müşteri",
    "deneyim",
    "hissetmeli",
    "kolaylık",
    "memnuniyet",
    "user",
    "customer",
    "experience",
    "feel",
];

const REQUIREMENT_MARKERS: &[&str] = &[
    "sistem",
    "modül",
    "servis",
    "katman",
    "must",
    "should",
    "gerekir",
    "yapılmalı",
    "implement edilmeli",
    "bağımlı olmamalı",
    "layer",
];

const RULE_MARKERS: &[&str] = &[
    "bağımlı olmamalı",
    "olmamalı",
    "gerekir",
    "malı",
    "meli",
    "must not",
    "should not",
    "requirement",
    "kural",
];

// Risk *türetme* sinyalleri (güven bağlamı — "güvende hissetmeli" → DerivesRisk).
// Not: bunlar packet type Risk YAPMAZ, UserVision kalır + DerivesRisk edge.
const RISK_SIGNAL_MARKERS: &[&str] = &["güven", "hissetmeli", "risk", "tehlike", "güvenlik"];

// Task *türetme* sinyalleri (Faz 5a — DerivesTask için). Packet type'dan bağımsız.
// Not: PR33a'da task signal + typed TaskCandidate:<Name> ref birlikte ister (extractor).
// Doğal dilden task adı türetme (NLP) PR33a dışı — typed ref zorunlu.
//
// D2 review patch: marker listesi `has_task_signal` içine inline edildi. `"task"`
// token-based eşleşir (substring değil) — `TaskCandidate:Foo` typed ref'i yanlışlıkla
// task signal saymasın diye. TR marker'lar substring (ek toleransı: "görevler" vb.).

#[cfg(test)]
mod tests {
    use super::*;

    fn cls() -> Classifier {
        Classifier::new()
    }

    #[test]
    fn precedence_antigoal_first() {
        // "olmamalı" → AntiGoal (Requirement marker'lar var olsa bile)
        assert_eq!(
            cls().classify("Controller'larda business logic olmamalı.", "tr"),
            ConceptPacketType::AntiGoal
        );
    }

    #[test]
    fn precedence_decision_before_assumption() {
        assert_eq!(
            cls().classify("Event Sourcing kararını referans alarak tasarla.", "tr"),
            ConceptPacketType::Decision
        );
    }

    #[test]
    fn precedence_assumption_explicit() {
        assert_eq!(
            cls().classify("Teknik bilgi seviyesi orta varsayılıyor.", "tr"),
            ConceptPacketType::Assumption
        );
    }

    #[test]
    fn precedence_user_vision_fix_001() {
        // fix_001: "Kullanıcı ödeme yaparken kendini güvende hissetmeli."
        assert_eq!(
            cls().classify("Kullanıcı ödeme yaparken kendini güvende hissetmeli.", "tr"),
            ConceptPacketType::UserVision
        );
    }

    #[test]
    fn precedence_requirement() {
        // "sistem" + "gerekir" ama kullanıcı/müşteri yok
        assert_eq!(
            cls().classify("Sistem logları 7 gün tutmalı.", "tr"),
            ConceptPacketType::Requirement
        );
    }

    #[test]
    fn precedence_risk_direct() {
        assert_eq!(
            cls().classify("Bu bir güvenlik açığı riski.", "tr"),
            ConceptPacketType::Risk
        );
    }

    #[test]
    fn default_assumption_when_no_marker() {
        assert_eq!(
            cls().classify("Belki hafta sonu bazı şeyleri gözden geçirmek lazım.", "tr"),
            ConceptPacketType::Assumption
        );
    }

    #[test]
    fn risk_signal_for_derives_risk_edge() {
        // fix_001: UserVision ama güven/hissetmeli → DerivesRisk sinyali
        assert!(cls().has_risk_signal("Kullanıcı ödeme yaparken kendini güvende hissetmeli."));
        assert!(!cls().has_risk_signal("Sistem logları tutmalı."));
    }

    #[test]
    fn rule_signal_for_derives_rule_edge() {
        assert!(cls().has_rule_signal("Domain katmanı Infrastructure'a bağımlı olmamalı."));
    }

    #[test]
    fn glossary_canonical_for_alias() {
        let g = Glossary::seed_default();
        assert_eq!(g.canonical_for("ödeme"), Some("Payment"));
        assert_eq!(g.canonical_for("Payment"), Some("Payment"));
        assert_eq!(g.canonical_for("güven"), Some("Trust"));
        assert_eq!(g.canonical_for("yok"), None);
    }

    #[test]
    fn glossary_matches_case_insensitive() {
        let g = Glossary::seed_default();
        assert!(g.matches("PAYMENT"));
        assert!(g.matches("Checkout"));
        assert!(!g.matches("Foo"));
    }

    #[test]
    fn glossary_aliases_of() {
        let g = Glossary::seed_default();
        let aliases = g.aliases_of("Payment");
        assert!(aliases.contains(&"ödeme".to_string()));
        assert!(aliases.contains(&"checkout".to_string()));
    }

    // ── Faz 5a (T9): has_task_signal ──────────────────────────────────────────

    #[test]
    fn task_signal_detects_markers() {
        let c = cls();
        assert!(c.has_task_signal("Bu bir görev olarak planlanmalı."));
        assert!(c.has_task_signal("AuthServiceRefactor task olarak işaretlendi."));
        assert!(c.has_task_signal("Bu özellik yapılmalı."));
        assert!(c.has_task_signal("Modül implement edilmeli."));
    }

    #[test]
    fn task_signal_absent_without_markers() {
        let c = cls();
        assert!(!c.has_task_signal("Ödeme modülü yüksek coupling'e sahip."));
        assert!(!c.has_task_signal("Bu bir görüştür."));
        assert!(!c.has_task_signal("Belki hafta sonu bakarız."));
    }

    #[test]
    fn task_signal_typed_ref_alone_is_not_signal() {
        // D2 review patch: "task" token-based — TaskCandidate:Foo typed ref tek başına
        // task signal sayılmaz. lowercase "taskcandidate:foo" → "task" substring var
        // AMA token split sonrası "taskcandidate" token'i "task" != eşit.
        let c = cls();
        assert!(
            !c.has_task_signal("TaskCandidate:AuthServiceRefactor sadece referans."),
            "typed ref tek başına task signal değil (D2 fix)"
        );
        assert!(
            !c.has_task_signal("TaskCandidate:SomeTask"),
            "TaskCandidate:X → 'task' substring ama token değil"
        );
        // Ama gerçek "task" token'i signal sayılır.
        assert!(
            c.has_task_signal("Bu bir task olarak planlandı."),
            " gerçek 'task' token → signal"
        );
    }
}
