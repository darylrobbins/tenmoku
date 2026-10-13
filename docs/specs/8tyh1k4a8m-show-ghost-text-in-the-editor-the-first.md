---
task_uid: "01m4rqejd62nepse8tyh1k4a8m"
task: "T52"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::ghost_text_tests::inline_after_the_caret: \"bar()\" at the caret in \"foo|;\" paints inline after the caret in gutter_foreground with \";\" moved right by its width, and the buffer text is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::ghost_text_tests::extra_lines_as_rows_below: a 3-line suggestion paints its first line inline and 2 block rows under the caret's row, and a click on the text row below them puts the caret on that row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::ghost_text_tests::typing_consumes_and_stale_drops: typing the next char repaints the suggestion one char shorter; a mismatched char, a click elsewhere or vim normal mode leaves ghost_text() None and paints nothing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::ghost_text_tests::other_buffer_dropped: set_ghost_text with a suggestion for another buffer shows nothing and doesn't panic"
    required: true
    manual: false
---

# Show ghost text in the editor: the first line dimmed inline after the caret, further lines as rows below it, set by the host and dropped once stale (Mac T390)

Filed on helium from Mac T390 (~/coordination/helium-cards/T390.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

## Problem

The ghost-text model (`fe_editor::ghost_text::GhostText`, the model card this depends on) and fe-display's
inlay (`layout_frame_with_inlay`, the inlay card) are GPUI-free; nothing shows a suggestion. The user's ask
(2026-10-11): "inline AI suggestions rendered as dimmed virtual text at/after the caret (single- and
multi-line), not part of the buffer". The host app drives suggestions, as it drives proposals (T255's
`set_proposals`), so the API is on `EditorView`. Lines after the first need rows of their own under the caret's
row: T255 brings the block layer (`BlockMap`, `layout_frame_with_blocks`, block-aware `FrameLayout::char_at`)
into `crates/fe-gpui/src/element.rs`; this card adds its block to the same `BlockMap` rather than plumbing blocks
a second time.

## Change

- New file `crates/fe-gpui/src/view/ghost_text.rs` (an `impl EditorView` block, declared from `view.rs`):
  `set_ghost_text(&mut self, ghost: Option<GhostText>, cx)` replaces the suggestion shown and notifies; a
  suggestion for another buffer is dropped with `log::warn!`. `ghost_text(&self) -> Option<&GhostText>`.
  `pub(crate) fn drop_stale_ghost_text(&mut self)` drops it when `!self.accepts_text()` (vim normal or visual
  mode) or `!ghost.is_live(&self.model)`; it runs where `drop_stale_completion` runs before a frame. Each view
  holds its own (it follows that view's caret). `view.rs` gets only the field, the `mod` lines and that call.
- Each frame with a live suggestion, in the new file `crates/fe-gpui/src/element/ghost_text.rs` (declared from
  `element.rs`), from `ghost.remaining(&model)`:
  - its first line is an `Inlay` at the caret, styled `theme.gutter_foreground`, laid out with
    `layout_frame_with_inlay`; the caret paints before it;
  - its other lines are one block of that many rows, placed above the display row that follows the caret's
    buffer row (after the last row at the end of the text), painted one row per line, unwrapped, tabs expanded
    (`ExpandedRow`), in `theme.gutter_foreground`, from the text area's left edge;
  - colours come from `EditorTheme` as is; no theme change.
- Record what's painted for tests, like `painted_chevrons`: `#[cfg(test)] painted_ghost: Option<(String /*
  inline */, Vec<String> /* block rows */, Bounds<Pixels> /* inline */)>` on the view.

Not in this card: keys, commands, escape and blur, events to the host (the keys card that follows).

## Tests

New file `crates/fe-gpui/src/view/ghost_text_tests.rs` (declared `#[cfg(test)]` from `view.rs`), in the style of
`crates/fe-gpui/src/view/fold_hover_tests.rs`.

## IME (from T429)

T429 composes IME text in the same single inlay. While composing, the composition owns the inlay: ghost text is hidden until the composition commits or is cancelled.

## Implementation notes

- `GhostText` has no public buffer id, and fe-editor is outside this card's scope, so `set_ghost_text`
  tells "another buffer's" by liveness: it catches the model up with the buffer, then drops (with
  `log::warn!`) any suggestion that doesn't show at the caret now. That covers another buffer's, and
  also one already over by the time the host sets it.
- The block's id is `u64::MAX`, past the host's (their index) and the proposals' (`1 << 32` plus index).
  Block rows scroll horizontally with the host's blocks, and are cut to `MAX_SHAPED_ROW_BYTES`.
- The first line's tabs expand from the caret's column; the other lines' from column 0.
- The composition underline is now drawn only for a composition, not for any inlay.
- Extra test `composition_hides_it` covers the IME note: a composition takes the inlay and hides the
  suggestion, which shows again once the composition is cancelled.
- Not handled: in vim replace mode the underline caret at the caret's grapheme spans the inline ghost
  text too, as `block_rect` measures to the next grapheme past the inlay.
