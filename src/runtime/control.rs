use super::{Control, Event, RunError};
use crate::session::{AgentSession, EntrySource, UserInput};
use rig_core::completion::Message;
use std::collections::VecDeque;
use std::future::Future;
use tokio::sync::mpsc;

pub(super) fn valid_input(input: &UserInput) -> bool {
    !input.id.trim().is_empty() && !input.text.trim().is_empty() && input.text.len() <= 1_000_000
}

pub(super) fn accept(session: &mut AgentSession, input: UserInput, emit: &mut dyn FnMut(Event)) {
    session
        .transcript
        .append(Message::user(input.text), EntrySource::User);
    emit(Event::InputAccepted { input_id: input.id });
}

fn queue(
    control: Control,
    steering: &mut VecDeque<UserInput>,
    followups: &mut VecDeque<UserInput>,
) -> Result<(), RunError> {
    let (queue, input) = match control {
        Control::Cancel => return Err(RunError::Cancelled),
        Control::Steer(input) => (steering, input),
        Control::Followup(input) => (followups, input),
    };
    // Reject queue overflow; do not allow a bounded transport to hide an unbounded backlog.
    if !valid_input(&input) || queue.len() >= 64 {
        return Err(RunError::InvalidInput);
    }
    queue.push_back(input);
    Ok(())
}

pub(super) fn drain(
    session: &mut AgentSession,
    controls: &mut mpsc::Receiver<Control>,
) -> Result<(), RunError> {
    while let Ok(control) = controls.try_recv() {
        queue(control, &mut session.steering, &mut session.followups)?;
    }
    Ok(())
}

pub(super) async fn wait<T>(
    future: impl Future<Output = T>,
    steering: &mut VecDeque<UserInput>,
    followups: &mut VecDeque<UserInput>,
    controls: &mut mpsc::Receiver<Control>,
    deadline: tokio::time::Instant,
) -> Result<T, RunError> {
    tokio::pin!(future);
    loop {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => return Err(RunError::Timeout),
            control = controls.recv(), if !controls.is_closed() || !controls.is_empty() => {
                if let Some(control) = control { queue(control, steering, followups)?; }
            }
            result = &mut future => return Ok(result),
        }
    }
}
