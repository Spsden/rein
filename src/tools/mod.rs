use std::collections::HashSet;
pub mod echo;
use std::error::Error;
use std::fmt;

use rig_core::completion::ToolDefinition;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCatalogError {
    DuplicateName(String),
}

impl fmt::Display for ToolCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateName(name) => write!(f, "duplicate tool name: {name}"),
        }
    }
}

impl Error for ToolCatalogError {}

/// Model-visible definitions. An executor owns implementations and permissions.
#[derive(Debug, Default)]
pub struct ToolCatalog {
    definitions: Vec<ToolDefinition>,
}

impl ToolCatalog {
    pub fn new(definitions: Vec<ToolDefinition>) -> Result<Self, ToolCatalogError> {
        let mut names = HashSet::new();
        for definition in &definitions {
            let name = definition.name.as_str();
            if !names.insert(name) {
                return Err(ToolCatalogError::DuplicateName(name.to_owned()));
            }
        }
        Ok(Self { definitions })
    }

    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }

    pub fn find(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.name.as_str() == name)
    }
}

#[derive(Debug)]
pub struct PendingToolCall {
    pub id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[cfg(test)]
mod tests;
