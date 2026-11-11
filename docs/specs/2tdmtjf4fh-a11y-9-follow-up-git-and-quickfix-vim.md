---
task_uid: "01m75tenyg1xx8xs2tdmtjf4fh"
task: "T587"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Failures that git_commands.rs answer_git_op, quickfix_commands.rs say and git_commit.rs host_command put on the vim line are set with EditorView::set_vim_error and announced assertively; reports stay polite; tests assert both through the announcer's Status node"
    required: true
    manual: false
---

# A11Y-9 follow-up: Git and quickfix vim-line failures announced assertively

# A11Y-9 follow-up: uncoded Git and quickfix errors on the vim line are polite

T585 (A11Y-9) announces vim messages through the window's Announcer: assertively for errors, politely for reports. The host marks an error with `EditorView::set_vim_error` (fe-gpui `pane_item.rs`); `set_vim_message` is a report, assertive only when the text has nvim's code (`E13:`, `E37:`).

Some workbench answers reach the vim line through helpers that pass successes and failures alike to `set_vim_message`, so their uncoded failures are spoken politely:

- `Workbench::answer_git_op` (`crates/fe-workbench/src/git_commands.rs:~360`), used by the git features (stash, remote, branch …) for typed `:G…` commands;
- `quickfix_commands::say` (`crates/fe-workbench/src/quickfix_commands.rs:~476`);
- the commit box's `host_command` message closure (`crates/fe-workbench/src/git_commit.rs:~111`), whose E-coded errors are already assertive.

Give each helper an error/report distinction (an `error: bool` or a `Result`-shaped answer) and route failures through `set_vim_error`, with a test per helper that a failure is announced assertively and a report politely (the announcer's `Status` node, as `crates/fe-workbench/tests/a11y_announce.rs` reads it). Note `quickfix_commands.rs` carries T517 F1.
