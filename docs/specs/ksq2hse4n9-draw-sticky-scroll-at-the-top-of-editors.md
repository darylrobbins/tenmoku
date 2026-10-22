---
task_uid: "01m5f7fwdyr81993ksq2hse4n9"
task: "T1029"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "A gpui test scrolled into a fn inside an impl has band rows for the impl and fn headers with their 1-based line numbers, each row's text runs equal to that row's runs when laid out unscrolled (sticky_scroll_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "As the fn's last row scrolls under the band, the fn row moves up by the same px; the band is empty at scroll 0 and in a file with no scopes"
    required: true
    manual: false
  - id: "C3"
    text: "editor.sticky_scroll.enabled = false applied live removes the band without reopening; the [editor.sticky_scroll] table defaults to enabled = true and is in the template and schema"
    required: true
    manual: false
  - id: "C4"
    text: "sticky_scroll_background/border/shadow/hover parse in all six bundled themes with the spec table's values, and a user theme without them gets the bundled palette's (crates/fe-config/tests/sticky_scroll_colors.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "Scrolling by sub-row px within one top row computes scopes once per (top row, version)"
    required: true
    manual: false
  - id: "C6"
    text: "cargo run -p fe-gpui --example screenshots passes with the new sticky_scroll golden (two pinned rows, line numbers, shadow); existing goldens unchanged"
    required: true
    manual: false
  - id: "C7"
    text: "With the minimap on (T1033), the band spans only the text area: it ends at the minimap's left edge, as VS Code's does, and the minimap's slider and clicks are unaffected by the band"
    required: true
    manual: false
---

# Draw sticky scroll at the top of editors: pinned scope headers with full highlighting and line numbers, a shadow, editorStickyScroll theme keys, editor.sticky_scroll.enabled and a sticky_scroll golden

# Sticky scroll 2/4: draw the band, its theme keys, editor.sticky_scroll.enabled and a golden (Mac)

Draws card 1's rows (`fe_display::sticky::sticky_rows` over `fe_syntax::outline_scopes`) at the
top of every editor, as VS Code does (see the user's screenshot: `60 pub(crate) fn config(…) {`
pinned over rows 62…, a shadow under it).

## Rendering: new `crates/fe-gpui/src/element/sticky_scroll.rs`

- Laid out in `EditorElement::prepaint` (`crates/fe-gpui/src/element.rs`) after the frame
  settles, painted after the text, selections and carets and before popovers, clipped to the
  editor's bounds. Each row: the band background across gutter and text, the buffer row's line
  number in the gutter (`gutter_foreground`; `gutter_active_foreground` on the caret's row), and the
  row's text shaped with the same spans as the row has in the text area (`styled_highlights` with
  bracket colours and `Foreground` decorations), at the frame's horizontal scroll (card 4 makes
  that a setting). A header wider than the view is cut at the right edge. No selections, carets,
  squiggles, inlay hints, fold chevrons or change bars in the band.
- Under the last row: a 1 px `sticky_scroll_border` line when the theme sets a visible one, and a
  shadow in `sticky_scroll_shadow` fading down over ~3 px (VS Code's `scrollbar.shadow` look).
- Scopes are found once per (top buffer row, text version), cached on the view, not per frame.
- Off with no syntax tree for the text (card 4 adds the fallbacks).

## Theme keys (`[editor]` table)

Add to `colors!` in `crates/fe-config/src/theme.rs` **right after `indent_guide_active`** (the
minimap card appends at the end; keep the inserts apart): `sticky_scroll_background`,
`sticky_scroll_border`, `sticky_scroll_shadow`, `sticky_scroll_hover` (VS Code's
`editorStickyScroll.background`, `.border`, `.shadow`, `editorStickyScrollHover.background`), with
fields on `EditorTheme` (`crates/fe-gpui/src/lib.rs`, `crates/fe-gpui/src/theme.rs`). Values, from
VS Code 1.142's theme files and `colorRegistry` defaults (background = editor.background, border
none, shadow = scrollbar.shadow, hover dark #2A2D2E / light #F0F0F0):

| theme file | background | border | shadow | hover |
|---|---|---|---|---|
| vscode-2026-dark.toml | #121314 | #00000000 | #191B1D4D | #2A2D2E |
| vscode-dark-modern.toml | #1F1F1F | #00000000 | #000000 | #2A2D2E |
| vscode-2026-light.toml | #FFFFFF | #00000000 | #00000000 | #F0F0F0 |
| fastedit-dark.toml | #16181d | #00000000 | #000000 | #2A2D2E |
| fastedit-ember.toml | #1c1814 | #00000000 | #000000 | #2A2D2E |
| fastedit-light.toml | #ffffff | #00000000 | #DDDDDD | #F0F0F0 |

(The hover colour is used by card 3.)

## Setting

`editor.sticky_scroll.enabled` (default true) as a `[editor.sticky_scroll]` table
(`StickyScrollSettings` in `crates/fe-config/src/settings.rs`, so card 4 can add `max_line_count`,
`default_model` and `scroll_with_editor`), in `settings-template.toml` and `settings.schema.json`,
applied live to open editors through `EditorTheme::set_metrics` and kept by `set_colors`.

## Screenshot scene

A `sticky_scroll` scene in `crates/fe-gpui/examples/screenshots/scenes.rs` (Rust: an `impl` with
a long `fn` scrolled so the view's top is inside the fn, two rows pinned), which needs scenes to
take a scroll position (`crates/fe-gpui/examples/screenshots/main.rs`), with its golden
`golden/sticky_scroll.png`; add the scene to TESTING.md's screenshot list.

## Implementation notes

- `element/sticky_scroll.rs`: `scopes` runs in prepaint's view update once the frame has settled,
  and asks `fe_syntax::outline_scopes` for the buffer rows from the top display row to the one
  `MAX_ROWS + 1` screen rows below it, keyed by (language, those rows, text version) on
  `EditorView::sticky_scopes`. The rows come from the whole screen row at the top, so sub-row
  scrolling keeps the key. An unparsed tree (`NoOutline::NotParsed`) isn't cached and leaves the
  band off for that frame; an unsupported or too-large file caches no scopes.
- `layout` places `fe_display::sticky_rows` (at most `MAX_ROWS` = 5, VS Code's default
  `maxLineCount`, which card 4 makes a setting; no `keep_above`, which card 3 adds) and shapes each
  header's first display row with `row_runs` over `styled_highlights` (bracket colours and the
  row's `Foreground` decorations) at the frame's horizontal scroll. A header longer than
  `MAX_SHAPED_ROW_BYTES` with soft wrap off is shaped from its `RowIndex` slice around the view, as
  the text area's long rows are. Line numbers are right-aligned where the gutter's are, in
  `gutter_active_foreground` on a row with a caret.
- `paint` runs after the gutter's numbers, marks and chevrons and before the minimap, inside the
  editor's content mask: rows bottom first, so a pushed-up row slides under the one above, each
  over `sticky_scroll_background`, with its text clipped to the text area; then a 1 px
  `sticky_scroll_border` line when its alpha isn't 0, and a 3 px `linear_gradient` from
  `sticky_scroll_shadow` to transparent. The band is `frame.width` wide, which is the editor's
  width less the minimap's, so it ends at the minimap's left edge.
- The band has no hitbox in this card: clicks, hover and the caret under it are T1030's.
- The screenshot scenes already took a scroll position (`top`, from T1033), so `main.rs` is
  unchanged. Scenes gained `sticky_scroll: bool`, true only in the new scene, so the minimap
  scene (scrolled into `mod part_4`) keeps its golden.
