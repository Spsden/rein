# Agent-task evaluation

This is a small regression/evaluation suite, **not a Pi/OpenCode parity claim**.
It separates runtime correctness from a model's ability to solve tasks.

## Offline regression

```sh
cargo test --locked --test agent_tasks
cargo run --locked --example evaluate -- offline /tmp/rein-offline-001
```

The output directory must not exist; previous reports are never overwritten.
The offline backend knows the solutions. Its 4/4 score establishes that the loop,
tools, feedback, task verification and persistence work together—not that Rein
has independently reasoned through these problems.

| Task | Independent check | Injected offline recovery |
| --- | --- | --- |
| Arithmetic | Signed addition across 1,600 input pairs | Wrong multiplication patch, failed tests, re-read, repair, passing tests |
| Unicode | Unicode scalar counting across ASCII, non-Latin and combining/emoji inputs | None |
| Boundaries | Inclusive bounds and reversed ranges across 640 input triples | Stale edit conflict, re-read and corrected exact edit |
| Multi-file | Discount rounding and remaining balance across 24 input pairs | Coordinated fixes in two source files |

Each case first checks that its seeded implementation fails the verifier. The
host then supplies `run_tests`: a fixed Rust compiler/test command with a host
oracle outside the file-tool workspace. This gives the agent genuine failed-test
feedback without letting it edit the oracle through file tools. Final grading
reruns the oracle independently, requires a successful run and at least one test
tool call, and rejects unexpected workspace files. Calling an unchanged task
"done" cannot pass. A negative control explicitly tests this:

```sh
cargo run --locked --example evaluate -- negative-control /tmp/rein-negative-001
```

That command should report **0/4 and exit nonzero**. It is a grader check, not a
runtime regression. The test suite asserts the expected failures.

## Live OpenCode Go evaluation

Configure `OPENCODE_GO_API_KEY` outside the repository. Select a current Go model
ID from [the provider documentation](https://opencode.ai/v2/docs/console/go/).
Run in your own OS/container sandbox with only the synthetic task data:

```sh
cargo run --locked --example evaluate -- live YOUR_MODEL /tmp/rein-live-001 --allow-code-execution
```

**Live mode compiles and executes model-edited Rust.** The opt-in is intentional:
directory-scoped tools do not sandbox compiled programs. Verification subprocesses
do not inherit provider keys/other credential variables, but retain toolchain
discovery variables including PATH/HOME. Their filesystem/network permissions are
those of the enclosing process. Timeouts kill direct children, not arbitrary
descendant process trees. Use an external sandbox before live runs; this runner
is not a security boundary against hostile generated code.

Live mode calls the actual configured model, not the offline solution script.
It uses at most 12 model calls, 8,192 reported output tokens and 180 seconds per
case, with 30 seconds per compilation and 5 seconds per test process. Provider
enforcement/unknown usage means these are not a financial spending guarantee.
The runner never changes subscription/billing settings or enables extra usage.
Cancellation of the entire evaluator process is not crash-safe tool journaling.

## Artifacts and interpretation

`report.json` records suite version, mode, model, pass/fail, model-call count,
reported completion tokens, tool errors, test calls, elapsed time and run error.
Every case also retains `workspace/`, `oracle.rs`, `events.jsonl`, `session.json`
and `verification.json`. Reports are updated after each completed case. Inspect
failed traces/source before changing the harness or blaming the model. Output
directories are private on Unix; traces can include reasoning and source text.

For comparisons, pin the repository commit and exact model/provider, repeat each
task on fresh workspaces, use the same task/oracle and equivalent budgets, and
report raw pass counts plus latency/call/token distributions. Do not compare an
offline known-solution script with Pi/OpenCode or compare harnesses using
different models and call the difference an architectural improvement.

These four tiny tasks are insufficient for broad agentic capability claims.
Next meaningful work is live repeated runs, larger held-out repository fixes,
command/process sandboxing, context pressure/compaction and interruptions during
effectful work. No live score is recorded until an authenticated run completes.
