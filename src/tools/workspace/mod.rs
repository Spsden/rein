//! Capability-scoped text tools. The host explicitly grants a workspace and writes.
mod catalog;
mod files;

use crate::runtime::{ToolError, ToolExecutor};
use cap_std::fs::Dir;
use rig_core::completion::ToolDefinition;
use rig_core::message::ToolCall;
use serde::de::DeserializeOwned;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const MAX_FILE_BYTES: usize = 256_000;
const MAX_OUTPUT_BYTES: usize = 48_000;

#[derive(Clone)]
pub struct WorkspaceTools {
    directory: Arc<Mutex<Dir>>,
    writable: bool,
}

impl WorkspaceTools {
    /// Opens a directory capability once. All subsequent paths are relative to it.
    pub fn read_only(root: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self {
            directory: Arc::new(Mutex::new(Dir::open_ambient_dir(
                root,
                cap_std::ambient_authority(),
            )?)),
            writable: false,
        })
    }

    /// Host authorization, never a value selected by a model or restored snapshot.
    pub fn with_edits(mut self) -> Self {
        self.writable = true;
        self
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        catalog::definitions(self.writable)
    }

    fn dispatch(&self, call: &ToolCall) -> Result<String, ToolError> {
        let directory = self
            .directory
            .lock()
            .map_err(|_| ToolError::Host("workspace lock poisoned".into()))?;
        let value = serde_json::Value::Object(call.function.arguments.clone());
        if call.function.invalid_arguments.is_some() {
            return Err(ToolError::InvalidArguments(
                "arguments must be a JSON object",
            ));
        }
        match call.function.name.as_str() {
            "list_files" => files::list(&directory, arguments(value)?),
            "read_file" => files::read(&directory, arguments(value)?),
            "search" => files::search(&directory, arguments(value)?),
            "edit_file" if self.writable => files::edit(&directory, arguments(value)?),
            "write_file" if self.writable => files::write(&directory, arguments(value)?),
            _ => Err(ToolError::InvalidArguments(
                "tool not available in this workspace policy",
            )),
        }
    }
}

fn arguments<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(|_| {
        ToolError::InvalidArguments(
            "fields must exactly match the tool schema, including required types",
        )
    })
}

impl ToolExecutor for WorkspaceTools {
    async fn execute(&self, call: &ToolCall) -> Result<String, ToolError> {
        let tools = self.clone();
        let call = call.clone();
        tokio::task::spawn_blocking(move || tools.dispatch(&call))
            .await
            .map_err(|_| ToolError::Host("workspace worker failed".into()))?
    }
}

#[cfg(test)]
mod tests;
