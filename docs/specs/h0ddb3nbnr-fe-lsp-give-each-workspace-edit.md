---
task_uid: "01m5hm1nw3f60grjh0ddb3nbnr"
task: "T222"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test repeated_skippable_renames_have_distinct_identities passes: the two identical renames in a→b, create a, a→b, edit b.rs decode with distinct OperationIds"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes and cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-lsp: give each workspace-edit operation occurrence a stable identity (FileEdits.operation, OperationId) (T188 F10, part 1 of T210)

First half of argon T210 (Follow-up of T188 review finding F10), split at the Mac lead's z40 decision: Finish's regression base run compiles the whole workspace, and fe-lsp's changed test files can't compile at the base without the new API. So the API and its fe-lsp tests land first as a non-bugfix card; T210 (bugfix) then depends on this and keeps only the fe-workbench behaviour change and its regression test.

## Change
Give each operation occurrence in `crates/fe-lsp/src/workspace_edit.rs` a stable identity that survives decoding and willRename joining: add `FileEdits.operation` and `OperationId` (re-export from `lib.rs`). Behaviour of fe-workbench must not change in this card: only the minimum edit in `crates/fe-workbench/src/workspace_edit.rs` so the workspace still compiles.

Reference: branch `keep/t210` (commit 30b1a1fc) holds the whole original change, fe-lsp side included. Port the fe-lsp side; the fe-workbench behaviour fix and its test are T210's.

## Tests
fe-lsp: the repeated-rename sequence (rename a→b, create a with ignoreIfExists, the same rename, edit b.rs) decodes with distinct identities for the two renames (`repeated_skippable_renames_have_distinct_identities` in `workspace_edit_ops_tests.rs`), and the existing fe-lsp workspace_edit tests are updated for the new field.
