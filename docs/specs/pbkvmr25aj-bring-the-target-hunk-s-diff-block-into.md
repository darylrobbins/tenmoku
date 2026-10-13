---
task_uid: "01m4sjg65qym5gt0pbkvmr25aj"
task: "T77"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::proposal_command_tests::next_hunk_reveals_a_tall_replacement_block: in a view shorter than a replacement hunk's old rows, proposal.next_hunk and proposal.previous_hunk leave one caret at the hunk's start and the next frame paints its block's header with Accept and Reject inside the text area (F1)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::proposal_command_tests::navigation_reveals_an_insertion_block: next and previous hunk to a pure insertion taller than the view leave its header and buttons in view (F1)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::proposal_command_tests::hunk_jump_splits_an_insert_session: vim i, type a, alt-f5, type b, escape: u undoes only b, a second u undoes a, and . repeats only b (F2)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::proposal_command_tests::hunk_jump_splits_a_replace_session: vim R, type zz, host EditorView::next_hunk, type y: backspace restores only what y replaced, and u undoes the post-jump change and then zz as separate steps (F2)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy -p fe-gpui -p fe-display --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Bring the target hunk's diff block into view on next/previous hunk, and split vim's insert or replace session at the jump (T51 review F1, F2) (Mac T259 follow-up)

Follow-up to T51 (Mac T259, proposal commands, d66ce63): review 01m4s9myb49dqv0kw9vb9x01c9 findings F1 and F2,
both still open on main (a61649c). Same stream as T49/T50/T51 (the proposal chain in crates/fe-gpui/src/view).

## Problem

`EditorView::step_hunk` (`crates/fe-gpui/src/view/proposal_commands.rs:180`), behind `proposal.next_hunk`,
`proposal.previous_hunk` and the host's `EditorView::next_hunk`/`previous_hunk`, has two faults.

**F1 (medium, proposal_commands.rs:199): the block isn't brought into view.** It puts the caret at the hunk's
`range.start` and calls `changed`, so layout autoscrolls only the primary caret (`FrameInput::autoscroll`,
`crates/fe-gpui/src/element.rs` ~line 638). A hunk's block sits *after* its old range (`element/proposal.rs`
`above`). For a replacement whose old rows span more than the viewport, the jump leaves the block, its header
and its Accept/Reject buttons offscreen, contrary to T51's spec ("autoscroll so its block is in view").

**F2 (medium, proposal_commands.rs:198): the jump doesn't split vim's insert/replace session.** `step_hunk` calls
`cancel_vim` (which returns early in Insert and Replace, `crates/fe-gpui/src/vim.rs:603`) then
`self.model.set_cursor(offset)` directly. Proposal ops are dispatched before the normal command path that splits a
session (`crates/fe-gpui/src/commands.rs` ~303, ~441), so nothing splits it. Text typed before and after the jump
joins one undo transaction, and the session keeps its old insertion regions and replace history, so `.` and
Backspace in replace mode act on the wrong text.

## Change

All in the files in scope; no change to `crates/fe-gpui/src/commands.rs`.

- F2, in `step_hunk`: after `cancel_vim` and `self.composition = None`, do the unfold and the caret move through
  the view's existing `split_session` (`crates/fe-gpui/src/view.rs:843`) with `fe_vim::Splits::Moves`, as a mouse
  click does (`view.rs:619`): `self.split_session(fe_vim::Splits::Moves, |model| { model.unfold_rows(..);
  model.set_cursor(offset) }, cx)`. The host methods go through `step_hunk`, so they split too.
- F1, a pending reveal the next frame honours:
  - `view.rs`: a field `pub(crate) reveal_hunk: Option<HunkId>` on `EditorView`, `None` in `new`.
  - `step_hunk` sets `self.reveal_hunk = Some(target.hunk.id)` (and still calls `changed`, so the caret
    autoscroll runs first).
  - `crates/fe-display/src/block_map.rs`: `pub fn screen_rows(&self, id: u64) -> Option<Range<u32>>`, the screen
    rows of the block with `id` (`starts[ix]..starts[ix] + rows`), `None` if no block has it.
  - `crates/fe-gpui/src/element/proposal.rs`: `pub(super) fn reveal(hunks: &[Hunk], id: HunkId, blocks:
    &BlockMap, scroll_top_rows: f32, visible_rows: f32) -> Option<f32>`: the block of hunk `id` (`IDS + its index
    in hunks`); if its screen rows are all inside the view, `None`; else the new top in screen rows that scrolls
    the least to show it whole, or, for a block taller than the view, puts its header row at the top.
  - `crates/fe-gpui/src/element.rs`: after `let mut frame = lay_out(&input);`, if the view has a `reveal_hunk`
    and `proposal::reveal` returns a top, lay out again with `scroll.y` at that top and `autoscroll: false` (as
    the composition case lays out a second time). Clear `reveal_hunk` in the update that sets
    `view.autoscroll = false`, whether or not it scrolled. The caret stays at the hunk's start even when that
    leaves it out of view.

## Tests

Add the four tests to the existing `crates/fe-gpui/src/view/proposal_command_tests.rs` (already declared from
`view.rs` at base), using its `editor`, `keys` and `run` helpers. Not a new file: Regatta's regression check applies
only test files to base, so a test module declared in `view.rs` can't run there (rev 1's new
`proposal_navigation_tests.rs` failed the check with "no tests to run"). For the viewport cases use a view in a
fixed-height wrapper (about 200px tall, as `proposal_accept_tests::Narrow` does) so a hunk can span more rows than
it shows.

- `next_hunk_reveals_a_tall_replacement_block`: a proposal replacing about 40 old rows (more than the view
  shows) with a few new lines, caret above it; `proposal.next_hunk` leaves one caret at the hunk's start and the
  next frame paints its block's header with its Accept and Reject buttons inside the text area
  (`painted_proposals` / `proposal_buttons`). From below it, `proposal.previous_hunk` does the same.
- `navigation_reveals_an_insertion_block`: a pure insertion of more lines than the view shows; next and previous
  hunk each leave the block's header and buttons in view.
- `hunk_jump_splits_an_insert_session`: vim on, `i`, type `a`, `alt-f5`, type `b`, `escape`: `u` undoes only `b`,
  a second `u` undoes `a`; after redoing both, `.` elsewhere inserts `b`, not `ab`.
- `hunk_jump_splits_a_replace_session`: vim on, `R` over row 0, type `zz`, then the host's
  `EditorView::next_hunk`, type `y`, `backspace` restores only the char `y` replaced (not row 0's), `escape`: `u`
  undoes only the post-jump change, a second `u` undoes `zz`.

Not in scope: changing which hunk next/previous picks, the keymap, or `commands.rs`'s dispatch.

## Implementation notes

- The reveal lays the frame out a second time only when the target's block isn't wholly in view after the caret's
  autoscroll; a block wholly in view leaves the caret's scroll as it is. The second layout keeps the first's
  horizontal scroll.
- Carried finding T52 F2 (ghost rows' horizontal extent, `element.rs:782` and `element/ghost_text.rs:141`) is not
  addressed here: its fix needs a width measure and a viewport slice in `element/ghost_text.rs`, and its tests in
  the ghost-text test files, all outside this task's scope. It wants its own card.
