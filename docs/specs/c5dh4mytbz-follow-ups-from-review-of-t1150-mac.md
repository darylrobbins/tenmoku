---
task_uid: "01m6yhwnd0v0nym0c5dh4mytbz"
task: "T535"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "peek_references_tests::a_root_change_closes_reference_peeks: when the workspace root changes, open reference peeks are closed and their searches cancelled, and no target resolves against the old root (review F1)"
    required: true
    manual: false
  - id: "C2"
    text: "peek_references_tests::streamed_targets_get_baselines: a disk-backed target that streams in after the peek opens gets its baseline registered and loaded, deduplicated and within the budget, and released when the peek closes (review F2)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench -p fe-keymap and cargo clippy -p fe-workbench -p fe-keymap --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Follow-ups from review of T1150 (Mac T1621)

Findings of the review of T1150 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/peek_references.rs:53: The peek-owned ReferencesPanel survives workspace root changes. Workbench::set_root clears only the dock's panel and retains open editor tabs, so this search can continue delivering old-root results. Selecting another reference then resolves its relative path through target_of using the new root, potentially displaying and opening an unrelated file.
  fix: Close and cancel reference peeks when the workspace root changes, or bind their lifetime and target resolution to the originating root. Add a regression covering a root change during streaming and after completion.
  instances: crates/fe-workbench/src/peek_references.rs:53

- F2 medium crates/fe-workbench/src/peek_references.rs:99: Streamed targets never get baselines: open_peek_on receives an empty target vector, schedules an empty baseline read, and references_changed only appends targets. Consequently, even ordinary small files lose the existing peek mapping behavior. If an initially unopened reference file is opened and edited in another pane before selection, its original byte offsets are applied directly to the edited buffer, highlighting and opening the wrong text. This newly affects all streamed disk locations, beyond the carried oversized/budget-exclusion limitations.
  fix: Register and load baselines for incoming disk-backed targets, preserving deduplication and the budget, and remap the selected excerpt when those reads finish. Add a regression that opens and edits a streamed target in another pane before selecting it.
  instances: crates/fe-workbench/src/peek_references.rs:54, crates/fe-workbench/src/peek_references.rs:99
