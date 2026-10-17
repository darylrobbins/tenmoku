---
task_uid: "01m546711t1wdwz0yxvn6t2cjr"
task: "T137"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-terminal-view test vi_scroll_lines::ctrl_e_and_ctrl_y_keep_the_vi_cursor_on_its_line passes: with line 1..line 60 printed and the vi cursor on line 20 at screen row 19, ctrl-e scrolls the view one line and leaves the cursor on line 20 (row 18), ctrl-y and counted 2 ctrl-e likewise keep it on its line, and the cursor moves only to the view's top or bottom row when its line would leave the view; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Where the view can't scroll (ctrl-e at the bottom of the screen, ctrl-y at the top of history) the vi cursor doesn't move; ctrl-d, ctrl-u, ctrl-f and ctrl-b behave as at the base, and tests/vi_mode.rs keys_move_the_vi_cursor passes with only its ctrl-e and 2 ctrl-e steps changed to expect the cursor to stay on its line"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal -p fe-terminal-view passes; cargo clippy -p fe-terminal -p fe-terminal-view --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep the terminal vi cursor on its line when ctrl-e or ctrl-y scrolls the view, as vim does, moving it only to stay in view (Mac T743)

## Problem

In the terminal's vi copy mode, ctrl-e and ctrl-y move the vi cursor to the next or previous line along
with the view, so they act like a one-line ctrl-d/ctrl-u. In vim (and alacritty's vi mode) they scroll
the view a line and leave the cursor on its text, moving it only when its line would leave the view.

Probed with `nvim --clean --headless` (60 lines, 10-row window, `20Gzt4j`): the cursor is on line 24 with
line 20 at the top; `<C-E>` gives cursor 24, top 21; `<C-Y>` cursor 24, top 20; `2<C-E>` cursor 24, top 21.

fastedit at Mac main 9328d2ca (fe-terminal and fe-terminal-view are identical at mac/main de87d2b5): a
view running `echo line 1` .. `line 60`, `ctrl-shift-space g g 1 9 j` puts the vi cursor on `line 20` at
screen row 19 (offset 39); `ctrl-e` then leaves it on `line 21` at row 19 (offset 38) instead of on
`line 20` at row 18. At the bottom (offset 0) `ctrl-e` moves the cursor down a row where vim's leaves it.

Cause: crates/fe-terminal-view/src/vi.rs `ViKeys::key` maps `ViKey::Ctrl('e')`/`Ctrl('y')` to
`ViCommand::Scroll(±lines(1))`, which runs crates/fe-terminal/src/vi.rs `Terminal::vi_scroll`: that moves
the cursor by the same lines as the view (it keeps the cursor's screen row, right for ctrl-d/ctrl-u). The
T614 spec's "the cursor keeps its screen row, as vim's do" is wrong for ctrl-e and ctrl-y, and
tests/vi_mode.rs `keys_move_the_vi_cursor` pins the wrong rows for them.

## Change

- crates/fe-terminal/src/vi.rs: add `pub fn Terminal::vi_scroll_view(&self, lines: i32)`: in vi mode only,
  scroll the display by `lines` (negative back into history), then `vi_goto_point` the cursor's own line
  clamped to the view's top and bottom rows, same column (so a selection's end follows), and call
  `view_moved` as `vi_scroll` does. `vi_scroll` is unchanged.
- crates/fe-terminal-view/src/vi.rs: add `ViCommand::ScrollView(i32)`; ctrl-e and ctrl-y map to
  `ScrollView(±lines(1))` (count kept) and `vi_command` runs `vi_scroll_view`. ctrl-d/u/f/b stay
  `Scroll`. Update the in-file `typed(...)` unit test's ctrl-e/ctrl-y expectations.
- crates/fe-terminal-view/tests/vi_mode.rs `keys_move_the_vi_cursor`: the `ctrl-e` and `2 ctrl-e` steps
  expect the cursor still on `out 1 12` (down 0), with the same offsets.
- New crates/fe-terminal-view/tests/vi_scroll_lines.rs (its own helpers; vi_mode.rs's are private):
  `ctrl_e_and_ctrl_y_keep_the_vi_cursor_on_its_line`.
