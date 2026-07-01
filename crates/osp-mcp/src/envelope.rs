//! Standart output envelope (`docs/mcp-design.md` §7).
//!
//! Her tool ortak envelope döner — AI agent deterministic error code ile kendini
//! düzeltir (örn `MANEUVER_LIMIT_EXCEEDED` → "başka approach dene", `TASK_NOT_FOUND`
//! → "listele ve tekrar dene").
//!
//! ## Success
//! ```json
//! { "ok": true, "schema_version": "osp.mcp.v1", "request_id": "...", "tool": "...",
//!   "result": {...}, "invariants_checked": ["INV-T3"], "warnings": [] }
//! ```
//!
//! ## Error
//! ```json
//! { "ok": false, "schema_version": "osp.mcp.v1", "request_id": "...", "tool": "...",
//!   "error_code": "TASK_NOT_FOUND", "message": "task 42 not found",
//!   "invariants_checked": ["INV-T5"], "recoverable": true }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Envelope schema versiyonu ( Paper 1 v1'le uyumlu, gelecekte bump edilebilir).
pub const SCHEMA_VERSION: &str = "osp.mcp.v1";

/// Monotonik request counter (request_id üretimi için — deterministic değil, unique).
static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Yeni request_id üret (`req_<n>`).
pub fn next_request_id() -> String {
    let n = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req_{n}")
}

/// Deterministic error codes (`docs/mcp-design.md` §7). Agent bu kod ile self-correct eder.
///
/// **Kritik:** Bu kodlar invariant kökenli — her biri hangi INV'yi ihlal ettiğini söyler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // ── INV-T1 (target coordinate leak) ────────────────────────────────────
    /// AgentTaskView serialization contained preferred_vector — INV-T1 ihlali.
    /// RECOVERABLE DEĞİL — bu olursa MCP implementasyon hatası (panic-level).
    TargetCoordinateLeakBlocked,

    // ── INV-T2 (operator-only) ─────────────────────────────────────────────
    /// Agent mode'da operator tool çağrıldı (trajectory_init, task_add, ...).
    /// Recoverable: operator mode ile tekrar dene (insan/trusted orchestrator).
    OperatorCapabilityRequired,

    // ── INV-T4 (source provenance) ─────────────────────────────────────────
    /// Placeholder/heuristic source ile task kapatılamaz.
    /// Recoverable: gerçek SCIP ölçüm gerek (workspace'i SCIP index ile analyze et).
    PlaceholderMetricInsufficient,

    // ── INV-T7 (maneuver limit) ────────────────────────────────────────────
    /// N ardışık reject → operator approval gerek.
    /// Recoverable: başka yaklaşım dene veya operator approval iste.
    ManeuverLimitExceeded,

    // ── Navigator loop result (G2) ─────────────────────────────────────────
    /// Navigator `RequiresOperatorApproval` döndü (mutation decision).
    /// Recoverable: operator approval iste veya task predicate'ini gözden geçir.
    OperatorApprovalRequired,
    /// Navigator `LlmError` döndü (network/parse/no-more-proposals).
    /// Recoverable: model/endpoint kontrol et, tekrar dene.
    NavigatorLlmError,

    // ── INV-T5 (Task≠Claim binding) ────────────────────────────────────────
    /// claim task_id None (standalone) veya resolver'da task bulunamadı.
    /// Recoverable: task listele, doğru task_id ile retry.
    TaskNotFound,

    // ── Workspace security ─────────────────────────────────────────────────
    /// Workspace register edilmemiş (raw path verilmiş — allowlist dışı).
    /// Recoverable: operator `--workspace` ile server başlat.
    WorkspaceNotRegistered,

    // ── Generic ────────────────────────────────────────────────────────────
    /// DeltaProposal JSON parse hatası (Q4 syntax agent-shell'de).
    /// Recoverable: DeltaProposal şemasını düzelt ve retry.
    InvalidDeltaProposal,
    /// Tool input şema hatası (eksik/zorunlu alan).
    InvalidToolInput,
    /// İç hata (engine/analyze failure). Recoverable: workspace'i tekrar analyze et.
    InternalError,
}

/// Envelope error (error_code + message + invariants + recoverable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeError {
    pub error_code: ErrorCode,
    pub message: String,
    pub invariants_checked: Vec<String>,
    pub recoverable: bool,
}

impl EnvelopeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let (invariants, recoverable) = code.inv_context();
        Self {
            error_code: code,
            message: message.into(),
            invariants_checked: invariants,
            recoverable,
        }
    }
}

impl ErrorCode {
    /// Bu error code hangi INV'leri test etti + recoverable mı?
    fn inv_context(&self) -> (Vec<String>, bool) {
        match self {
            ErrorCode::TargetCoordinateLeakBlocked => (vec!["INV-T1".into()], false),
            ErrorCode::OperatorCapabilityRequired => (vec!["INV-T2".into()], true),
            ErrorCode::PlaceholderMetricInsufficient => (vec!["INV-T4".into()], true),
            ErrorCode::ManeuverLimitExceeded => (vec!["INV-T7".into()], true),
            ErrorCode::OperatorApprovalRequired => (vec!["INV-T7".into()], true),
            ErrorCode::NavigatorLlmError => (vec!["navigator".into()], true),
            ErrorCode::TaskNotFound => (vec!["INV-T5".into()], true),
            ErrorCode::WorkspaceNotRegistered => (vec!["INV-security".into()], true),
            ErrorCode::InvalidDeltaProposal => (vec!["INV-#12".into()], true),
            ErrorCode::InvalidToolInput => (vec![], true),
            ErrorCode::InternalError => (vec![], true),
        }
    }
}

