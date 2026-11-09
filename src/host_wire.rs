//! Versioned stdio host mode. The caller owns model transport, tools and persistence.
mod messages;

use rein::runtime::{
    self, Control, Event, ModelBackend, RunError, RunOptions, ToolError, ToolExecutor,
};
use rein::session::{EntrySource, ModelSelection, ToolCatalog, UserInput};
use rein::{AgentSession, run::RunBudget};
use rig_core::completion::{
    CompletionRequest, CompletionResponse, FinishReason, ToolDefinition, Usage,
};
use rig_core::message::{AssistantContent, Origin, ToolCall};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::Cell;
use std::io::{self, BufRead, Read, Write};
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

const MAX_FRAME: usize = 2_097_152;
const MAX_CONTEXT: usize = 1_000_000;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Start {
        run_id: String,
        instructions: String,
        history: Vec<messages::InitialMessage>,
        prompt: String,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    },
    ModelResult {
        id: u64,
        text: String,
        reasoning: String,
        calls: Vec<messages::Call>,
        output_tokens: Option<u64>,
        finish: Finish,
    },
    ToolResult {
        id: u64,
        content: String,
    },
    Cancel,
    Steer {
        id: String,
        text: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Finish {
    Stop,
    ToolCalls,
    Invalid,
}

#[tokio::main(flavor = "current_thread")]
pub(super) async fn main() -> Result<(), &'static str> {
    let (sender, mut input) = mpsc::channel(2);
    let (controls, mut control_rx) = mpsc::channel(16);
    let stop = controls.clone();
    std::thread::spawn(move || {
        let mut reader = io::stdin().lock();
        loop {
            let mut line = Vec::new();
            let Ok(size) = (&mut reader)
                .take(MAX_FRAME as u64 + 1)
                .read_until(b'\n', &mut line)
            else {
                break;
            };
            if size == 0 || size > MAX_FRAME || !line.ends_with(b"\n") {
                break;
            }
            let Ok(message) = serde_json::from_slice(&line) else {
                break;
            };
            let sent = match message {
                Input::Cancel => controls.try_send(Control::Cancel).is_ok(),
                Input::Steer { id, text } => controls
                    .try_send(Control::Steer(UserInput { id, text }))
                    .is_ok(),
                other => sender.blocking_send(other).is_ok(),
            };
            if !sent {
                break;
            }
        }
        drop(sender);
        let _ = controls.blocking_send(Control::Cancel);
    });
    let Some(Input::Start {
        run_id,
        instructions,
        history,
        prompt,
        tools,
        max_tokens,
    }) = input.recv().await
    else {
        return Err("invalid host-v1 start");
    };
    if !(1..=100_000).contains(&max_tokens) || tools.len() > 256 || instructions.len() > MAX_CONTEXT
    {
        return Err("invalid host-v1 run bounds");
    }
    let allowed_tools = tools.iter().map(|t| t.name.to_string()).collect();
    let mut session = AgentSession::new(
        &run_id,
        ModelSelection {
            connection_id: "host".into(),
            model_id: "host".into(),
        },
        ToolCatalog::new(tools).map_err(|_| "invalid host-v1 tools")?,
    )
    .map_err(|_| "invalid host-v1 session")?;
    for message in history {
        session
            .transcript
            .append(message.into_message(), EntrySource::User);
    }
    let options = RunOptions {
        instructions,
        allowed_tools,
        budget: RunBudget {
            max_model_calls: 32,
            max_completion_tokens: max_tokens,
            max_active_time: Duration::from_secs(600),
        },
        max_context_bytes: MAX_CONTEXT,
        ..RunOptions::default()
    };
    send(json!({"type":"ready","version":1})).map_err(|_| "host-v1 output closed")?;
    let host = Host {
        input: Mutex::new(input),
        next: Cell::new(0),
        protocol_failed: Cell::new(false),
    };
    let mut emit = |event| {
        let frame = match event {
            Event::TurnAccepted { .. } => json!({"type":"accepted"}),
            Event::InputAccepted { input_id } if input_id != "prompt" => {
                json!({"type":"steer_accepted","id":input_id})
            }
            _ => return,
        };
        if send(frame).is_err() {
            let _ = stop.try_send(Control::Cancel);
        }
    };
    let result = runtime::run(
        &mut session,
        &run_id,
        UserInput {
            id: "prompt".into(),
            text: prompt,
        },
        &host,
        &host,
        &options,
        &mut control_rx,
        &mut emit,
    )
    .await;
    let terminal = match result {
        Ok(outcome) => json!({"type":"finished","output_tokens":outcome.completion_tokens}),
        Err(error) => json!({"type":"failed","code":error.to_string()}),
    };
    send(terminal).map_err(|_| "host-v1 output closed")
}

