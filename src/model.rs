use std::error::Error;
use std::fmt;

/// Selection only; credentials and live Rig clients belong to the runtime.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelSelection {
    pub connection_id: String,
    pub model_id: String,
}

impl ModelSelection {
    pub fn validate(&self) -> Result<(), ModelSelectionError> {
        if self.connection_id.trim().is_empty() || self.model_id.trim().is_empty() {
            return Err(ModelSelectionError);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSelectionError;

impl fmt::Display for ModelSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("connection ID and model ID cannot be empty")
    }
}

impl Error for ModelSelectionError {}
