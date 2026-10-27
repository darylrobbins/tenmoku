---
task_uid: "01m5x1340kj4nbjgehz41jtvs3"
task: "T366"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T61

Findings of the review of T61 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp_rename.rs:152: Completion does not recheck the originating buffer's version. If that buffer changes while rename is pending and the server's edit contains only other files, apply_file_edits checks only those targets and applies the rename despite the stale source. This violates the requirement that a stale version changes nothing.
  fix: Before applying any returned edits, verify that the originating buffer still exists at request.version; otherwise show the stale-version prompt and discard all edits. Add a delayed-response test whose edits omit the originating file.
  instances: crates/fe-workbench/src/lsp_rename.rs:152

- F2 medium crates/fe-workbench/src/lsp_rename.rs:233: The placeholder is never actually selected in TextInput: selected is only a delegate boolean, while the input renders and reports a collapsed selection at the end. Inferring edits from string prefixes also loses valid platform replacement edits—for example, replacing 'foo' with 'fo' is interpreted as deletion and clears the input. The existing test checks text and append-style typing, so it does not establish the required selection behavior.
  fix: Use an input that supports actual selection, select the entire placeholder when opening, and let input editing handle replacement rather than inferring it from resulting strings. Test the selected range and platform replacement/deletion behavior.
  instances: crates/fe-workbench/src/lsp_rename.rs:233, crates/fe-workbench/src/lsp_rename.rs:263
