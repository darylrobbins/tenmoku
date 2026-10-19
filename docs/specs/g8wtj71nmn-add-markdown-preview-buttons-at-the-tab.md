---
task_uid: "01m5aq6rkqpqgs3gg8wtj71nmn"
task: "T1007"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-ui test every_icon_is_bundled_and_parses (src/tests/assets.rs) and bundled_files_match_the_enum pass with Icon::ALL.len() == 59: Icon::Eye, Icon::Code and Icon::PanelRightOpen are bundled eye.svg, code.svg and panel-right-open.svg, byte-identical to Lucide 1.52.0 (500620a2e8123f8d1db191538886dc0c223f69a9), and kiln/THIRD_PARTY.md lists all three and the right SVG count."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test markdown_preview_buttons_tests::buttons_show_only_for_markdown (new file): with README.md active the pane-actions strip draws markdown-toggle then markdown-side, both left of split-right (bounds ordered, same row), with tooltips 'Open Preview' and 'Open Preview to the Side' and Icon::Eye / Icon::PanelRightOpen; with main.rs, a FileViewer and a diff tab active neither id has bounds and the strip is split-right, split-down, pane-menu; switching tabs between the two updates the strip on the next draw; an untitled buffer switched to Markdown by the language picker gains the buttons."
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test markdown_preview_buttons_tests::toggle_swaps_the_body_in_the_same_tab: clicking markdown-toggle leaves tabs().len() and the active index unchanged, the tab's title, path and dirty flag are the file's (no 'Preview' prefix), the pane body is the MarkdownPreview and active_editor still returns the file's editor; the button now has Icon::Code and tooltip 'Show Source', and markdown-side is gone; clicking again shows the same EditorView entity with the caret, selection, scroll offset and undo history it had, and focus is in the editor; a source edit made while the preview shows re-renders it after DELAY."
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test markdown_preview_buttons_tests::side_button_opens_the_split_preview: clicking markdown-side does what markdown.show_preview_to_side does (preview in the pane to the right, split off if none, focus left in the source); a second click reuses that preview rather than adding another; the source tab is still showing its editor, and the existing markdown_preview_tests and markdown_scroll_sync_tests pass unedited."
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test markdown_preview_buttons_tests::the_command_toggles_and_in_place_state_is_not_persisted: markdown.toggle_preview is a registered typed command titled 'Markdown: Toggle Preview' with no default key binding (fe-keymap markdown_preview_bindings test unchanged and passing), toggles the active Markdown tab from the palette and with the preview focused, is a no-op on a non-Markdown tab, closing a dirty tab shown as preview asks to save as for any dirty tab, and a layout serialized while the preview shows restores that tab as the editor."
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p kiln-ui -p fe-workbench -p fe-keymap passes with no existing test edited except the 56 -\u003e 59 count in kiln-ui src/tests/assets.rs, and cargo clippy --workspace --all-targets and cargo fmt --check are clean."
    required: true
    manual: false
---

# Add Markdown preview buttons at the tab bar's end: toggle the tab between source and preview in place, and open the preview to the side

## Problem

The user, 2026-10-19: "Let's add icon buttons on the tab bar for markdown preview: one that toggles between view preview and view source. It uses the same tab instead of opening another and then another to open a preview split. They should only show up when it's a markdown file. They should be beside the current split icon buttons now. Similar to the VS Code model for previews."

Today (T848, with scroll sync from T849 / helium T224):

- `markdown.show_preview` (cmd-shift-v) opens the preview as a **separate tab** after the source's; `markdown.show_preview_to_side` (cmd-k v) opens it in the pane to the right, splitting one off if none. Both are `#[kiln::command]` methods in `workbench_commands.rs` that call `Workbench::open_markdown_preview` in `markdown_preview.rs`. Neither has a button; a user must know the keys or use the palette.
- `MarkdownPreview` (`markdown_preview.rs`) is a `kiln_shell::PaneItem` holding a `WeakEntity<EditorView>` for its source, so it cannot outlive the editor's tab.
- A pane's tab bar ends in the `pane-actions` strip (`Pane::tab_row`, `pane.rs`): `split-right` (`Icon::Columns2`), `split-down` (`Icon::Rows2`) and `pane-menu` (`Icon::Ellipsis`), built by `Pane::button(id, icon, title, command)` with a tooltip showing the command's key. T127 added them.
- `Pane::render` draws `tabs[active].item()` as the body.

## Model: VS Code

- In a Markdown editor's title bar VS Code shows **Open Preview to the Side** (`markdown.showPreviewToSide`, a split-with-magnifier icon). It opens the preview in a split beside the source and leaves focus in the source.
- Toggling the same tab: `markdown.showPreview` (cmd-shift-v) shows the preview where the source was. In the preview, the title-bar button is **Show Source** (`markdown.showSource`, same cmd-shift-v), which puts the editor back in that tab.
- The buttons are shown only for Markdown. In a preview, Show Source is the button.

## Change

Two icon buttons at the end of the `pane-actions` strip, **before** `split-right`, drawn only while the pane's active tab is a Markdown file (an editor for which `is_markdown(..)` is true, or that tab while showing its preview):

