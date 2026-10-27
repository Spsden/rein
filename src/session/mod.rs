use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use crate::model::ModelSelectionError;

// Preserve the existing session import paths while each type has its own owner.
pub use crate::memory::{WorkingMemory, WorkingNote};
pub use crate::model::ModelSelection;
pub use crate::run::{RunBudget, RunPhase, RunState};
pub use crate::tools::{PendingToolCall, ToolCatalog};
pub use crate::transcript::{EntryId, EntrySource, Transcript, TranscriptEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    EmptyId,
    InvalidModelSelection(ModelSelectionError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => f.write_str("session ID cannot be empty"),
            Self::InvalidModelSelection(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidModelSelection(error) => Some(error),
            Self::EmptyId => None,
        }
    }
}

/// One conversation's state. The runtime is responsible for scheduling mutations.
#[derive(Debug)]
pub struct AgentSession {
    id: String,
    pub transcript: Transcript,
    pub working_memory: WorkingMemory,
    pub tools: ToolCatalog,
    pub model: ModelSelection,
    pub steering: VecDeque<UserInput>,
    pub followups: VecDeque<UserInput>,
    pub active_run: Option<RunState>,
}

impl AgentSession {
    pub fn new(
        id: impl Into<String>,
        model: ModelSelection,
        tools: ToolCatalog,
    ) -> Result<Self, SessionError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(SessionError::EmptyId);
        }
        model
            .validate()
            .map_err(SessionError::InvalidModelSelection)?;
        Ok(Self {
            id,
            transcript: Transcript::default(),
            working_memory: WorkingMemory::default(),
            tools,
            model,
            steering: VecDeque::new(),
            followups: VecDeque::new(),
            active_run: None,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct UserInput {
    pub id: String,
    pub text: String,
}

#[cfg(test)]
mod tests;
