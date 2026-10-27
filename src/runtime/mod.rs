//! Rein's loop. Provider and tool implementations are supplied by the host.
mod control;
mod driver;
mod execution;
mod types;
use crate::run::{RunPhase, RunState};
use crate::session::{AgentSession, UserInput};
use tokio::sync::mpsc;
pub use types::*;

/// One exclusive owner borrows the session for a run. Send controls through a
/// bounded channel while it runs; closing the channel does not cancel it.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    session: &mut AgentSession,
    run_id: &str,
    input: UserInput,
    backend: &impl ModelBackend,
    executor: &impl ToolExecutor,
    options: &RunOptions,
    controls: &mut mpsc::Receiver<Control>,
    emit: &mut dyn FnMut(Event),
) -> Result<RunOutcome, RunError> {
    if session.active_run.is_some() {
        return Err(RunError::AlreadyRunning);
    }
    if !control::valid_input(&input) || run_id.trim().is_empty() {
        return Err(RunError::InvalidInput);
    }
    if options.budget.max_model_calls == 0
        || options.budget.max_completion_tokens == 0
        || options.budget.max_active_time.is_zero()
        || options.max_tools_per_turn == 0
        || options.max_response_bytes == 0
        || options.max_context_bytes == 0
        || options.max_tool_result_bytes < 128
        || session.steering.len() > 64
        || session.followups.len() > 64
        || session
            .steering
            .iter()
            .chain(&session.followups)
            .any(|i| !control::valid_input(i))
    {
        return Err(RunError::InvalidOptions);
    }
    let deadline = tokio::time::Instant::now()
        .checked_add(options.budget.max_active_time)
        .ok_or(RunError::InvalidOptions)?;
    session.active_run = Some(RunState {
        id: run_id.into(),
        phase: RunPhase::PreparingContext,
        budget: options.budget.clone(),
        model_calls: 0,
        completion_tokens: Some(0),
        pending_tools: Vec::new(),
        draft_response: String::new(),
    });
    emit(Event::Started {
        run_id: run_id.into(),
    });
    control::accept(session, input, emit);
    let result = driver::drive(
        session, backend, executor, options, controls, deadline, emit,
    )
    .await;
    let state = session.active_run.take().expect("owned run");
    match result {
        Ok(()) => {
            emit(Event::Finished {
                model_calls: state.model_calls,
                completion_tokens: state.completion_tokens,
            });
            Ok(RunOutcome {
                model_calls: state.model_calls,
                completion_tokens: state.completion_tokens,
            })
        }
        Err(error) => {
            emit(Event::Failed {
                code: error.to_string(),
            });
            Err(error)
        }
    }
}

#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod tests;
