use super::{Control, Event, RunError, RunOptions, ToolExecutor, control};
use crate::session::{AgentSession, EntrySource, RunPhase};
use rig_core::completion::Message;
use rig_core::message::{ToolCall, ToolResultContent, UserContent};
use tokio::sync::mpsc;

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    session: &mut AgentSession,
    calls: Vec<ToolCall>,
    executor: &impl ToolExecutor,
    options: &RunOptions,
    controls: &mut mpsc::Receiver<Control>,
    deadline: tokio::time::Instant,
    emit: &mut dyn FnMut(Event),
) -> Result<(), RunError> {
    session.active_run.as_mut().expect("owned run").phase = RunPhase::ExecutingTools;
    session
        .active_run
        .as_mut()
        .expect("owned run")
        .pending_tools = calls
        .iter()
        .map(|call| crate::tools::PendingToolCall {
            id: call.id.to_string(),
            tool_name: call.function.name.to_string(),
            arguments: serde_json::Value::Object(call.function.arguments.clone()),
        })
        .collect();
    let mut results = Vec::new();
    let mut interrupted = None;
    for call in calls {
        let name = call.function.name.as_str();
        let result = if interrupted.is_some() {
            Err("Not executed: run interrupted".into())
        } else if session.tools.find(name).is_none()
            || !options.allowed_tools.iter().any(|n| n == name)
        {
            Err("Tool is not authorized by the host".into())
        } else if call.function.invalid_arguments.is_some() {
            Err("Malformed tool arguments".into())
        } else {
            emit(Event::ToolStarted {
                call_id: call.id.to_string(),
                name: name.into(),
            });
            match control::wait(
                executor.execute(&call),
                &mut session.steering,
                &mut session.followups,
                controls,
                deadline,
            )
            .await
            {
                Ok(Ok(text)) if text.len() <= options.max_tool_result_bytes => Ok(text),
                Ok(Ok(_)) => Err("Tool result exceeded byte limit".into()),
                Ok(Err(_)) => Err("Tool execution failed".into()),
                Err(error) => {
                    interrupted = Some(error);
                    Err("Tool interrupted: external outcome may be unknown; do not automatically retry".into())
                }
            }
        };
        let is_error = result.is_err();
        let content = vec![ToolResultContent::text(result.unwrap_or_else(|e| e))];
        let result = if is_error {
            call.error_result(content)
        } else {
            call.result(content)
        };
        results.push(UserContent::ToolResult(result));
        emit(Event::ToolFinished {
            call_id: call.id.to_string(),
            is_error,
        });
    }
    // Settle every advertised call, even on interruption, so recovery has no orphan calls.
    session
        .transcript
        .append(Message::User { content: results }, EntrySource::HostTool);
    session
        .active_run
        .as_mut()
        .expect("owned run")
        .pending_tools
        .clear();
    interrupted.map_or(Ok(()), Err)
}
