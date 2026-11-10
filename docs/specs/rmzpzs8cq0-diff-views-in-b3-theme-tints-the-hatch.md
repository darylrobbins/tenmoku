---
task_uid: "01m725chdm8d59n1rmzpzs8cq0"
task: "T558"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in diff_b3 (cargo nextest run -p fe-gpui --test diff_b3) pass: a missing line is drawn as a hatch; changed words use diff_*_word over the row tint; a hunk header names its enclosing symbol and change count; proposal blocks use the theme's diff tokens (the hard-coded ADDED/REMOVED are gone)"
    required: true
    manual: false
  - id: "C2"
    text: "tests::commit_diff_b3 (cargo nextest run -p fe-workbench) passes for the commit diff's hunk headers"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist); cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui::diff_b3.the_glyph_column_is_current_after_an_edit_through_another_model passes: with texts over 64 KiB (diff in the background) and edits made only through another model of the right side's buffer, after each single draw with the diff still pending (DiffView::diff_pending), the right side's gutter annotation version equals its buffer's version; DiffView::show_glyph_column builds the annotation from model().buffer().snapshot(), not model().snapshot() (review F5)"
    required: true
    manual: false
---

# Diff views in B3: theme tints, the hatch where a line does not exist, hunks named by their symbol, proposal tints from the theme (Mac T1554)

# Diff views in B3: tints from the theme, hatch, symbol-named hunks

Spec: `docs/design/design-system.md#review-diff` (the diff part).

## Change

- Side-by-side diffs (`crates/fe-gpui/src/diff_view.rs`, `diff_view/inline.rs`,
  `diff_fold.rs`) and the commit diff (`crates/fe-workbench/src/commit_diff_view.rs`,
  `commit_diff.rs`, `commit_rows.rs`): row tints celadon-soft / shinsha-soft,
  stronger tints on the exact words that changed (theme `diff_*`), a clear
  diagonal hatch where a line does not exist (today a flat filler), collapsed
  unchanged runs (exist).
- Hunk headers named by the enclosing symbol from the outline:
  `impl MinimapTile › render · 4 changes` (a plural message), code small.
- Agent proposal blocks (`crates/fe-gpui/src/element/proposal.rs`) drop their
  hard-coded `ADDED` / `REMOVED` tints for the theme's diff tokens.
- Glyphs (+ / −) in the gutter of each side so colour is never the only cue.
- The review panel (task, viewed checkboxes, Accept all, Request changes,
  comments to the agent) is separate work.

## i18n

