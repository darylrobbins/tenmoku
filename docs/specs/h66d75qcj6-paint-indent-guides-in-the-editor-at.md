---
task_uid: "01m4sbzpj9r8ca6kh66d75qcj6"
task: "T584"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test indent_guides_setting: EditorSettings::default().indent_guides is true; a settings.toml with [editor] indent_guides = false loads false with no diagnostics; template.rs (key list gains indent_guides) and schema_matches_golden pass"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test indent_guide_colors: EditorColors::KEYS has 21 entries; fastedit-dark, -light and -ember each set indent_guide and indent_guide_active, both with alpha \u003c 0.3 and the active one more opaque; a kiln theme whose [editor] sets indent_guide = \"#123456\" gets it and takes indent_guide_active from the bundled palette"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui indent_guides_tests: a drawn 4-space sample with a blank row inside a nested block paints one 1-logical-px-wide rect (2 device px at 2x scale; the user chose logical px, 2026-10-14) per guide run at x = frame.view_x(column * space width) for columns 0 and 4, over exactly the rows fe_editor::indent_guides returns (blank row included); Spaces(2) gives columns 0, 2, 4; a tab-indented file with tab_size 4 gives 0 and 4; a soft-wrapped indented row's run covers all its pieces"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui indent_guides_tests: with the caret on a block's opening row, the guide starting on the next row is drawn in indent_guide_active and every other guide in indent_guide; after apply_settings with indent_guides false no guide is laid out or painted, and applying true draws them again"
    required: true
    manual: false
  - id: "C5"
    text: "Every existing test passes with only the template.rs key list and editor_palette.rs KEYS count edited; the fe-gpui and fe-workbench screenshot goldens are regenerated and their diffs show only guides, except that workbench.png may also carry the pre-existing T603 terminal-icon drift (about x 1950-1977, y 1099-1132; the user accepted it, 2026-10-14); clippy on fe-config and fe-gpui --all-targets -D warnings is clean; nothing outside the scope changes except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Paint indent guides in the editor at each indent level, behind an editor.indent_guides setting (on by default), in new indent_guide theme colours

## Problem

The user, 2026-10-12: "We should also optionally support indent guides." Their reference (a JetBrains
editor) shows thin grey vertical lines at each indent level through nested blocks, continuing through
blank lines.

The computation is done: T491 (Mac; landed as 9f94bad9) added `crates/fe-editor/src/indent_guides.rs`
with `indent_guides(text, rows, indent_width, tab_width) -> Vec<IndentGuide { column, rows }>` (blank
rows inside a block keep its guides, guides at column 0, w, 2w, ...) and `active_indent_guide(text,
caret_row, indent_width, tab_width)`, tested in `indent_guides_tests.rs`. Its spec left "painting and
passing the file's detected indent width" to a later card. Nothing paints them, there is no setting and no
theme colour. Indent detection (T458, 08358f92) and .editorconfig (T478) set
`EditorModel::indent_style()`.

## Change

**Setting** (`crates/fe-config/src/settings.rs`, the pattern of `editor.auto_save` in 74af6906):
`EditorSettings::indent_guides: bool`, default `true` (VS Code's `editor.guides.indentation` and Zed's
`indent_guides.enabled` both default on), doc comment "Draw a thin vertical line at each indent level".
Add `# indent_guides = true` with a one-line comment to `crates/fe-config/settings-template.toml`,
regenerate `crates/fe-config/settings.schema.json` with `UPDATE_GOLDEN=1 cargo test -p fe-config
schema_matches_golden`, and add `indent_guides` to the
editor key list in `crates/fe-config/tests/template.rs`.

**Theme** (the T485 pattern, 016efde2): two `EditorColors` keys in `crates/fe-config/src/theme.rs`,
`indent_guide` and `indent_guide_active`; `EditorColors::KEYS` goes from 19 to 21 (update the count in
`crates/fe-config/tests/editor_palette.rs`). Set them in the `[editor]` table of each bundled theme:
fastedit-dark.toml `#ffffff12` / `#ffffff30`, fastedit-ember.toml `#ffffff12` / `#ffffff30`,
fastedit-light.toml `#0000001a` / `#00000040`. Matching `EditorTheme` fields in `crates/fe-gpui/src/lib.rs`,
set in `EditorTheme::from_palette` (`crates/fe-gpui/src/theme.rs`). While in those two files, drop "and row
background" from the `change_added` / `change_modified` doc comments (the change-rail card removes the
background).

**The flag reaches the view through the theme**: `EditorTheme` gains `pub indent_guides: bool`, set by
`EditorTheme::set_metrics` from `settings.indent_guides` (it already takes the font metrics from
`EditorSettings` and is called by `EditorView::apply_settings`, settings reloads and fe-workbench's pane),
so `crates/fe-gpui/src/view.rs` doesn't change.

**Paint** (fe-gpui only paints; no guide arithmetic outside fe-editor):

- New `crates/fe-gpui/src/element/indent_guides.rs`: from the frame's shown buffer rows (first display
  row's `buffer_row` to the last's, inclusive) call `fe_editor::indent_guides::indent_guides` with
  `indent_width` = n for `IndentStyle::Spaces(n)`, else `tab_size`, and `tab_width` = `tab_size`; and
  `active_indent_guide` at the primary selection head's buffer row. Each guide becomes one rect per run of
  its display rows shown one under the next (soft-wrapped pieces join a run, a block or fold between rows
  ends it, as the change bar's runs in `decorations.rs` do), at `x = frame.view_x(column * space)` where
  `space` is the glyph width of ' ', 1 device pixel wide (`(1. * scale).round().max(1.) / scale`), from
  the run's first row top to its last row bottom. The active guide's rects are in `indent_guide_active`,
  the others in `indent_guide`. With `theme.indent_guides` false it returns nothing and calls neither
  function.
- `crates/fe-gpui/src/element.rs`: lay them out with the frame and paint them inside the text-bounds
  content mask right after `Layer::Backgrounds`, so they sit under search matches, the selection and the
  text, and never paint into the gutter when scrolled sideways. Keep the element.rs lines to the call
  sites and the `mod` line.
- Tests in a new `crates/fe-gpui/src/element/indent_guides_tests.rs`, included from indent_guides.rs with
  `#[cfg(test)] #[path = "indent_guides_tests.rs"] mod tests;`, drawing a headless `EditorView`.

**Goldens**: guides default on, so the screenshot goldens with indented code change. Regenerate them with
`UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots` and the fe-workbench one, and check the
image diffs show only the guides.

## Implementation notes

- `EditorTheme::set_colors` keeps `indent_guides` along with the metrics, so a theme change doesn't turn
  guides back on.
- A guide is drawn active when it has the active guide's column and its rows overlap the active guide's
  (the active guide's rows aren't clipped to the frame; the frame's are).
- `element.rs` gains, besides the `mod` line and the layout and paint call sites, the
  `FrameLayout::indent_guides` field and its initialiser. Tests read what was painted from a test-only
  thread-local in `indent_guides.rs`, so `view.rs` is unchanged.
- Goldens: each changed fe-gpui PNG differs only in 2-device-px-wide columns at the guide x's (a 1 device
  px quad at a fractional x is antialiased over two columns). `workbench.png` also gains the terminal
  icon at x 1950–1977, y 1099–1132: rendering with guides off gives that icon and nothing else against
  the committed golden, so it is drift from the base (T603's dock change), not this card.
