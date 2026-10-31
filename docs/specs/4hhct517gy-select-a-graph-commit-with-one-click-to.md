---
task_uid: "01m67vc6w53g9vvt4hhct517gy"
task: "T261"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_select_tests::click_expands: one click on a commit row selects and expands it (Enter and o still toggle); a click on an already expanded commit's row collapses it"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_select_tests::file_focus: clicking a file row opens its diff; clicking another file of the same commit replaces the same preview tab instead of adding one; a double click or Enter keeps it, including repeated keep requests (a triple click, repeated Enter) for a diff still being read, whichever read lands first, and a keep made after navigating to another pane does not replace that pane's preview; a request in another window does not cancel this window's preview"
    required: true
    manual: false
  - id: "C3"
    text: "git_log_select_tests::header: the expanded commit shows 'N files changed, +X -Y' above its file rows"
    required: true
    manual: false
  - id: "C4"
    text: "git_log_open_tests still pass; cargo test -p fe-workbench passes; clippy clean"
    required: true
    manual: false
---

# Select a Graph commit with one click to show its changed-file summary, and a click on a file focuses that file's diff in one reused tab (Mac T1223)

## Problem
The user: "You can see the summary of all the changes by selecting the commit and then click a file to focus in on it." Enter or click already expands (T610). Make selection and focus match VS Code: one click expands, file clicks reuse a preview tab.

## Review findings folded in (round 3)
F5: keep the promotion and the replacement slot across repeated keep requests for the same pending diff (a triple click, repeated Enter while it loads), preferably by coalescing reads, so the permanent diff replaces the preview whichever read lands first.
F6: tie the promotion to the original preview request and its pane/navigation state; when that context has changed (the user moved to another pane), open the permanent diff without replacing the current preview.
F7: sequence requests per window/workbench instead of the application-global Opened.asked counter, keeping any shared view registry separate, so a request in another window cannot cancel this window's preview.
Each has a controlled-completion test: triple click and repeated Enter with an existing preview, an intervening-pane-navigation test, and a two-window test.
