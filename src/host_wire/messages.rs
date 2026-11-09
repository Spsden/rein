use rein::runtime::RunError;
use rig_core::completion::Message;
use rig_core::message::{AssistantContent, ToolCall, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum InitialMessage {
    User {
        text: String,
    },
    Assistant {
        text: String,
        reasoning: Option<String>,
    },
}

impl InitialMessage {
    pub(super) fn into_message(self) -> Message {
        match self {
            Self::User { text } => Message::user(text),
            Self::Assistant { text, reasoning } => {
                let mut message = Message::assistant(text);
                if let (Message::Assistant(turn), Some(reasoning)) = (&mut message, reasoning)
                    && !reasoning.trim().is_empty()
                {
                    turn.content.push(AssistantContent::reasoning(reasoning));
                }
                message
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Call {
    id: String,
    name: String,
    arguments: Map<String, Value>,
}

impl Call {
    pub(super) fn into_content(self) -> Result<AssistantContent, RunError> {
        if self.id.trim().is_empty()
            || self.id.len() > 256
            || self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(RunError::InvalidResponse);
        }
        Ok(AssistantContent::tool_call(
            self.id,
            self.name
                .try_into()
                .map_err(|_| RunError::InvalidResponse)?,
            self.arguments.into(),
        ))
    }
}

impl From<&ToolCall> for Call {
    fn from(call: &ToolCall) -> Self {
        Self {
            id: call.id.to_string(),
            name: call.function.name.to_string(),
            arguments: call.function.arguments.clone(),
        }
    }
}

pub(super) fn project(messages: &[Message]) -> Result<Vec<Value>, RunError> {
    let mut result = Vec::new();
    for message in messages {
        match message {
            Message::System { content } => result.push(json!({"role":"system","text":content})),
            Message::User { content } => {
                for part in content {
                    result.push(match part {
                        UserContent::Text(text) => json!({"role":"user","text":text.text}),
                        UserContent::ToolResult(tool) => {
                            let text: String = tool.content.iter().map(|part| part.as_text().ok_or(RunError::Context)).collect::<Result<_,_>>()?;
                            json!({"role":"tool","text":text,"call_id":tool.call.to_string(),"name":tool.name.to_string()})
                        }
                        _ => return Err(RunError::Context),
                    });
                }
            }
            Message::Assistant(turn) => {
                let mut text = String::new();
                let mut reasoning = String::new();
                let mut calls = Vec::new();
                for part in &turn.content {
                    match part {
                        AssistantContent::Text(part) => text.push_str(&part.text),
                        AssistantContent::Reasoning(part) => reasoning.push_str(&part.text),
                        AssistantContent::ToolCall(call) => calls.push(Call::from(call)),
                        _ => return Err(RunError::Context),
                    }
                }
                result.push(json!({
                    "role":"assistant","text":text,"reasoning":reasoning,"calls":calls,
                    "response_id":turn.origin.as_ref().and_then(|o| o.response_id.as_deref())
                }));
            }
        }
    }
    Ok(result)
}
