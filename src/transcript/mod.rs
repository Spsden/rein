use rig_core::completion::Message;

/// Append-only accepted history, separate from inputs awaiting scheduling.
#[derive(Debug, Default)]
pub struct Transcript {
    entries: Vec<TranscriptEntry>,
}

impl Transcript {
    pub fn append(&mut self, message: Message, source: EntrySource) -> EntryId {
        let id = EntryId(self.entries.len() as u64 + 1);
        self.entries.push(TranscriptEntry {
            id,
            message,
            source,
        });
        id
    }

    pub fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    pub fn get(&self, id: EntryId) -> Option<&TranscriptEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

/// An identity scoped to its original session's transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntryId(u64);

impl EntryId {
    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub struct TranscriptEntry {
    pub id: EntryId,
    pub message: Message,
    pub source: EntrySource,
}

/// Attribution assigned by ingestion code; it is not an authorization grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EntrySource {
    User,
    Model,
    HostTool,
    Summary,
}

#[cfg(test)]
mod tests;
