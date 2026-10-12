---
task_uid: "01m4rqehgz63kx81t6fqjnj900"
task: "T49"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::proposal_tests::blocks_under_hunks: with a from_text proposal holding a 1-to-2-line replacement and a 1-line deletion, the frame paints two Pending blocks of 3 and 1 rows below their hunks' old rows, and the text row after the first hunk sits 3 line heights lower than without the proposal"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::proposal_tests::edited_hunk_shows_stale: typing inside a hunk's old text makes its block paint as Stale on the next frame, with the dry run Conflict's Display text as its header"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::proposal_tests::folded_hunks_hide: a hunk whose old rows are folded away paints no block, and unfolding paints it again"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::proposal_tests::clicks_land_on_text_rows: clicking a text row below a block puts the caret on that row, and clicking inside a block's rows puts it at the end of the display row above the block"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test view::proposal_tests::proposals_per_buffer: set_proposals drops a proposal for another buffer without panicking, and two split views of one buffer given the same Rc proposal both paint its blocks"
    required: true
    manual: false
---

# Show a ProposedChange's hunks in the editor as diff blocks: the new text in rows under the text it replaces, removed rows banded, stale hunks with their reason (Mac T255)

Filed on helium from Mac T255 (~/coordination/helium-cards/T255.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

## Problem

Part 1 of the approved plan's T240 follow-up (2026-10-08): "proposal diff blocks — showing a ProposedChange's
hunks inline in the editor as diff blocks the user can accept or reject per hunk". `ProposedChange`
(`crates/fe-buffer/src/proposed.rs`, T240) and its dry run (the dry-run card this depends on) are GPUI-free;
the block layer (`fe_display::BlockMap`, `layout_frame_with_blocks`, `Frame::blocks`) is the other dependency.
Nothing in fe-gpui shows a proposal. This card shows them, read-only; accepting and rejecting is the next card.

The host app (the AI-first dev tool; see T242, `docs/decisions/app-shell.md`) will drive proposals, so the API is
on `EditorView`, not in fe-workbench.

## Change

- Host API, in the new file `crates/fe-gpui/src/view/proposal.rs` (an `impl EditorView` block, declared from
  `view.rs`): `EditorView::set_proposals(&mut self, proposals: Vec<Rc<RefCell<ProposedChange>>>, cx)` replaces the
  proposals shown and notifies; `proposals(&self) -> &[Rc<RefCell<ProposedChange>>]`. A proposal whose
  `ProposedChange::buffer()` isn't this view's buffer is dropped with a `log::warn!` (never a panic). Proposals are
  shared `Rc<RefCell<…>>` so several views of one buffer (splits) show the same one. `view.rs` gets only the field,
  the `mod` lines and the calls below.
- Each frame, for every hunk that is `Pending` or `Stale` in `ProposedChange::hunks(model.snapshot())`, one block
  (in the new file `crates/fe-gpui/src/element/proposal.rs`, declared from `element.rs`):
  - placed above the display row that follows the hunk's old range: the row of `range.end` when `range.end`
    starts a buffer row (line hunks from `from_text` end there), else the first display row of the next buffer
    row; after the last row at the end of the text;
  - `1 + lines(new_text)` rows: a header row, then the new text one row per line, unwrapped, tabs expanded
    (`ExpandedRow`), in the editor's foreground with no syntax colours, on an "added" background;
  - the header reads `Proposed change` for a pending hunk; for a stale one, the reason from
    `ProposedChange::dry_run`'s `Conflict` (`Display`), dimmed;
  - the hunk's old rows (non-empty old range) get a full-width "removed" background, like the active-line bands.
  - A hunk whose old range is inside folded rows shows no block and no band.
  - Colours are constants in `element/proposal.rs` until the theme files gain diff colours; no `EditorTheme` change.
- `element.rs` lays out with `layout_frame_with_blocks` and paints `Frame::blocks`. Its own y-to-row maths
  honours blocks: `FrameLayout::char_at` (element.rs:82) uses `Frame::screen_row_at`, and the
  `wrap_pending_at` top (element.rs:441) converts with `BlockMap::to_display_top`/`to_screen_top`.
- Record the painted blocks for tests, like `painted_chevrons`: `#[cfg(test)] painted_proposals:
  Vec<(HunkId, HunkStatus, String /* header text */, Bounds<Pixels>)>` on the view.

Not in this card: accept/reject buttons, events, keyboard commands, more than plain text in blocks.

## Tests

New file `crates/fe-gpui/src/view/proposal_tests.rs` (declared `#[cfg(test)]` from `view.rs`), in the style of
`crates/fe-gpui/src/view/fold_hover_tests.rs`.

## Implementation notes

- **Built on T48's host blocks (scope expansion: `crates/fe-gpui/src/element/host_blocks.rs`).** T48 (Mac
  T544) already lays out with `layout_frame_with_blocks`, paints `Frame::blocks` and does the block-aware y maths
  (`row_at` via `Frame::screen_row_at`, the `wrap_pending_at` top via `to_display_top`/`to_screen_top`), and a
  click in a block already goes to the end of the display row above it (fe-display's `block_offsets`). The two
  kinds share one `BlockMap`, built in `element.rs` (`block_map`): host blocks keep their index as id, a hunk's
  block is `1 << 32` plus its index in the frame's hunks. `host_blocks.rs` changes only as that needs:
  `block_map` became `blocks` (an iterator `element.rs` chains with the hunks'), its layout skips block ids that
  aren't the host's (it painted an unknown id as a filler), and `slice` and `clusters` are `pub(super)` so a long
  line of new text is shaped and measured as a host line is.
- **Placement.** A hunk's block goes above one past the display row holding the end of its old text's last row
  (`display.row_for_offset`): the row `range.end` starts when it starts one, else the next buffer row's first,
  the row count at the end. When that last row heads a fold, the block follows the fold. A hunk is hidden (no
  block, no band) when a fold hides all its old rows (an insertion: the row it's at).
- **Bands** are full-width rects (as the active lines) behind every laid-out display row of the old rows, painted
  just after the active-line bands, so under the text and selection. Kept on `FrameLayout::proposals.removed`.
- **Stale reasons.** `ProposedChange::dry_run` is run only for a proposal that has a stale hunk; the header is
  the `Conflict`'s `Display`. A proposal whose anchors are newer than the view's snapshot is skipped for that
  frame (so `hunks` can't panic); after `sync_display` that can't normally happen.
- **Per frame cost.** Only rows of a block in view are tab-expanded and shaped; the new text is held as the
  hunk's `Arc<str>`. Lines of new text wider than the rows extend the horizontal scroll as host lines do.
- **Test hooks.** `painted_proposals` as specified; also `FrameLayout::proposals.removed` (bands) and
  `proposals.texts()` (each block's rows in view as shaped), read from `view.layout` in the tests.
