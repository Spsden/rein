use serde_json::{Map, Value, json};

use crate::memory::{WorkingMemory, WorkingNote};
use crate::transcript::Transcript;

use super::ContextError;

pub(super) fn prepare_memory(
    memory: &WorkingMemory,
    transcript: &Transcript,
) -> Result<Option<Value>, ContextError> {
    for note in memory.choices.values().chain(memory.progress.iter()) {
        for &id in &note.evidence {
            if transcript.get(id).is_none() {
                return Err(ContextError::MissingEvidence(id));
            }
        }
    }
    if memory.goal.is_none()
        && memory.choices.is_empty()
        && memory.progress.is_empty()
        && memory.next_step.is_none()
    {
        return Ok(None);
    }
    let choices: Map<_, _> = memory
        .choices
        .iter()
        .map(|(name, note)| (name.clone(), note_value(note)))
        .collect();
    Ok(Some(json!({
        "goal": memory.goal,
        "choices": choices,
        "progress": memory.progress.iter().map(note_value).collect::<Vec<_>>(),
        "next_step": memory.next_step,
    })))
}

fn note_value(note: &WorkingNote) -> Value {
    json!({
        "text": note.text,
        "evidence": note.evidence.iter().map(|id| id.value()).collect::<Vec<_>>(),
    })
}
