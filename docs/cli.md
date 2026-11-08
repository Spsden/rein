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

The CLI grants file tools inside the chosen workspace. See
[workspace tool boundaries](workspace-tools.md). Model quality and broad
Pi/OpenCode parity are not established by the terminal interface.

## Running commands and tests

Launch from a foreground Unix terminal with explicit opt-in:

~~~sh
./target/release/rein chat /tmp/rein-chat.json glm-5.3-flash /path/to/project --allow-shell
~~~

At the prompt, try: “Fix the failing test without changing the test itself,
then run cargo test and tell me what passed.” Rein shows the working directory
and exact shell input (with nonprintable/non-ASCII characters escaped). Type
the displayed confirmation, such as **run 4f1d90ac**, to run that one command.
Enter or an incorrect response denies it. Ctrl-C cancels the turn, including
any pending approval. The launch flag applies only to this invocation;
resuming without it disables shell even if earlier turns used it.

- **This is not a sandbox.** The command starts at the workspace root, but
  can access other paths, the network and anything your account can access.
  Build/test scripts can themselves execute arbitrary code. Only enable this
  in repositories you trust, and inspect every command before approving.
- Commands run as non-interactive **/bin/sh -c** with no stdin. Provider
  keys and other environment variables are not inherited automatically; a
  small toolchain allow-list is kept. Commands can still read credentials
  from files or other accessible sources.
- The tool returns the real exit status and the first 4 KiB of each output
  stream, with truncation flags. Output is sent to the provider and saved
  in the conversation. Do not approve commands that print secrets.
- Approval expires after 60 seconds and a command times out after 120 seconds;
  the overall turn deadline still applies. Cancellation/timeout kills the
  command's process group, but cannot undo effects or stop detached processes
  that escaped the group. Do not run background jobs or interactive commands.
- Without this flag, run tests yourself in another terminal. Shell is not
  available to piped input, the offline demo, or the one-shot JSON modes.

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
pseudo-terminals for editing/history, paste cancellation, EOF/resume, cancelling
a streamed model response, and exercising shell opt-in, approval, output limits
and cancellation. Neither needs a real key.
