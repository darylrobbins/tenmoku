---
task_uid: "01m4sjp3jjfsg8a7qxxcstwp9e"
task: "T87"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::ghost_text_tests::wide_rows_scroll_into_view: on a short buffer the view scrolls right far enough to show the end of a ghost row wider than the view, and a ghost row longer than MAX_SHAPED_ROW_BYTES scrolled to its end paints its last chars (F2)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::ghost_text_tests::replace_caret_ignores_the_inlay: in vim replace mode at foo|; the caret block is one ; wide before and after bar() is suggested (F3)"
    required: true
    manual: false
  - id: "C3"
    text: "view::ghost_text_tests and view::ghost_text_key_tests pass unchanged; cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Let wide ghost text rows scroll into view and keep the vim replace caret one grapheme wide beside ghost text (T52 review F2, F3) (Mac T390 follow-up)

Follow-up from T52 (Mac T390, ghost text display, landed as 5c86e48), filed on helium. Review 01m4saxgdy9yj6nbgtn71gd23x
(`rgt review show T52`), findings F2 and F3. (F1, a stale suggestion surviving a peer edit for a frame, was fixed by
T53/f7aae89: `drop_stale_ghost_text` now syncs the model first. It is covered by
`view::ghost_text_key_tests::shared_buffer_edit_drops_it_in_one_frame`.)

## Problem

- **F2 (`crates/fe-gpui/src/element.rs` :782, `crates/fe-gpui/src/element/ghost_text.rs` :141).** Ghost block rows
  are drawn at `host_x`, but they don't count toward how far the view may scroll right. The `widest`
  calculation (element.rs, about :743) takes only `host_blocks::widest` and `proposal::widest`. On a short buffer, a
  suggestion row wider than the view can't be scrolled into view. Also, `ghost_text::layout` cuts each row to
  `MAX_SHAPED_ROW_BYTES` from its start (`&text[..floor_char_boundary(text, MAX_SHAPED_ROW_BYTES)]`), so the end of
  a longer row never shows, however far the view scrolls.
- **F3 (`crates/fe-gpui/src/element/ghost_text.rs` :73).** In vim's replace mode, the suggestion's inlay sits after
  the caret. `block_rect` (element.rs, about :1588) measures the caret's width up to the next grapheme's x, and that
  x includes the inlay's width. So for `foo|;` with `bar()` suggested, the underline caret covers `bar()` and the
  `;`, not just the `;`.

## Change

- **Scrolling (F2).** Add `ghost_text::widest(frame, ghost, view, shaper, glyph_widths)`. It returns the widest
  block row's width the way `host_blocks::widest` does, and element.rs includes it in `widest` along with the host
  and proposal blocks. For a row longer than `MAX_SHAPED_ROW_BYTES`, `ghost_text::layout` shapes the slice around
  the scrolled view, the same way `host_blocks::slice` does, and positions it at that slice's x.
- **Replace caret (F3).** When the frame has the ghost inlay at the caret's offset, `block_rect` measures the
  caret's width without the inlay: the next grapheme's x minus the inlay's width. So the caret covers only the
  buffer grapheme after it. The composition caret (`composition_block`) is unchanged.

## Tests

Add the tests to the existing `crates/fe-gpui/src/view/ghost_text_tests.rs` (already declared from view.rs at base),
which already paints ghost text in a headless window, leaving its existing tests unchanged. Not a new file:
Regatta's regression check applies only test files to base, so a test module declared in element/ghost_text.rs
can't run there.

- `wide_rows_scroll_into_view`: on a short buffer, a suggestion whose second line is wider than the view lets the
  view scroll right far enough to show that line's end. A second line longer than `MAX_SHAPED_ROW_BYTES`, scrolled
  to its end, paints its last chars.
- `replace_caret_ignores_the_inlay`: with vim in replace mode at `foo|;`, the caret block has the same width before
  and after `bar()` is suggested (one `;` wide).

## Implementation notes

- `ghost_text::in_view` yields the block rows in view, shared by `widest` and `layout`; both measure or slice a row
  longer than `MAX_SHAPED_ROW_BYTES` with `host_blocks::clusters` and `host_blocks::slice`, as the host's blocks do.
- `ghost_text::Placed` keeps the suggestion's caret offset (`inlay_offset`). `block_rect` subtracts the row's
  `inlay_width` from the next grapheme's x when that offset is the head. While composing there's no ghost text, so
  `composition_block`'s use of `block_rect` still includes the composition's inlay.
