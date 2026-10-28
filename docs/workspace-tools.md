# Workspace tools

`WorkspaceTools::read_only(root)` grants a directory capability using `cap-std`.
`with_edits()` is an explicit host grant adding edit/create operations. Runtime
catalog and allow-list checks still apply. Never derive these grants from a
model response or a saved tool catalog.

Tools: `list_files`, paginated `read_file`, literal `search`, `edit_file`, and
create-only `write_file`. All accept strict JSON arguments and reject unknown
fields. File paths are relative, UTF-8 and exclude parent traversal, symlinks,
hidden names, `target`, `node_modules`, `vendor` and `venv`. The directory
capability constrains path resolution; it is not a process/network sandbox.
The exclusions reduce accidental secret/build access, but cannot discover all
secrets embedded in normal source files. Only grant a directory you intend to
share with the configured model.

Edits require nonempty `old_text` matching exactly one position, including
overlapping matches. A conflict leaves the file unchanged and gives the model
static corrective feedback to re-read and choose a unique fragment. Successful
edits use a same-directory temporary file and rename, preserving file permissions.
New-file writes use create-new semantics and never overwrite an existing file.
Parent directories must exist. No deletion or arbitrary command execution is
provided by these tools.

Operations run on blocking workers under a per-workspace lock. This prevents
concurrent Rein calls from racing each other, but external editors are not
locked. A workspace needs one cooperating owner; this is not transactional
concurrency control against hostile local processes. A filesystem operation may
continue after its async wait is cancelled, so interrupted edits have uncertain
outcomes and must be inspected before retrying. Disk errors can leave a partial
new file; forced termination may leave an excluded temporary edit file. This
layer is not durable effect journaling.

Limits: 256 KB per file, 2,000 traversed directory entries, 32 directory levels,
200 listed files, 100 search matches and 500 lines per read page. Search skips
unreadable, binary and oversized files and reports the skipped count. Listing and
search indicate truncation; reads return `next_line`. Narrow the directory or
paginate instead of assuming truncated results are complete.

Tests use disposable directories and verify edits, UTF-8, pagination, read-only
policy, create collisions, stale/ambiguous/overlapping edits, size limits,
permissions, excluded paths and symlink escape attempts. This is a useful coding
tool substrate, not evidence of model-level parity with Pi/OpenCode.
