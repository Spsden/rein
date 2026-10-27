use std::time::Duration;

use crate::tools::PendingToolCall;

/// Attempt data. Live streams, timers and cancellation handles are runtime-owned.
#[derive(Debug)]
pub struct RunState {
    pub id: String,
    pub phase: RunPhase,
    pub budget: RunBudget,
    pub model_calls: u32,
    pub completion_tokens: Option<u64>,
    pub pending_tools: Vec<PendingToolCall>,
    pub draft_response: String,
}

#[derive(Debug, Clone)]
pub struct RunBudget {
    pub max_model_calls: u32,
    pub max_completion_tokens: u64,
    pub max_active_time: Duration,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            max_model_calls: 16,
            max_completion_tokens: 32_768,
            max_active_time: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    PreparingContext,
    CallingModel,
    WaitingForCheckpoint,
    WaitingForApproval,
    ExecutingTools,
    Finalizing,
}
