---
task_uid: "01m490bxnbrt2qx54gybef7jq7"
task: "T117"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-keymap test tests/menu.rs::default_menus_parse: DEFAULT_MACOS_MENUS parses with no diagnostics into menus named exactly fastedit, File, Edit, View, Go, Window; Edit's Undo, Redo, Cut, Copy, Paste and Select All carry os actions undo, redo, cut, copy, paste and select_all; the app menu holds a Services system item; item ids are unique within each menu"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap test tests/menu.rs::shortcuts_follow_the_keymap: with the default keymap in [workbench, editor], the item shortcuts of search.show, editor.save, workbench.next_tab and app.hide are cmd-shift-f, cmd-s, cmd-shift-] and cmd-h; pane.close's item has no shortcut while shortcuts() lists cmd-k cmd-w; a user layer binding editor.save to alt-cmd-s (context editor) makes alt-cmd-s its first shortcut; a user layer unbinding cmd-s in editor (command = \"\") leaves editor.save none; a user editor-context binding of cmd-b to editor.select_all hides dock.toggle's cmd-b in [workbench, editor]"
    required: true
    manual: false
  - id: "C3"
    text: "fe-keymap test tests/menu.rs::alternatives_follow_the_context: Edit \u003e Paste's invocation is text_input.paste in [workbench, search_panel, text_input] and editor.paste in [workbench, editor], each with source Menu and that stack as its context; an item's args reach the invocation unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap test tests/menu.rs::hosts_extend_and_replace: extending the default bar with a layer adds a Tools menu just before Window, inserts an item after dock.toggle in View, and removes search.show from Edit with the separator left trailing dropped; parsing with a catalog lacking outline.show drops that item with exactly one warning naming it; an unknown item field is an error and None; a host table parsed alone replaces the defaults"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test menu_tests::install_sets_menus_and_display_bindings: after menu::install with the default bar and keymap, cx.get_menus() has the six menus in order; View \u003e Search's MenuCommand has one GPUI binding, cmd-shift-f, in context fastedit_menu; chord-only items (Close Pane, Fold All) have none; every GPUI key binding is a MenuCommand in context fastedit_menu; fe_gpui::init alone still leaves GPUI's keymap empty"
    required: true
    manual: false
  - id: "C6"
    text: "fe-gpui test menu_tests::keys_still_dispatch_once: with menus installed, simulate_keystrokes(\"cmd-a\") in a focused editor records exactly one editor.select_all invocation, with source Keyboard"
    required: true
    manual: false
  - id: "C7"
    text: "fe-gpui test menu_tests::keymap_reload_refreshes_menus: after set_user_keymap with editor.save bound to alt-cmd-s and cmd-s unbound, File \u003e Save's display binding is alt-cmd-s and no binding has cmd-s; with no menus installed, set_user_keymap sets no menus and binds nothing in GPUI"
    required: true
    manual: false
  - id: "C8"
    text: "fe-gpui test menu_tests::choosing_an_item_runs_in_the_focused_context: with an EditorView focused inside a parent with key_context workbench, cx.dispatch_action of Edit \u003e Select All's MenuCommand selects all and records one editor.select_all invocation with source Menu and context [workbench, editor]; View \u003e Toggle Full Screen's flips window.is_fullscreen(); with no window open, it records window.toggle_fullscreen with source Menu through the app registry without panicking"
    required: true
    manual: false
  - id: "C9"
    text: "app.hide, app.hide_others, app.show_all, window.minimize, window.zoom, window.toggle_fullscreen and window.close are registered in fe-gpui's app registry and known to fe_gpui::commands::command_catalog(); default-macos.toml binds cmd-h, alt-cmd-h, cmd-m, ctrl-cmd-f and cmd-shift-w to them with no context; command_tests and keymap_tests pass with only their pinned counts and context-free list updated; rg 'KeyBinding::new|actions!' over the files this card adds or changes finds only the display-binding call in crates/fe-gpui/src/menu.rs"
    required: true
    manual: false
  - id: "C10"
    text: "fastedit test tests/menus.rs::default_menus_know_every_command: MenuBar::default_macos(Some(\u0026*fe_workbench::command_catalog())) returns a bar and no diagnostics; crates/fastedit/src/main.rs installs it after fe_workbench::init; scripts/check-no-gpui.sh still prints ok: fe-keymap; Cargo.toml and Cargo.lock do not change"
    required: true
    manual: false
---

# Add a native macOS menu bar from a host-replaceable menu table, with shortcuts from the active keymap

# Add a native macOS menu bar from a host-replaceable menu table, with shortcuts from the active keymap

Milestone M3 "Browse", after the splits and tree-keys cards. It builds on:

- the action-model task's command registry (T68): `Invocation`, `Source::Menu`, `ContextStack`
- fe-keymap: `Keymap::bindings_for` ("for menus and the palette"), `Matcher`, `KeymapLayer::parse`'s
  diagnostics, `CommandCatalog`, and `KeymapStore` hot reload
- fe-gpui: `RunCommand`, the key adapter in `keymap.rs` (`context_stack`, `dispatch`,
  `set_user_keymap`) and the app registry in `commands.rs`
- the commands of T63, T76, T77, T78 (`outline.show`), T79 (`pane.*`) and T80
  (`project_panel.reveal_active`)

## Problem

The standalone app sets no menu bar. Commands are reachable only by key, nothing shows which key runs
what, and the standard macOS app and window items (Hide, Minimize, Services) are missing.

fastedit is embeddable, so the menus can't be hard-coded in the standalone binary. A host must be able
to replace or extend them without GPUI code.

## What gpui-pre 0.3.7 offers (checked in its Apache-2.0 source)

- `App::set_menus(Vec<Menu>)`, with `Menu { name, items, disabled }` and
  `MenuItem::{action, os_action, separator, submenu, os_submenu(name, SystemMenuType::Services)}`.
  `OsAction` is `Cut | Copy | Paste | SelectAll | Undo | Redo`. The first menu is the app menu, and a
  menu named `Window` becomes AppKit's windows menu, which lists the open windows.
- **Shortcuts.** The macOS platform's `create_menu_item` reads GPUI's *own* keymap,
  `bindings_for_action(action)`, matching by `Action::partial_eq`. It sets a key equivalent only for a
  single-keystroke binding. fastedit's GPUI keymap is empty (fe-keymap matches keys through an
  interceptor), so no shortcut shows unless GPUI's keymap holds a binding for the item's action.
