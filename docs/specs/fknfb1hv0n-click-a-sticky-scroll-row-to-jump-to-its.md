---
task_uid: "01m5f7fyyvm7ppdefknfb1hv0n"
task: "T1030"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Clicking the second pinned row puts the caret at its header's first non-blank with no selection, and the next frame shows that header row directly under a one-row band of its parent (sticky_scroll_click_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "The click pushes navigation history (Back returns), with vim on '' returns to the row before it, and in visual mode it leaves the visual selection"
    required: true
    manual: false
  - id: "C3"
    text: "Hovering a sticky row paints sticky_scroll_hover, and no hover popover or click reaches the text under the band"
    required: true
    manual: false
  - id: "C4"
    text: "Moving the caret up (k and arrow-up, vim on and off) into rows the band would cover scrolls so the caret's row sits below the band (sticky_scroll_caret_tests.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "After vim zt on a row inside a nested fn, the view's top row is that row, as in nvim, and the band shows only the scopes above it; fe-vim's oracle tests pass unedited"
    required: true
    manual: false
---

# Click a sticky scroll row to jump to its header, hover it, and keep the caret out from under the band: autoscroll counts it, vim scrolls shrink it

# Sticky scroll 3/4: click a sticky row to reveal it, hover, and never hide the caret

## Click and hover

- Hovering a sticky row paints its background in `sticky_scroll_hover` (card 2's key) with a
  pointing-hand cursor; the band takes the pointer, so hover popovers, fold-column hover and text
  clicks under it don't fire.
- Clicking a sticky row (any button-1 click, plain or with vim on) puts the caret at the header
  row's first non-blank, with no selection, and scrolls so the header row sits directly under the
  band its own enclosing scopes then form (VS Code: `top = header − ancestors pinned`), so the
  clicked row is visible as itself, not pinned. It is a jump: it pushes the navigation history and,
  with vim on, vim's jump list (`''` returns), as go-to-definition does. In vim visual mode it ends
  the visual selection, as a click in the text does.
- Hit areas are inserted in prepaint next to the lens hitboxes; handlers in
  `crates/fe-gpui/src/element/sticky_scroll.rs`, the reveal in `crates/fe-gpui/src/view.rs`.

## The caret is never under the band (decision)

1. **Autoscroll counts the band.** When the frame autoscrolls (a caret move, an edit, a search or
   go-to reveal, vim motions such as `k`, `gg`, `{`, `n`), the top margin is the band's height for
   the scroll it ends at plus one row, not just fe-display's 3-row margin: after laying the frame
   out, if the caret's row top is above the band's bottom, `prepaint` re-settles once with the
   scroll moved up by the overlap (one extra layout at most).
2. **Explicit vim scrolls keep nvim's viewport.** `zt`, `z<CR>`, `<C-e>`, `<C-y>`, `<C-d>`,
   `<C-u>`, `<C-f>`, `<C-b>` and `H` keep their top rows exactly as nvim with 'scrolloff' 0 (the
   fe-vim oracle tests stay unchanged and `EditorHost::viewport` doesn't shrink). There the band
   shrinks instead: card 1's `keep_above` drops pinned rows at or below the caret's screen row,
   so after `zt` the caret's row shows as itself with only the rows above it pinned.
3. The mouse wheel and scrollbar never move the caret; the band may cover the caret's row then,
   as in VS Code, until the next caret move autoscrolls.

## Review round fixes (lead respec, 2026-10-23)

The findings_override timebox passed with no decision; fix both findings, no deferral:

- **F10** (view.rs): `record_vim_jump` now runs before `reveal_top`, so when an insert-session cleanup
  edits the buffer, syntax is still at the previous version, `sticky_scroll::scopes` finds no ancestors
  and the header is revealed at the viewport top; the next render restores the band over it. Compute
  the reveal once syntax and layout match the cleaned-up text, keeping the header anchor across any
  deferred frame. Extend the autoindent regression to assert the parent band and the header directly
  below it.
- **F11** (view.rs): the resolved anchor is treated as a row start, but `anchor_after` moves past text
  inserted at that position (e.g. a vim block insert of `pub ` across nested headers, ended by clicking
  a later pinned header before Escape). Resolve the anchor to its current buffer row, then take that
  row's real start before slicing and finding the first non-blank. Add a regression where finishing a
  block insert adds a non-blank prefix at the clicked header's start.
