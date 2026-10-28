# Embedding and events

Build `cargo build --release --locked` for the binary. Use Rein as a Rust library
through `runtime::run`; see `examples/library.rs` for a compiling example. The
public modules are exported from `src/lib.rs`; existing `rein::session` type
re-exports remain supported.

## Host contracts

Construct `AgentSession` with an ID, `ModelSelection` and `ToolCatalog`. Supply a
`ModelBackend`, a `ToolExecutor`, `RunOptions`, a bounded Tokio control receiver
and a synchronous event callback to `runtime::run`.

`ModelBackend::complete` receives the connection ID, owned Rig request and stream
byte allowance. Stream through `emit(Event::Stream { item })` and return a fully
folded Rig response. Propagate truncation/error as `RunError`, never as a complete
assistant turn. Model implementations are trusted host code, not plugins loaded
from a session. `OpenAiBackend` checks the connection ID before any network IO.

`ToolExecutor::execute` receives a complete Rig `ToolCall` and returns text or a
`ToolError`. `InvalidArguments` and `Conflict` carry static corrective messages
that the model may see; `Host(String)` preserves private operational details
without exposing them through tool results. Do not put secrets into public error
messages, and do not blindly log the error's Debug representation.
Validate typed arguments, including unknown fields and size, even if the
catalog advertises a JSON schema. The runtime does not implement a general JSON
Schema validator. The builtin `Echo` uses a strict serde argument type matching
its schema. The host owns sandboxing, secrets, approvals and output redaction;
an allow-list is not an OS sandbox. Raw executor error strings are deliberately
not exposed to the model/events.

Use the channel sender to submit `Control::Steer(UserInput)`,
`Control::Followup(UserInput)` or `Control::Cancel`. Steering is read while the
model/tool is awaiting, but enters history only at the next safe model boundary.
Follow-ups wait until a tool-free assistant turn. Closing the channel does not
cancel. Queue overflow/invalid control input terminates with `InvalidInput`.
Initial/preexisting-queue validation failures occur before startup.

Callbacks must be fast, nonblocking and nonpanicking. They execute in the run's
owner task, in order. They receive conversation and possibly reasoning content;
do not blindly log or expose all items to users. Sink failure/backpressure policy
belongs to the host. The CLI flushes JSON Lines synchronously and reports output
errors after settling and saving the run; it is a terminal host, not a high-load
streaming server.

## Event protocol (initial, not yet a stable Dart RPC)

Every event is one JSON object with a snake-case `event` discriminator. The CLI
prints one object per line. Consumers should ignore unknown event kinds and
optional fields for forward compatibility.

| Event | Fields | Meaning |
| --- | --- | --- |
| `started` | `run_id` | Runtime has accepted ownership. |
| `input_accepted` | `input_id` | Input entered canonical history. |
| `model_started` | `call` | One-based model attempt within this run. |
| `stream` | `item` | Rig `Item<StreamEvent>`, including start/text/reasoning/arguments/end or unknown payload. |
| `turn_accepted` | `entry_id`, `output_tokens` | Complete assistant turn entered history; token count may be null. |
| `tool_started` | `call_id`, `name` | An authorized executor is about to be awaited. |
| `tool_finished` | `call_id`, `is_error` | Tool result prepared, including denied/not-executed results. |
| `finished` | `model_calls`, `completion_tokens` | Run ended normally; aggregate usage may be null. |
| `failed` | `code` | Runtime terminated, including cancellation/budget exhaustion. |

For example:

```json
{"event":"model_started","call":1}
{"event":"turn_accepted","entry_id":2,"output_tokens":12}
{"event":"tool_started","call_id":"call-1","name":"echo"}
{"event":"tool_finished","call_id":"call-1","is_error":false}
```

Stream events are provisional, not persisted partial messages. Tool argument
fragments must never trigger execution. `turn_accepted` is a logical in-memory
commit, not a disk checkpoint. Tool results are committed together after the
batch; a `tool_finished` event alone is not a durability acknowledgement.

After startup, one terminal event is emitted on normal returned paths. Preflight
errors return directly without `started`/`failed`. Callers must check the Rust
result or CLI exit status. In particular, CLI `finished` precedes the final disk
save, which can still fail. Billing usage from failed/incomplete requests may be
unavailable; these events are not an accounting ledger.

Error codes: `AlreadyRunning`, `InvalidInput`, `InvalidOptions`, `Context`,
`Model`, `InvalidResponse`, `ResponseTooLarge`, `ModelCallBudget`, `TokenBudget`,
`Timeout`, `Cancelled`. Provider errors are sanitized; credentials/raw HTTP error
bodies are not emitted. If detailed diagnostics are added later, keep them behind
an explicit host-controlled redaction boundary.

## Synapse / Dart boundary

For a concrete host with workspace tools and a fixed test executor, inspect
`evaluation/mod.rs` and `examples/evaluate.rs`. That host deliberately keeps
verification commands outside the core runtime and requires opt-in before live
generated-code execution. General coding CLI modes expose file tools only.

This implementation has no Dart FFI, HTTP endpoint or bidirectional stdio RPC.
The CLI accepts one prompt per invocation and resumes from a session path. The
library channel already defines live control semantics. A future Dart process
adapter can map those controls into the channel and events out as JSON Lines,
with explicit request/run IDs and protocol versioning. Keep that adapter outside
the session/core loop; do not let serialized tool requests become permissions.