- **Enabled state.** AppKit validates each item through `validateMenuItem:`, which calls
  `App::is_action_available(action)`. That looks only at the action's type and global listeners, and it
  overrides the `disabled` flag given to `set_menus`. So enabling items from each command's context
  predicate **isn't feasible** with gpui-pre 0.3.7. Every item is enabled, and an item that doesn't
  apply where focus is runs nothing, as its key wouldn't.
- Each `set_menus` call appends its items' actions to the platform's `menu_actions` list, which is never
  cleared. Menus are therefore reinstalled only when the keymap changes, never per focus change.
- Choosing an item calls `App::dispatch_action`: the active window's focused path, then global
  listeners. A key equivalent the window didn't handle (GPUI returns `NO`) goes on to the main menu, so
  an item's shown shortcut can fire it when the keymap didn't match the key.
- The test platform keeps the menus for `App::get_menus()` and never calls back.

## Change

### The menu table: `fe_keymap::menu` (GPUI-free)

New file `crates/fe-keymap/src/menu.rs`. New data file `crates/fe-keymap/menus/default-macos.toml`,
exported from `crates/fe-keymap/src/lib.rs` as `DEFAULT_MACOS_MENUS` (`include_str!`), beside a `mod`
line and `pub use` lines.

It lives in fe-keymap, not a new crate, because resolving shortcuts needs `Keymap` and `Matcher`, and
loading reuses the keymap's TOML and `Diagnostic` plumbing. That also means no new crate and no
`Cargo.lock` change. It must stay GPUI-free (`scripts/check-no-gpui.sh`).

**Format:**

```toml
[[menus]]
name = "Edit"

[[menus.items]]
title = "Paste"
command = "editor.paste"
os_action = "paste"
alternatives = [{ command = "text_input.paste", when = "text_input" }]

[[menus.items]]
separator = true

[[menus.items]]
title = "Services"
system = "services"
```

**Item fields:**

- `title`, `command` and `args` (a table, as in keymaps)
- `id`: defaults to the command ID, and must be unique within its menu
- `os_action`: one of `cut`, `copy`, `paste`, `select_all`, `undo` or `redo`
- `alternatives`: a list of `{ command, args?, when }`, where `when` is a `ContextPredicate`
- `shortcut_context`: the frame names to resolve the shortcut in. The default is
  `["workbench", "editor"]`.
- `separator = true`, or `system = "services"`
- in a host's extension layer only: `before`, `after` and `remove`

Unknown fields are errors.

**API:**

