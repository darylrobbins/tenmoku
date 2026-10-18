---
task_uid: "01m55c1w2ktd11trs3n49kjmq7"
task: "T847"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-markdown-view test images_load_locally_and_remote_ones_are_blocked (tests/render.rs): a relative image inside the document's folder draws an img with non-zero bounds, while an https:// image and a ../ path outside the root draw md-image-blocked placeholders showing their alt text, and nothing is fetched"
    required: true
    manual: false
  - id: "C2"
    text: "fe-markdown-view test link_clicks_emit_their_destination (tests/render.rs): a click on a link's text emits MarkdownViewEvent::LinkClicked with its raw dest, a click elsewhere emits nothing, and a task item's checkbox doesn't toggle"
    required: true
    manual: false
  - id: "C3"
    text: "fe-markdown-view test style_follows_the_editor_theme (tests/render.rs): MarkdownStyle::new from fastedit Dark and fastedit Light take each palette's background, foreground and syntax colours, and a Rust fence's keyword run is drawn in the theme's keyword colour"
    required: true
    manual: false
  - id: "C4"
    text: "cargo run -p fe-markdown-view --example screenshots on macOS matches the committed goldens examples/screenshots/golden/markdown_preview_dark.png and markdown_preview_light.png, which show every construct the spec's fixture lists"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check-no-gpui.sh passes with fe-markdown-view in UI_CRATES; THIRD_PARTY.md has a row per gpui-kit-adapted file and each starts with the adapted-from header; cargo clippy -p fe-markdown-view --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Render fe-markdown documents in a new fe-markdown-view GPUI view: headings, lists, task boxes, tables, highlighted code, local images and blocked remote ones, in the editor theme's light or dark colours

## Problem

The user, 2026-10-17: "We also need a markdown preview view", modelled on VS Code's built-in preview. The
preview should render CommonMark plus GFM in the theme's colours, light and dark, highlight fenced code, render
local images and block remote ones.

