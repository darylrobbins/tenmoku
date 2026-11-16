---
task_uid: "01m7j21t7z15th162t88je7ksp"
task: "T1897"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-gpui tests view::context_menu::tests::right_click_outside_the_selection_moves_the_caret_and_opens_the_menu, right_click_inside_a_selection_keeps_it, no_provider_opens_no_menu, the_line_number_column_keeps_the_breakpoint_menu, arrows_pass_separators_and_disabled_items, a_disabled_item_does_not_run, a_chosen_item_runs_on_the_editor_after_focus_returns (RunCommand with Source::Menu), scroll_edit_and_save_as_close_it, extend_inserts_after_its_anchor_and_keeps_the_highlight and a_stale_ticket_extends_nothing pass"
    required: true
    manual: false
  - id: "C2"
    text: "Focus (as T1888's): view::context_menu::tests::the_menu_takes_its_own_focus_and_no_input_handler (the menu's own FocusHandle with key context editor_menu carrying HOLDS_KEYS is focused, the editor's is not, no input handler registered), opening_mid_composition_settles_it (marked_text_range None, text unchanged, at open not on_blur), escape_choose_and_outside_press_give_focus_back and vim_keys_do_not_reach_the_buffer_while_open (vim on: j, x, d, i leave text and caret alone while open; x deletes after Escape) pass"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-ui tests tests::menu_panel::disabled_rows_take_no_click_or_hover and item_nodes_are_menu_items_with_their_disabled_state pass; MenuEntry is unchanged and cargo nextest run -p kiln-ui passes"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench tests tests::editor_menu::lists_groups_in_order_with_their_shortcuts, read_only_editor_leaves_out_cut_and_paste, untitled_buffer_leaves_out_the_path_items, lsp_items_are_disabled_until_a_server_is_ready, lsp_items_follow_the_servers_capabilities, plain_text_has_no_lsp_items, format_selection_needs_a_selection, go_to_definition_from_the_menu_jumps, right_click_in_an_inactive_split_acts_on_it and other_editors_get_only_the_clipboard pass"
    required: true
    manual: false
  - id: "C5"
    text: "Unchanged and passing: fe-gpui view::breakpoints::tests, mouse_binding_tests, ime_tests and keymap_tests; fe-workbench tests::project_menu and lsp_lightbulb tests; cargo nextest run -p fe-workbench --test l10n and -p fe-gpui --test l10n pass"
    required: true
    manual: false
  - id: "C6"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden unchanged (the editor_menu audit scene and Copy Reference moved to T1898, so this card doesn't wait on T1852's audit_scenes.rs and command test tables)"
    required: true
    manual: false
---

# Editor context menu (1/2): right-click menu in the text with Cut/Copy/Paste, Copy Path/Reference, Go to Definition/Implementation, Peek, Find References, Rename and Format, with its own focus

# Editor context menu (1 of 2): the menu, clipboard, path and LSP navigation items

## Why

The user, on finding that a right-click in the editor does nothing: "We have no
context menu for the editor?" and, on the proposed list: "Yes please, let's
prioritize this one. Seems like a big gap."

Today the only right-click the editor handles is the breakpoint gutter's
(`crates/fe-gpui/src/view/breakpoints.rs` `on_right_down`, wired in
`crates/fe-gpui/src/view.rs` render as `.on_mouse_down(MouseButton::Right, ..)`).
A right-click in the text does nothing. This card adds the menu and its first
items. Card 2 (depends on this one) adds Quick Fix / Code Actions, Run Test,
Toggle Breakpoint, Git Blame and Ask an Agent, through the API built here, with
no further fe-gpui changes.

## What the user sees

A right-click in an editor's text area (and on macOS a ctrl-click, which GPUI's
macOS platform already turns into a right mouse-down with `control` cleared, in
`gpui-pre-macos` `window.rs`: no editor code is needed for it) opens a menu at
the pointer:

1. **Go to Definition** (`nav.goto_definition`), **Peek Definition**
   (`editor.peek_definition`), **Go to Implementation**
   (`nav.goto_implementation`), **Find References** (`nav.find_references`),
   **Peek References** (`editor.peek_references`)
2. **Rename Symbol** (`lsp.rename`), **Format Document**
   (`lsp.format_document`), **Format Selection** (`lsp.format_selection`)