- **Parsing.**
  `MenuBar::parse(path: &Path, source: &str, catalog: Option<&dyn CommandCatalog>) -> (Option<MenuBar>, Vec<Diagnostic>)`
  works like `KeymapLayer::parse`.
  - A syntax error or an unknown field gives `None` and an error.
  - With a catalog, an item or alternative whose command is unknown, or whose args don't check, is
    dropped with a warning. The rest loads.
- **The default.** `MenuBar::default_macos(catalog)` parses `DEFAULT_MACOS_MENUS`.
- **Extending.** `MenuBar::extend(&mut self, layer: MenuBar)` merges a host's layer:
  - A layer menu whose name exists merges into that menu:
    - an item with `before` or `after` set to an `id` is inserted there, and other items are appended
    - an item with `remove = true` removes the item with its `id`
  - A layer menu with a new name goes in before `Window`, or at the end if there's no `Window`.
  - Afterwards, leading, trailing and doubled separators are dropped, and so are menus left empty.
- **Replacing.** A host that wants none of the defaults parses its own table instead.
- **Shortcuts.**
  `shortcuts(keymap: &Keymap, command: &CommandId, args: &Value, stack: &ContextStack) -> Vec<KeySequence>`
  lists every key sequence that, pressed in `stack`, runs `command` with `args`.
  - It takes `keymap.bindings_for(command)` with equal args, and keeps each one only if a fresh
    `Matcher::new(keymap)`, fed its keys in `stack` (and flushed after a chord), dispatches exactly this
    command with these args.
  - Those left are ordered by precedence: the later layer first, then the later entry.
  - So a user binding moves a shortcut, and an unbinding (`command = ""`), or a deeper binding for the
    same keys, hides it.
  - `CommandItem::shortcut(&Keymap)` is the first single-keystroke entry of `shortcuts` in the item's
    `shortcut_context`. gpui-pre shows no chords.
  - The rail card reuses `shortcuts` for tooltips.
- **Invocations.** `CommandItem::invocation(&self, stack: &ContextStack) -> Invocation` uses the first
  alternative whose `when` holds in `stack`, or else the item's own command. It carries that command's
  args, `Source::Menu`, and `stack` as its context.

### The default menus

Only commands that exist are listed. Keys in parentheses are the default keymap's, and "—" is a
separator.

- **fastedit** (the app menu):
  - Hide fastedit `app.hide` (cmd-h), Hide Others `app.hide_others` (alt-cmd-h), Show All
    `app.show_all`
  - — Services (system) —
  - Quit fastedit `app.quit` (cmd-q)
- **File:**
  - Save `editor.save` (cmd-s)
  - — Close Tab `workbench.close_tab` (cmd-w)
  - Close Pane `pane.close` (a chord, so it shows no key)
  - Close Window `window.close` (cmd-shift-w)
- **Edit:**
  - Undo and Redo (os actions `undo` and `redo`)
  - — Cut, Copy, Paste and Select All (os actions). Paste has the alternative `text_input.paste` when
    `text_input` holds.
  - — Find in Files `search.show` (cmd-shift-f)
- **View:**
  - Project `dock.show_project`, Search `search.show`, Outline `outline.show`, Toggle Dock
    `dock.toggle`
  - — Soft Wrap `editor.toggle_soft_wrap`
  - — Fold, Unfold, Fold All and Unfold All
  - — Toggle Full Screen `window.toggle_fullscreen` (ctrl-cmd-f)
- **Go:**
  - Go to File… `quick_open.toggle` (cmd-p)
  - — Next Tab and Previous Tab
  - — Focus Left, Right, Above and Below Pane (`pane.focus_*`)
  - — Reveal Active File in Project `project_panel.reveal_active`
- **Window:**
  - Minimize `window.minimize` (cmd-m), Zoom `window.zoom`
  - — Split Right `pane.split_right`, Split Down `pane.split_down`
  - Move Tab Left, Right, Up and Down (`pane.move_tab_*`)

### New app and window commands

In `crates/fe-gpui/src/commands.rs`, these go in the app registry next to `app.quit`, with no context,
so fe-gpui's `command_catalog()` knows them. Window commands act on `cx.active_window()`, and do nothing
without one.

| ID | Title | Default binding |
|---|---|---|
| `app.hide` | Hide fastedit | `cmd-h` |
| `app.hide_others` | Hide Others | `alt-cmd-h` |
| `app.show_all` | Show All | none |
| `window.minimize` | Minimize | `cmd-m` |
| `window.zoom` | Zoom | none |
| `window.toggle_fullscreen` | Toggle Full Screen | `ctrl-cmd-f` |
| `window.close` | Close Window | `cmd-shift-w` |

