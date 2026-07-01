//! FileMockLlm — JSON dosyasından scripted proposals yükleyen LlmClient adapter.
//! D1 MockLlmClient'ın dosya-tabanlı versiyonu. D3'te RuntimeLlmClient ile değiştirilebilir.
//!
//! **G2:** `call_count: AtomicUsize` (Cell → AtomicUsize) — `LlmClient: Send + Sync`
//! supertrait gereği (osp-core navigator trait güncellendi). AtomicUsize Sync'tir.

use osp_core::agent::DeltaProposal;
use osp_core::navigator::{LlmClient, LlmError};
use osp_core::trajectory::{AgentTaskView, TokenCost};
use std::sync::atomic::{AtomicUsize, Ordering};

/// JSON dosyasından yüklenen scripted proposals. `osp trajectory attempt --proposals file.json`.
pub struct FileMockLlm {
    proposals: Vec<DeltaProposal>,
    call_count: AtomicUsize,
}

impl FileMockLlm {
    pub fn new(proposals: Vec<DeltaProposal>) -> Self {
        Self {
            proposals,
            call_count: AtomicUsize::new(0),
        }
    }
}

impl LlmClient for FileMockLlm {
    fn complete(&self, _view: &AgentTaskView) -> Result<DeltaProposal, LlmError> {
        // MockLlmClient ile tutarlı: NoMoreProposals durumunda counter artmaz.
        let idx = self.call_count.load(Ordering::SeqCst);
        let proposal = self
            .proposals
            .get(idx)
            .cloned()
            .ok_or(LlmError::NoMoreProposals)?;
        self.call_count.store(idx + 1, Ordering::SeqCst);
        Ok(proposal)
    }

    fn last_token_cost(&self) -> TokenCost {
        TokenCost::default()
    }
}
