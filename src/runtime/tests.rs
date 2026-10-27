use super::*;
use crate::prepare_context;
use crate::run::RunBudget;
use crate::tools::ToolCatalog;
use crate::tools::echo::{self, Echo};
use crate::transcript::EntrySource;
use rig_core::completion::{CompletionRequest, CompletionResponse, FinishReason, Message, Usage};
use rig_core::message::{AssistantContent, Origin, ToolCall, ToolFunction, UserContent};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::pending;

pub(super) struct Script {
    pub(super) turns: RefCell<VecDeque<Result<CompletionResponse, RunError>>>,
    pub(super) requests: RefCell<Vec<CompletionRequest>>,
}
impl Script {
    pub(super) fn new(turns: Vec<CompletionResponse>) -> Self {
        Self {
            turns: RefCell::new(turns.into_iter().map(Ok).collect()),
            requests: RefCell::default(),
        }
    }
}
impl ModelBackend for Script {
    async fn complete(
        &self,
        _: &str,
        request: CompletionRequest,
        _: usize,
        _: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        self.requests.borrow_mut().push(request);
        self.turns
            .borrow_mut()
            .pop_front()
            .expect("unexpected model call")
    }
}
fn response(content: Vec<AssistantContent>) -> CompletionResponse {
    CompletionResponse::new(
        content,
        Usage::new().output_tokens(3),
        Origin::new("test", "test", "model"),
        json!({}),
    )
    .with_finish_reason(FinishReason::Stop)
}
pub(super) fn answer() -> CompletionResponse {
    response(vec![AssistantContent::text("done")])
}
fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall::from_wire(id, ToolFunction::new(name.try_into().unwrap(), args))
}
pub(super) fn tool_turn() -> CompletionResponse {
    response(vec![AssistantContent::ToolCall(call(
        "c1",
        "echo",
        json!({"text":"hello"}),
    ))])
}
pub(super) fn input(text: &str) -> UserInput {
    UserInput {
        id: text.into(),
        text: text.into(),
    }
}
pub(super) fn setup() -> (AgentSession, RunOptions) {
    let mut session = crate::test_support::session("runtime");
    session.tools = ToolCatalog::new(vec![echo::definition()]).unwrap();
    (
        session,
        RunOptions {
            allowed_tools: vec!["echo".into()],
            ..RunOptions::default()
        },
    )
}
async fn invoke(
    session: &mut AgentSession,
    backend: &impl ModelBackend,
    executor: &impl ToolExecutor,
    options: &RunOptions,
) -> (Result<RunOutcome, RunError>, Vec<Event>) {
    let (_tx, mut rx) = mpsc::channel(16);
    let mut events = vec![];
    let result = run(
        session,
        "run",
        input("start"),
        backend,
        executor,
        options,
        &mut rx,
        &mut |e| events.push(e),
    )
    .await;
    (result, events)
}

#[tokio::test]
async fn complete_tool_loop_has_correlated_history_and_ordered_events() {
    let (mut session, options) = setup();
    let backend = Script::new(vec![tool_turn(), answer()]);
    let (result, events) = invoke(&mut session, &backend, &Echo, &options).await;
    let outcome = result.unwrap();
    assert_eq!(outcome.model_calls, 2);
    assert_eq!(outcome.completion_tokens, Some(6));
    assert!(session.active_run.is_none());
    assert_eq!(session.transcript.entries().len(), 4);
    assert!(matches!(events.first(), Some(Event::Started { .. })));
    assert!(matches!(events.last(), Some(Event::Finished { .. })));
    let requests = backend.requests.borrow();
    assert_eq!(requests[0].max_tokens, Some(32_768));
    assert_eq!(requests[1].max_tokens, Some(32_765));
    assert!(prepare_context(&session, "test", 100_000).is_ok());
}

struct Counter(Cell<u32>);
impl ToolExecutor for Counter {
    async fn execute(&self, _: &ToolCall) -> Result<String, String> {
        self.0.set(self.0.get() + 1);
        Ok("ok".into())
    }
}

#[tokio::test]
async fn unauthorized_and_unknown_tools_never_reach_executor() {
    for name in ["echo", "shell"] {
        let (mut session, mut options) = setup();
        options.allowed_tools.clear();
        let backend = Script::new(vec![
            response(vec![AssistantContent::ToolCall(call("x", name, json!({})))]),
            answer(),
        ]);
        let executor = Counter(Cell::new(0));
        assert!(
            invoke(&mut session, &backend, &executor, &options)
                .await
                .0
                .is_ok()
        );
        assert_eq!(executor.0.get(), 0);
        let Message::User { content } = &session.transcript.entries()[2].message else {
            panic!()
        };
        assert!(matches!(&content[0], UserContent::ToolResult(r) if r.is_error));
    }
}

