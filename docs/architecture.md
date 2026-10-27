# Architecture

Rein owns the agent semantics. Rig 0.44.0 owns the provider wire protocol,
provider-neutral messages, stream folding and native replay metadata. We do not
nest Rig's high-level agent loop inside Rein's loop.

```text
CLI / future Synapse host
  -> exclusive AgentSession + RunOptions + control receiver + event callback
    -> Rein lifecycle / scheduler
      -> prepare_context -> ModelBackend -> Rig stream -> validated assistant turn
      -> catalog + host allow-list -> ToolExecutor -> correlated results
    -> idle session -> atomic checkpoint
```

## Ownership

- `session`: conversation state, not transport handles or credentials.
- `transcript`: append-only accepted messages, IDs and provenance. IDs count from
  one, and only have meaning inside their original session.
- `memory`: goal, choices, progress and next step. Evidence links point to history;
  they do not establish that a claim is true or authorized.
- `context`: request projection, canonical history validation and byte admission.
- `runtime/types`: host contracts, controls, events and limits.
- `runtime/mod`: run preflight, active-run ownership and terminal cleanup.
- `runtime/driver`: explicit model/tool loop and input scheduling.
- `runtime/control`: FIFO queues, cancellation and deadline-aware awaits.
- `runtime/execution`: sequential authorized tool dispatch and result settlement.
- `provider`: one host-configured OpenAI Chat Completions connection using Rig.
- `storage`: bounded, validated, versioned session checkpoints.
- `cli`: environment configuration, signals, output and checkpoint policy.

Helpers stay inside the owning feature, not a generic utilities folder. Types
remain beside their behavior; only larger capabilities are split into modules.

## Lifecycle

`run` rejects an already active run, blank/oversized input and invalid options.
It accepts the initial input into history, creates `active_run`, then repeats:

1. Read controls; accept pending steering before the next model call.
2. Check model-call/token allowance. Prepare and byte-check the request, including
   the explicit remaining output-token limit.
3. Stream the model turn. Text updates the provisional draft; all stream items,
   including reasoning and unknown items, are forwarded to the host.
4. Accept only a complete, nonempty, valid response with a normal/tool-use stop.
   Length-limited, filtered, failed and unknown-stop responses fail closed.
   Preserve Rig's origin and provider-native artifacts in accepted history.
5. Reject duplicate call IDs or too many tools before effects. For each call,
   require a catalog entry, host authorization and parseable arguments. The
   executor then validates its typed arguments and performs the operation.
6. Append one user-role message containing all correlated tool results, then
   loop. Steering never splits a tool-call/result exchange.
7. After a tool-free assistant turn, steering has priority; otherwise accept one
   queued follow-up and continue. With no pending input, finish.

Every normal/error return after startup clears `active_run`. Remaining queued
inputs survive budget exhaustion or failure. An input already accepted into
history stays accepted even if its response subsequently fails. Input IDs are
correlation labels, not deduplication or exactly-once keys.

There is no separate planner, multi-agent swarm, plugin loader or global mutable
session registry. `RunPhase` includes reserved checkpoint/approval states, but
the current runtime does not implement human approval pauses.

## Limits and failure policy

Defaults: 16 model calls, 32,768 output tokens, 300 seconds elapsed time, 1 MB
serialized context, 1 MB stream-event/final-response admission, 64 KB per tool
result, 16 tools per turn and 64 inputs in each pending queue. The control channel
is host-provided and should also be bounded.

Unknown output usage is never reported as zero. It reserves the entire remaining
allowance, preventing another model call, though an otherwise complete final
answer may finish successfully. Provider max-token requests cannot guarantee a
spending cap. Context size is serialized Rig bytes, not provider wire bytes or
model tokens; oversized history is rejected, never silently truncated.

The deadline includes model and tool waiting. Cooperative async execution cannot
interrupt blocking callbacks, blocking executor code or CPU-bound provider work.
Byte checks are admission limits, not hard process-memory caps: a provider frame
must first be decoded. Custom backends must honor the streaming byte limit.

Cancellation drops the in-flight await. Partial model content is not accepted.
If tools were advertised, all receive results: completed results are preserved,
the interrupted tool is marked outcome-unknown, and later tools are marked not
executed. No automatic retries are performed. A failed tool produces a sanitized
error result and the model can recover; provider/runtime failures terminate.

Hosts must request `Control::Cancel` and **await `run`**. Dropping/aborting the
entire run future or panicking inside callbacks bypasses settlement and cleanup.
Executors must be cancellation-safe and must not perform blocking work directly
inside their async futures. Runtime futures do not promise `Send`; a host may
drive them on its owner task/local executor.

## Persistence and trust

Snapshot v1 explicitly records the Rig payload version, session ID, selection,
working memory, tool definitions, ordered transcript entries and queues. IDs are
reconstructed by entry order. Unknown snapshot fields/versions, malformed JSON,
bad model selections, invalid tool exchanges and dangling evidence are rejected.
Snapshots are limited to 32 MiB. Active runs cannot be saved or resumed.

Saving validates the replacement before touching the old checkpoint, writes a
same-directory private temporary file, syncs it, atomically renames it and, on
Unix, syncs the parent directory. Use a local filesystem and a single writer per
path; network-filesystem guarantees and concurrent writers are not supported.
If directory sync fails after rename, save reports failure although the new file
may already be visible. Parents are not created implicitly.

Credentials and allowed-tool policy are never serialized. Snapshots still contain
private conversation/tool data; protect them like source or logs. Unix snapshots
are created mode 0600; other platforms depend on host ACLs. This is neither
encryption nor a tamper-proof audit log. Rig payload changes require an explicit
migration/version bump, not blind loading.

The CLI checkpoints before and after a run, including returned failures. This is
not crash-safe execution journaling. SIGKILL/power loss can lose accepted inputs,
results and effects since the previous checkpoint. Before enabling effectful
tools in Synapse, add durable operation intent/outcome records, explicit host
approval and idempotency keys. A model message or a saved catalog grants none of
these permissions.
