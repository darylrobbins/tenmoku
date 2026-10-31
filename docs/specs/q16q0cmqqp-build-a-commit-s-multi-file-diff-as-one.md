---
task_uid: "01m62qj63be3ka3gq16q0cmqqp"
task: "T1227"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test commit_diff_tests::sections: a commit adding n.rs and editing a.rs gives two sections in the commit's file order, each with kind, counts (from commit_stats), left text (empty for added) and right text (empty for deleted)"
    required: true
    manual: false
  - id: "C2"
    text: "commit_diff_tests::lazy: texts load per section on the background executor, nearest-to-viewport first; a section not yet loaded is a 'Loading…' placeholder and a failed read is a muted error row, not a panic"
    required: true
    manual: false
  - id: "C3"
    text: "commit_diff_tests::collapse: collapsing a section keeps its header and drops its rows from the layout; collapse-all and expand-all work; state survives a reload of the same commit"
    required: true
    manual: false
  - id: "C4"
    text: "commit_diff_tests::big: a file over the DiffView size limit or a binary file is a header with 'not shown' instead of a diff; 200 sections lay out without reading every text up front"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench passes; clippy clean"
    required: true
    manual: false
---

# Build a commit's multi-file diff as one model: a section per changed file with its status, counts, the two texts, the folded hunks and collapse state

## Problem
The user: "Add one scrolling view as card". VS Code opens a commit as one scrollable editor with a collapsible section per file. This card is the headless half, and the larger one: model for the view card that follows it. If it is more than one session, stop after sections and lazy loading and say so. git_log.rs is touched only to hand a commit to it (a `commit_diff_for(commit)` accessor).
