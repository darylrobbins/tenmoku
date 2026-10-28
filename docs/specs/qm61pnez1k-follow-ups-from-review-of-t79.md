---
task_uid: "01m5waxq7t2ajzt1qm61pnez1k"
task: "T248"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T79

Findings of the review of T79 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-lsp/src/rename.rs:160: Capability checks run before document admission, and unknown capabilities are treated as permission to proceed. When called during initialization, prepare_rename chooses the local fallback even if the server subsequently advertises prepareProvider, bypassing its validation and placeholder. rename queues a request even if initialization subsequently reveals no renameProvider, violating the no-request requirement. The actor processes these admissions after initialization without rechecking capabilities.
  fix: Check the initialized session's rename capabilities when admitting the operation, or explicitly reject operations while capabilities are unknown. Add delayed-initialization tests for a preparing server and a server without renameProvider.
  instances: crates/fe-lsp/src/rename.rs:80, crates/fe-lsp/src/rename.rs:127, crates/fe-lsp/src/rename.rs:160

- F2 medium crates/fe-lsp/src/rename.rs:93: The local prepare fallback returns the admission receiver without RENAME_TIMEOUT. Admission is asynchronous and can wait behind initialization or a blocked outgoing queue, so this path can exceed the advertised ten-second deadline; slow initialization can leave it waiting until the separate thirty-second initialization timeout.
  fix: Apply within with RENAME_TIMEOUT to the fallback receiver as well, and test a queued admission with ManualClock to verify Timeout after ten seconds.
  instances: crates/fe-lsp/src/rename.rs:93

## Implementation notes

Both findings were already fixed in the code by T190 (`admit_once_known`
waits for capabilities, decides at admission, and applies `within` with
`RENAME_TIMEOUT` to the whole wait, fallback included). The delayed-init
tests for a preparing server and a server without `renameProvider` already
existed in `tests/provider_wait.rs`. Added the missing test: a queued
prepare-rename fallback (and rename) answers `Timeout` on the manager's
`ManualClock` while `initialize` is held.