The bindings go in `crates/fe-keymap/keymaps/default-macos.toml`, with no context. Update the pinned
counts and the context-free list in `crates/fe-gpui/src/command_tests.rs` and
`crates/fe-gpui/src/keymap_tests.rs`.

### The GPUI adapter

New file `crates/fe-gpui/src/menu.rs`. `crates/fe-gpui/src/lib.rs` gets `pub mod menu;`, a
`#[cfg(test)] mod menu_tests;` line, and `pub fn current_keymap(cx: &App) -> Arc<Keymap>`: the
matcher's keymap, which the rail card's tooltips read too.

- **`pub fn install(cx: &mut App, menus: MenuBar)`** stores the bar in a global, then builds and sets
  the GPUI menus. Each command item becomes `MenuItem::action`, or `MenuItem::os_action` when it has
  one, carrying a new action `MenuCommand { menu, item }` (`#[action(namespace = fastedit, no_json)]`).
  A `system = "services"` item becomes `os_submenu`.
- **Display-only key bindings.** For each item with a `shortcut`, `install` binds a `gpui::KeyBinding`
  of that keystroke to its `MenuCommand`, in the key context `fastedit_menu`.
  - No element sets that context, so GPUI's own matcher never runs these bindings. They only give
    macOS its key equivalents.
  - They replace GPUI's keymap wholesale (`clear_key_bindings`, then `bind_keys`).
  - A keystroke GPUI can't parse shows no key, and is logged at debug.
  - This is the only `KeyBinding::new` in fastedit. Comment it with the reason.
- **Choosing an item.** A global `cx.on_action::<MenuCommand>` handles it. Nothing in the element tree
  handles `MenuCommand`, so it always gets there.
  - It takes the active window's focused stack (`keymap::context_stack(&window.context_stack())`),
    resolves `invocation(stack)`, and dispatches `RunCommand` exactly as a key does
    (`keymap::dispatch`).
  - With no window, it runs the invocation through the app registry, so Quit and Hide work with no
    window open.
  - An invocation that doesn't apply runs nothing, as the same key wouldn't.
- **Hot reload.** `set_user_keymap` (in `crates/fe-gpui/src/keymap.rs`) reinstalls installed menus
  (bindings, then `set_menus`) when the keymap changes. With no menus installed it does nothing, so
  embedding hosts are unaffected.

### The standalone app

`crates/fastedit/src/main.rs`, after `fe_workbench::init`, calls
`fe_gpui::menu::install(cx, MenuBar::default_macos(Some(&*fe_workbench::command_catalog())))` and logs
the diagnostics. The existing keymap hot reload already calls `set_user_keymap`.

### Tests

- New file `crates/fe-keymap/tests/menu.rs`. It's an integration test, so `src/tests/mod.rs` isn't
  touched.
- New file `crates/fe-gpui/src/menu_tests.rs`. It uses headless GPUI, so `get_menus()` and
  `key_bindings()` show what would reach AppKit.
- New file `crates/fastedit/tests/menus.rs`, which checks the default table against the full catalog.
- The test platform's `hide`, `hide_other_apps`, `unhide_other_apps`, `minimize` and `zoom` are
  `unimplemented!()`, so tests that run an item use `window.toggle_fullscreen`, which the test window
  supports, or editor commands.

## Out of scope

- Enabled and disabled state from context predicates (not feasible with gpui-pre 0.3.7; see above).
- Checkmarks, such as Soft Wrap's state.
- Showing chord shortcuts in menus.
- File > Open Folder…, Open File… and Settings…: the follow-ups T123 and T124 add the commands and
  their menu items.
- About fastedit.
- A dock menu.
- A user menu file in the config directory.
- Menus on Linux or Windows.

## Decided (lead defaults; the user may revisit)

- **The table is TOML in fe-keymap** (`fe_keymap::menu`). Hosts can replace it or extend it with a
  layer, and only `crates/fastedit` installs it.
- **Menu items dispatch the same invocations keys do**, stamped with the focused element's context and
  `Source::Menu`. One item can name per-context alternatives, such as Paste in a text input.
- **The shortcut shown is the one the keymap would actually run there**, as checked by a `Matcher`. A
  user's rebinding moves it, and an unbinding removes it.
- **Every item is enabled. A command that doesn't apply runs nothing.** The user accepted this
  gpui-pre 0.3.7 limit (2026-10-06). It's documented as a known limit, not a defect.
- Standard macOS keys for the new app and window commands: `cmd-h`, `alt-cmd-h`, `cmd-m`,
  `ctrl-cmd-f` and `cmd-shift-w`.
