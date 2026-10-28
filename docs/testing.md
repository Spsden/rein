# Verification

## Local result — 2026-10-28

Verified on macOS with rustc/cargo 1.96.1: 42 unit tests, 5 CLI subprocess tests,
3 real-Rig loopback HTTP/SSE tests and 2 README doctests passed (52 total).
All-target tests also passed with serial execution. Formatting, strict Clippy,
warning-free rustdoc and the optimized release build passed. The release binary
completed an offline tool loop and resumed the resulting session successfully.
No authenticated model call or hosted CI run was performed.

## Reproduce

The initial local result above is historical. The iteration adds queued-budget
regressions, OpenCode Go identity/credential checks, typed corrective tool errors,
capability-scoped workspace tools and full coding-task integration. Run
`cargo test --locked --test agent_tasks` for task execution and independent grading;
see [evaluation](evaluation.md) for artifacts, negative controls and live runs.

Run from the repository root:

```sh
cargo fmt --check
cargo test --locked
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
cargo build --release --locked
```

After the dependencies are cached, add `--offline` to Cargo build/test/check/doc
commands. Provider tests bind only to loopback and do not contact a real model.
Use a normal local environment or allow loopback networking in your sandbox.
No API keys, paid requests or real effectful tools are required by any test.

## Coverage by layer

- Session/transcript: initialization, isolation, validation, append IDs, queues,
  provenance, evidence and separation of run lifetime from conversation lifetime.
- Context: exact byte boundaries and UTF-8, schemas/memory included in sizing,
  deterministic memory projection, summaries as untrusted reference data, system
  injection rejection, incomplete/orphan/duplicate/mismatched tool exchanges,
  missing evidence, malformed content and immutable snapshots.
- Runtime: full tool loop, decreasing token allowance, model-call/token/time
  exhaustion, missing usage, response/tool-count limits, invalid stops, malformed
  arguments, unknown/unauthorized tools, strict echo arguments, FIFO scheduling,
  live controls during a model wait, cancellation during model/tool waits,
  remaining-call settlement, queue overflow and closed control channels.
- Storage: round trip, evidence/ID/queue preservation, private Unix permissions,
  invalid replacement preserving an old checkpoint, active-run refusal, corrupt
  and incompatible snapshots, dangling evidence, oversized files and IO errors.
- CLI subprocesses: offline loop, resume, valid JSON Lines, help without keys,
  invalid arguments and corrupt-session preservation.
- Real Rig adapter: local HTTP request path/body, streamed text and tool calls,
  correlated result replay, provider usage, connection isolation, authentication
  failure redaction, truncated SSE and streaming admission limits.
- Workspace tools: stale/ambiguous/overlapping edits, permissions, UTF-8, strict
  arguments, read-only policy, create collisions, bounded output and symlink escapes.
- Agent tasks: four real compile/test workflows, failed-test repair, edit-conflict
  recovery, two-file edits, independent grading, persisted valid transcripts and
  a no-op agent that cannot pass by claiming success.
- README examples are Rust doctests; the library example is compiled by
  `cargo clippy --all-targets` and `cargo test --all-targets`.

Tests prove these cases, not every possible provider input or OS behavior.
Additional providers, real authenticated inference, very large workloads,
crash/power-loss durability, cross-platform filesystem semantics and concurrent
snapshot writers are not certified. Human approval workflows, model-specific
tokenizers, context compaction, side-effect journaling, retries and Dart IPC are
not implemented. See architecture documentation before enabling effectful tools.

CI runs formatting, tests, strict linting and rustdoc on Linux and macOS. A CI
configuration is not evidence of a successful hosted run until it has actually
executed in your GitHub repository.
