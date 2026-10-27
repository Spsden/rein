use std::collections::BTreeMap;

use crate::transcript::EntryId;

/// Task knowledge. Long-term memory and private model reasoning are separate.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WorkingMemory {
    pub goal: Option<String>,
    // Stable key order keeps generated context independent of insertion order.
    pub choices: BTreeMap<String, WorkingNote>,
    pub progress: Vec<WorkingNote>,
    pub next_step: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkingNote {
    pub text: String,
    pub evidence: Vec<EntryId>,
}
