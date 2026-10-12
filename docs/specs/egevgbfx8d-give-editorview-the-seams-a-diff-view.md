---
task_uid: "01m4rqeh41f890ykegevgbfx8d"
task: "T48"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::diff_seams_tests::read_only_refuses_edits: on a read-only view, typing, paste, backspace, IME commit and every mutates_buffer editor command leave the text and undo history unchanged; moving, selecting, copy and find still work"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::diff_seams_tests::read_only_vim: with vim on, 'i' then 'x', 'dd', 'p' and 'o' each leave the text unchanged, the mode normal and vim's message 'E21: Cannot make changes, 'modifiable' is off'; 'j', 'w', 'v' then 'y', and '/foo' work"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::diff_seams_tests::host_blocks: a Filler block of 2 rows above row 3 and a Lines block of 2 lines above row 6 put rows 3 and 6 two and four screen rows lower; the Lines text paints on its tint; a click on the text row below each block puts the caret on that row; a block above a folded row isn't painted"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::diff_seams_tests::scroll_sync_api: scrolling with the wheel emits Scrolled with the new top in screen rows; set_scroll_top moves the view without emitting; scroll_top counts block rows"
    required: true
    manual: false
  - id: "C5"
    text: "Existing fe-gpui tests and screenshot goldens pass unchanged (no blocks set means no layout change); cargo test -p fe-gpui passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Give EditorView the seams a diff view needs: read-only with vim's E21, host-set filler and text blocks, and scroll position get, set and a scrolled event (Mac T544)

Filed on helium from Mac T544 (~/coordination/helium-cards/T544.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

## Problem

The user (2026-10-12) asked for a diff view, side by side and inline, with a read-only left side and synced
scrolling. `EditorView` (`crates/fe-gpui/src/view.rs`) can't be made read-only, shows no blocks (fe-display's block
layer, T254, is laid out by `element.rs` with `BlockMap::default()` at :605), and has no public scroll position or
scroll notification (`scroll_to_top_row` :470 is `pub(crate)`). The vim-mode requirement (coordinator, same day):
the read-only side refuses edits with a vim-style message.

T255 (proposal diff blocks, inbox, waiting on T231) also wires blocks into `element.rs`; this card does the general
host-block wiring, and T255 should build on it (see the lead's note).

## Change

New file `crates/fe-gpui/src/view/host.rs` (an `impl EditorView` block declared from `view.rs`; `view.rs` gets only
fields and `mod` lines):

- Read-only: `set_read_only(bool)`, `is_read_only()`. While set, `EditorView`'s dispatch (`crates/fe-gpui/src/commands.rs`,
  where `mutates` is computed, ~:261) refuses every editor or vim command whose `mutates_buffer` is true, and IME and
  typed text (`crates/fe-gpui/src/view/ime.rs`) insert nothing. With vim on, a refused edit or entering insert mode
  (`i a o O c s R` and the like, which stay in normal mode) sets vim's message `E21: Cannot make changes,
  'modifiable' is off` (nvim's message for a nomodifiable buffer); vim motions, visual mode, yank, search and `:`
  commands that don't edit work as usual. With vim off a refused edit does nothing. Selection, copy, find and
  scrolling work.
- Host blocks: `set_blocks(Vec<HostBlock>)`, `HostBlock { above: u32 /* buffer row */, rows: u32, kind:
  HostBlockKind, id: u64 }`, `HostBlockKind::Filler` (blank rows with a faint diagonal hatch, VS Code's diff filler)
  or `HostBlockKind::Lines { lines: Vec<SharedString>, tint: fe_editor::decorations::Tint }` (text rows on a tinted
  band, tabs expanded, no syntax colours). Placed above the first display row of buffer row `above` (after the last
  row when `above` is the row count); a block whose row is folded away isn't shown.
- New file `crates/fe-gpui/src/element/host_blocks.rs` (declared from `element.rs`): `element.rs` lays out with
  `layout_frame_with_blocks` and paints `Frame::blocks` through it; y-to-row maths honour blocks (`FrameLayout::char_at`
  via `Frame::screen_row_at`, the `wrap_pending_at` top via `BlockMap::to_display_top`/`to_screen_top`), so clicks
  below a block land on the right row.
- Scrolling: `scroll_top() -> f32` in screen rows (blocks counted), `set_scroll_top(f32)`, and a new
  `EditorViewEvent::Scrolled { top: f32 }` (`impl EventEmitter<EditorViewEvent> for EditorView`) emitted when the top
  changes by any means except `set_scroll_top`, so two views can follow each other without echo.

## Tests

New file `crates/fe-gpui/src/view/diff_seams_tests.rs` (declared `#[cfg(test)]` from `view.rs`), in the style of
`crates/fe-gpui/src/view/fold_hover_tests.rs`.

## Implementation notes

- **Public path for the new types.** `HostBlock`, `HostBlockKind` and `EditorViewEvent` live in
  `view/host.rs`. The crate root (`crates/fe-gpui/src/lib.rs`) is outside this card's scope, and the
  workspace's `unreachable_pub` lint (clippy runs with `-D warnings`) needs a public path, so they're
  re-exported from `fe_gpui::commands` for now. The first card that touches `lib.rs` (T255 or the diff
  view itself) should add `pub use view::{EditorViewEvent, HostBlock, HostBlockKind};` at the root.
