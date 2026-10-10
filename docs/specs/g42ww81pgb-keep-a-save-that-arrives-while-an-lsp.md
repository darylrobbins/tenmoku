---
task_uid: "01m4j9vem89tat4rg42ww81pgb"
task: "T317"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::lsp_resolve_retry_pending::a_save_during_a_failing_root_retry_opens_the_file_after_it: with a root retry in flight that fails canonicalization, creating the root and saving before that retry completes leads to one more resolution once it does, and the file's didOpen reaches the server with no further save"
    required: true
    manual: false
  - id: "C2"
    text: "A successful retry or a root replacement clears the pending save: no extra resolution follows (asserted in the same new test file)"
    required: true
    manual: false
  - id: "C3"
    text: "T237's tests in crates/fe-workbench/src/tests/lsp_resolve_retry.rs and the existing tests in crates/fe-workbench/src/tests/lsp.rs pass unchanged"
    required: true
    manual: false
---

# Keep a save that arrives while an LSP root retry is in flight, and resolve it if the retry fails

# Keep a save that arrives while an LSP root retry is in flight

Follow-up to T237 (finding F1 of its approving review). T237 retries failed LSP path resolutions at
the next save and at a root change. Saves received while a root retry is in flight are drained and
discarded (`crates/fe-workbench/src/lsp.rs` around lines 767 and 776). If that attempt has already
failed canonicalization but its completion has not landed, creating the root and saving again
consumes the recovery-triggering save while `root_retrying` is true; the failed completion then
leaves the manager absent until yet another save.

## Change (`crates/fe-workbench/src/lsp.rs`)

Keep a pending save while a root retry runs. If that attempt fails, schedule one further resolution
for the pending save. Clear the pending state on success and on a root replacement.

## Tests (new file)

`crates/fe-workbench/src/tests/lsp_resolve_retry_pending.rs`, declared in `tests/mod.rs` (commit
the `mod` line with the file: the regression check at base needs it), deterministic: hold the
in-flight retry's completion until the save has been delivered, as T237's `lsp_resolve_retry.rs`
tests sequence their steps. No sleeps.

## Out of scope

Other retry triggers; T237's existing tests stay unchanged.
