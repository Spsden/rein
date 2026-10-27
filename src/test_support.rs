use rig_core::completion::ToolDefinition;
use serde_json::json;

use crate::model::ModelSelection;
use crate::session::AgentSession;
use crate::tools::ToolCatalog;

pub(crate) fn model() -> ModelSelection {
    ModelSelection {
        connection_id: "local".into(),
        model_id: "test-model".into(),
    }
}

pub(crate) fn session(id: &str) -> AgentSession {
    AgentSession::new(id, model(), ToolCatalog::default()).unwrap()
}

pub(crate) fn tool(name: &str) -> ToolDefinition {
    ToolDefinition::new(
        name.try_into().unwrap(),
        "Test tool",
        json!({"type": "object", "properties": {}}),
    )
}