3. **Cut** (`editor.cut`), **Copy** (`editor.copy`), **Paste** (`editor.paste`)
4. **Copy Path** (`workbench.copy_active_file_path`), **Copy Reference** (new
   `workbench.copy_reference`)

Groups are separated; an empty group and its separator are left out (the tree
menu's `kept` rule in `crates/fe-workbench/src/project_panel_menu.rs`).

Each item shows its shortcut, looked up as the tree menu does
(`crate::pane_toolbar::menu_shortcut(command, &stack, cx)`), in the editor's
own context stack as it was at the right-click (so vim's frames and the
editor's flags count), not the menu's.

Where an item applies:

- **LSP group (1, 2):** shown when the buffer's language has a configured
  server; **disabled** (drawn muted, not clickable, skipped by the arrows)
  while no server the buffer is on has initialized (`manager.servers_of` /
  `server_of` with `capabilities(server)` still `None`); once one has, an item
  whose provider no such server offers is **left out** (`definitionProvider`,
  `implementationProvider`, `referencesProvider`, `renameProvider`,
  `documentFormattingProvider`, `documentRangeFormattingProvider`). Read through
  `servers_of`, not only the lead, so T1739's multi-server routing doesn't
  change the answer. Format Document also shows (enabled) when a plugin
  formatter would format the buffer, as `lsp.format_document` would pick one.
  Format Selection only with a non-empty primary selection. A buffer with no
  configured server (plain text) shows none of group 1 or 2.
- **Cut / Paste:** left out in a read-only editor (`EditorView::is_read_only`
  or not accepting text). Copy always.
- **Copy Path / Copy Reference:** only for a buffer with a path.
- **Copy Reference:** puts `path:line` on the clipboard, path as
  `workbench.copy_active_file_relative_path` gives it (relative to the window's
  folder with `/`, absolute outside it), line 1-based from the primary
  selection's head; a selection over several rows gives `path:start-end`.

The caret: a right-click outside every selection moves the caret there first
(one caret, as a plain left click places it, vim visual mode ended as a click
ends it); a right-click inside a selection keeps all selections. A right-click
in an editor that isn't the active one makes it active first, so every item
(Copy Path included) acts on the clicked editor.

Keys while open: Up/Down (wrapping, passing separators and disabled items),
Home/End, Enter runs the highlighted item, Escape closes. Nothing typed reaches
the buffer, in vim mode or not; vim keeps working after the menu closes.
Pointer: hover highlights, a click runs the item, a press outside closes.
Scrolling the editor, an edit (text version change), a path change (Save As)
or the tab going away closes it (a stale menu runs nothing, as the breakpoint
menu's `menu_is_current`).

Right-click on the line-number column with breakpoint handlers set still opens
the breakpoint menu, unchanged. Right-clicks on lens rows, the minimap, the
scrollbar and popovers open nothing new.

## Design

### fe-gpui: draws, owns focus and keys, dispatches (new files)

`crates/fe-gpui/src/view/context_menu.rs` (new, `pub(crate) mod context_menu;`
in `view.rs`, types re-exported from `lib.rs`):

- `EditorMenuItem { title: SharedString, shortcut: Option<SharedString>,
  invocation: Invocation, enabled: bool }`; entries are
  `Vec<Option<EditorMenuItem>>`, `None` a separator.
- `EditorMenuRequest { editor: WeakEntity<EditorView>, ticket: EditorMenuTicket,
  offset: usize, row: u32, inside_selection: bool, read_only: bool,
  contexts: ContextStack }` (`contexts` from the view's `context_stack`, taken
  before focus moves).
- An app-wide provider, a `Global`: `fe_gpui::set_editor_menu_provider(cx,
  Rc<dyn Fn(&EditorMenuRequest, &mut Window, &mut App) -> Vec<Option<EditorMenuItem>>>)`.
  No provider, or an empty answer, opens no menu (embedders get nothing until
  they register one). The provider is asked from `window.defer` once the view
  is done with the press, as the breakpoint menu asks its host.
- `EditorView::extend_editor_menu(ticket, after: &str, items, cx)`: inserts
  `items` into the open menu right after the item whose invocation runs command
  `after`, or as a new last group if there's none, if `ticket` is still the
  open menu's (ignored otherwise, and after a close). The highlight stays on
  the item it was on. For card 2's code actions, which arrive after the menu
  opens.
- The right-press entry point the view's render wires instead of the
  breakpoint one: on the line-number column with breakpoint handlers
  (`breakpoint_row_at`, `has_breakpoint_handlers`), it calls the breakpoint
  `on_right_down` as today; in the text area it places the caret (or not) and
  asks the provider. Opening either menu closes the other.
- Drawn with kiln-ui's shared `MenuPanel` (`fe_ui::MenuPanel`/`MenuEntry`,
  `kiln/crates/kiln-ui/src/menu_bar.rs`), the component the project tree menu,
  its New menu, the git branch and git log menus use, in a
  `deferred(anchored().position(..).snap_to_window())` layer with
  `on_mouse_down_out` closing, wrapped as `fe_ui::Role::Menu` named
  "Editor menu" (`crates/fe-gpui/l10n/en-US/fe-gpui.ftl`), as
  `project_panel_menu.rs::render_menu` does.
- Choosing an item: closes the menu, gives focus back to the editor, then
  dispatches `RunCommand(invocation.source(Source::Menu))` on the editor's
  focus handle from `window.defer`, as `press_lens` dispatches a lens's.

### Focus: the menu's own, from the start (consistent with T1888)

`crates/fe-gpui/src/view/menu_focus.rs` (new): a small reusable piece the
editor menu uses and T1888's breakpoint menu can adopt later. While the menu is
open, its own `FocusHandle` (tracked by the menu's element, with its own key
context `editor_menu` carrying `kiln_platform::keys::HOLDS_KEYS`, and its own
`on_key_down`) has focus; the editor's handle does not, and the menu registers
no input handler, so an input method can neither commit into the buffer nor
take the arrows. Opening settles any composition explicitly
(`marked_text_range` becomes `None`, buffer text unchanged), not left to
`on_blur`; if `view/ime.rs` has no such entry point, add a `pub(crate)` one
there. Escape, choosing an item (focus restored before the item's deferred
run), a mouse-down outside and a stale close give focus back to the editor; a
close without a window gives it back at the next render. Focus leaving the menu
for elsewhere (another window) closes it.

This card does not change `breakpoints.rs`: T1888 (ready, waiting on T1885,
lent to helium) owns that menu's focus. Both cards touch `view.rs` and
`view/ime.rs` at different sites; whichever lands second rebases.

### kiln-ui: disabled rows in MenuPanel

`MenuPanel` has no disabled state today. Add, without changing `MenuEntry` (14
match sites): `MenuPanel::disabled(impl IntoIterator<Item = usize>)`, whose rows
are drawn in the muted text colour, take no click and no hover callback; and
`MenuPanel::item_nodes()`, opt-in, registering each item row as a
`Role::MenuItem` a11y node named by its title (`fe_ui::node_with`, the disabled
state on disabled rows). The editor menu uses both; existing panels are
unchanged. Tests in `kiln/crates/kiln-ui/src/tests/menu_panel.rs` (new).

### fe-workbench: supplies the items (new file)

`crates/fe-workbench/src/editor_menu.rs` (new, `mod editor_menu;` and its
`init` registering the provider in `crates/fe-workbench/src/lib.rs`): finds the
window's `Workbench`; for an editor that is one of its pane editors, builds the
groups above from the request (LSP readiness and capabilities from
`self.lsp.manager`, path from the model, read-only from the request); for other
editors in the window (diff sides, the peek's editor) offers Cut/Copy/Paste
only. Titles are menu strings in `crates/fe-workbench/l10n/en-US/editor.ftl`
("Go to Definition", "Rename Symbol", "Copy Path", ...), not the commands'
palette labels with their category prefixes; shortcuts from
`pane_toolbar::menu_shortcut` with `request.contexts`.

`workbench.copy_reference` ("Copy Reference") is declared in
`crates/fe-workbench/src/path_commands.rs` beside the other active-file path
commands and handled by its `init` listeners; its title in
`crates/fe-workbench/l10n/en-US/commands.ftl`; its rows in
`typed_command_ids_tests.rs` and `command_catalog_tests.rs` as every new
command has; no default key.

### Audit scene (Mac)

`crates/fe-workbench/examples/screenshots/audit/editor_menu.rs` (new,
registered in `audit_scenes.rs` as `project_tree_menu` is): a Rust file with a
fake ready server and a selection, the menu opened by a real right-click
through deferred `dispatch_event` (T1750's way), probes on the menu's role,
its items' titles and shortcuts, one disabled item in a second shot with the
server starting.

## Out of scope

- Card 2: Quick Fix / Code Actions, Run Test, Toggle Breakpoint, Git Blame,
  Ask an Agent.
- Debug Test: no command exists yet; T1856 adds `debug.test_at_cursor`.
- The breakpoint menu's focus (T1888).


## Lead respec (2026-11-16)
To start now beside T1852 (the user made this the priority), Copy Reference (workbench.copy_reference, path_commands.rs and the command-id test tables) and the editor_menu UI audit scene moved to T1898. This card adds no new command. Shared append-only files (crate lib.rs, l10n.rs, l10n-allow.toml, src/tests/mod.rs, kiln-ui tests/mod.rs) may gain lines without being in scope (regatta.toml [scopes] shared_append_only).

## Implementation notes

- **fe-gpui.** `view/context_menu.rs` holds the menu (`EditorMenuItem`,
  `EditorMenuRequest`, `EditorMenuTicket`, `set_editor_menu_provider`,
  `EditorView::extend_editor_menu`, `close_editor_menu`), wired in `view.rs`
  as the right press (`on_right_press`, which hands the line-number column to
  the breakpoint `on_right_down` unchanged), a layer beside the breakpoint
  menu's, a wheel scroll closing it, and `Press::Right` so held vim keys
  settle before the press acts, as for the left and middle buttons.
  `view/menu_focus.rs` is the reusable focus piece (`MenuFocus::take`,
  `give_back`, `give_back_later`, `key_context`).
- **ime.rs.** `EditorView::cancel_composition` was already the `pub(crate)`
  entry point that drops a composition without typing; opening calls it (at
  the press and again at open). Review r1 (F1): until the menu's first frame
  the platform still holds the editor's input handler, so `takes_input`
  refuses every input method call while the menu is open
  (`input_method_calls_before_the_first_frame_do_nothing`).
- **"Not accepting text".** Cut and Paste are left out when the view is
  read-only (`EditorView::is_read_only`, which `request.read_only` carries).
  fe-gpui's `accepts_text` is false in vim's normal and visual modes, where
  `editor.cut` and `editor.paste` still run, so it doesn't drop them.
- **Not a text press:** the gutter (without breakpoint handlers too), lens
  rows, the minimap, sticky scroll's band, and whatever covers the text (its
  hitbox isn't hovered there). With no provider registered a right press
  does nothing at all, the caret included.
- **The highlight** starts on no item, as the tree menu's; Down from none
  goes to the first item, Up to the last.
- **Context of a run.** An item whose invocation names no context runs in the
  editor's context stack taken at the press (`request.contexts`), so
  commands the workbench checks with `invoked_in("editor")`
  (`editor.peek_definition`, …) apply.
- **No input handler:** GPUI gives no way to read the window's registered
  input handler from a test. The view's element registers one only for the
  editor's own focused handle and the menu registers none, so
  `the_menu_takes_its_own_focus_and_no_input_handler` asserts the editor's
  handle isn't focused, the menu's is (with `editor_menu` and `holds_keys`
  in its context), and typed text reaches nothing.
- **Closing on scroll** (review r1, F3): the wheel over the editor closes it
  at once (`on_scroll`); any other scroll (a diff's other side following its
  own, a host's `set_scroll_top`) makes it stale at the next render, against
  the scroll recorded after the first frame painted the open menu, so the
  autoscroll revealing the caret the press put doesn't close it
  (`a_linked_diff_scroll_closes_it`).
- **Review r1, F2:** focus leaving the menu is told in the draw's focus
  phase, whose notify asks for no frame, so the close also defers a notify
  (`focus_moving_elsewhere_takes_the_menu_away`).
- **Review r1, F4, F5:** a press is inside a selection by half-open
  containment (`the_character_after_a_selection_is_outside_it`), and the
  request's offset and row are the clicked place anchored before the caret
  moves and resolved after, through any edit the move makes
  (`the_request_follows_an_edit_the_caret_move_makes`).
