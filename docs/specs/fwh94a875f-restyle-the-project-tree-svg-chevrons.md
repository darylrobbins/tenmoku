---
task_uid: "01m48zxyma4xmw83fwh94a875f"
task: "T116"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-ui test tests::tree_row::colors_follow_state: TreeRow colors(): selected+focused → background row_selected with border row_focus_border; selected unfocused → row_selected_inactive, no border; unselected → no background; ignored → label, icon and chevron muted_text; error → label and icon error_text; plain directory icon folder_icon, plain file icon icon, label text"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::tree_style::rows_carry_icons_and_chevrons: with src/ expanded, debug_bounds finds tree-icon:main.rs:file-code, tree-icon:Cargo.toml:package, tree-icon:README.md:file-text, tree-icon:src:folder-open, tree-icon:util:folder, tree-chevron:src:open and tree-chevron:util:closed, each 16×16; no tree-chevron selector exists for main.rs; after clicking src it is tree-chevron:src:closed and tree-icon:src:folder"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::tree_style::layout_metrics: every row is ROW_HEIGHT tall; util's chevron starts exactly TREE_INDENT right of src's; main.rs's icon starts at the same x as util's icon (chevron column reserved on files); each icon's vertical centre equals its row's within 0.5 px; the gap from chevron to icon is 2 px"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::tree_style::indent_guides_follow_depth: top-level rows have no tree-guide selector; util/mod.rs (depth 2) has tree-guide:mod.rs:0 and :1 and no :2, each ROW_HEIGHT tall and 1 px wide, centred on src's and util's chevron centres respectively"
    required: true
    manual: false
  - id: "C5"
    text: "Passing, unchanged except where a test pins the 22 px height: fe-workbench tests::project_panel::* (incl. only_visible_rows_are_built over 100,000 files), tests::outline_panel::*, tests::tree_keys::*; the outline panel compiles without edits; render_rows adds no per-row heap allocation beyond the label (icon lookup is file_icon/folder_icon on \u0026str)"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::tree_style::every_list_row_is_24px: ROW_HEIGHT is px(24.) and defined once, in crates/fe-ui/src/theme.rs; laid-out rows are exactly 24 px tall in the project panel (row_bounds), the quick-open picker (Picker::row_bounds, and its list is 24 px × visible rows), the search panel (a file header row and a match row, SearchPanel::row_bounds) and the outline panel (OutlinePanel::row_bounds); rg 'px\\(22' in crates/fe-ui/src and crates/fe-workbench/src finds no row height"
    required: true
    manual: false
---

# Restyle the project tree: SVG chevrons, file and folder icons, indent guides and focus-aware selection

# Restyle the project tree: SVG chevrons, file and folder icons, indent guides and focus-aware selection

Polish for the project tree, asked for by the user ("pretty up the file explorer: icons, make the
triangles a bit bigger, a nice modern file explorer experience"). Second of two cards. It builds on:

- T115 (the icon card): `fe_ui::{Assets, Icon, file_icon, folder_icon}`, with Lucide SVGs served through
  the app's `AssetSource`
- T63's `TreeRow` and `ProjectPanel`
- T80's keyboard selection and the panel's focus handle

It lands before T81's golden screenshot, so the golden captures this look.

## Problem

`TreeRow` draws its disclosure triangle as a text glyph (`▸`/`▾`) at the panel's 12 px font. That's
small and soft. Rows have no file-type or folder icons and no indent guides, and the selected row
looks the same whether or not the panel has focus.

## Change

### `crates/fe-ui/src/tree_row.rs`

All additions are additive builder methods, so T78's outline panel (`new`, `depth`, `on_click`)
compiles unchanged and gains only the indent guides.

- **Metrics.** `ROW_HEIGHT` moves to `crates/fe-ui/src/theme.rs` (see below) and becomes 24 px.
  New public consts:
  - `TREE_INDENT = 14 px`
  - `CHEVRON_WIDTH = 16 px`
  - `ICON_SIZE = 16 px`
- **Row layout**, left to right:
  - 6 px left padding plus `depth × TREE_INDENT`
  - a 16 px chevron column, reserved on every row so file icons align with their sibling folders'
  - 2 px gap
  - a 16×16 icon, when one is set
  - 6 px gap
  - the label, single line, ending in an ellipsis (`text_ellipsis`) when it's too long

  Everything is centred vertically in `ROW_HEIGHT`.
- **Chevron.** For a directory, `svg().path(Icon::ChevronRight.path())` at 16×16 replaces the text
  glyph.
  - When expanded, it's rotated 90° with `Transformation::rotate`. This is static, with no
    animation.
  - It's drawn in the new `chevron` colour.
  - The whole row is still the click target, as now.
- **Icon.** A new `.icon(Icon)` builder method: `svg().path(icon.path())` at `ICON_SIZE`, drawn in
  the row's icon colour.
- **Indent guides.** For each ancestor level `0..depth`, a 1 px vertical line spans the full row
  height, in `indent_guide` colour.
  - Its x is the centre of that level's chevron column: `6 + level × TREE_INDENT + CHEVRON_WIDTH / 2`.
  - Stacked rows draw continuous lines.
  - They're drawn absolutely positioned, so they don't change the layout.
- **Selection and focus.** A new `.focused(bool)` builder method, meaning the list has focus.
  - The row's background is an inner rounded box (4 px radius) inset 4 px from the panel's sides.
  - Selected and focused: `row_selected`, plus a 1 px `row_focus_border`.
  - Selected and unfocused: `row_selected_inactive`.
  - Otherwise: transparent, with `row_hover` on hover.
- **Colours.** These come from one pure `pub(crate) fn colors(&self) -> RowColors`, so they can be
  tested without GPUI:
  - Ignored rows draw label, icon and chevron in `muted_text`.
  - Error rows draw label and icon in `error_text`.
  - Otherwise the label is `text`, a directory's icon is `folder_icon` and a file's is `icon`.
- **Debug selectors** for tests, through GPUI's `debug_selector`, which is a no-op outside
  test-support builds and so free in release:
  - `tree-chevron:<label>:open` or `tree-chevron:<label>:closed`
  - `tree-icon:<label>:<icon name>`
  - `tree-guide:<label>:<level>`

### `crates/fe-ui/src/theme.rs`

**One row height.** `pub const ROW_HEIGHT: Pixels = px(24.)` moves here from `tree_row.rs`, as the only
row-height token. It changes from 22 px to 24 px, the user's choice (2026-10-06). It's re-exported as
`fe_ui::ROW_HEIGHT` from `crates/fe-ui/src/lib.rs`, so imports keep working.

Every list row uses it, and nothing else in fe-ui or fe-workbench hard-codes a row height:

- `TreeRow`, which also draws the outline panel's rows (`outline_panel.rs` uses `TreeRow`, and needs no
  edit)
