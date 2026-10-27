use super::*;
use crate::test_support::{model, session, tool};
use rig_core::completion::Message;
use std::time::Duration;

#[test]
fn new_session_has_all_state_and_is_idle() {
    let session = AgentSession::new(
        "first",
        model(),
        ToolCatalog::new(vec![tool("browse")]).unwrap(),
    )
    .unwrap();
    assert_eq!(session.id(), "first");
    assert_eq!(session.model.model_id, "test-model");
    assert_eq!(session.tools.definitions().len(), 1);
    assert!(session.transcript.entries().is_empty());
    assert!(session.working_memory.goal.is_none());
    assert!(session.steering.is_empty());
    assert!(session.followups.is_empty());
    assert!(session.active_run.is_none());
}

#[test]
fn rejects_blank_session_ids() {
    for id in ["", "  ", "\n"] {
        assert_eq!(
            AgentSession::new(id, model(), ToolCatalog::default()).unwrap_err(),
            SessionError::EmptyId,
        );
    }
}

#[test]
fn rejects_missing_connection_or_model() {
    for (connection, model_id) in [("", "test"), ("local", " ")] {
        let model = ModelSelection {
            connection_id: connection.into(),
            model_id: model_id.into(),
        };
        assert_eq!(
            AgentSession::new("first", model, ToolCatalog::default()).unwrap_err(),
            SessionError::InvalidModelSelection(ModelSelectionError),
        );
    }
}

#[test]
fn sessions_have_independent_transcripts_and_working_memory() {
    let mut first = session("first");
    let second = session("second");
    first
        .transcript
        .append(Message::user("Find a movie"), EntrySource::User);
    first.working_memory.goal = Some("Find a screening".into());
    assert_eq!(first.transcript.entries().len(), 1);
    assert!(second.transcript.entries().is_empty());
    assert!(second.working_memory.goal.is_none());
}

#[test]
fn history_preserves_order_and_memory_references_original_evidence() {
    let mut session = session("first");
    let user_message = Message::user("Hindi");
    let first = session
        .transcript
        .append(user_message.clone(), EntrySource::User);
    let second = session.transcript.append(
        Message::assistant("I'll look for Hindi screenings"),
        EntrySource::Model,
    );
    session.working_memory.choices.insert(
        "language".into(),
        WorkingNote {
            text: "Hindi".into(),
            evidence: vec![first],
        },
    );
    assert_eq!(session.transcript.entries()[0].id, first);
    assert_eq!(session.transcript.entries()[1].id, second);
    assert_eq!(first.value(), 1);
    assert_eq!(second.value(), 2);
    let evidence = session.working_memory.choices["language"].evidence[0];
    let entry = session.transcript.get(evidence).unwrap();
    assert_eq!(entry.source, EntrySource::User);
    assert_eq!(entry.message, user_message);
}

#[test]
fn queues_are_independent_and_keep_pending_input_out_of_history() {
    let mut session = session("first");
    session.steering.push_back(UserInput {
        id: "steer-1".into(),
        text: "Broadway instead".into(),
    });
    for id in ["follow-1", "follow-2"] {
        session.followups.push_back(UserInput {
            id: id.into(),
            text: "Find dinner afterwards".into(),
        });
    }
    assert!(session.transcript.entries().is_empty());
    assert_eq!(session.steering.pop_front().unwrap().id, "steer-1");
    assert_eq!(session.followups.pop_front().unwrap().id, "follow-1");
    assert_eq!(session.followups.pop_front().unwrap().id, "follow-2");
    assert!(session.steering.is_empty());
    assert!(session.followups.is_empty());
    assert!(session.transcript.entries().is_empty());
}

#[test]
fn an_attempt_can_end_without_discarding_session_history_or_task() {
    let mut session = session("first");
    session
        .transcript
        .append(Message::user("Find a movie"), EntrySource::User);
    session.working_memory.goal = Some("Find a screening".into());
    session.active_run = Some(RunState {
        id: "run-1".into(),
        phase: RunPhase::CallingModel,
        budget: RunBudget {
            max_model_calls: 32,
            max_completion_tokens: 100_000,
            max_active_time: Duration::from_secs(600),
        },
        model_calls: 1,
        completion_tokens: None,
        pending_tools: vec![],
        draft_response: String::new(),
    });
    assert!(
        session
            .active_run
            .as_ref()
            .unwrap()
            .completion_tokens
            .is_none()
    );
    session.active_run = None;
    assert_eq!(session.transcript.entries().len(), 1);
    assert_eq!(
        session.working_memory.goal.as_deref(),
        Some("Find a screening")
    );
}
