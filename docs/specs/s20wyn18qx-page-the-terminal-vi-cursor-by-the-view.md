---
task_uid: "01m54cva478z1tf7s20wyn18qx"
task: "T139"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-terminal-view test vi_page_keys::ctrl_f_and_ctrl_b_put_the_vi_cursor_on_the_new_top_or_bottom_row passes: in a view of R rows (20 \u003c R \u003c 40) over 120 numbered lines, with the vi cursor on the top screen row's line + 4 at column 3 and the view at the top of history, ctrl-f scrolls R - 2 lines and puts the cursor on row 0 at column 3; ctrl-b then scrolls back R - 2 lines to the top of history with the cursor on row R - 1 at column 3; 2 ctrl-f scrolls 2 × (R - 2) lines; pagedown and pageup do what ctrl-f and ctrl-b do; ctrl-b with the view at the top of history moves nothing; ctrl-f within R - 2 lines of the bottom scrolls to the bottom with the cursor on row 0, and ctrl-f at the bottom puts the cursor on row R - 1; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "ctrl-d, ctrl-u, ctrl-e and ctrl-y are unchanged: tests/vi_scroll_lines.rs passes unedited, and in tests/vi_mode.rs only the ctrl-f and ctrl-b steps of its scroll table (and the 2 ctrl-e step after them, where the cursor's line carries over) change; fe-workbench terminal_vi_mode_tests pass unedited"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal -p fe-terminal-view -p fe-workbench passes; cargo clippy -p fe-terminal -p fe-terminal-view --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Page the terminal vi cursor by the view's height less two rows with ctrl-f and ctrl-b, onto the new top or bottom row, as nvim does, instead of a full page on the same screen row (Mac T751)

## Problem

In the terminal's vi copy mode, ctrl-f / PageDown and ctrl-b / PageUp page by the view's full height
and leave the vi cursor on its screen row. crates/fe-terminal-view/src/vi.rs `ViKeys::key` maps them to
`ViCommand::Scroll(±rows × count)`, the same `Terminal::vi_scroll` (crates/fe-terminal/src/vi.rs) that
ctrl-d and ctrl-u use, which keeps the cursor's row on screen.

nvim (0.12.6, `-u NONE`: scrolloff=0, nostartofline) pages by two rows less than the window and puts the
cursor on the new edge row. Checked headless on a 200-line buffer in a 20-row window:

- view 1–20, cursor on line 3, 10, 18, 19 or 20, `<C-f>`: view 19–38, cursor on line 19 (the new top row);
- view 50–69, cursor 55 col 3, `<C-f>`: view 68–87, cursor 68 col 3; `<C-b>`: view 32–51, cursor 51 col 3
  (the new bottom row; the column is kept);
- `2<C-f>` from view 1–20: view 37–56, cursor 37; `3<C-b>` from view 50–69: view 1–20, cursor 20;
- view 10–29 (can only go back 9 lines), `<C-b>`: view 1–20, cursor 20;
- `<C-b>` with view 1–20: nothing moves; `<C-f>` with the last line in view: cursor on the last line;
- `<PageDown>` is `<C-f>`.

Reproduced at Mac main 80e6c76d in a 22-row TerminalView over 120 lines of output (`gg 4j`, cursor on
`line 5`, row 4): ctrl-f scrolls 22 lines and leaves the cursor on row 4 (`line 27`); nvim would scroll
20 and put it on row 0 (`line 21`). From the bottom, ctrl-b scrolls 22, not 20.

## Change

- crates/fe-terminal/src/vi.rs: add `Terminal::vi_scroll_page(&self, pages: i32)` (positive toward the
  bottom, negative back into history; nothing outside vi mode). It scrolls the view by
  `pages × max(screen_lines − 2, 1)` lines, as far as it can, then puts the vi cursor, keeping its column,
  on the view's top row after paging forward or its bottom row after paging back. When the view can't
  move at all: paging forward puts the cursor on the bottom row (the last line, as nvim's `<C-f>` with the
  last line in view does); paging back leaves it where it is. A selection's moving end follows, and
  `view_moved` is told, as `vi_scroll` does.
- crates/fe-terminal-view/src/vi.rs: a `ViCommand::Page(i32)` variant; ctrl-f and PageDown give
  `Page(count)`, ctrl-b and PageUp `Page(−count)`, run through `vi_scroll_page`. ctrl-d, ctrl-u, ctrl-e
  and ctrl-y are unchanged. Update the `ViKeys` unit test in the same file for the new commands.
- crates/fe-terminal-view/tests/vi_mode.rs: its ctrl-f / ctrl-b steps (the "ctrl-d, ctrl-u, ctrl-f and
  ctrl-b move the cursor with the view" table) pin the old behavior; update those two steps to the new one
  (and the `2 ctrl-e` step after them, whose expected line follows from where ctrl-b leaves the cursor, or
  put the cursor back on its line before it), and leave the rest of the file alone.

The regression test is a new integration test, crates/fe-terminal-view/tests/vi_page_keys.rs, whose test
fn sits at the top level of the file; it copies the `style`/`until`/`draw`/`screen`/`open`/`at` helpers
from crates/fe-terminal-view/tests/vi_scroll_lines.rs and drives the view with keystrokes only, so it
builds at the base.

Drafted at Mac main 80e6c76d, which has Mac T743 (ctrl-e/ctrl-y in both vi.rs files). Start it only on a
trunk that contains T743 (Mac 1fbebd4f, or the host's own landing of it).
