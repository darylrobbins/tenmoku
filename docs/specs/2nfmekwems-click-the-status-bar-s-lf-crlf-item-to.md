---
task_uid: "01m6rmdahd7rceyn2nfmekwems"
task: "T478"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench test line_ending_picker_tests::picker_lists_lf_and_crlf_with_the_current_marked: on a CRLF file the CRLF row is marked"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test line_ending_picker_tests::picking_converts_and_marks_the_buffer_modified: picking LF on a CRLF file changes the buffer's ending, marks it modified, and saving writes LF; undo restores CRLF"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test line_ending_picker_tests::the_status_item_runs_the_command: the line_ending status item carries workbench.change_line_ending and a tooltip; a read-only viewer's item has no command"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Click the status bar's LF/CRLF item to pick the end of line sequence, with a palette command, using the existing convert commands (Mac T1401)

Lent from Mac T1401 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

The status bar shows LF/CRLF (`crates/fe-gpui/src/pane_item.rs` `texts`, the `(line_ending.into(), None)` entry has no command) but clicking it does nothing, while VS Code opens a "Select End of Line Sequence" picker. The commands `set_end_of_line_lf`/`set_end_of_line_crlf` exist in `crates/fe-editor/src/eol_commands.rs` (convert the buffer's save ending) but are palette-only. Verified: the encoding item has a picker (`encoding_picker.rs`); the line-ending item does not.

Linux-OK.

## Change

- `crates/fe-workbench/src/line_ending_picker.rs` (new): `workbench.change_line_ending` (palette: Change End of Line Sequence...) shows a two-row picker, LF and CRLF, the buffer's current one marked; picking runs the existing editor command for the active editor (no new conversion code; do not add a third ending). Reuse the `Picker` shape of `encoding_picker.rs`.
- `crates/fe-gpui/src/pane_item.rs`: give the line-ending status item `Invocation::of("workbench.change_line_ending")` with tooltip "Select End of Line Sequence". Update `status_bar.rs` module docs where they list the clickable items.
- A buffer with mixed endings is out of scope (the buffer normalises to one on load).
- Read-only / loading viewers: the item stays inert (as the encoding item's rule).

## Conflicts

`pane_item.rs` is touched by nothing queued. `lib.rs`, `workbench_commands.rs` and the command catalog test are shared with other cards.


## Helium notes
- New bindings or commands have broken count tests before (fe-gpui binding counts / DEDICATED, the git_keys chord, crates/fastedit/src/vim_remaps.rs for lazyvim.vimrc). Run the affected crates' tests early and respec any count-pinning test file into scope before the first finish (`rgt task respec --scope`, which replaces the list).