1. **Preview / source toggle**, id `markdown-toggle`.
   - Source showing: icon `Icon::Eye`, tooltip "Open Preview", runs the new command `markdown.toggle_preview`.
   - Preview showing: icon `Icon::Code`, tooltip "Show Source", runs the same command.
   - The tab stays one tab: same position, same title and path (the file's name, not "Preview ..."), same dirty marker, same close and save behaviour. Only the pane body changes between the editor and the rendered `MarkdownPreview`. The editor entity must stay alive while its preview is shown (the preview's source is weak), so edits, undo state, caret, scroll and vim state are all there when the user toggles back.
   - Showing source puts focus in the editor; showing the preview focuses the preview.
2. **Preview to the side**, id `markdown-side`, icon `Icon::PanelRightOpen`, tooltip "Open Preview to the Side", runs the existing `markdown.show_preview_to_side`. Shown while the source is showing; in preview mode it is hidden (the tab already is the preview).

Order in the strip, Markdown source showing: `markdown-toggle`, `markdown-side`, `split-right`, `split-down`, `pane-menu`. Preview showing: `markdown-toggle`, then the three standard buttons. Any other tab (non-Markdown editor, viewer, diff, preview tab of the existing separate-tab kind): the strip is exactly today's three buttons.

Implementation guidance (the card author's reading, change if the code says otherwise):

- The toggle state belongs to the tab, not the editor: e.g. `Tab` (in `pane.rs`) gains an optional `Entity<MarkdownPreview>` that `Pane::render` draws instead of the editor, with the editor still the tab's item. Everything keyed on the tab's item (`path`, `active_editor`, save, close prompts, tab restore, `preview_of`) keeps seeing the editor. A preview shown in place is not serialized; the tab restores as the editor.
- New command `markdown.toggle_preview`, title "Markdown: Toggle Preview", context `editor`, **no default key binding** (cmd-shift-v keeps meaning `markdown.show_preview`; see the open question). It is a no-op with a log line on a non-Markdown tab, as `open_markdown_preview` is. It must also be reachable while the preview has focus, so the new command's context must cover the in-place preview (use a context the in-place preview carries, and say which in the test).
- Icons: add Lucide `eye`, `code` and `panel-right-open`, copied unmodified from the pinned Lucide 1.52.0 (`500620a2e8123f8d1db191538886dc0c223f69a9`) into `kiln/crates/kiln-ui/assets/icons/`, as `Icon::Eye`, `Icon::Code`, `Icon::PanelRightOpen` in the `icons!` table, and list them in `kiln/THIRD_PARTY.md` (the file list and its "40 SVGs" count; check the count is still right). `Icon::ALL.len()` goes 56 -> 59 in `kiln/crates/kiln-ui/src/tests/assets.rs`. If the Lucide files cannot be fetched, stop and report blocked; do not draw replacements.
- Text edits in the source while the preview shows in place still re-render the preview (it observes the editor), and the scroll sync keeps working for the side preview.
- A pane's `Pane::tab_row` is also where the buttons' test hooks go: give both buttons `debug_selector`s (`markdown-toggle`, `markdown-side`) so tests can find their bounds, and use the existing `last_tooltip` capture.

Do not change `markdown.show_preview`, `markdown.show_preview_to_side`, their bindings, or any existing test (`markdown_preview_tests`, `markdown_scroll_sync_tests`, `tests/pane_toolbar.rs`, `fe-keymap/tests/markdown_preview_bindings.rs`).

## Implementation notes

- `Tab` (pane.rs) holds `preview: Option<Entity<MarkdownPreview>>`. The
  editor stays the tab's item, so title, path, dirty marker, save, close
  prompts and saved layouts are the editor's; the preview isn't an item and
  isn't serialized. `Pane::render` draws the preview in a div carrying the
  `markdown_preview` key context; `PaneView::shown_item` (focus on pane
  activation, `focus_target`, a split's copy) is the preview while it shows,
  and `activated` refocuses it over kiln-shell's focus of the item.
- `markdown.toggle_preview` has context `editor || markdown_preview`, so the
  palette lists it in the editor and in the in-place preview. `invoked_in`
  now parses its context as a `ContextPredicate` (every other caller passes
  a bare identifier, which parses the same). The buttons dispatch their
  commands invoked in `workbench` + `editor` (or `markdown_preview` for Show
  Source), since `pane_toolbar::dispatch` only gives `workbench`, which the
  editor commands' handlers refuse.
- A reveal applied to a tab showing its preview (go to line, a link, a nav
  place) shows the editor again, so the place is visible.
- A link in the in-place preview opens in its pane (`open_preview_link`
  looks at in-place previews too).
- `kiln/THIRD_PARTY.md`'s file list was stale (41 of 56 listed): it now
  lists all 59 SVGs, and the Feather-derived lists add `code`, `hash`,
  `link`, `type` (in Lucide's Feather list) and `box`, `eye`, `list`,
  `table` (colebemis a contributor at the pinned commit). The host
  repository's `THIRD_PARTY.md` carries the same row but is outside this
  card's scope; it still needs the same update.
- The buttons' debug selectors are `icon-button:markdown-toggle` and
  `icon-button:markdown-side`: `IconButton` names its selector from its id,
  as the split buttons' are `icon-button:split-right` and so on.
