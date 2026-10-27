//! Versioned bounded snapshots. One writer per path is required.
use crate::memory::WorkingMemory;
use crate::model::ModelSelection;
use crate::session::UserInput;
use crate::tools::ToolCatalog;
use crate::transcript::EntrySource;
use crate::{AgentSession, prepare_context};
use rig_core::completion::{Message, ToolDefinition};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub const MAX_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;
#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    InvalidSnapshot,
    ActiveRun,
    TooLarge,
}
impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "session IO failed",
            Self::InvalidSnapshot => "invalid or incompatible session snapshot",
            Self::ActiveRun => "cannot snapshot an active run",
            Self::TooLarge => "session snapshot exceeds byte limit",
        })
    }
}
impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    rig_version: String,
    id: String,
    model: ModelSelection,
    working_memory: WorkingMemory,
    tools: Vec<ToolDefinition>,
    transcript: Vec<StoredEntry>,
    steering: VecDeque<UserInput>,
    followups: VecDeque<UserInput>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEntry {
    message: Message,
    source: EntrySource,
}

pub fn save(session: &AgentSession, path: impl AsRef<Path>) -> Result<(), StorageError> {
    if session.active_run.is_some() {
        return Err(StorageError::ActiveRun);
    }
    let value = serde_json::json!({
        "version": 1, "rig_version": "0.44.0", "id": session.id(),
        "model": session.model, "working_memory": session.working_memory,
        "tools": session.tools.definitions(),
        "transcript": session.transcript.entries().iter().map(|e| StoredEntry { message: e.message.clone(), source: e.source }).collect::<Vec<_>>(),
        "steering": session.steering, "followups": session.followups,
    });
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| StorageError::InvalidSnapshot)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(StorageError::TooLarge);
    }
    decode(&bytes)?;
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|e| StorageError::Io(e.error))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn load(path: impl AsRef<Path>) -> Result<AgentSession, StorageError> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(StorageError::TooLarge);
    }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Result<AgentSession, StorageError> {
    let snapshot: Snapshot =
        serde_json::from_slice(bytes).map_err(|_| StorageError::InvalidSnapshot)?;
    if snapshot.version != 1
        || snapshot.rig_version != "0.44.0"
        || snapshot.steering.len() > 64
        || snapshot.followups.len() > 64
        || snapshot
            .steering
            .iter()
            .chain(&snapshot.followups)
            .any(|i| i.id.trim().is_empty() || i.text.trim().is_empty() || i.text.len() > 1_000_000)
    {
        return Err(StorageError::InvalidSnapshot);
    }
    let tools = ToolCatalog::new(snapshot.tools).map_err(|_| StorageError::InvalidSnapshot)?;
    let mut session = AgentSession::new(snapshot.id, snapshot.model, tools)
        .map_err(|_| StorageError::InvalidSnapshot)?;
    for entry in snapshot.transcript {
        session.transcript.append(entry.message, entry.source);
    }
    session.working_memory = snapshot.working_memory;
    session.steering = snapshot.steering;
    session.followups = snapshot.followups;
    if !session.transcript.entries().is_empty() {
        prepare_context(&session, "", MAX_SNAPSHOT_BYTES)
            .map_err(|_| StorageError::InvalidSnapshot)?;
    } else if session
        .working_memory
        .choices
        .values()
        .chain(&session.working_memory.progress)
        .any(|n| !n.evidence.is_empty())
    {
        return Err(StorageError::InvalidSnapshot);
    }
    Ok(session)
}

#[cfg(test)]
mod tests;