- **Where read-only gates vim.** In `EditorView::run_vim_once` (`vim.rs`, in scope from revision 3),
  which every vim command passes through however its keys came: typed (`feed_vim`'s commit), replayed by
  the keymap (a failed, timed-out or handed-over chord, through `vim::feed`), replayed by a macro (`@`,
  whose replay now stops at the first refused command, as nvim's does), or dispatched as an action. A
  refused key is taken with nothing pending and `E21` is set (`EditorView::refuse_vim`, in `host.rs`); a
  refused `vim.command_line_submit` cancels the line. Round 1's key-listener probe is gone.
  Refused (`EditorView::refuses_vim`): any vim command with `mutates_buffer` except `vim.normal_mode` /
  `vim.replace_normal_mode`; `vim.operate` and an operator `vim.jump` unless the operator is a yank; and
  a `vim.command_line_submit` that would edit. That last is decided from what the line would run, not by
  running it (round 1's dry run on a scratch model was unsound: no undo history or clipboard, and vim
  anchors from the real buffer): a search line by the operator it was opened with, which
  `EditorView::note_command_line` records as every `vim.command_line` passes `run_vim_once` (so a line
  opened before read-only was set is known too; fe-vim doesn't expose it); an ex line by its parsed
  command (`ex_edits`: `:d :j :m :t :> :< :sort :s` edit, `:g` as its command does); and `:normal` by
  the commands its keys commit when a fresh reducer reads them from normal mode (`keys_edit`): allowed
  only if every one is a motion, scroll, fold, mark, jumplist move or yank, which change neither the
  text nor the mode, so the reducer reads each later key as `:normal` would. Anything else (`u`,
  `<C-r>`, `"+p`, `v`, `/`, ...) refuses the line. fe-vim runs `:normal`'s keys inside its own
  registry, so no per-command gate in fe-gpui can see them; this allowlist is sound but may refuse a
  line that, run, wouldn't edit.
  A key that kiln matches afresh after replaying a failed chord ends vim's message as any key does, so
  after `x j` only the unchanged text shows the refusal (`read_only_keymap_replays`).
- **Other edit paths refused.** Editor commands with `mutates_buffer`, plus `completion.trigger` and
  `completion.accept`; the input handler (`takes_input` in `ime.rs`); and a middle-click paste (a
  guard in `view.rs`). With vim on, a refused input method's text or middle-click paste says `E21`.
  `set_read_only(true)` first ends an insert session with `vim.normal_mode`, or a replace session with
  `vim.replace_normal_mode`, while edits are still allowed (so a count's copies and a block insert's
  rows go in), then drops a composition and closes the completion menu.
- **Scrolled.** The frame emits `EditorViewEvent::Scrolled` when it settles a top (in screen rows,
  after its clamp) different from the last one it reported or the host set. It doesn't emit only when
  the frame starts exactly where `set_scroll_top` left it with no caret to reveal
  (`EditorView::host_top_holds`); a wheel, vim scroll or reveal in between is reported.
- **Blocks.** A `HostBlock`'s fe-display `Block::id` is its index in the view's list, so duplicate host
  ids don't matter; the host's `id` is echoed only in the test hooks. While wrapping is pending, a top
  inside a block keeps its offset above the display row below it. The filler hatch is stroked only over
  the part of the block in view, anchored to the block's corner so it scrolls with the block. A text
  block's lines in view extend the horizontal scroll range past fe-display's clamp (the frame is shifted,
  as for a composition), and a line longer than `MAX_SHAPED_ROW_BYTES` is shaped as a slice around the
  view, a view's width either side, cut between grapheme clusters and measured by cluster as the long
  rows are (`CharWidth::cluster_width`); the scroll bound takes a shaped line's shaped width. With soft
  wrap on the rows don't scroll, but the blocks' lines still do: their offset is kept in the view's
  horizontal scroll, which the wrapped layout ignores.
- **Carried findings.** T47 F3: an input method's explicit-range commit now types through the typing
  path, so a recording holds it. In replace mode the range is kept: as many of its graphemes as the text
  has chars are overwritten through fe-vim's replace path (so `<BS>` puts them back), the rest of the
  range is taken out first, or the extra chars go in first. Where typing wouldn't overwrite char for
  grapheme (a line break or tab in the range or the text, or a char that joins the one before it) the
  range is replaced as given, recorded as typed, and `<BS>` has nothing of it to put back. Elsewhere the
  range is selected and typed over. With vim on, an input method's empty commit over a range (a
  deletion) is refused in a read-only view with `E21`, as text is. T47 F6: composing in replace mode, the underline is at the input method's caret, one grapheme
  of the composition (or, past its end, of the buffer after it) wide.
  T47 F1 and F2 (`vim.rs`, touched by revision 3): text committed at once in insert or replace mode
  that holds a line break or a tab is typed as its keys are (`vim::type_as_keys`): `\r\n` and a lone
  `\r` are one line break, which goes in as `Enter` (`VimState::line_break`, copying the row's
  indentation), a tab as `Tab` (`editor.tab`), the rest as typed (overwriting in replace mode). A
  recording holds them as `<CR>` and `<Tab>`, `editor.tab` records as `<Tab>`, and a replay's `<Tab>`
  runs `editor.tab` in insert and replace mode, so a replay types what the commit did
  (`committed_breaks_and_tabs_replay_alike`).
  T47 F4 (a replayed `:w`, `:e` or `:q` is dropped) is left as it was: the host commands are run by the
  workbench's command line (fe-workbench), outside this card's scope; connecting a replay to them needs
  that crate and a way for the view to hand a host command over mid-replay.
  The carried findings' regressions are in `diff_seams_tests.rs`, as the scope has no other test file
  for them.
