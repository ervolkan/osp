//! TypedNodeRef — typed-prefix node ID parser ("Concept:Payment").
//!
//! Fixture target'ları ve ConceptNodeId'ler hep `"Kind:Name"` formatında.
//! Bu helper split'leri merkezileştirir (Faz 3 Kuzu persistence için de işe yarar).

use crate::anchoring::types::{ConceptNodeId, ConceptNodeKind};

/// Parse edilmiş typed node referansı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedNodeRef {
    pub kind: ConceptNodeKind,
    pub name: String,
}

impl TypedNodeRef {
    /// `"Concept:Payment"` → `Concept` + `"Payment"`. Geçersiz format/kind → `None`.
    pub fn parse(s: &str) -> Option<Self> {
        let (prefix, name) = s.split_once(':')?;
        let kind = ConceptNodeKind::from_prefix(prefix)?;
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Some(Self { kind, name: name.to_string() })
    }

    /// `"Concept:Payment"` formatına geri.
    pub fn to_id_string(&self) -> String {
        format!("{}:{}", self.kind.as_prefix(), self.name)
    }

    /// ConceptNodeId'ye.
    pub fn to_node_id(&self) -> ConceptNodeId {
        ConceptNodeId(self.to_id_string())
    }
}

impl std::fmt::Display for TypedNodeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_id_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_concept_prefix() {
        let r = TypedNodeRef::parse("Concept:Payment").unwrap();
        assert_eq!(r.kind, ConceptNodeKind::Concept);
        assert_eq!(r.name, "Payment");
    }

    #[test]
    fn parse_code_entity_candidate_prefix() {
        let r = TypedNodeRef::parse("CodeEntityCandidate:PaymentModule").unwrap();
        assert_eq!(r.kind, ConceptNodeKind::CodeEntityCandidate);
        assert_eq!(r.name, "PaymentModule");
    }

    #[test]
    fn parse_decision_prefix() {
        let r = TypedNodeRef::parse("Decision:NoDirectPaymentProviderDependency").unwrap();
        assert_eq!(r.kind, ConceptNodeKind::Decision);
    }

    #[test]
    fn parse_risk_candidate_prefix() {
        let r = TypedNodeRef::parse("RiskCandidate:PaymentTrustLoss").unwrap();
        assert_eq!(r.kind, ConceptNodeKind::RiskCandidate);
    }

    #[test]
    fn parse_invalid_no_colon() {
        assert!(TypedNodeRef::parse("Payment").is_none());
    }

    #[test]
    fn parse_invalid_unknown_kind() {
        assert!(TypedNodeRef::parse("Foo:Bar").is_none());
    }

    #[test]
    fn parse_invalid_empty_name() {
        assert!(TypedNodeRef::parse("Concept:").is_none());
        assert!(TypedNodeRef::parse("Concept:   ").is_none());
    }

    #[test]
    fn roundtrip_to_id_string() {
        let r = TypedNodeRef::parse("Concept:Payment").unwrap();
        assert_eq!(r.to_id_string(), "Concept:Payment");
        assert_eq!(r.to_node_id(), ConceptNodeId("Concept:Payment".to_string()));
    }
}