#[tokio::test]
async fn malformed_arguments_do_not_execute_and_echo_checks_schema() {
    let (mut session, options) = setup();
    let malformed = call("x", "echo", json!("not JSON"));
    let backend = Script::new(vec![
        response(vec![AssistantContent::ToolCall(malformed)]),
        answer(),
    ]);
    let executor = Counter(Cell::new(0));
    invoke(&mut session, &backend, &executor, &options)
        .await
        .0
        .unwrap();
    assert_eq!(executor.0.get(), 0);
    for args in [json!({}), json!({"text":5}), json!({"text":"x","extra":1})] {
        assert!(Echo.execute(&call("x", "echo", args)).await.is_err());
    }
    assert_eq!(
        Echo.execute(&call("x", "echo", json!({"text":"नमस्ते"})))
            .await
            .unwrap(),
        "नमस्ते"
    );
}

#[tokio::test]
async fn model_and_token_budgets_stop_loops_and_clear_run() {
    for tokens in [false, true] {
        let (mut session, mut options) = setup();
        if tokens {
            options.budget.max_completion_tokens = 3;
        } else {
            options.budget.max_model_calls = 1;
        }
        let (result, events) = invoke(
            &mut session,
            &Script::new(vec![tool_turn()]),
            &Echo,
            &options,
        )
        .await;
        assert_eq!(
            result.unwrap_err(),
            if tokens {
                RunError::TokenBudget
            } else {
                RunError::ModelCallBudget
            }
        );
        assert!(session.active_run.is_none());
        assert!(matches!(events.last(), Some(Event::Failed { .. })));
        assert!(prepare_context(&session, "", 100_000).is_ok());
    }
}

#[tokio::test]
async fn missing_usage_is_unknown_and_reserves_remaining_budget() {
    let (mut session, options) = setup();
    let mut turn = tool_turn();
    turn.usage = Usage::new();
    let (result, _) = invoke(&mut session, &Script::new(vec![turn]), &Echo, &options).await;
    assert_eq!(result.unwrap_err(), RunError::TokenBudget);
    let (mut session, _) = setup();
    let mut turn = answer();
    turn.usage = Usage::new();
    let (result, _) = invoke(&mut session, &Script::new(vec![turn]), &Echo, &options).await;
    assert_eq!(result.unwrap().completion_tokens, None);
}

#[tokio::test]
async fn invalid_or_incomplete_response_is_not_committed() {
    let turns = [
        response(vec![]),
        answer().with_finish_reason(FinishReason::Length),
        answer().with_finish_reason(FinishReason::ContentFilter),
        answer().with_finish_reason(FinishReason::Other("unexpected".into())),
        response(vec![
            AssistantContent::ToolCall(call("same", "echo", json!({}))),
            AssistantContent::ToolCall(call("same", "echo", json!({}))),
        ]),
    ];
    for turn in turns {
        let (mut session, options) = setup();
        let (result, _) = invoke(&mut session, &Script::new(vec![turn]), &Echo, &options).await;
        assert_eq!(result.unwrap_err(), RunError::InvalidResponse);
        assert_eq!(session.transcript.entries().len(), 1);
        assert!(session.active_run.is_none());
    }
}

#[tokio::test]
async fn steering_precedes_followups_and_never_splits_tool_exchange() {
    let (mut session, options) = setup();
    session.steering.push_back(input("steer"));
    session.followups.push_back(input("follow"));
    let backend = Script::new(vec![tool_turn(), answer(), answer()]);
    invoke(&mut session, &backend, &Echo, &options)
        .await
        .0
        .unwrap();
    assert_eq!(backend.requests.borrow().len(), 3);
    assert!(session.steering.is_empty() && session.followups.is_empty());
    let entries = session.transcript.entries();
    assert!(
        matches!(&entries[1].message, Message::User { content } if matches!(&content[0], UserContent::Text(t) if t.text == "steer"))
    );
    assert!(matches!(entries[3].source, EntrySource::HostTool));
    assert!(
        matches!(&entries[5].message, Message::User { content } if matches!(&content[0], UserContent::Text(t) if t.text == "follow"))
    );
}

struct Hanging;
impl ModelBackend for Hanging {
    async fn complete(
        &self,
        _: &str,
        _: CompletionRequest,
        _: usize,
        _: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        pending().await
    }
}
#[tokio::test]
async fn deadline_interrupts_a_hanging_model() {
    let (mut session, mut options) = setup();
    options.budget.max_active_time = std::time::Duration::from_millis(1);
    assert_eq!(
        invoke(&mut session, &Hanging, &Echo, &options)
            .await
            .0
            .unwrap_err(),
        RunError::Timeout
    );
    assert!(session.active_run.is_none());
}

