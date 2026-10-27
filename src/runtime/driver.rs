use super::{Control, Event, ModelBackend, RunError, RunOptions, ToolExecutor, control, execution};
use crate::prepare_context;
use crate::run::RunPhase;
use crate::session::{AgentSession, EntrySource};
use rig_core::completion::CompletionRequest;
use std::collections::HashSet;
use tokio::sync::mpsc;

#[allow(clippy::too_many_arguments)]
pub(super) async fn drive(
    session: &mut AgentSession,
    backend: &impl ModelBackend,
    executor: &impl ToolExecutor,
    options: &RunOptions,
    controls: &mut mpsc::Receiver<Control>,
    deadline: tokio::time::Instant,
    emit: &mut dyn FnMut(Event),
) -> Result<(), RunError> {
    let mut charged_tokens = 0_u64;
    loop {
        control::drain(session, controls)?;
        while let Some(input) = session.steering.pop_front() {
            control::accept(session, input, emit);
        }
        let state = session.active_run.as_mut().expect("owned run");
        if state.model_calls >= options.budget.max_model_calls {
            return Err(RunError::ModelCallBudget);
        }
        if charged_tokens >= options.budget.max_completion_tokens {
            return Err(RunError::TokenBudget);
        }
        state.phase = RunPhase::PreparingContext;
        let remaining = options.budget.max_completion_tokens - charged_tokens;
        let mut context =
            prepare_context(session, &options.instructions, options.max_context_bytes)
                .map_err(|_| RunError::Context)?;
        context.request.max_tokens = Some(remaining);
        if serde_json::to_vec(&context.request)
            .map_err(|_| RunError::Context)?
            .len()
            > options.max_context_bytes
        {
            return Err(RunError::Context);
        }
        let state = session.active_run.as_mut().expect("owned run");
        state.phase = RunPhase::CallingModel;
        state.model_calls += 1;
        emit(Event::ModelStarted {
            call: state.model_calls,
        });
        state.draft_response.clear();
        let mut stream_emit = |event| {
            if let Event::Stream {
                item:
                    rig_core::streaming::Item::Event(rig_core::streaming::StreamEvent::Text {
                        text,
                        ..
                    }),
            } = &event
            {
                state.draft_response.push_str(text);
            }
            emit(event);
        };
        let response = control::wait(
            backend.complete(
                &context.connection_id,
                context.request,
                options.max_response_bytes,
                &mut stream_emit,
            ),
            &mut session.steering,
            &mut session.followups,
            controls,
            deadline,
        )
        .await??;
        if serde_json::to_vec(&response)
            .map_err(|_| RunError::InvalidResponse)?
            .len()
            > options.max_response_bytes
        {
            return Err(RunError::ResponseTooLarge);
        }
        if !matches!(
            response.stop(),
            rig_core::message::StopReason::Stop | rig_core::message::StopReason::ToolUse
        ) {
            return Err(RunError::InvalidResponse);
        }
        let state = session.active_run.as_mut().expect("owned run");
        state.completion_tokens = state
            .completion_tokens
            .zip(response.usage.output_tokens)
            .and_then(|(total, used)| total.checked_add(used));
        let used = response.usage.output_tokens.unwrap_or(remaining);
        charged_tokens = charged_tokens.saturating_add(used);
        if used > remaining {
            return Err(RunError::TokenBudget);
        }
        let message = response.message().ok_or(RunError::InvalidResponse)?;
        CompletionRequest::new(message.clone())
            .validate_message_content()
            .map_err(|_| RunError::InvalidResponse)?;
        let calls: Vec<_> = response.tool_calls().cloned().collect();
        let mut ids = HashSet::new();
        if calls.len() > options.max_tools_per_turn
            || calls.iter().any(|c| !ids.insert(c.id.clone()))
        {
            return Err(RunError::InvalidResponse);
        }
        let entry = session.transcript.append(message, EntrySource::Model);
        emit(Event::TurnAccepted {
            entry_id: entry.value(),
            output_tokens: response.usage.output_tokens,
        });
        if !calls.is_empty() {
            execution::execute(session, calls, executor, options, controls, deadline, emit).await?;
            continue;
        }
        control::drain(session, controls)?;
        if !session.steering.is_empty() {
            continue;
        }
        if let Some(input) = session.followups.pop_front() {
            control::accept(session, input, emit);
            continue;
        }
        return Ok(());
    }
}
