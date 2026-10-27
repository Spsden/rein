use super::tests::{Script, answer, input, setup, tool_turn};
use super::*;
use crate::tools::echo::Echo;
use rig_core::completion::{CompletionRequest, CompletionResponse, Message};
use std::future::pending;

struct Injecting {
    script: Script,
    sender: mpsc::Sender<Control>,
}
impl ModelBackend for Injecting {
    async fn complete(
        &self,
        connection: &str,
        request: CompletionRequest,
        limit: usize,
        emit: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        if self.script.requests.borrow().is_empty() {
            self.sender
                .try_send(Control::Steer(input("steer")))
                .unwrap();
            self.sender
                .try_send(Control::Followup(input("follow")))
                .unwrap();
            tokio::task::yield_now().await;
        }
        self.script.complete(connection, request, limit, emit).await
    }
}

#[tokio::test]
async fn live_controls_wait_until_tool_results_and_then_follow_fifo() {
    let (mut session, options) = setup();
    let (sender, mut rx) = mpsc::channel(8);
    let backend = Injecting {
        script: Script::new(vec![tool_turn(), answer(), answer()]),
        sender,
    };
    let mut events = vec![];
    run(
        &mut session,
        "live",
        input("start"),
        &backend,
        &Echo,
        &options,
        &mut rx,
        &mut |e| events.push(e),
    )
    .await
    .unwrap();
    let accepted: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::InputAccepted { input_id } => Some(input_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(accepted, ["start", "steer", "follow"]);
    assert!(matches!(
        session.transcript.entries()[2].source,
        crate::transcript::EntrySource::HostTool
    ));
    assert!(matches!(
        &session.transcript.entries()[3].message,
        Message::User { .. }
    ));
    assert!(crate::prepare_context(&session, "", 100_000).is_ok());
}

struct CancelMidway(mpsc::Sender<Control>);
impl ModelBackend for CancelMidway {
    async fn complete(
        &self,
        _: &str,
        _: CompletionRequest,
        _: usize,
        emit: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        let item = serde_json::from_value(
            serde_json::json!({"item":"event", "value":{"event":"text","part":0,"text":"partial"}}),
        )
        .unwrap();
        emit(Event::Stream { item });
        self.0.try_send(Control::Cancel).unwrap();
        pending().await
    }
}
#[tokio::test]
async fn cancel_during_model_drops_partial_turn_and_session_can_run_again() {
    let (mut session, options) = setup();
    let (sender, mut rx) = mpsc::channel(8);
    let mut events = vec![];
    let result = run(
        &mut session,
        "partial",
        input("start"),
        &CancelMidway(sender),
        &Echo,
        &options,
        &mut rx,
        &mut |e| events.push(e),
    )
    .await;
    assert_eq!(result.unwrap_err(), RunError::Cancelled);
    assert!(session.active_run.is_none());
    assert_eq!(session.transcript.entries().len(), 1);
    assert!(events.iter().any(|e| matches!(e, Event::Stream { .. })));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::TurnAccepted { .. }))
    );
    run(
        &mut session,
        "retry",
        input("retry"),
        &Script::new(vec![answer()]),
        &Echo,
        &options,
        &mut rx,
        &mut |_| {},
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn closed_control_channel_is_not_cancellation() {
    let (mut session, options) = setup();
    let (sender, mut rx) = mpsc::channel(1);
    drop(sender);
    run(
        &mut session,
        "closed",
        input("start"),
        &Script::new(vec![answer()]),
        &Echo,
        &options,
        &mut rx,
        &mut |_| {},
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn invalid_controls_and_queue_overflow_fail_without_model_calls() {
    for overflow in [false, true] {
        let (mut session, options) = setup();
        if overflow {
            for _ in 0..64 {
                session.followups.push_back(input("queued"));
            }
        }
        let (sender, mut rx) = mpsc::channel(2);
        sender
            .try_send(Control::Followup(input(if overflow {
                "overflow"
            } else {
                ""
            })))
            .unwrap();
        let backend = Script::new(vec![]);
        let result = run(
            &mut session,
            "invalid",
            input("start"),
            &backend,
            &Echo,
            &options,
            &mut rx,
            &mut |_| {},
        )
        .await;
        assert_eq!(result.unwrap_err(), RunError::InvalidInput);
        assert!(backend.requests.borrow().is_empty());
        assert!(session.active_run.is_none());
    }
}
