---
task_uid: "01m5fk8620f4htxr3p8qv0escy"
task: "T210"
rev: 7
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test workspace_edit::tests::ops::a_repeated_skippable_rename_resolves_each_occurrence passes: with rename a→b, create a (ignoreIfExists), the same rename, edit b.rs, the edit lands in a.rs's buffer and workspace/applyEdit is answered applied"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes and cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-workbench: guard test for a repeated skippable rename resolving each occurrence by operation identity (T188 F10; the fix landed in T222)

Follow-up of T188 review finding F10 (round 3, codex review vmarea), filed at the lead's request.

## Problem

fe-lsp decodes a text edit that depends on whether a create or rename with `ignoreIfExists` (without `overwrite`) is made as `EditTarget::Depends { on, made, skipped }`. `on` is the operation itself (`Arc<FileEdits>`), and the workbench (`resolve_depends`, `crates/fe-workbench/src/workspace_edit.rs`) resolves it against the latest earlier operation equal to it. Two identical operations in one edit are confused. With open `a.rs` and absent `b.rs`: rename a→b (`ignoreIfExists`), create a, the same rename again (skipped, since b.rs now exists), then edit b.rs. Both dependencies resolve as skipped, so the edit goes to b.rs's disk file (Closed) instead of a.rs's buffer, and the edit aborts after the resource operations.

## Change

Give each operation occurrence a stable identity that survives decoding and the willRename joining in `crates/fe-workbench/src/lsp_file_operations.rs` (`joined`, which concatenates several servers' answers). For example, a per-edit token minted by fe-lsp, carried on both the operation entry and the `Depends`. Resolve each dependency against that occurrence's recorded outcome.

## Tests

- fe-lsp: the repeated-rename sequence above decodes with distinct identities for the two renames.
- fe-workbench (`workspace_edit_ops_tests.rs`): the same sequence pushed through `workspace/applyEdit` is answered applied, with the edit in a.rs's buffer, now at b.rs.

## Split (Mac lead z40)
The fe-lsp half (FileEdits.operation / OperationId and its fe-lsp tests) is now T222, which this task depends on; do not touch crates/fe-lsp here. This task keeps only the fe-workbench behaviour change (resolve ignoreIfExists dependencies by the operation's identity, in crates/fe-workbench/src/workspace_edit.rs) and its regression test `workspace_edit::tests::ops::a_repeated_skippable_rename_resolves_each_occurrence`. The abandoned lane's full change is on branch `keep/t210` (commit 30b1a1fc).
