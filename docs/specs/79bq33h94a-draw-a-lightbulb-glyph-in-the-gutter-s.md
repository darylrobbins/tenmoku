---
task_uid: "01m55v1ebvk23sev79bq33h94a"
task: "T182"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::fold_hover_tests::gutter_glyph::the_glyph_replaces_the_rows_dot_and_follows_edits passes: painted_glyph is on its row inside the mark slot with no dot there, follows an inserted row, and the dot returns when it's removed; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui unit test view::fold_hover_tests::gutter_glyph::clicking_the_glyph_calls_back passes: a click in its bounds calls on_click once and moves no caret; a click elsewhere in the slot calls nothing"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui passes with no existing test edited beyond what the spec names; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Draw a lightbulb glyph in the gutter's mark slot on a host-chosen row, replacing that row's diagnostic dot, with a click callback (Mac T938)

## Problem

LSP completeness track, item 4 (user-approved 2026-10-17: "lightbulb"). VS Code shows a lightbulb in the glyph
margin on the caret's row when the server has code actions there (`editor.lightbulb.enabled`); clicking it opens
the Quick Fix list. fe-gpui has nowhere to draw one. The gutter's left 2 em hold the diagnostics' marks
(`gutter_width`, `crates/fe-display/src/layout.rs:525`; dots from `diagnostic_marks`,
`crates/fe-gpui/src/element.rs:1494`, painted at `:1227`), which is fastedit's glyph margin.

T816's gutter annotation column (the brief suggested it) is the wrong home: it is host text left of the line
numbers, sized to its widest entry and absent when empty, and git blame (T820) fills it; a lightbulb there
would come and go with the column's width and fight blame for it. VS Code keeps the two apart too (blame-like
annotations are text decorations; the lightbulb is a glyph). This card draws a one-row glyph in the mark slot
instead; asking the server is the next card.

## Change

1. `crates/fe-gpui/src/view.rs`:
   - New `pub struct GutterGlyph { pub row: u32 /* buffer row */, pub kind: GlyphKind }` and `pub enum
     GlyphKind { Lightbulb, LightbulbAutofix }` (VS Code's two: an action, and a preferred quick fix).
   - `EditorView::set_gutter_glyph(&mut self, glyph: Option<GutterGlyph>, on_click: Option<Rc<dyn Fn(&mut
     Window, &mut App)>>)`; the row is anchored and follows edits; `gutter_glyph()` reads it back.
   - `painted_glyph: Option<(u32, Bounds<Pixels>)>` recorded for tests.
2. `crates/fe-gpui/src/element.rs`: on the glyph's row, if shown (not folded away), paint a lightbulb path
   (a bulb and its base, drawn as `paint_chevron` draws its path) in the mark slot, centred on the row's first
   display row, instead of that row's diagnostic dot: amber `#FFCC00` for `Lightbulb`, blue `#3794FF` for
   `LightbulbAutofix` (VS Code's `editorLightBulb.foreground` and `editorLightBulbAutoFix.foreground`). The dot
   returns when the glyph goes. A left click inside its bounds calls `on_click` and changes neither the
   selection nor the folds.
3. Tests: new `crates/fe-gpui/src/view/gutter_glyph_tests.rs`, declared at the end of
   `crates/fe-gpui/src/view/fold_hover_tests.rs` as `#[path = "gutter_glyph_tests.rs"] mod gutter_glyph;`, in a
   headless window as `view/diagnostics_tests.rs` builds one:
   - `the_glyph_replaces_the_rows_dot_and_follows_edits`: with an error on row 2 and the glyph on row 2,
     `painted_glyph` is on row 2 within the mark slot (x < 2 em) and row 2 has no dot; inserting a row above
     moves it to row 3; `set_gutter_glyph(None, None)` brings the dot back.
   - `clicking_the_glyph_calls_back`: a click inside `painted_glyph`'s bounds calls `on_click` once and
     leaves the caret where it was; a click on row 1's slot calls nothing.

Order: `element.rs` and `view.rs` are held by T816 (mergeable), T858 (ready), T917, T198, T802 and T898; this
follows T816, T858 and T917. Linux-OK (painted state, no goldens).

## Implementation notes

- `GutterGlyph` and `GlyphKind` are re-exported from `lib.rs` (added to the scope on review) so hosts can construct them.
- The click callback is deferred with `window.defer`, as the annotation column's is, so the host may update the view from it.
- The row anchor is biased after insertions, so a row inserted exactly at the glyph's row start goes above it.