/// Standart output envelope. Success = { ok: true, result }; Error = { ok: false, error }.
///
/// **Serde untagged DEĞİL** — `ok` field ayrımı ile discriminated union. Bu sayede
/// agent parse ederken "ok mu değil mi" önceden karar verebilir.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "ok")]
pub enum McpEnvelope {
    /// Başarılı tool çıktısı.
    #[serde(rename = "true")]
    Success {
        schema_version: String,
        request_id: String,
        tool: String,
        result: serde_json::Value,
        invariants_checked: Vec<String>,
        #[serde(default)]
        warnings: Vec<String>,
    },
    /// Hata çıktısı (deterministic error code).
    #[serde(rename = "false")]
    Error {
        schema_version: String,
        request_id: String,
        tool: String,
        error: EnvelopeError,
    },
}

impl McpEnvelope {
    /// Success envelope üret. `result` serialize edilmiş JSON value (caller serialize eder).
    pub fn success(tool: &str, result: serde_json::Value, invariants_checked: Vec<String>) -> Self {
        Self::Success {
            schema_version: SCHEMA_VERSION.into(),
            request_id: next_request_id(),
            tool: tool.to_string(),
            result,
            invariants_checked,
            warnings: Vec::new(),
        }
    }

    /// Error envelope üret (deterministic error code ile).
    pub fn error(tool: &str, err: EnvelopeError) -> Self {
        Self::Error {
            schema_version: SCHEMA_VERSION.into(),
            request_id: next_request_id(),
            tool: tool.to_string(),
            error: err,
        }
    }

    /// INV-T1 ⭐ leak tespiti — serialized envelope'da "preferred_vector" / "target_region"
    /// / "milestone_target_vector" string geçiyorsa panic-level ihlal. Bu method her
    /// agent-facing tool çağrısından SONRA çalışır; leak varsa `TargetCoordinateLeakBlocked`
    /// envelope döner (asıl result çöpe gider).
    ///
    /// **Kritik:** Bu test SADECE agent-facing read tools için. Operator-only tools
    /// (`trajectory_init`, ...) coordinate içerebilir (operator görebilir).
    pub fn assert_no_coordinate_leak(&self, tool: &str) -> Self {
        let json = match serde_json::to_string(self) {
            Ok(s) => s,
            Err(_) => return self.clone(),
        };
        for forbidden in &[
            "preferred_vector",
            "target_region",
            "milestone_target_vector",
        ] {
            if json.contains(forbidden) {
                // INV-T1 ihlali — leak var. Asıl envelope çöpe, hata envelope döner.
                return Self::error(
                    tool,
                    EnvelopeError::new(
                        ErrorCode::TargetCoordinateLeakBlocked,
                        format!(
                            "AgentTaskView serialization contained '{forbidden}' — INV-T1 violation"
                        ),
                    ),
                );
            }
        }
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_envelope_has_schema_version() {
        let env = McpEnvelope::success(
            "osp_check_predicate",
            serde_json::json!({ "predicate_completion": "Completed" }),
            vec!["INV-T3".into(), "INV-T4".into()],
        );
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("osp.mcp.v1"));
        assert!(json.contains("\"ok\":\"true\""));
        assert!(json.contains("INV-T3"));
    }

    #[test]
    fn error_envelope_has_deterministic_code() {
        let env = McpEnvelope::error(
            "osp_get_agent_task_view",
            EnvelopeError::new(ErrorCode::TaskNotFound, "task 42 not found in resolver"),
        );
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("TASK_NOT_FOUND"));
        assert!(json.contains("\"ok\":\"false\""));
        assert!(json.contains("INV-T5"));
        assert!(json.contains("\"recoverable\":true"));
    }

    #[test]
    fn leak_detection_replaces_envelope_when_preferred_vector_present() {
        // Bir "leak içeren" success envelope üret — result preferred_vector taşıyor.
        let leaky = McpEnvelope::success(
            "osp_get_agent_task_view",
            serde_json::json!({ "preferred_vector": { "x": 0.5 } }),
            vec![],
        );
        let checked = leaky.assert_no_coordinate_leak("osp_get_agent_task_view");
        match checked {
            McpEnvelope::Error { error, .. } => {
                assert_eq!(error.error_code, ErrorCode::TargetCoordinateLeakBlocked);
                assert!(!error.recoverable);
            }
            _ => panic!("INV-T1 leak should have been caught"),
        }
    }

    #[test]
    fn leak_detection_passes_when_clean() {
        let clean = McpEnvelope::success(
            "osp_get_agent_task_view",
            serde_json::json!({ "task_id": 1, "label": "clean view" }),
            vec!["INV-T1".into()],
        );
        let checked = clean.assert_no_coordinate_leak("osp_get_agent_task_view");
        assert!(matches!(checked, McpEnvelope::Success { .. }));
    }

    #[test]
    fn error_codes_have_inv_context() {
        assert_eq!(
            ErrorCode::TargetCoordinateLeakBlocked.inv_context(),
            (vec!["INV-T1".into()], false)
        );
        assert!(ErrorCode::OperatorCapabilityRequired.inv_context().1);
        assert!(ErrorCode::ManeuverLimitExceeded.inv_context().1);
        assert!(ErrorCode::PlaceholderMetricInsufficient.inv_context().1);
    }
}
