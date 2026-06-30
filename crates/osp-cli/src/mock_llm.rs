//! FileMockLlm — JSON dosyasından scripted proposals yükleyen LlmClient adapter.
//! D1 MockLlmClient'ın dosya-tabanlı versiyonu. D3'te RuntimeLlmClient ile değiştirilebilir.

use osp_core::agent::DeltaProposal;
use osp_core::navigator::{LlmClient, LlmError};
use osp_core::trajectory::{AgentTaskView, TokenCost};
use std::cell::Cell;

/// JSON dosyasından yüklenen scripted proposals. `osp trajectory attempt --proposals file.json`.
pub struct FileMockLlm {
    proposals: Vec<DeltaProposal>,
    call_count: Cell<usize>,
}

impl FileMockLlm {
    pub fn new(proposals: Vec<DeltaProposal>) -> Self {
        Self {
            proposals,
            call_count: Cell::new(0),
        }
    }
}

impl LlmClient for FileMockLlm {
    fn complete(&self, _view: &AgentTaskView) -> Result<DeltaProposal, LlmError> {
        let idx = self.call_count.get();
        let proposal = self
            .proposals
            .get(idx)
            .cloned()
            .ok_or(LlmError::NoMoreProposals)?;
        self.call_count.set(idx + 1);
        Ok(proposal)
    }

    fn last_token_cost(&self) -> TokenCost {
        TokenCost::default()
    }
}
