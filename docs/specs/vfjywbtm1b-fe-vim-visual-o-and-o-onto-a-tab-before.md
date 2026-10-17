---
task_uid: "01m54r2ef34c7m8tvfjywbtm1b"
task: "T134"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test visual_swap_onto_a_tab_aims_at_its_first_column (tests/visual_swap_tab_goal.rs, top level) passes, as nvim --clean does with tab stops of 4: on \"xyzw\\n\\ta\" from (1,0) vlOk and vlok leave the cursor at (0,0), from (1,1) vhoOk at (0,0); on \"xyzw\\n\\ta\\nxyzw\" from (1,0) Vlok, \u003cC-v\u003elok and \u003cC-v\u003ejlok at (0,0); on \"xyzwvu\\n\\t\\ta\" from (1,1) vlOk at (0,4); vlOD on \"xyzw\\n\\ta\" from (1,0) leaves \"xyzw\" with the caret at (0,0); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: a swap that leaves the cursor after the anchor (vhOk on \"xyzwvu\\n\\t\\ta\" from (1,1): (0,5)), blockwise O's own goal rules, and Esc and yank after a visual move; the first-column rule lives in one helper that move_cursor and swap share"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: visual o and O onto a tab before the anchor aim at the tab's first column, as nvim does, so j, k and D from there land under it (Mac T765)

## Problem

`o` and `O` in a visual mode that move the cursor onto a tab (or another character wider than a column) before
the anchor leave fe-vim's cursor aiming at the tab's last column, so a following `j`, `k` or a delete of the
row lands too far right. nvim aims at the tab's first column there, as it shows the visual cursor at the start
of a tab when the cursor is before the anchor, and as fe-vim's own visual motions already do
(`crates/fe-vim/src/visual.rs` `move_cursor`: "A goal set afresh on a tab is its first column if the cursor
is before the anchor"). `visual::swap` sets `goal: None` for every swap but blockwise `O`, so the goal is worked
out later from the cursor's position as normal mode would: the tab's last column.

Reproduced at Mac main 838c0b35 with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings; tab stops every 4 columns):

| buffer | cursor | keys | nvim cursor | fe-vim cursor |
|---|---|---|---|---|
| `xyzw\n\ta` | (1,0) | `vlOk` | (0,0) | (0,3) |
| `xyzw\n\ta` | (1,0) | `vlok` | (0,0) | (0,3) |
| `xyzw\n\ta` | (1,1) | `vhoOk` | (0,0) | (0,3) |
| `xyzw\n\ta\nxyzw` | (1,0) | `Vlok` | (0,0) | (0,3) |
| `xyzw\n\ta\nxyzw` | (1,0) | `<C-v>lok` | (0,0) | (0,3) |
| `xyzw\n\ta\nxyzw` | (1,0) | `<C-v>jlok` | (0,0) | (0,3) |
| `xyzwvu\n\t\ta` | (1,1) | `vlOk` | (0,4) | (0,5) |
| `xyzw\n\ta` | (1,0) | `vlOD` | text `xyzw`, (0,0) | (0,3) |

Already the same as nvim, and must stay so: `vhOk` on `xyzwvu\n\t\ta` from (1,1) (the cursor ends after the
anchor: (0,5)), `vlOdk` on `xyzw\n\ta` ((0,0)), and blockwise `O`'s own goal rules.

A prototype of the change below (3 lines in `swap` plus a 6-line helper) fixed every row above and took two
random fuzzes against nvim (3,000 cases each of `v`/`V`/`<C-v>` with motions over tabs, wide characters and
short rows, then `o`/`O` and `j`/`k`/`<Esc>`/`d`/`y`/`D`/`X`) from 23 mismatches to 16 with no new mismatch;
`cargo test -p fe-vim` passed unchanged. The 16 left are the separate `<Esc>` and yank rules below.

## Change

In `crates/fe-vim/src/visual.rs` `swap`: give the swapped ends the goal a cursor set afresh there takes, the
same rule as `move_cursor`'s: on a character before the anchor, its first column
(`cursor.line.start_column`), else none (worked out from the position as now). Blockwise `O` (`same_row`)
keeps setting its own goal after that, as now. Factor the rule into one helper that `move_cursor` and `swap`
both use rather than writing it twice.

Out of scope (separate rules, left as they are): `<Esc>` and yank after a horizontal visual move onto a tab
before the anchor (`vh<Esc>k` on `xyzw\n\ta` from (1,1): nvim (0,0), fe-vim (0,3)), which nvim keeps but only
when the goal was set afresh, not after `j`/`k`; the yank's caret lives in `operator.rs`.

Regression test: a new integration test `crates/fe-vim/tests/visual_swap_tab_goal.rs`, at the top level of
the file, `visual_swap_onto_a_tab_aims_at_its_first_column`, driving `fe_vim::HeadlessVim` through the
table's rows (cursor, mode, and the text for `vlOD`). Probe any added row with `nvim --clean --headless` first.

## Implementation notes

- `before_anchor_goal` in `visual.rs` is shared by `move_cursor` and `swap`.
- `vhOk` on `xyzwvu\n\t\ta` from (1,1): `nvim --clean` leaves the visual cursor at (0,6) (the row's newline, 1-based col 7), and fe-vim gives (0,6) both before and after this change, so the regression test asserts (0,6); the spec's (0,5) is the normal-mode clamp.
