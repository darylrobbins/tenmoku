---
task_uid: "01m64wge3mfppfb2gk17jcgq0p"
task: "T1241"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::caret_keys_lines::vim_line_takes_caret_keys (new file, declared from tests/mod.rs): in the vim ex line, typing abc, Left, X gives abXc; Home, End, word-left and Delete behave as in TextInput; the line stays open and Esc/Enter still end it. Real key presses."
    required: true
    manual: false
  - id: "C2"
    text: "tests::caret_keys_lines::git_commit_box_takes_caret_keys: same for the commit message box; Backspace on an empty input still ends the line (vim_line: end_line(None, Backspace); git_panel: end_line(None, true))."
    required: true
    manual: false
  - id: "C3"
    text: "The Backspace-on-empty arms stay ahead of the generic op.edit() path, and vim_line still calls resolve_line_prefix before any op, including the new caret ops (test: a held c map resolves before a caret key)."
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests pass unedited: cargo test -p fe-workbench (vim_line, git_panel) and cargo test -p fe-gpui vim. cargo clippy --all-targets and cargo fmt --check pass in the root workspace; kiln/ is its own workspace, so run clippy and fmt there too."
    required: true
    manual: false
---

# Vim command line and git commit box inputs take the TextInput caret keys through PickerOp::edit, keeping their Backspace-on-empty exit
