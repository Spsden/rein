# Rein

A Rust agent harness with its own session and execution model, using Rig for
model/provider primitives.

## Run it

Requires Rust 1.95 or newer. The first build downloads the locked dependencies.

```sh
cargo build --locked
cargo run --locked -- demo /tmp/rein-demo.json "Hello Rein"
cargo run --locked -- demo /tmp/rein-demo.json "Continue this conversation"
```

The offline demo deterministically requests `echo`, executes it, produces an
assistant response and saves the session. It is not an LLM. Reusing the path
resumes history. Each stdout line is a JSON event; errors go to stderr.

For a real model, set `OPENAI_API_KEY` in your environment, then:

```sh
cargo run --locked -- run /tmp/rein-live.json YOUR_MODEL "Hello"
```

`OPENAI_BASE_URL` optionally selects an OpenAI-compatible Chat Completions
endpoint (typically ending in `/v1`). A local server that ignores authentication
can use a placeholder key. A custom URL receives your key and conversation;
only configure endpoints you trust. The model/connection must match when resuming.
Ctrl-C requests cancellation and saves the settled session before exiting nonzero.

Only the typed, side-effect-free `echo` tool is enabled by the CLI. There is no
shell, filesystem tool, automatic retry, HTTP server or Dart binding.

See [architecture](docs/architecture.md), [embedding and events](docs/api.md),
[testing](docs/testing.md), and the runnable [library example](examples/library.rs).

The session state model is in `src/session/mod.rs`:

- Accepted transcript entries retain Rig messages, local entry IDs and provenance.
- Working memory holds the task goal, choices, progress and evidence references.
- The tool catalog stores unique model-visible definitions; execution is separate.
- Model selection names a connection and model, without storing credentials.
- Steering and follow-up queues remain separate from accepted history.
- An optional run holds attempt phase, budgets, usage, pending calls and draft text.

The runtime owns scheduling and mutation during a run. It executes models and
authorized tools, enforces budgets and emits events. Storage saves idle snapshots.
Memory evidence is checked for existence, not semantic truth.
Transcript entry IDs and evidence references are scoped to their original session.

```rust
use rein::AgentSession;
use rein::session::{EntrySource, ModelSelection, ToolCatalog, WorkingNote};
use rig_core::completion::Message;

let mut session = AgentSession::new(
    "cinema-chat",
    ModelSelection {
        connection_id: "local".into(),
        model_id: "my-model".into(),
    },
    ToolCatalog::default(),
)?;

let evidence = session.transcript.append(Message::user("Hindi"), EntrySource::User);
session.working_memory.choices.insert(
    "language".into(),
    WorkingNote { text: "Hindi".into(), evidence: vec![evidence] },
);
# Ok::<(), rein::SessionError>(())
```

`prepare_context` in `src/context/mod.rs` builds an owned Rig `CompletionRequest`
without changing the session or consuming its steering/follow-up queues. Schedule
input into the accepted transcript before calling it.

```rust
use rein::{AgentSession, prepare_context};
use rein::session::{EntrySource, ModelSelection, ToolCatalog};
use rig_core::completion::Message;

let mut session = AgentSession::new(
    "cinema-chat",
    ModelSelection { connection_id: "local".into(), model_id: "my-model".into() },
    ToolCatalog::default(),
)?;
session.transcript.append(Message::user("Find a movie tonight"), EntrySource::User);
let prepared = prepare_context(&session, "Help the user find suitable options.", 96_000)?;
assert_eq!(prepared.connection_id, "local");
assert_eq!(prepared.request.model.as_deref(), Some("my-model"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Trusted instructions form the sole system message. Working memory and summaries
are labelled user-role reference data; other system messages in history are
rejected. Evidence references must resolve in the current transcript, though
their semantic accuracy is not proven. Tool exchanges must remain complete, with
unique call IDs per assistant turn and matching result names.

The budget covers the entire serialized Rig request, including tools and media.
It is a byte admission bound, not a token count or a guarantee of provider context
window fit. Oversized context fails explicitly; automatic pruning, summarization
and model-specific token accounting are not implemented.

## Code organization

Modules own a capability and its types, behavior and errors:

```text
src/
├── lib.rs                  Public module and API exports
├── session/
│   ├── mod.rs              Session state, input and session errors
│   └── tests.rs
├── transcript/
│   ├── mod.rs              History entries, identities and lookup
│   └── tests.rs
├── memory.rs               Working memory and evidence-linked notes
├── model.rs                Model selection and shared validation
├── run.rs                  Attempt state, phases and budgets
├── runtime/                Lifecycle, model loop, controls, tool dispatch and API types
├── provider.rs             Rig-backed OpenAI Chat Completions streaming adapter
├── storage.rs              Versioned atomic snapshots
├── storage/tests.rs
├── cli/                    Thin command-line host and offline demo backend
├── main.rs                 Executable entry point
├── tools/
│   ├── mod.rs              Catalog, pending calls and tool errors
│   └── tests.rs
├── context/
│   ├── mod.rs              Context API and preparation sequence
│   ├── memory.rs           Private memory-to-context projection
│   ├── validation.rs       Private transcript/tool-result validation
│   └── tests.rs
└── test_support.rs         Shared fixtures, compiled only for tests
```

Keep helpers with their owner: both session construction and context preparation
call `ModelSelection::validate()`. Context-specific helpers are visible only
within `context`; unrelated code cannot depend on them. Small cohesive modules
stay single files until their behavior justifies splitting them.

Tool catalog errors belong to `tools::ToolCatalogError`. Invalid model selection
is represented by `model::ModelSelectionError`, wrapped by session/context errors
with an error source. Existing type imports through `rein::session` remain
available as re-exports; definitions live in their owning modules.

Session fields remain public state. Hosts must validate ingestion and use one
exclusive owner per session; a snapshot is data, not a capability grant. The CLI
rebuilds the tool catalog and grants only `echo` on every invocation.

## Important limits

Persistence is checkpoint-based, not a durable execution journal. A forced kill
can lose the current run. Tool cancellation cannot undo external effects; custom
effectful executors need host approvals, idempotency and durable journaling before
production use. Never automatically replay an interrupted operation.

The deadline is elapsed run time, including tool waits; human approval pauses are
not implemented. Request bytes are not tokens. Output-token usage is provider
reported; if absent, it stays unknown and consumes the remaining run allowance
for scheduling. Provider limits are best-effort, not a financial spending cap.

Verify with `cargo test --locked`, `cargo fmt --check`, and
`cargo clippy --locked --all-targets -- -D warnings`.