fe-gpui's text is already in its catalogue (card G1). Any text this card adds
or rewrites goes to `crates/fe-gpui/l10n/en-US/` as keys; the crate's lint
fails otherwise.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `commit_diff_view.rs`, `commit_diff.rs` and `commit_rows.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/diff.ftl` (keys `workbench-diff-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Review-cap respec (2026-11-10)

Review `vnpj2a` (candidate `fa9h5r`, round 3) left one finding open: F5 (high, `crates/fe-gpui/src/diff_fold.rs:1040`), not fixed in rounds 2 and 3. F6 is fixed; C1–C3 are met. This respec spells out F5's fix and adds C4 for its regression test. C1–C3 are unchanged.

### The bug
`DiffView::show_glyph_column` decides whether the glyph column is current by comparing `annotations.version` with `view.model().buffer().version()`, the shared buffer's live version. It then builds the new `GutterAnnotations` from `view.model().snapshot()`, the model's snapshot as of its last catch-up. Those two differ when the buffer was edited through another model of the same buffer (a split, or the file's own editor), because nothing in `DiffView::render` catches the side's model up before `show_glyph_column` runs:
- `keep_aligned` skips a side whose `folded` version isn't the buffer's (and with folding off it doesn't refold);
- `sync_diff` sees a large text (`Diffed::is_large`, over 64 KiB) and starts the diff with `background_spawn`, so `land` → `refold` → `sync_display` doesn't happen this frame.

So the column is rebuilt with the stale version and stale anchors, the next render sees `version != buffer().version()` again and rebuilds it stale again, and when the editor's prepaint catches the model up, `resolved_annotations` → `resolve` → `edited_ranges(snapshot, annotations.version)` replays every edit since that stale version. With many disjoint edits before the diff lands that is the quadratic work round 2's F5 was about. The round-2 test (`the_glyph_column_keeps_up_with_edits`) edits through the side's own model with a small, synchronous diff, so it never takes this path.

### Required fix (one place)
In `show_glyph_column`'s `view.update` closure (`diff_fold.rs` ~1040), build the annotation from the shared buffer's current snapshot, not the model's cached one:

```rust
let snapshot = view.model().buffer().snapshot(); // fe_buffer::Buffer::snapshot, an O(1) clone; the Ref drops here
let rows = snapshot.anchor_before(0)..snapshot.anchor_after(snapshot.len());
crate::GutterAnnotations { version: snapshot.version(), /* entries, hover, on_click as now */ }
```

The version, both anchors and the `current` check then all use the same buffer version. This is safe because `EditorView`'s prepaint calls `model.sync_display()` before `resolved_annotations()` (`element.rs` ~822/831). The model is never behind the annotation when it lays out, so `resolve` maps from that version with no edits to replay. If some other caller (hover) resolves while the model is still behind, `resolve` finds the version not mappable, caches an empty result for that older model version, and resolves again once the model catches up; nothing paints wrong. Don't sync the model from `show_glyph_column` instead (`model_mut().sync_display()`). It would fix the version too, but it runs the model's catch-up side effects (diagnostics, decoration providers, snippets) from the diff view's render, outside the editor's own frame.

Check the other `GutterAnnotations` the card builds or compares in `diff_fold.rs` / `diff_view.rs`. Any that takes `model().snapshot()` while comparing against `buffer().version()` gets the same change. There is no other in the candidate as far as review `vnpj2a` shows.

### Required regression test (C4)
Add to `crates/fe-gpui/tests/diff_b3.rs`:

`the_glyph_column_is_current_after_an_edit_through_another_model`:
1. Texts over 64 KiB so the diff runs in the background, with at least one hunk. For example left = 8000 rows `row {n:05}\n`, right = the same with one row changed.
2. Make the right side's model yourself (`EditorModel::new(Buffer::new(&right), Some("a.txt".into()))`) and keep `let mut other = right.split();` before handing `right` to `DiffSide::new` (add a `diff_view_of`-style helper that takes the right model, as `diff_fold_tests.rs::a_caret_moved_by_an_edit_elsewhere` does). Draw with `draw(cx)` until the first diff lands, and assert the right side has the glyph column at the buffer's version.
3. Loop at least 50 times. Edit through `other` only, at a row that differs each time: `other.buffer_mut().edit([(at..at, "x")], Origin::User)`. Then `diff.update(cx, |_, cx| cx.notify())` and exactly one `cx.update(|window, cx| window.draw(cx).clear(cx))`, with no `run_until_parked` and no `advance_clock` (both would run the background diff and land it).
   - Assert that the diff is still pending: add a `#[doc(hidden)] pub fn diff_pending(&self) -> bool { self.diffing.is_some() }` seam on `DiffView` (`diff_view.rs`, in scope).
   - Assert `right.read_with(cx, |view, _| view.gutter_annotations().map(|a| a.version)) == Some(view.model().buffer().version())`.

   On the candidate this fails on the first iteration (the column stays at the model's pre-edit version). With the fix it passes.
4. At the end, `draw(cx)` (parks; the diff lands) and assert `drawn_glyphs(1, cx)` is not empty.

Keep `the_glyph_column_keeps_up_with_edits`. Add a line to the Implementation notes on round 3: the column is built from the buffer's snapshot because the side's model can lag the shared buffer while a background diff is pending.

### Scope of this round
Only F5. Don't change anything F6 or C1–C3 already cover. Finish once.

## Implementation notes

- **What was there.** fe-gpui's diff view already tinted rows and changed
  words with the theme's `diff_*` tokens and hatched fillers in
  `diff_filler` (`element/host_blocks.rs`); unchanged runs already fold.
  This card adds the hunk headers, the glyphs and the proposal tints, and
  brings the commit diff level with it.
- **Hunk headers (DiffView).** A one-row block above each hunk's first
  row on both sides (inline: above the removed rows), ids from `1 << 33`,
  placed before the side's other blocks so a header sits above a filler at
  the same row; the sides stay lined up. It's a `Lines` block of no lines,
  not a reserve block: the editor places every reserve block each frame,
  in view or not, with a linear lookup each (review round 1, F2), while a
  band is laid out only in view. The label layer finds the headers in the
  frame's laid-out blocks and draws them, code small (12/13 of the editor
  size), muted, on the filler colour. The text is `DiffView::hunk_header`:
  the symbols around the hunk's first row on a side it has rows on, the
  right's, else for a modified hunk the left's (`diff_fold::crumb`, now
  public); a side with no rows in the hunk isn't asked, so a comment added
  just above a function isn't put in it (F3). Then its count of rows
  removed plus rows added, as GitHub counts "changes"; the count alone in
  no symbol. Outlines are now found whenever there are hunks, not only when
  blocks are hidden. Resolved each frame, for the headers in view.
- **Glyphs.** `−` on the left's rows in a hunk, `+` on the right's, inline
  `−` on each row of a removed block in the frame (only those: a large
  block is mostly out of it, F1), in the change colours. They have a
  column of their own: each side shown gets a gutter annotation column
  while there are hunks, one blank entry over the whole text, which makes
  room left of the gutter's 2 em slot where diagnostics and lightbulbs go
  (F4). The entry is made again as of each buffer version the side is
  drawn at, so the editor never replays old edits to place it (round 2,
  F5). A host's own column (blame) is left be and takes the place, no
  glyphs drawn over it (F6); they come back when it goes. `DiffView::drawn_glyphs`, `drawn_tints`
  and `drawn_lightbulb` are `#[doc(hidden)]` seams for `tests/diff_b3.rs`,
  as is `EditorView::drawn_proposal_tints`.
- **Proposals.** Block `diff_added_row`, old-row band `diff_removed_row`,
  Accept `diff_added_word`, Reject `diff_removed_word`, chosen at layout
  from the view's theme and kept on the layout for paint.
- **Commit diff.** A `ViewRow::Hunk` header row above each hunk's pairs,
  full width; navigation stops on it. Symbols come from
  `Diffed::symbols`, found on the background read: the file's language by
  its path and first 512 bytes, each side parsed within 500 ms only if a
  hunk needs it; a side with no rows in a hunk isn't asked, as above. A changed cell has its `−` / `+` after the line number
  (1.5 em, inside the gutter width the sideways scroll counts).
- **i18n.** fe-gpui's new keys are in `l10n/en-US/diff.ftl`, added to
  `resources!`. Every string of `commit_diff_view.rs`, `commit_diff.rs` and
  `commit_rows.rs` is in fe-workbench's `diff.ftl`; en-US is byte-equal,
  including the window title's `— fastedit` (passed as a `verbatim!`
  argument). `commit_rows::ago` and `texts` keep returning `String`s, built
  each render by the History and Git Log panels. Debug selectors are
  listed in `l10n-allow.toml`. The diff view's navigation message (carried
  medium F2 of T549 for `diff_view.rs`) is now kept as what was said and
  resolved each frame; `DiffView::message` keeps its signature.
- **Round 3 (F5).** The glyph column's annotation is built from the shared
  buffer's snapshot (`model().buffer().snapshot()`), not the side's
  model's: the model can lag the buffer after an edit through another
  model while a background diff is pending, and the column then took the
  model's old version and the editor replayed every edit since it.
  `DiffView::diff_pending` is a `#[doc(hidden)]` seam for C4's test. The
  test reads the column inside the same `cx.update` as its one draw: the
  test platform draws again as an update ends when the draw asked for it,
  and that second draw (with the model caught up by the first's prepaint)
  hid the stale column, so read after the update the test passed without
  the fix.
