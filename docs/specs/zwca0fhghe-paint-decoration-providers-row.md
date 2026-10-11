---
task_uid: "01m4p2a0nrsyp0zgzwca0fhghe"
task: "T11"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "decorations_tests: RowBackground and Gutter(Bar) give a rect on every display row of each covered buffer row (wrapped pieces included); Gutter(Deletion) sits on the first covered row's top edge; folded or scrolled-off rows give nothing"
    required: true
    manual: false
  - id: "C2"
    text: "Highlight and Underline rects match the range's x extent per display row (an empty range is a space wide); row backgrounds and highlights paint before search matches and the selection, underlines after the text"
    required: true
    manual: false
  - id: "C3"
    text: "AfterLine text sits one em past the end of the last display row of its last covered row, in its tint at alpha 0.6, several on a row one em apart, and none when that row isn't in the frame"
    required: true
    manual: false
  - id: "C4"
    text: "Each Tint resolves through tint_color from EditorTheme as specified; the model is queried once per frame with the frame's shown range; an editor with no providers paints nothing new and cargo test -p fe-gpui passes with the screenshot goldens unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "git diff --numstat against the base shows at most 30 added lines in crates/fe-gpui/src/element.rs"
    required: true
    manual: false
---

# Paint decoration providers' row backgrounds, gutter marks, highlights, underlines and after-line text in the editor (Mac T387)

Filed on helium from Mac T387 (~/coordination/helium-cards/T387.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The previous card (the M8 decoration-provider model, crates/fe-editor/src/decorations.rs) lets a host register
`DecorationProvider`s on an `EditorModel` and read `EditorModel::decorations(range)`, but nothing paints them.
The user asked on 2026-10-11 to "go ahead with the M8 drafters"; the plan's M8 row lists "decoration
providers" among the AI seams. Its first users are the git changed-rows highlighter (T374/T375) and AI
features (rows an AI edit touched, notes after a line, a streaming edit's extent).

Painting happens in crates/fe-gpui/src/element.rs, which is hot: T198, T231, T255, T258, T320, T354, T363,
T368, T370 and T375 all have it in scope. Diagnostics show the shapes to reuse: `diagnostic_marks`
(element.rs:978) finds a range's x extent on each display row with `RowLayout::x_for_offset` and paints
wavy underlines and a gutter dot; `match_rects` (element.rs:1270) does the same for backgrounds.

## Change

Put the work in a new `crates/fe-gpui/src/element/decorations.rs`. element.rs only declares the module
(`mod decorations;`), adds a `FrameLayout` field for what it laid out, calls it once in `prepaint` after
`match_rects`, and paints its quads at the points below: at most 30 added lines.

**Layout** (`prepaint`): one `view.model.decorations(first.start..last.end)` call over the frame's rows (the
shown range, as `diagnostic_marks` takes it; nothing when there are no rows). For each decoration, on the
frame's display rows only (folded and scrolled-off rows give nothing; every soft-wrapped piece counts):

- `RowBackground`: the text area's width (`Frame::text_bounds`), full row height, on each display row of
  each buffer row it covers.
- `Gutter(Bar)`: 2 px wide (rounded to device pixels), full row height, at the fold column's left edge
  (`Frame::fold_column`), on each display row of each covered row. `Gutter(Deletion)`: 6 px wide, 2 px tall,
  at the same x, centred on the top edge of the first covered row (the bottom edge of the last row when the
  range is at the text's end). Neither covers the diagnostics' dot at the gutter's left.
- `Highlight`: the range's x extent on each display row, full row height, as `match_rects`; an empty range
  is a space wide on the row it is on.
- `Underline { wavy }`: the same extents, 1 px just below the baseline as `diagnostic_marks` places them,
  painted with `paint_underline` (`wavy` as asked).
- `AfterLine(text)`: on the last display row of the range's last covered buffer row, one em right of that
  row's end, shaped with the editor font in the tint's colour at alpha 0.6. Several on one row go left to
  right in paint order, one em apart. Clipped to the text bounds; not hit-tested; doesn't change wrap or the
  horizontal scroll extent. None when that display row isn't in the frame.

**Paint order** (`paint`), each kind in `decorations()`'s order (later on top): row backgrounds then
highlights just after the active-line quads, so under search matches and the selection; underlines with the
diagnostics' squiggles, after the text; after-line text after the text; gutter marks with the diagnostics'
gutter marks.

**Colours**: `fn tint_color(theme: &EditorTheme, tint: Tint) -> Hsla` in this module, until the theme files
gain slots for them (no `EditorTheme` change, as T255 does): Error, Warning, Information, Hint → the matching
`EditorTheme` fields; Accent → `cursor`; Muted → `foreground`; Modified → `information`; Removed → `error`;
Added → `information` with its hue set to 130° (green). Alpha by kind: row background 0.08, highlight 0.22
(as `search_match_color`), gutter marks 0.8, underline and after-line as above.

**Testing record**: the `FrameLayout` field keeps the laid-out rects with their kind and colour; tests read
it through `view.layout` after a frame (as diagnostics_tests reads `caret_bounds`). No view.rs change.

## How a provider plugs in (the git card's shape)

A host builds `Rc<RefCell<P>>` with `P: DecorationProvider` and calls
`view.model_mut().add_decoration_provider(p.clone())`, then pushes updates through its own `p` and calls
`cx.notify()`. A git changed-rows provider holds the base text, diffs `snapshot.text()` once per version
(T374's diff), and for the queried range emits `RowBackground` + `Gutter(Bar)` with `Tint::Added`/`Modified`
over changed rows and `Gutter(Deletion)` with `Tint::Removed` as an empty range at the start of the row after
removed ones. It paints through this card with no element.rs code of its own.

## Not in this card

Migrating diagnostics, matches or selections; clicks on gutter marks; theme slots; minimap or scrollbar marks.

## Implementation notes

- `crates/fe-gpui/src/element/decorations.rs` holds the layout (`Decorations::layout`, called in
  `prepaint` after `match_rects` with the frame's em) and the painting (`Decorations::paint` per
  `Layer`). `FrameLayout::decorations.placed` keeps each laid-out rect in window coordinates with its
  kind and colour; an after-line one also keeps its shaped text.
- A covered row's display rows are found by binary search on `frame.rows` (sorted by buffer row),
  so each decoration costs only the rows it covers.
- `Gutter(Deletion)` "at the text's end": taken as an empty range at `text.len()` on a non-empty last
  row (no trailing newline), which goes on that row's bottom edge. With a trailing newline the range
  is at the start of the empty last row and sits on its top edge, the usual case.
- After-line text has line breaks replaced by spaces before shaping.
- Paint order is tested through a test-only log (`decorations::PAINTED`), which element.rs notes
  "matches", "selection" and "text" into under `#[cfg(test)]`.