struct Host {
    input: Mutex<mpsc::Receiver<Input>>,
    next: Cell<u64>,
    protocol_failed: Cell<bool>,
}

impl Host {
    async fn exchange(&self, kind: &str, payload: Value) -> Option<Input> {
        if self.protocol_failed.get() {
            return None;
        }
        let id = self.next.get() + 1;
        self.next.set(id);
        if send(json!({"type":kind,"id":id,"payload":payload})).is_ok() {
            let reply = self.input.lock().await.recv().await;
            let matched = match &reply {
                Some(Input::ModelResult {
                    id: response_id, ..
                }) => kind == "model" && *response_id == id,
                Some(Input::ToolResult {
                    id: response_id,
                    content,
                }) => kind == "tool" && *response_id == id && !content.trim().is_empty(),
                _ => false,
            };
            if matched {
                return reply;
            }
        }
        // Never ask the host to perform another effect after the exchange loses correlation.
        self.protocol_failed.set(true);
        None
    }
}

impl ModelBackend for Host {
    async fn complete(
        &self,
        _: &str,
        request: CompletionRequest,
        _: usize,
        _: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        let history = messages::project(&request.chat_history)?;
        let Some(Input::ModelResult {
            id,
            text,
            reasoning,
            calls,
            output_tokens,
            finish,
        }) = self
            .exchange(
                "model",
                json!({"messages":history,"max_tokens":request.max_tokens}),
            )
            .await
        else {
            return Err(RunError::InvalidResponse);
        };
        let reason = match finish {
            Finish::Stop if calls.is_empty() && !text.trim().is_empty() => FinishReason::Stop,
            Finish::ToolCalls if !calls.is_empty() => FinishReason::ToolCalls,
            _ => return Err(RunError::InvalidResponse),
        };
        let mut parts = Vec::new();
        if !text.trim().is_empty() {
            parts.push(AssistantContent::text(text));
        }
        if !reasoning.trim().is_empty() {
            parts.push(AssistantContent::reasoning(reasoning));
        }
        for call in calls {
            parts.push(call.into_content()?);
        }
        let mut origin = Origin::new("host-v1", "host", "host");
        origin.response_id = Some(id.to_string());
        let mut usage = Usage::new();
        usage.output_tokens = output_tokens;
        Ok(CompletionResponse::new(parts, usage, origin, Value::Null).with_finish_reason(reason))
    }
}

impl ToolExecutor for Host {
    async fn execute(&self, call: &ToolCall) -> Result<String, ToolError> {
        match self
            .exchange(
                "tool",
                serde_json::to_value(messages::Call::from(call)).expect("serializable call"),
            )
            .await
        {
            Some(Input::ToolResult { content, .. }) => Ok(content),
            _ => Err(ToolError::Host("Invalid host tool reply".into())),
        }
    }
}

fn send(message: Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(&message).map_err(io::Error::other)?;
    if bytes.len() + 1 > MAX_FRAME {
        return Err(io::Error::other("host-v1 output too large"));
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
