//! INV-C11 agent-surface regression test — dar adlandırma (Review 1#17/R2#1/R3#10).
//!
//! **Kapsam:** osp-mcp source'unda review/supersede authority tool'ları yokluğu.
//! **INV-C11'in tamamı DEĞİL** — INV-C11 deployment boundary'dir; bu test yalnızca
//! *evaluated agent-facing MCP surface*'in review/supersession authority operations
//! kaydetmediğini (statik kaynak taraması ile) doğrular (partial deployment-surface
//! check). Process-level isolation (agent'ın shell erişimi, credential isolation)
//! INV-C11'in deployment sorumluluğudur, bu test onu kanıtlamaz.
//!
//! **Neden statik tarama:** `get_tool_router`/`OspMcpServer::get_tool_router` private;
//! osp-mcp bir binary crate (library değil). Runtime `ToolRouter::list_all()` çağrısı
//! library expose gerektirir — bu test için overkill. Statik tarama eşit derecede
//! regression koruması sağlar: biri `#[tool(name = "osp_review_...")]` eklerse kırılır.
//!
//! Bu test anlamlı hale geldi: CLI `osp review` operator-facing yüzeyi tanımlandıktan
//! sonra, agent-facing MCP yüzeyinde bu authority operations'ların OLMAMASI sabitlenir.

use std::path::PathBuf;

/// MCP server.rs kaynak kodunun yolunu bul (crate root'a göreli).
fn server_src_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("server.rs")
}

/// Agent-facing MCP surface'de review/supersede authority operations yokluğunu doğrula.
///
/// `#[tool(name = "osp_review...")]` veya `#[tool(name = "osp_supersede...")]` gibi
/// authority tool literal'ları source'da geçmemeli. MCP = agent yüzeyi; review/supersede
/// = CLI operator yüzeyi (INV-C11 yeniden sınıflandırma — Review 2#1).
#[test]
fn agent_mcp_source_excludes_operator_authority_tools() {
    let src = std::fs::read_to_string(server_src_path())
        .expect("src/server.rs readable — CARGO_MANIFEST_DIR doğru olmalı");

    // Authority operation tool isimleri — MCP surface'de OLMAMALI.
    let forbidden_tool_names = [
        "osp_review",
        "osp_review_accept",
        "osp_review_reject",
        "osp_review_list",
        "osp_review_show",
        "osp_supersede",
        "osp_open_for_operator",
        "osp_operator_session",
        "osp_supersede_session",
    ];

    for forbidden in &forbidden_tool_names {
        let pattern = format!("name = \"{forbidden}\"");
        assert!(
            !src.contains(&pattern),
            "INV-C11 violation: agent-facing MCP source contains tool '{forbidden}' \
             (pattern `{pattern}`). Operator authority operations must NOT be exposed \
             on the agent surface — they belong to the CLI `osp review` operator surface."
        );
    }

    // Bilinen 8 agent-facing tool'un varlığını doğrula (regression — biri kaldırırsa).
    let expected_tools = [
        "osp_analyze_workspace",
        "osp_get_agent_task_view",
        "osp_check_predicate",
        "osp_submit_delta",
        "osp_trajectory_init",
        "osp_task_add",
        "osp_run_task",
        "osp_get_attempt_history",
    ];
    let tool_count = expected_tools
        .iter()
        .filter(|name| src.contains(&format!("name = \"{name}\"")))
        .count();
    assert_eq!(
        tool_count,
        expected_tools.len(),
        "expected {} agent-facing MCP tools, found {tool_count} — registry changed, update test",
        expected_tools.len()
    );
}

/// `OperatorReviewSession::open_for_operator` / `SupersedeSession::open_for_operator`
/// çağrılarının MCP server source'unda OLMAMASI (INV-C11 — bu constructor'lar operator
/// yüzeyine ait, agent yüzeyine değil).
#[test]
fn agent_mcp_source_excludes_operator_session_constructors() {
    let src = std::fs::read_to_string(server_src_path()).expect("src/server.rs readable");

    // Operator session constructor'ları — MCP source'da çağrılmamalı.
    assert!(
        !src.contains("OperatorReviewSession::open_for_operator"),
        "INV-C11 violation: MCP source invokes OperatorReviewSession::open_for_operator — \
         operator review authority belongs to CLI operator surface, not agent-facing MCP"
    );
    assert!(
        !src.contains("SupersedeSession::open_for_operator"),
        "INV-C11 violation: MCP source invokes SupersedeSession::open_for_operator — \
         supersede authority belongs to CLI operator surface, not agent-facing MCP"
    );
}