`fe-markdown` (the card this depends on) parses a document into blocks with source lines but draws nothing.
gpui-pre 0.3.7 has no Markdown element. gpui-kit (Apache-2.0, which the plan's reuse map allows copying from)
has a rich-text view in `crates/base/src/text/` at gpui-kit@2154ba84afd2e771f117fd17e55c060dd5e03fd4. It is
built on mdast and gpui-kit's own theme, selection and GlobalState, so only some of its rendering pieces are
worth adapting.

This is card 2 of 4 (model, renderer, workbench item and commands, scroll sync).

## Change

New crate `crates/fe-markdown-view`: "The GPUI view of an `fe-markdown` document: blocks, inlines, highlighted
code, tables, task lists and images, in the editor's theme." Its dependencies are gpui, fe-markdown,
fe-syntax, fe-gpui (for `EditorTheme`) and kiln-ui (for `UiTheme`). Its dev-dependencies are gpui with
`test-support`, gpui_platform with `test-support`, kiln-platform with `test-support` (the golden harness), and
tempfile. Register it in workspace `Cargo.toml` as `fe-markdown-view = { path = "crates/fe-markdown-view" }`,
and add `fe-markdown-view` to `UI_CRATES` in `scripts/check-no-gpui.sh`.

- `src/style.rs`: `MarkdownStyle::new(editor: &EditorTheme, ui: &UiTheme)`.
  - Background and body text come from the editor theme's `background` and `foreground`.
  - Muted text, borders and rules come from `ui.muted_text` and `ui.border`; links from `ui.accent`; code
    backgrounds from `ui.panel_background`.
  - Code uses `editor.font_family` and `font_size`, and its syntax colours are `editor.syntax` by
    `Highlight::index`. Body text is in the window's default UI font, 14px, line height 1.6.
  - Headings are sized as VS Code's preview sizes them: h1 2em with a rule under it, h2 1.5em with a rule,
    h3 1.25em, h4 1em, h5 0.875em, h6 0.85em in muted text.
  - A light editor theme gives a light preview with no other switch.
- `src/view.rs`: `pub struct MarkdownView`, an entity with `Render` and `Focusable`.
  - API: `new(document: Arc<Document>, doc_path: PathBuf, root: Option<PathBuf>, style, cx)`,
    `set_document`, `set_style`, `scroll_to_heading(slug) -> bool`, and
    `EventEmitter<MarkdownViewEvent>`, where `MarkdownViewEvent::LinkClicked { dest: String }`.
  - Top-level blocks are children of one `overflow_y_scroll` div tracked by a `gpui::ScrollHandle`, with a
    centred column at most 880px wide (VS Code's preview width) and 26px padding.
  - Each block element gets a `debug_selector` of `md-block-<ix>` for tests. Card 4 maps source lines to
    these bounds.
- `src/blocks.rs`: paragraphs, headings, quotes (a 4px left bar in `ui.border`), lists (nested bullets and
  ordinals), task items (a read-only checkbox, filled when checked), rules and raw HTML (escaped text).
  Fenced and indented code goes in a rounded block in the code font, coloured by its `highlights`, scrolling
  horizontally instead of wrapping.
- `src/inlines.rs`: `StyledText` runs. Emphasis is italic, strong is bold, strikethrough is struck, and
  inline code uses the code font on the code background. Links are in the link colour, underlined while
  hovered, and clickable through `InteractiveText`. A click emits `LinkClicked` with the raw `dest`; acting
  on it is the workbench's job.
- `src/table.rs`: a header row in bold, cells aligned per column (`Align`), 1px borders, and alternate rows
  tinted with `ui.row_hover`.
- `src/image.rs`: `fe_markdown::image_source(dest, doc_path, root)`.
  - `Local(path)` renders `gpui::img(path)`, scaled down to the column width with its aspect ratio kept
    (`ObjectFit::ScaleDown`). While it loads, or if it fails, it shows the alt text muted.
  - `Blocked(_)` renders a bordered placeholder with the alt text and "Remote image blocked" (or "Image
    outside the workspace blocked"), with `debug_selector` `md-image-blocked-<n>`. Nothing is fetched.
- Copied from gpui-kit@2154ba84 and adapted, each file headed
  `// Adapted from longbridge/gpui-kit@2154ba8, crates/base/src/text/…; Apache-2.0` with what was changed,
  plus a row each in `THIRD_PARTY.md`:
  - `utils.rs`'s `list_item_prefix` and `ordered_list_ordinal`, into `blocks.rs`;
  - `node.rs`'s `CHECK_SVG_LIGHT` and `CHECK_SVG_DARK`, into `blocks.rs`;
  - `node.rs`'s table column sizing (`render_table`, `render_table_rows`), into `table.rs`.
  Don't copy its selection, streaming fade or plugin layers.
- Golden screenshots: `examples/screenshots/main.rs` and `examples/screenshots/scenes.rs` use
  `kiln_platform::screenshots` (`Goldens`, `Settle`) as `crates/fe-gpui/examples/screenshots/` does, and run
  on macOS only.
  - Two scenes at 800×600: `markdown_preview_dark` (`EditorPalette::fastedit_dark()` with `UiTheme` dark) and
    `markdown_preview_light` (`EditorPalette::fastedit_light()` with kiln-ui's `Theme::light().ui_theme()`).
  - Both render the same fixture document:
    - h1, h2 and h3 headings;
    - a paragraph with emphasis, strong, strikethrough, inline code and a link;
    - a nested bullet list, an ordered list, and a task list with checked and unchecked items;
    - a three-column table with left, centre and right alignment;
    - a Rust fence and a ```` ```py3 ```` fence, both highlighted;
    - a blockquote and a rule;
    - a local SVG that the scene writes to a temp dir next to the document;
    - a blocked `https://` image.
  - Goldens: `examples/screenshots/golden/markdown_preview_dark.png` and `markdown_preview_light.png`.
- Tests: `crates/fe-markdown-view/tests/render.rs` (gpui `TestAppContext`, `VisualTestContext::debug_bounds`).

Out of scope: text selection and copy in the preview, keyboard scrolling, rendering HTML, a setting that allows
remote images.