- the picker's rows, and its list height (`ROW_HEIGHT × visible rows`), in
  `crates/fe-workbench/src/picker.rs`
- the search panel's file-header and match rows, in `crates/fe-workbench/src/search_panel.rs`

**Colour tokens.**

New `UiTheme` tokens, with their `default_dark` values. The worker may tune the values against the
running app, but not the token names.

| Token | `default_dark` |
|---|---|
| `icon` | `0x8b8f98` |
| `folder_icon` | `0x7f9fd8` |
| `chevron` | `0x6c717c` |
| `indent_guide` | `rgba 0xffffff14` |
| `row_selected_inactive` | `rgba 0xffffff0f` |
| `row_focus_border` | `rgba 0x4f7cc8aa` |

### `crates/fe-workbench/src/project_panel.rs`

`render_rows` sets the following, with no new allocation per row beyond the label it already
builds:

- `.icon(folder_icon(row.expanded))` for directories
- `.icon(Icon::FileSymlink)` for symlinks
- `.icon(file_icon(name))` for files
- `.focused(...)`, from the panel's focus handle (T80) as it renders

## Performance

The icon lookup is O(1) per built row: at most 4 hash probes, no allocation. GPUI caches each SVG's
raster by path and size in its sprite atlas, so scrolling never re-parses an SVG.

T63's `tests::project_panel::only_visible_rows_are_built`, which expands 100,000 files, must still
pass unchanged. So must T78's `tests::outline_panel::*` and T80's `tests::tree_keys::*`.

## Tests

- New file `crates/fe-ui/src/tests/tree_row.rs`, declared in `crates/fe-ui/src/tests/mod.rs`.
- New file `crates/fe-workbench/src/tests/tree_style.rs`, declared in
  `crates/fe-workbench/src/tests/mod.rs`. It uses headless GPUI (`VisualTestContext::debug_bounds`),
  with a fixture written into a `tempfile` dir:
  - `src/` holding `main.rs` and `util/` (which holds `mod.rs`)
  - `Cargo.toml`
  - `README.md`
  - `.gitignore`, ignoring `target/`
  - `target/`

## Out of scope

- Git status colouring. `fe-project` exposes no git status; a backend would be its own later card.
- Chevron rotation animation.
- Special folder icons.
- Per-language icon colours.
- A light `UiTheme`.
- Changing the panel's font size.
- The dock header's styling.
- Rename, create and drag-and-drop.

## Decided (lead defaults; the user may revisit)

- Chevrons, as VS Code and Zed draw them, rather than Finder's filled triangles: 16 px, rotated
  when open. A filled triangle would be one new `Icon` variant.
- Rows are 24 px everywhere: the tree, the picker, search results and the outline. That's one token,
  the user's choice (2026-10-06).
- Icons are monochrome: files in one colour, folders in an accent colour.
- Indent guides are always shown, at every level.
