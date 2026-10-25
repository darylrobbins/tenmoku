---
task_uid: "01m5qychgcv9nycrqftek3g2wm"
task: "T331"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "LspManager::document_colors and color_presentations decode to bytes and clamped components, Ok(None) without colorProvider"
    required: true
    manual: false
  - id: "C2"
    text: "EditorView::set_color_swatches reserves a one-em gap before each offset and paints a bordered rounded square in the colour (alpha over a checkerboard), independent of inlay hints, following edits, with the caret skipping the gap and wrapped rows intact"
    required: true
    manual: false
  - id: "C3"
    text: "With editor.color_decorators on (default) the workbench shows swatches for each editor's server colours, refreshed after edits settle; off or an empty reply clears them"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in document_color_tests.rs, color_swatches_tests.rs, lsp_colors_tests.rs and color_decorators_setting.rs pass headless; no screenshot golden changes"
    required: true
    manual: false
---

# Draw colour swatches before the colours a language server finds (textDocument/documentColor, editor.color_decorators), as VS Code does (Mac T1113)

## Problem

fastedit has no `textDocument/documentColor` (`rg documentColor colorProvider crates` is empty). VS Code draws a small colour square before each colour literal a language server finds (CSS, SCSS, JSON-with-colours, Tailwind servers; `editor.colorDecorators`, default on). The picker that opens on click is out of scope here (swatches first).

## Change

1. fe-lsp: new `crates/fe-lsp/src/document_color.rs` (+ lib.rs mod, `pub use ColorInformation`): `LspManager::document_colors(buffer, version) -> Future<Result<Option<Vec<ColorInformation>>, LspError>>`, `ColorInformation { range: Range<usize>, color: [f32; 4] }` (bytes of the request version, sorted, components clamped to 0..=1), and `LspManager::color_presentations(buffer, version, info) -> Future<Result<Vec<ColorPresentation>, LspError>>` (`textDocument/colorPresentation`: `label`, `text_edit` in bytes, additional edits) for the later picker (decoded and tested, unused by the UI). `Ok(None)`/nothing sent without `colorProvider`. Client capability `color_provider: { dynamic_registration }` in server.rs; both methods in the hover timeout group in requests.rs.
2. fe-gpui: new `crates/fe-gpui/src/color_swatches.rs`: `EditorView::set_color_swatches(Vec<ColorSwatch { offset: usize, rgba: [f32; 4] }>)`, a list separate from the inlay hints (setting one never replaces the other; do not change `InlayHint`). Each swatch takes an inline gap before `offset`, laid out through the same mechanism inlay hints use for their gaps (crates/fe-gpui/src/inlay_hints.rs, fe-display `layout_frame_with_hints`), one em wide, and is painted as a rounded square 0.85 em tall in the colour (alpha over a checkerboard of the theme's two grey steps) with a 1 px border in the theme's border colour so white and black stay visible on both themes. Swatches follow edits until the next set; the caret is never inside the gap; hit-testing and wrapping treat the gap as hints' are. A test hook `painted_swatches` (like `painted_inlay_hints`) records the bounds painted.
3. Setting `editor.color_decorators` (bool, default true): fe-config settings.rs, settings-template.toml, settings.schema.json; propagated like `code_lens_on` (workbench.rs, workbench_root.rs).
4. fe-workbench: new `lsp_colors.rs` (mod in lsp.rs; `sync_colors`): per open editor (setting on), ask on open and 500 ms after an edit settles, `set_color_swatches`; empty, error or setting off clears. Answers for old versions are dropped.

## Tests

- `crates/fe-lsp/src/document_color_tests.rs` (#[path] mod in document_color.rs): `colors_decode_to_bytes_and_clamp`, `no_provider_sends_nothing`, `color_presentations_decode_edits`, `null_reply_and_timeout_are_empty`.
- `crates/fe-gpui/src/color_swatches_tests.rs` (#[path] mod in color_swatches.rs, headless view tests as inlay_hints_tests.rs): `a_swatch_reserves_a_gap_and_paints_in_it`, `swatches_and_inlay_hints_coexist`, `swatches_follow_edits`, `swatch_alpha_paints_over_the_checkerboard`, `the_caret_skips_the_gap`, `wrapped_rows_keep_swatches`.
- `crates/fe-workbench/src/lsp_colors_tests.rs` (#[path] mod in lsp_colors.rs, fake server): `colors_become_swatches`, `setting_off_asks_nothing`, `edit_asks_again_and_drops_stale_answers`.
Linux-OK (no pixel goldens: assertions are on painted bounds and colours). A Mac-only screenshot golden of a swatch row would be a separate small card. Size l: expected 1.5k to 2k lines; the fe-gpui part (item 2) is the risk, so if it runs over, land items 1 and 3-4 with swatch painting stubbed behind the same `set_color_swatches` API, and item 2 as a follow-up.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y58), not truncated.

## Implementation notes

- **One em.** fe-gpui's `em` is the font's em advance (`em_advance`, one column of a monospace font), as the wrap margin and fold chevrons use it. A swatch's gap is a single space laid out as an inlay hint's text is (transparent, appended to the frame's hints after the view's own, so a hint at the same offset comes first); the square is 0.85 of the gap's width on a side, centred, with corners rounded at a fifth of its side (at most 3 px).
- **Greys and border.** `EditorTheme` has no grey ramp and new fields would break the struct literals in the screenshot scenes (out of scope), so the checkerboard's two greys are the theme's `gutter_foreground` at 30% and 70% over its `background` (opaque, from each theme's own colours), and the 1 px border is `gutter_foreground`, a mid grey on both bundled themes. The checkerboard is 2 × 2 and painted only under a colour that isn't opaque.
- **Edits.** Swatches follow edits with inlay hints' `follow` (now `pub(crate)`): text typed at a swatch's offset goes before it, one whose offset an edit deleted text strictly around goes, and all go once the buffer compacts past where they were set.
- **Beside blame (review px81xb F1).** fe-display laid out no hints beside the gutter's annotation column, so swatches vanished with blame shown. Revision 2 adds `crates/fe-display/src/layout.rs` to the scope for `layout_frame_with_annotation_column_and_hints`, which element.rs now uses there, so swatches (and inlay hints with them) show beside the column too (`swatches_show_beside_the_annotation_column`).
- **Workbench.** `lsp_colors.rs` mirrors `lsp_links.rs`: per buffer, its editors given the same swatches (an editor joining gets the others'), asked at once on open and 500 ms after the version changes, retried on `ContentModified` up to 8 times, after which the swatches clear as for any error (review px81xb F2, `exhausted_retries_clear_the_swatches`), re-asked when the server's opening changes. An answer for a version or opening that isn't the latest is dropped; a newer ask also drops the one in flight, which cancels it. An empty reply, a server without `colorProvider`, any other error, or `editor.color_decorators` off clears the swatches.
- **Presentations.** `color_presentations` without `colorProvider` is `Unavailable("colorPresentation")` with nothing sent, as `resolve_document_link` is without its provider; `null` and a timeout are no presentations.
