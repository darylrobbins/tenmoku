---
task_uid: "01m4935jars5h4r52gqdc5fry6"
task: "T125"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::status_bar::caret_moves_update_line_and_column: with a.rs holding 'fn main() {}\\n    let é = 1;\\n' open and focused, the right side reads 'Ln 1, Col 1'; after simulate_keystrokes('down end') it reads 'Ln 2, Col 15' (chars, not the 16 bytes); after 'shift-left shift-left shift-left' it reads 'Ln 2, Col 12 (3 selected)'"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::status_bar::multiple_selections_are_counted: three carets set through EditorModel::set_selections (then notify) read 'Ln 3, Col 1 (3 selections)' for carets at the starts of rows 1–3; collapsing to one caret drops the suffix"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::status_bar::follows_the_active_editor: with a.rs then b.md open in one pane the language item reads Markdown, and cmd-shift-[ makes it Rust; cmd-\\ then splits a.rs into a new right pane and b.md opens there from the tree (Markdown); cmd-k cmd-left reads Rust and cmd-k cmd-right Markdown; after switching away, edits to the inactive editor do not re-render the bar (its render counter is unchanged)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::status_bar::file_facts: an LF file reads 'Spaces: 4', 'UTF-8', 'LF'; a CRLF file reads 'CRLF'; a file starting with EF BB BF reads 'UTF-8 with BOM'; a .xyz file reads 'Plain Text'; after Workbench::reload_settings with tab_size 2 the open editor reads 'Spaces: 2'"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::status_bar::language_picker_sets_the_language: clicking the language item's laid-out bounds opens the picker with Plain Text first and the current row marked current; typing pyth and enter makes the editor's syntax python, the item reads Python, the picker is gone and the editor is focused; dispatching language.select then picking Plain Text leaves syntax() None and reads Plain Text; escape closes it with the language unchanged and the editor focused"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::status_bar::toggle_hides_the_bar: dispatching view.toggle_status_bar as a RunCommand hides the bar (status_bar_visible() false, no items drawn next frame, the pane's bottom edge moves down by ROW_HEIGHT); a second dispatch shows it again; set_status_bar_visible(false) hides it the same way"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::status_bar::host_items: set_item adds a Left item with command dock.toggle that draws on the left, and clicking it hides the dock; set_item with id language replaces the built-in's text; remove_item removes it; with the pane showing no tabs only host items draw"
    required: true
    manual: false
  - id: "C8"
    text: "fe-text test tests::char_offsets::offset_to_char_counts_chars: for 'aé😀\\nb', offset_to_char gives 0,1,2,3,4,5 at byte offsets 0,1,3,7,8,9, and len() for any offset past the end"
    required: true
    manual: false
  - id: "C9"
    text: "view.toggle_status_bar and language.select (workbench context, no default binding) are registered through the fe-command registry and known to fe_workbench::command_catalog(); the View menu in crates/fe-keymap/menus/default-macos.toml lists Status Bar after Toggle Dock and fastedit's tests/menus.rs::default_menus_know_every_command still passes; command_tests and keymap_tests pass unchanged; rg 'KeyBinding::new|actions!' finds no match in the files this card adds; theme.rs, Cargo.toml and Cargo.lock do not change"
    required: true
    manual: false
---

# Add a status bar: cursor, selections, language picker, encoding, line endings and indentation

# Add a status bar along the workbench's bottom edge: cursor, selections, language, encoding, line endings and indentation

Milestone M3 "Browse", workbench chrome. It builds on:

- T63's `Workbench` and `Pane`, T79's active pane, and the active-editor hook T78 added
  (`Workbench::follow_active_editor`, which already feeds the outline)
- T76's `Picker`/`PickerDelegate` and `PaletteFrame`, as hosted for quick open in `workbench.rs`
- `EditorView::set_language`, `syntax()`, `is_loading_language()` and `model()` (fe-gpui), and
  `fe_syntax::language_names()`
- `EditorModel::selections`/`primary_selection`/`tab_size`, `Buffer::line_ending` and `Text`
- T68's command registry, and T117's menu table (`crates/fe-keymap/menus/default-macos.toml`)
- T116's `fe_ui::ROW_HEIGHT` (24 px, in `theme.rs`) and the existing `UiTheme` tokens

## Problem

Nothing on screen says where the caret is, what language a file is highlighted as, or how it will be
saved (line endings). There's no way to change a buffer's language by hand, though
`EditorView::set_language` exists for it. Later cards (M4's navigation source, LSP diagnostics) need
somewhere to show a short status, and a host embedding the workbench needs to add its own.

## Change

### Data sources (what each item shows, and from where)

Every built-in item is computed from the active editor when the bar renders. Nothing new is tracked.

| Item | Text | Source |
|---|---|---|
| Cursor | `Ln 12, Col 5` | The primary selection's head (`primary_selection()`: the end, or the start if `reversed`). The line is the 1-based row. The column is 1-based, counted in **chars** (Unicode scalar values) from the row's start; a tab counts as one. It uses a new `Text::offset_to_char` (below), so it costs O(log n) and never scans the row. |
| Selections | ` (17 selected)` / ` (3 selections)` | Appended to the cursor text: with one non-empty selection, its length in chars; with more than one selection, their count. Nothing for one empty caret. |
| Indentation | `Spaces: 4` | `model().tab_size()`. `EditorModel::tab` always inserts spaces, so there is no "Tabs" variant today. |
| Encoding | `UTF-8`, or `UTF-8 with BOM` | The loader always decodes UTF-8. "with BOM" when `text().char_at(0) == Some('\u{feff}')`. |
| Line endings | `LF` / `CRLF` | `model().buffer().line_ending()`, which the loader records. |
| Language | `Rust`, `Plain Text`, `…` | `syntax().map(|s| s.language().name())`; `Plain Text` without syntax; `…` while `is_loading_language()`. Ids map to labels through a small table in `status_bar.rs` (`cpp` → `C++`, `csharp` → `C#`, `tsx` → `TSX`, …), falling back to the id. Clicking it opens the language picker. |

Right side, left to right: Cursor(+Selections), Indentation, Encoding, Line endings, Language (VS Code's
order). The left side has no built-in items yet (see Deferred). With no active editor (no tabs, a file
loading, a read error), no built-in items show; host items still do.

### `Text::offset_to_char`

`crates/fe-text/src/lib.rs`: `pub fn offset_to_char(&self, offset: usize) -> usize`, the number of chars
before `offset` (clamped to `len()`), from ropey's char metric (`metric_chars` is already enabled in the
workspace `Cargo.toml`). One method and its doc comment; nothing else in fe-text changes.

### The element: `crates/fe-ui/src/status_bar.rs`

`StatusBarFrame`, a stateless `RenderOnce`: a full-width row `ROW_HEIGHT` tall, `panel_background`, a
1 px top `border`, text in `text`. It takes left and right children. `StatusEntry`, a `RenderOnce` for one
item: a label with horizontal padding, and when it has a click handler, `row_hover` on hover and a pointer
cursor. Only existing `UiTheme` tokens are used; `theme.rs` doesn't change.
`crates/fe-ui/src/lib.rs` gets `mod` and `pub use` lines. fe-ui stays GPUI-only.

### The view: `crates/fe-workbench/src/status_bar.rs`

`StatusBar`, an entity the workbench owns:

- **Items are data.** `pub struct StatusItem { id: SharedString, side: StatusSide, priority: i32, text:
  SharedString, command: Option<fe_command::Invocation> }`, `pub enum StatusSide { Left, Right }`.
  Items sort by side, then `priority`, then insertion order. Built-ins are `StatusItem`s too, built each
  render, with documented ids (`cursor`, `indent`, `encoding`, `line_ending`, `language`) and priorities.
- **Host API.** `set_item(StatusItem)` adds an item, or replaces the one with its id; a host item with a
  built-in's id replaces that built-in. `remove_item(id)`. Both notify the bar only.
- **Clicks.** Clicking an item with a `command` dispatches `RunCommand(invocation)` on the window, so it
  takes the focused element's path to the workbench like a key would. The language item's command is
  `language.select`.
- **Following the editor.** `set_editor(Option<&Entity<EditorView>>, cx)`, same shape as
  `OutlinePanel::set_editor`: a no-op for the same editor, otherwise it observes the new one (dropping
  the old subscription) and notifies. The workbench calls it from `follow_active_editor`, next to the
  outline, so it follows the active pane's active tab.
- **Cheap per keystroke.** A caret move notifies the active editor; the bar's observation of that editor
  notifies the bar. The bar adds no workbench notification, and its render reads the editor directly
  (all lookups O(log n), plus O(selections) for the count). Embed it in the workbench with
  `Entity::cached` at its fixed `ROW_HEIGHT` and full width, so frames the workbench redraws for other
  reasons reuse the bar's last render.

### The language picker: `crates/fe-workbench/src/language_picker.rs`

`LanguagePicker`, a `PickerDelegate`:

- Rows: `Plain Text`, then every `fe_syntax::language_names()` entry by label, A–Z. The query filters by
  case-insensitive substring on the label or the id. The current language's row has a muted `current`
  suffix.
- It's opened for an editor (held weakly). Confirming calls `set_language(Some(id))`, or `None` for
  Plain Text, on that editor, then focuses it. Dismissing focuses it again. If the editor was closed
  meanwhile, confirming only dismisses the picker.

### `crates/fe-workbench/src/workbench.rs`

Changes are limited to:

- owning `status_bar: Entity<StatusBar>`, `status_bar_visible: bool` (default true) and
  `language_picker: Entity<Picker<LanguagePicker>>`;
- `pub fn status_bar(&self) -> &Entity<StatusBar>` and `pub fn set_status_bar_visible(&mut self, bool,
  cx)` / `status_bar_visible()`. A host that doesn't want the bar hides it with the setter;
- turning the quick-open-only `palette: bool` into "which palette is open" (none, quick open or
  language), with the language picker's dismiss and blur handled as quick open's are. One palette at a
  time: opening one dismisses the other;
- the render: the root becomes a column — the existing row (panes, dock and whatever T120 put beside
  them) as `flex_1`, then the cached status bar when visible. Palettes still overlay the whole window;
- `follow_active_editor` also calls `StatusBar::set_editor`;
- handling the two commands below in `on_run_command`.

### Commands

New registry in `status_bar.rs`, registered the way `dock.rs` registers its own (handlers only name an
op that the workbench does). `crates/fe-workbench/src/lib.rs` gets `mod` lines, `pub use StatusBar,
StatusItem, StatusSide`, one line in `init`, and the registry in `command_catalog()`'s chain. Don't add
`KeyBinding::new` or `actions!`.

| ID | Title | Default binding | Context |
|---|---|---|---|
| `view.toggle_status_bar` | Toggle Status Bar | none | `workbench` |
| `language.select` | Change Language Mode… | none | `workbench`; opens the language picker for the active editor; nothing without one |

No default bindings, so `crates/fe-keymap/keymaps/default-macos.toml` and the pinned counts in
`crates/fe-gpui/src/command_tests.rs` / `keymap_tests.rs` don't change.

`crates/fe-keymap/menus/default-macos.toml`: the View menu gets **Status Bar** (`view.toggle_status_bar`)
after Toggle Dock, in that group. Nothing else in the menu table changes.

### Tests

New file `crates/fe-workbench/src/tests/status_bar.rs`, declared in `tests/mod.rs`. Headless GPUI as
in T63/T78's tests, fixtures in a `tempfile` dir, clicks at laid-out bounds. Test-only hooks on
`StatusBar`: the texts of the items drawn last frame (by side), each item's laid-out bounds, and a render
counter. New file `crates/fe-text/src/tests/char_offsets.rs`, declared in `crates/fe-text/src/tests/mod.rs`.

## Deferred (not in this card)

- **Navigation source ("by search", later "LSP").** fe-nav doesn't exist yet (T103 is active) and T107,
  which wires the `NavigationProvider` into the workbench, lands after T81/T82. T107 can add a left-side
  `StatusItem` through `StatusBar::set_item`; no slot code is needed here.
- **Diagnostics counts.** Reserved for M4's LSP half: a left-side item a diagnostics provider will set.
  Nothing is built for it here.
- **Invalid UTF-8.** The loader replaces invalid bytes with U+FFFD silently and records nothing, and a
  U+FFFD in the text can't be told from a literal one. Showing "UTF-8 (lossy)" needs the loader to
  report replacements: a follow-up in fe-text/fe-buffer.
- **Other encodings, and converting encoding or line endings** from the bar. `Buffer::set_line_ending`
  exists, but changing it doesn't mark the buffer dirty; that's a follow-up.
- **Hard tabs and indentation detection.** No setting or detection exists; the item shows the tab size.
- **Tooltips** on items (T120 adds `fe_ui::tooltip`), go-to-line from the cursor item, and icons.
- Caching the rest of the workbench's views. The pane still notifies the workbench on every edit to
  retitle the window; this card only keeps the bar from adding to that.

## Decided (lead defaults; the user may revisit)

- **Columns count chars**, not graphemes, bytes or tab-expanded columns: O(log n) through ropey, and
  what Zed shows. A decomposed `é` counts 2; a tab counts 1.
- **VS Code's order and wording**: `Ln 12, Col 5`, `Spaces: 4`, `UTF-8`, `LF`, language last.
- **No default key** for either command; the bar toggles from the View menu or the command.
- The bar is `ROW_HEIGHT` (24 px) tall and spans the whole window width under the panes, dock and rail.
