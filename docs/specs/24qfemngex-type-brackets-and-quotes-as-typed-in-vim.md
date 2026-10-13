---
task_uid: "01m4sjp0wy7f603d24qfemngex"
task: "T82"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::insert_tests::vim_insert_types_pairs_as_typed: in a Rust view with vim on, i then typing ( [ { \" leaves exactly ([{\" with the caret after it, escape then u undoes it as one step, and with vim switched off typing ( gives () with the caret between"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test commands::pairs_host_tests::per_language passes unchanged (vim off still pairs per language), as do the existing vim tests"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Type brackets and quotes as typed in vim insert mode, unpaired, as before auto-close (T69 review F1) (Mac T477 follow-up)

Follow-up from T69 (Mac T477, auto-close pairs, landed as 5298ee4), filed on helium. Review 01m4sf07t4srn20cd6g5rtmd8m
(`rgt review show T69`), finding F1. This is a regression against T69's own spec, so it lands first.

## Problem

T69's spec excluded vim's insert mode: "Not in this card: vim's insert mode (vim types through fe-vim, not
`editor.insert`)". That assumption was wrong. In vim's insert mode, ordinary typed text reaches
`editor.insert` through the `vim::TEXT_INPUT` branch of `EditorView::run_command`
(`crates/fe-gpui/src/commands.rs` :437, `command if vim::TEXT_INPUT.contains(&command) => dispatch(...)`). Only
replace mode and text with tabs or newlines go through `vim::type_as_keys` instead. `editor.insert`
(`crates/fe-editor/src/commands.rs` :480) now sends a single char to `EditorModel::insert_typed(ch, host.auto_pairs())`.
`ViewHost::auto_pairs` (`crates/fe-gpui/src/commands.rs`) answers from the loaded language whether vim is on or
not. So with vim on, typing `(` in insert mode in a Rust view gives `()`. The reviewer reported: "typing '(' in a
Rust Vim view now inserts '()'. The existing Vim tests use views without a loaded language and miss this
regression."

## Change

With vim on, typing pairs nothing: `editor.insert` inserts exactly what was typed, as it did before T69.

- `ViewHost` (`crates/fe-gpui/src/commands.rs`) gets a `pub(crate) vim: bool` field, and `auto_pairs` returns `&[]`
  when it is set. `comment_tokens` and every other host answer stay the same.
- `EditorView::run_command` (commands.rs, the `let mut host = ViewHost { .. }` at about :382) sets
  `vim: self.vim.is_some()`. The vim reducer's host (`crates/fe-gpui/src/vim.rs` :1012) sets `vim: true`.
- Do not touch fe-editor. With vim off, pairing is unchanged.

## Tests

Add the test to the existing `crates/fe-gpui/src/vim/insert_tests.rs` (already declared from vim.rs at base). Not a
new file: Regatta's regression check applies only test files to base, so a test module declared in vim.rs can't run
there (rev 1's auto_pairs_tests.rs failed the check with "no tests to run"). Build the editor as that file does (a
focused window, vim on), then `set_language(Some("rust"), cx)` and `run_until_parked`. Type through the window's keys and input, not by
calling the model:

- `vim_insert_types_pairs_as_typed`: in a Rust view, `i` then typing `(`, `[`, `{` and `"` leaves exactly
  `([{"` with the caret after it. Escape then `u` undoes the insert as one step. Turning vim off with
  `set_vim_enabled(cx, false)` and typing `(` in the same view gives `()` with the caret between them.