struct InterruptedTool(mpsc::Sender<Control>);
impl ToolExecutor for InterruptedTool {
    async fn execute(&self, _: &ToolCall) -> Result<String, String> {
        self.0.try_send(Control::Cancel).unwrap();
        pending().await
    }
}
#[tokio::test]
async fn cancellation_settles_all_tool_calls_without_executing_remaining_calls() {
    let (mut session, options) = setup();
    let backend = Script::new(vec![response(vec![
        AssistantContent::ToolCall(call("a", "echo", json!({}))),
        AssistantContent::ToolCall(call("b", "echo", json!({}))),
    ])]);
    let (tx, mut rx) = mpsc::channel(16);
    let mut events = vec![];
    let result = run(
        &mut session,
        "run",
        input("start"),
        &backend,
        &InterruptedTool(tx),
        &options,
        &mut rx,
        &mut |e| events.push(e),
    )
    .await;
    assert_eq!(result.unwrap_err(), RunError::Cancelled);
    assert!(session.active_run.is_none());
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ToolStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ToolFinished { .. }))
            .count(),
        2
    );
    assert!(prepare_context(&session, "", 100_000).is_ok());
}

#[tokio::test]
async fn preflight_and_context_limits_do_not_call_model() {
    let (mut session, mut options) = setup();
    options.budget.max_model_calls = 0;
    let backend = Script::new(vec![]);
    assert_eq!(
        invoke(&mut session, &backend, &Echo, &options)
            .await
            .0
            .unwrap_err(),
        RunError::InvalidOptions
    );
    assert!(session.transcript.entries().is_empty());
    options.budget.max_model_calls = 1;
    options.max_context_bytes = 1;
    assert_eq!(
        invoke(&mut session, &backend, &Echo, &options)
            .await
            .0
            .unwrap_err(),
        RunError::Context
    );
    assert!(backend.requests.borrow().is_empty());
}

#[tokio::test]
async fn excessive_tokens_response_size_and_tool_count_fail_before_effects() {
    for mode in 0..3 {
        let (mut session, mut options) = setup();
        let mut turn = tool_turn();
        let expected = match mode {
            0 => {
                turn.usage = Usage::new().output_tokens(50_000);
                RunError::TokenBudget
            }
            1 => {
                options.max_response_bytes = 1;
                RunError::ResponseTooLarge
            }
            _ => {
                options.max_tools_per_turn = 1;
                turn = response(vec![
                    AssistantContent::ToolCall(call("a", "echo", json!({}))),
                    AssistantContent::ToolCall(call("b", "echo", json!({}))),
                ]);
                RunError::InvalidResponse
            }
        };
        let executor = Counter(Cell::new(0));
        assert_eq!(
            invoke(&mut session, &Script::new(vec![turn]), &executor, &options)
                .await
                .0
                .unwrap_err(),
            expected
        );
        assert_eq!(executor.0.get(), 0);
        assert_eq!(session.transcript.entries().len(), 1);
    }
}

struct FailingOrLarge(bool);
impl ToolExecutor for FailingOrLarge {
    async fn execute(&self, _: &ToolCall) -> Result<String, String> {
        if self.0 {
            Err("private host detail".into())
        } else {
            Ok("x".repeat(129))
        }
    }
}

#[tokio::test]
async fn tool_failures_and_oversized_results_are_sanitized_and_recoverable() {
    for failure in [false, true] {
        let (mut session, mut options) = setup();
        options.max_tool_result_bytes = 128;
        let backend = Script::new(vec![tool_turn(), answer()]);
        let (result, events) =
            invoke(&mut session, &backend, &FailingOrLarge(failure), &options).await;
        result.unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ToolFinished { is_error: true, .. }))
        );
        let serialized = serde_json::to_string(&backend.requests.borrow()[1]).unwrap();
        assert!(!serialized.contains("private host detail"));
        assert!(!serialized.contains(&"x".repeat(129)));
        assert!(prepare_context(&session, "", 100_000).is_ok());
    }
}

#[tokio::test]
async fn model_error_is_terminal_and_active_run_cannot_be_overwritten() {
    let (mut session, options) = setup();
    let backend = Script::new(vec![]);
    backend.turns.borrow_mut().push_back(Err(RunError::Model));
    let (result, events) = invoke(&mut session, &backend, &Echo, &options).await;
    assert_eq!(result.unwrap_err(), RunError::Model);
    assert!(matches!(events.last(), Some(Event::Failed { .. })));
    assert_eq!(session.transcript.entries().len(), 1);
    session.active_run = Some(crate::run::RunState {
        id: "existing".into(),
        phase: crate::run::RunPhase::CallingModel,
        budget: RunBudget::default(),
        model_calls: 0,
        completion_tokens: None,
        pending_tools: vec![],
        draft_response: String::new(),
    });
    assert_eq!(
        invoke(&mut session, &backend, &Echo, &options)
            .await
            .0
            .unwrap_err(),
        RunError::AlreadyRunning
    );
    assert_eq!(session.active_run.as_ref().unwrap().id, "existing");
    assert_eq!(session.transcript.entries().len(), 1);
}
