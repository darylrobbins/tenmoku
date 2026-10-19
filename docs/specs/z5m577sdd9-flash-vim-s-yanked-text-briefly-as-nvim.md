---
task_uid: "01m569n2746jwktfz5m577sdd9"
task: "T198"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test yank_ranges_are_recorded (top level, in crates/fe-vim/tests/yank_ranges.rs) passes: VimState::last_yank() gives a new serial and the yanked ranges after yiw, yy, 2yy, viwy, a blockwise y (one range per row) and \"ayy, and keeps the old serial after dd, x and cw."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::clipboard_tests::yank_flash_tests::a_yank_flashes_its_text (new crates/fe-gpui/src/vim/yank_flash_tests.rs, declared in crates/fe-gpui/src/vim/clipboard_tests.rs) passes: in a real view, yiw puts a Highlight decoration over the word, gone once the executor's clock passes 150 ms or at the next edit, a second yank replaces it, and dd shows none."
    required: true
    manual: false
---

# Flash vim's yanked text briefly, as nvim's vim.hl.on_yank and VSCodeVim's highlighted yank do, through an editor highlight decoration (Mac T889)

## Problem

At Mac main 6f032b12 a vim yank shows nothing: nvim users run `vim.hl.on_yank()` and VSCodeVim and IdeaVim
have "highlighted yank", a brief flash over the yanked text. The user approved it in the "Vim parity" track
on 2026-10-17. fastedit's editor already paints host decorations (`fe_editor::decorations`,
`DecorationKind::Highlight`, `EditorModel::add_decoration_provider`, painted by
`crates/fe-gpui/src/element/decorations.rs`), which is the seam.

## Change

- fe-vim: every yank (`y`, `Y`, a visual yank, blockwise too; not a delete or change) records its ranges
  (one per row for a block) with a serial on `VimState` (`crates/fe-vim/src/state.rs`, set where the yank
  writes its register in `crates/fe-vim/src/operator.rs`): `VimState::last_yank() -> Option<(u64,
  &[Range<usize>])>`.
- fe-gpui (`crates/fe-gpui/src/vim.rs`): after running a vim command, a new serial puts the ranges in a
  `DecorationSet` provider on the view's model as `Highlight` with `Tint::Accent`, and removes them 150 ms
  later (nvim's `on_yank` default) or at the next edit, whichever comes first; a new yank replaces it.
  Always on; no setting.

Tests: a new `crates/fe-vim/tests/yank_ranges.rs` (ranges for charwise, linewise, block and visual yanks;
none for `d`), and a new `crates/fe-gpui/src/vim/yank_flash_tests.rs`, declared in the existing
`crates/fe-gpui/src/vim/clipboard_tests.rs` by `#[path = "yank_flash_tests.rs"] mod yank_flash_tests;`,
asserting the decoration appears after `yiw` and is gone after the timeout (advance the test executor's
clock).
