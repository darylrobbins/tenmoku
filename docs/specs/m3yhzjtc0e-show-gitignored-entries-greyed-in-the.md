---
task_uid: "01m57yadxym59crqm3yhzjtc0e"
task: "T194"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The tree lists .idea, target and other gitignored entries greyed by default; .git stays hidden"
    required: true
    manual: false
  - id: "C2"
    text: "The new setting set to false hides them again"
    required: true
    manual: false
  - id: "C3"
    text: "project_ignored_tests.rs proves both"
    required: true
    manual: false
---

# Show gitignored entries greyed in the project tree by default, behind a setting (Mac T985)

## Problem
VS Code Parity Notes (claude.ai doc a15aa9a1, Explorer): fastedit hides .idea, .regatta, .worktrees and target; VS Code shows gitignored entries greyed. crates/fe-workbench/src/project_panel.rs tree_options() returns TreeOptions::default() (show_ignored false) outside tests, although fe-project's Row::ignored and the row's .ignored(row.ignored) styling already exist.
## Change
Make the project tree list ignored entries by default, greyed through the existing ignored styling, behind a new setting so it can be turned off. .git stays unlisted.
