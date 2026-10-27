use std::collections::HashMap;

use rig_core::completion::Message;
use rig_core::message::UserContent;
use rig_core::transcript::validate_canonical;

use super::ContextError;

pub(super) fn validate_history(messages: &[Message]) -> Result<(), ContextError> {
    validate_canonical(messages).map_err(ContextError::InvalidTranscript)?;
    let mut pending = HashMap::new();
    for message in messages {
        match message {
            Message::Assistant(turn) => {
                pending.clear();
                for call in turn.tool_calls() {
                    if pending.insert(&call.id, &call.function.name).is_some() {
                        return Err(ContextError::InvalidToolCalls);
                    }
                }
            }
            Message::User { content } => {
                for part in content {
                    match part {
                        UserContent::ToolResult(result) if result.content.is_empty() => {
                            return Err(ContextError::InvalidMessageContent);
                        }
                        UserContent::ToolResult(result)
                            if pending.get(&result.call).copied() != Some(&result.name) =>
                        {
                            return Err(ContextError::InvalidToolCalls);
                        }
                        _ => {}
                    }
                }
                pending.clear();
            }
            Message::System { .. } => unreachable!("transcript system messages were rejected"),
        }
    }
    Ok(())
}
