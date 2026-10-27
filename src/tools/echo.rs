//! Deliberately side-effect-free CLI tool. Hosts can supply another executor.
use crate::runtime::ToolExecutor;
use rig_core::completion::ToolDefinition;
use rig_core::message::ToolCall;
use serde::Deserialize;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition::new(
        "echo".try_into().expect("constant name"),
        "Return the supplied text",
        json!({
            "type": "object", "properties": {"text": {"type": "string"}},
            "required": ["text"], "additionalProperties": false,
        }),
    )
}
pub struct Echo;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    text: String,
}
impl ToolExecutor for Echo {
    async fn execute(&self, call: &ToolCall) -> Result<String, String> {
        if call.function.name.as_str() != "echo" || call.function.invalid_arguments.is_some() {
            return Err("unknown tool or malformed arguments".into());
        }
        let arguments: Arguments =
            serde_json::from_value(serde_json::Value::Object(call.function.arguments.clone()))
                .map_err(|_| "echo requires exactly one string field: text".to_owned())?;
        if arguments.text.len() > 64_000 {
            return Err("echo input too large".into());
        }
        Ok(arguments.text)
    }
}
