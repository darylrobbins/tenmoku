---
task_uid: "01m57yaaepkkw9qbsfxdk4r89e"
task: "T205"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A lane's rail is unbroken through expanded file rows"
    required: true
    manual: false
  - id: "C2"
    text: "File rows show name then dimmed directory"
    required: true
    manual: false
  - id: "C3"
    text: "git_log_rail_tests.rs proves both from what the rows paint (segments, text, resolved colour); the rail probe may take its endpoints from the canvas bounds rather than the painted path, a probe that fails when a stroke is shortened being a separate follow-up"
    required: true
    manual: false
---

# Draw the Git Log graph rail through a commit's expanded file rows, files name first with the directory dimmed (Mac T989)

## Problem
VS Code Parity Notes (Source control): an expanded Git Log commit (crates/fe-workbench/src/git_log.rs) breaks the graph rail through its file rows, and files show the full path truncated at the filename.
## Change
Draw each lane's rail continuously through a commit's expanded file rows; show a file as name then dimmed directory.
