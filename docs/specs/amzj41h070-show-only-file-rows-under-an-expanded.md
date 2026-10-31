---
task_uid: "01m69a22et8fwgyzamzj41h070"
task: "T266"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "An expanded Graph commit lists the commit row and then only its file rows: no message-body row, no trailer row ('Regatta-Task: ...', '(cherry picked from commit ...)') and no 'N files changed, +X -Y' row; Line::Body and Line::Summary and body_text/summary_text are removed from git_log.rs and their arms in git_log.rs and git_log_menu.rs go with them."
    required: true
    manual: false
  - id: "C2"
    text: "The commit hover card still shows the full message, trailers and the files-changed totals: git_log_card_tests (the files-changed and trailers_and_refs tests) pass unchanged, and nothing shows that metadata anywhere else."
    required: true
    manual: false
  - id: "C3"
    text: "git_log_select_tests::header (asserts the totals row at edit+1), git_log_rail_tests::rail_is_unbroken_through_a_commits_expanded_rows (body and totals at lines 1-2) and git_log_tests (totals row asserted at lines[at+1]) are changed to expect file rows directly under the commit; git_log_select_tests file_focus's edit+2/edit+3 indices and any other offsets are updated; git_log_expanded_tests::file_rows_only, new and declared from git_log.rs with #[path] like git_log_select_tests, proves a commit with a multi-line body and trailers expands to exactly its file rows."
    required: true
    manual: false
  - id: "C4"
    text: "Keyboard navigation, selection, Enter/o collapse, rail continuity through the file rows and the file context menu work over the shortened expansion, and the module doc comment of git_log.rs describes file rows only. cargo test -p fe-workbench passes; clippy clean."
    required: true
    manual: false
---

# Show only file rows under an expanded Graph commit, leaving its message body, trailers and files-changed totals to the hover card (Mac T1251)

## Problem
The user: "Can't we put this metadata in the hover rather than in the list like VS Code. The list should just be files. ... Actually looks like VS Code just doesn't show them. That's probably fine."
Expanding a Graph commit lists its message body as dimmed rows (trailers such as Regatta-Task, Regatta-Op, cherry-pick lines) and a '37 files changed +1308 -26' row before the files. The hover card (T1221) already shows the full message, trailers and totals, so these rows only add noise.
## Change
In relayout (git_log.rs ~1204), push only Line::File rows after Line::Commit (keeping Line::Note for Loading/error). Delete Line::Body, Line::Summary, body_lines use in the list, body_text, summary_text and their draw, key and menu arms (git_log.rs ~212-242, 1463, 1700-1745, 2431; git_log_menu.rs:164). Keep body_lines/split_trailers for the card. Do not add the metadata anywhere else.
