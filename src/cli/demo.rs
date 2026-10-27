use rein::runtime::{Event, ModelBackend, RunError};
use rig_core::completion::{CompletionRequest, CompletionResponse, FinishReason, Message, Usage};
use rig_core::message::{AssistantContent, Origin, ToolCall, ToolFunction, UserContent};
use serde_json::json;

/// Offline deterministic demonstration: user -> echo -> assistant. Not an LLM.
pub(super) struct Demo;
impl ModelBackend for Demo {
    async fn complete(
        &self,
        _: &str,
        request: CompletionRequest,
        _: usize,
        _: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        let last = request.chat_history.last().ok_or(RunError::Model)?;
        let (content, reason) = match last {
            Message::User { content }
                if content
                    .iter()
                    .any(|c| matches!(c, UserContent::ToolResult(_))) =>
            {
                (
                    vec![AssistantContent::text(
                        "Demo complete: the echo tool returned successfully.",
                    )],
                    FinishReason::Stop,
                )
            }
            Message::User { content } => {
                let text = content
                    .iter()
                    .filter_map(|c| match c {
                        UserContent::Text(t) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                (
                    vec![AssistantContent::ToolCall(ToolCall::from_wire(
                        "demo-call",
                        ToolFunction::new(
                            "echo".try_into().expect("constant"),
                            json!({"text": text}),
                        ),
                    ))],
                    FinishReason::ToolCalls,
                )
            }
            _ => return Err(RunError::Model),
        };
        Ok(CompletionResponse::new(
            content,
            Usage::new().output_tokens(1),
            Origin::new("demo", "demo", "demo"),
            json!({}),
        )
        .with_finish_reason(reason))
    }
}
