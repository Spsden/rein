# Interactive CLI

Build once, then start a conversation from the repository root:

```sh
cargo build --release --locked

# Only source a .env you own and trust. Rein does not auto-load .env files.
set -a
source .env
set +a

./target/release/rein chat /tmp/rein-chat.json glm-5.3-flash /path/to/project
```

`chat` uses `OPENCODE_GO_API_KEY`. Each live turn consumes the configured model's
provider allowance. Use a disposable project first. The model can read allowed
source files and make guarded edits immediately; there is no per-edit approval.
Do not select a workspace you do not intend to share with the provider.

```text
you > Inspect the project and explain its entry point.
[thinking · model call 1]
[tool] list_files
[tool ok]
...
rein > ...
[done · ... model calls · ... output tokens · session saved]
you > Now add a test for the edge case we discussed.
```

Output above is illustrative, not a measured execution. Answers stream as plain
text; Markdown remains readable source rather than a full-screen rendered TUI.
Reasoning, raw tool results and unknown provider payloads are not printed.
Terminal control characters from model text are removed before display.

## Input and commands

- Enter submits a prompt. Empty input is ignored.
- Cursor keys and normal line editing work; Up/Down browse up to 100 prompts.
  On restart, history is rebuilt from accepted user entries in the session.
- `/help` lists commands. `/status` shows model, session, workspace and entry count.
- `/paste` collects multiline text. A line containing exactly `/end` submits it.
  Ctrl-C discards the paste; EOF discards it and exits. Prompts are limited to
  1 MB; an oversized paste is discarded through `/end`, not submitted as tasks.
- `/exit` or `/quit` exits; Ctrl-D at an empty prompt also exits.
- Ctrl-C while editing clears input. During a run it requests cancellation and
  returns to the prompt after settlement/checkpointing.
- Unknown slash commands are local errors, not model requests. Start with `//`
  to submit a literal leading slash: `//help` sends `/help` to the model.

Piped newline-separated input also works. `/exit` stops processing subsequent
lines. Use `/paste` for multiline tasks; plain piped lines are separate turns.
Input typed during an active turn is not live steering. Wait for `you >` before
submitting another task. There is no concurrent run, model switching or compaction.

## Resume, errors and safety

Repeat exactly the same launch command to resume. The same snapshot also works
with the equivalent `go-code` or `code` one-shot command. Connection, model and
canonical workspace must match; mismatches and corrupt snapshots fail without
replacing the old session. Parent directories for session paths must already exist.
Keep snapshots private and use one process per session/workspace.

Sessions are checkpointed at startup and after every completed, failed or cancelled
turn. Provider/runtime failures stop that turn, print a notice and leave chat open
for your next instruction. Configuration, input/output and persistence errors exit
nonzero; a failed save is never presented as successful persistence.
The last fully saved snapshot is not a crash-recovery journal.

Streamed text is provisional until accepted. If cancelled mid-response, its draft
is not added to history; the accepted user input remains. Completed tool effects
are not undone. An interrupted filesystem operation may have an uncertain outcome:
inspect changes before asking for a retry. There are no automatic retries.

The CLI grants `list_files`, `read_file`, `search`, `edit_file` and `write_file`
inside the chosen workspace. It does **not** grant shell commands or run tests.
Use a separate terminal for `cargo test`, other verifiers and inspecting diffs.
See [workspace tool boundaries](workspace-tools.md). Model quality and broad
Pi/OpenCode parity are not established by the terminal interface.

## Other providers and offline testing

```sh
# OPENAI_API_KEY, optional trusted OPENAI_BASE_URL:
./target/release/rein chat-openai /tmp/rein-openai-chat.json YOUR_MODEL /path/to/project

# No API key or network needed; deterministic echo backend, NOT an LLM:
./target/release/rein chat-demo /tmp/rein-chat-demo.json
```

The offline demo grants only echo, not workspace access. Existing `demo`, `run`,
`go`, `code` and `go-code` commands retain one-task JSON Lines output.

## Verification

```sh
cargo test --locked
cargo build --locked
python3 tests/terminal_cli.py
# The terminal checks can also target the optimized binary:
python3 tests/terminal_cli.py target/release/rein
```

Cargo tests cover commands, multiline input, persistence, bounded-paste rejection,
corrupt-session protection, credentials, streamed output and local-provider file
edits after a failed turn. The Python standard-library checks use real POSIX
pseudo-terminals for editing/history, paste cancellation, EOF/resume and cancelling
a streamed model response before completing another turn. Neither needs a real key.
