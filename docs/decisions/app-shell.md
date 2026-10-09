# App shell: one `fe-shell` crate for the host app and fastedit

Status: **build**. Decided 2026-10-08. Source card T242. The extraction
cards in §5 are drafted from this document afterwards; this card changes no
code.

The user decided on 2026-10-08 that the host app (the AI-first dev tool that
replaces Eclipse Theia, `~/.claude/plans/glittery-foraging-graham.md` line 4;
"The host app is GPUI", line 14) and fastedit's standalone app share one app
shell: a new crate, `crates/fe-shell`, in this repository. fastedit's editor
and panels become items that the shell hosts.

**What was read.** fastedit at `25b6800`, this lane's base. `38b019d` on top of
it adds only this card's spec. Paths are relative to the repository root, and
line counts come from `wc -l` at that commit. GPUI facts come from the pinned
`gpui-pre` 0.3.7 sources in the cargo registry: `AnyView::downcast`
(`src/view.rs:58`), `ManagedView` (`src/window.rs:714`), and `Task::ready`
(`gpui-pre-scheduler` `src/executor.rs`). Any type or function that isn't in
the code today is marked **proposed**.

## 1. Decision and why

**Decision.** One crate, `fe-shell`, holds the application shell. It owns the
window frame (title bar, status bar), the pane tree with its tabs, splits and
tab drag, the dock and its activity rail, pickers and modal palettes,
workspace trust, command and key routing through `fe-command` and
`fe-keymap`, and layout persistence. Two apps build on it:

- **The host app.** It registers its own pane items (chat, agent logs, diff
  review, later terminals), dock panels, commands, key bindings, status items
  and theme.
- **fastedit's standalone app** (`crates/fastedit`). It is the same shell with
  fastedit's items and panels registered: the editor (`EditorView`), the
  project tree, search, the outline, references, quick open and the
  navigation pickers. They live in `fe-workbench`, which becomes "fastedit on
  `fe-shell`" (§2.4).

**Why one shell rather than two.**

- **The host gets the shell for free.** Docks, splits, tab drag, pickers,
  breadcrumbs, the status bar, workspace trust and key routing exist and are
  tested: `crates/fe-workbench/src/tests/` holds 31 test modules besides
  `mod.rs`, about a third of them on the shell (tabs, splits, tab drag, pane
  cycling, the pane toolbar and its history, the status bar, the title bar,
  the activity rail).
  A second shell in the host would rebuild all of this, then drift from
  fastedit's.
- **The shell gets exercised every day.** fastedit's app is a thin client of
  the same shell. Every shell bug shows up in the editor we use daily, long
  before the host ships. A shell that only the host used would get no use
  until the host existed.
- **Editor panes behave the same in both apps.** The editor is a pane item
  in the host, as it is in fastedit. Splits, closing a dirty buffer,
  breadcrumbs and the status bar work the same way in both, because the same
  code runs them.
- **One place for cross-cutting changes.** Themes (T238 and T244–T247),
  multiple windows and layout persistence are each built once.

**Why it lives in this repository.**

- **Its dependencies are here.** `fe-ui`, `fe-command`, `fe-keymap` and
  `fe-config` are workspace crates, and so is the code being extracted. A
  separate repository would version five crates across a boundary that
  changes every week.
- **The gate covers it.** The extraction is a series of moves out of
  `fe-workbench` and `fe-gpui`. In one repository each move is one commit
  that the gate checks, including those tests and T81's screenshot
  golden. Across repositories, every move would be two coordinated releases.
- **The host will depend on this repository anyway.** It will embed
  `fe-gpui`'s editor, so taking `fe-shell` from the same place adds nothing.

## 2. Crate boundary

### 2.1 Dependencies

`fe-shell`'s `[dependencies]` are exactly `gpui`, `fe-ui`, `fe-command`,
`fe-keymap` and `fe-config`. It never depends, directly or transitively, on
`fe-editor`, `fe-gpui`, `fe-buffer`, `fe-syntax`, `fe-lsp`, `fe-nav` or
`fe-project`.

None of the five reaches one of the excluded seven today:

- `fe-ui` depends only on `gpui` (`crates/fe-ui/Cargo.toml`).
- `fe-command` depends on `schemars`, `serde`, `serde_json` and `thiserror`.
- `fe-keymap` depends on `fe-command`, `fe-config` and third-party crates.
- `fe-config` depends on third-party crates only.

What the shell needs from third-party crates it takes through the five:

- **JSON values** as `fe_command::Value`, which is `serde_json::Value`
  (`crates/fe-command/src/lib.rs:20`). Saved layouts are written with its
  `Display` impl and parsed with its `FromStr` impl.
- **Logging.** The code that moves logs in about a dozen places today:
  - action handlers with no caller to return an error to: `dispatch_app`
    (`fe-gpui/src/commands.rs`), every `op()`/`toggles()` helper's
    `log::warn!("{error}")`, `Workbench::on_run_command`;
  - `menu.rs`'s warnings and debug lines;
  - refused moves and closes (`PaneGroup::move_tab`, `Pane::close_active`);
  - failed trust and settings writes (`trust.rs`, `persist.rs`).

  In fe-shell, each of these calls a **proposed** `fe_shell::report(cx,
  Level, message)`, which hands it to the handler the host installs with
  `fe_shell::set_report_handler`. With none installed, reports are dropped.
  Background writers send their failures to the main thread for it.
  fastedit's handler calls `log`'s macros, so its logs read as today. Q2
  asks whether to allow `log` instead.

Card S4 (§5) makes this checkable. It adds `fe-shell` to `UI_CRATES` in
`scripts/check-no-gpui.sh`, because `fe-shell` depends on GPUI. It also adds
a check to the same script that fails if `cargo tree -p fe-shell -e normal`
lists any of the seven excluded crates. The `no_gpui` gate stage already runs
that script whenever a `Cargo.toml` changes (`regatta.toml`).

### 2.2 `fe-workbench`'s shell files

Each file is marked **generic** (moves to `fe-shell`), **fastedit-specific**
(stays in `fe-workbench`) or **split** (some of it moves, and the table names
the line it splits along). The files that use
`fe_gpui::commands::RunCommand` (dock, pane_group, pane_toolbar, picker,
status_bar, persist, trust, and workbench.rs) take it as
`fe_shell::RunCommand` instead (§2.3).

**Tests.** The tests that drive `Workbench` (the modules in `tests/`, declared
in `tests/mod.rs`) stay in fe-workbench and keep passing against the moved
code, so no card edits `tests/mod.rs`. Each new fe-shell module gets its own
unit tests in fe-shell, against a test item and a bare `Shell`.

| File | Lines | Verdict | Along what line |
|---|---|---|---|
| `dock.rs` | 471 | split | **Generic:** `Dock` as a container of registered panels (§3.5), the activity rail and its tooltips, show/toggle/visibility, the title row, and the commands `dock.toggle` and "show panel X". **fastedit:** `DockPanel`'s four variants and their fields (`project`, `search`, `outline`, `references`); the `search.*` commands and `DockOp::{ToggleRegex, ToggleCase, ToggleWholeWord, Submit}`, which are the search panel's; and `show_search` focusing the search input, which becomes the panel's own focus-on-show. |
| `pane.rs` | 1153 | split | **Generic:** the tab strip over `Box<dyn ItemHandle>` (§3.2): `tabs`, `active`, `activate`, `cycle`, `take`, `insert`, `reorder`, closing with the item's veto, the tab bar and its buttons, drop zones and the drop hint, `bounds`, `focus`, and the toolbar row's frame. **fastedit:** opening files into editors, which is `open_at`, `opening`, `file_read`, `reads`, `reveal`/`Reveal`/`Revealed`, `pending_place` (`nav::Place`), `peer_model`, `new_editor`, `reload_settings`, `EditorTheme`/`EditorSettings`, and the window title's "— fastedit". The pane's loading and error message becomes a generic **proposed** `Pane::set_placeholder(Option<SharedString>)` that fastedit's opener drives. |
| `pane_group.rs` | 716 | generic | `PaneGroup`, `Member`, `Members`, `Axis`, `Direction`, dividers, `neighbour`/`nearest`, and the `pane.*` commands. Two edits come with the move. `move_tab`'s checks go through the item. The dirty check (`model.is_dirty() && !shown_elsewhere`) uses `PaneItem::is_dirty` and `shares_document`. The "target already shows this file" check (`target.position(path)` with `tab.path`) uses `shares_document` against the target's items: a file open in two panes is two editors over one buffer (`Pane::peer_model`). `render(&self, ui, cx: &mut Context<Workbench>)` renders into the shell's root rather than `Workbench`. |
| `pane_drag.rs` | 143 | generic | `DraggedTab`, `TabDropped`, `TabGhost`, `DropZone`, `DropTarget`, `insertion` and `marker`. `TabId` becomes `ItemId`. |
| `pane_toolbar.rs` | 246 | split | **Generic:** `Toolbar` (visible, the leading buttons), `NavButtons` generalised to host-set leading buttons, `path_crumbs` (pure `std::path`, relative to the shell's root), `shortcut`, `dispatch`, and `view.toggle_breadcrumbs`. **fastedit:** `symbol_chain` and `caret` (`fe_syntax::Outline`, `EditorView`), `SymbolChain`, and `ToolbarRequest::Symbols`. Back/forward stay fastedit's commands (`nav.back`/`nav.forward`), and the workbench sets them as the pane's leading buttons. |
| `picker.rs` | 322 | split | **Generic:** `PickerDelegate`, `Picker<D>`, `PaletteOp` except `ToggleQuickOpen`, and the `picker.*` and `text_input.*` commands. **fastedit:** `quick_open.toggle`, which moves to quick open's own registry. |
| `quick_open.rs` | 371 | fastedit-specific | It is a `PickerDelegate` over `fe_project::FinderIndex` that opens into a `Pane`. |
| `toolbar_picker.rs` | 640 | fastedit-specific | It imports `EditorView` and `fe_syntax::Outline`, and lists directories for path crumbs. Its `PANE_COMMANDS` list stays as it is; once the command palette (S9) lands, the "…" button could open that palette instead (Q7). |
| `title_bar.rs` | 107 | generic | `TITLE_BAR_HEIGHT`, `TRAFFIC_LIGHTS`, `TRAFFIC_LIGHT_INSET`, `titlebar_options`, and the drag and double-click handling. The label comes from the host: `NO_ROOT` ("fastedit") becomes the app name that the host passes in. |
| `status_bar.rs` | 420 | split | **Generic:** `StatusSide`, `StatusItem`, `StatusBar` with `set_item`/`remove_item`, sorting, `entry`, `focused_context`, and `view.toggle_status_bar`. **fastedit:** `built_in_items`, `language_label`, `PLAIN_TEXT`, `vim_mode_label`, the built-in ids' meaning (`StatusItem::CURSOR` …), and `language.select`. `StatusBar::set_editor` becomes "follow the active item": the bar shows `PaneItem::status_items` (§3.1), and `EditorView` returns today's built-ins from there. `focused_context`'s `vim_` → `vim.` rewrite is the same rule as fe-gpui's `keymap::context_stack` (`crate::vim::GPUI_PREFIX`). It moves with the context conversion into fe-shell as a **proposed** `register_context_namespace(cx, "vim")`, leaving one copy instead of two. |
| `persist.rs` | 221 | split | **Generic:** the write queue (`Writes`, `Queue`, `write_pending`, quit waiting for the writer), generalised from `(&'static [&str], bool)` settings writes through `fe_config::write_setting` to any keyed write, so layout persistence (§4.6) can use it; and saving `status_bar.visible`. **fastedit:** `VimToggle` and its `RunCommand`/`MenuCommand` listeners for `vim.toggle` (`fe_gpui::vim_enabled`). |
| `trust.rs` | 408 | generic | `TrustState`, `set_trust_file`, `decide`, `RootTrust`, the prompt, and `workbench.trust_folder`/`untrust_folder` over `crates/fe-config/src/trust.rs`'s `TrustStore`. Today `RootTrust` hangs off `Workbench`; it moves to the shell's root, which owns the window's root folder. The prompt's detail text mentions fastedit and rust-analyzer, so the host supplies it. `TrustChanged` stays the event fastedit's `lsp.rs` follows. |

Two more files hold the wiring:

| File | Lines | Verdict | Along what line |
|---|---|---|---|
| `workbench.rs` | 1275 | split | **Generic:** the frame as a **proposed** `fe_shell::Shell` root, one per window. It holds the title bar, `PaneGroup` with the active pane and `peers`, the dock, the status bar, one modal at a time (today's `palette`, §3.5), `follow_palette`, `leave_hidden_panel`, `root`/`RootTrust`, the frame part of `Render`, and the command-routing chain in `on_run_command` for the shell's registries. **fastedit:** `Launch`, `resolve_launch` and `WELCOME`; the `editors`/`echoes` buffer-sharing notifications; `nav_state`; `lsp`; the outline, symbol chain and `chain_key`; `revealed`; `launch_read`; `open_settings`; and `prompt_open`. |
| `lib.rs` | 165 | split | **Generic:** `WorkbenchCommands`' tab commands (`workbench.close_tab`, `next_tab`, `previous_tab`, keeping their IDs per Q9), and `Catalog`, the composite `CommandCatalog`. **fastedit:** `init` for its modules and `command_catalog()`'s list of fastedit registries. |

The other modules are fastedit-specific and stay: `language_picker.rs`,
`loader.rs`, `lsp.rs`, `nav.rs`, `nav_picker.rs`, `open.rs`,
`outline_panel.rs`, `project_panel.rs`, `references_panel.rs` and
`search_panel.rs`. The fe-ui primitives the shell draws with
(`crates/fe-ui/src/status_bar.rs`'s `StatusBarFrame`/`StatusEntry`,
`TabBar`, `ActivityRail`, `PaletteFrame`, `Breadcrumbs`, `UiTheme`) stay in
`fe-ui`, which `fe-shell` depends on.

### 2.3 Outside `fe-workbench`: the GPUI command plumbing in `fe-gpui`

The boundary has one consequence the file list above doesn't show. Eight of
those files dispatch or match `fe_gpui::commands::RunCommand`, and keys reach
commands through `fe-gpui`'s key interceptor. `fe-shell` can't depend
on `fe-gpui`, so the GPUI side of `fe-command` and `fe-keymap` moves down
into `fe-shell`:

| File | Verdict | Along what line |
|---|---|---|
| `crates/fe-gpui/src/commands.rs` (356) | split | **Generic:** `RunCommand`; `AppCommands`; the app registry's `app.*` and `window.*` commands, with `app.hide`'s title ("Hide fastedit") taking the host's app name; `init`'s app-level `RunCommand` handler with the `propagated` stamp, which `EditorView::run_command` sets through a **proposed** public setter; `observe_invocations`/`notify_observers`; and `dispatch_app`. **Stays in fe-gpui:** `vim.toggle` and `vim.key` (registered into the shell's app registry by `fe_gpui::init`), `EDITOR_COMMANDS`, `ViewHost`, `EditorView::run_command`/`dispatched`/`type_text`, and the editor and vim part of `command_catalog`. |
| `crates/fe-gpui/src/keymap.rs` (405) | split | **Generic:** `KeyInput`, `PendingChord`, `on_keystroke`, `on_timeout`, `replay`, `lookup`, `context_stack`, `CHORD_TIMEOUT`, `set_user_keymap` and `keymap_report`. **Stays:** the editor hooks it calls today: vim's `crate::vim::live_stack`, `vim::KEY` and `vim_editor`/`feed`, and the editor's `crate::element::hover_with_modifiers` on a modifier change. They go behind a **proposed** `fe_shell::KeyOwner` trait, which fe-gpui implements and installs. It has four parts: stamp the context stack and say whether the focused view owns the keystroke; take a matched invocation (`vim.key`); take swallowed keystrokes; and hear modifier changes. `keymap_report` takes the base context stack as an argument instead of hard-coding `[editor]`. |
| `crates/fe-gpui/src/menu.rs` (240) | generic | `MenuCommand`, `install`, `refresh` and `choose` use `fe-keymap`'s `MenuBar`, `RunCommand`, the keymap, and `commands::dispatch_app` and `keymap::dispatch`/`context_stack`. All of those move with it. Its logging goes through `report` (§2.1). Its `KEY_CONTEXT`, `"fastedit_menu"`, keeps its value (Q9). |

`fe-gpui` then depends on `fe-shell` and re-exports these items
(`pub use fe_shell::{RunCommand, …}` under its current paths). Nothing that
imports `fe_gpui::commands::RunCommand` has to change in the move card (S4).
That adds the edge `fe-gpui → fe-shell`, besides `fe-workbench → fe-shell`.
It is also what lets `EditorView` implement the shell's `PaneItem` in
`fe-gpui` once the trait moves (Q1).

### 2.4 Where `fe-workbench` ends up

`fe-workbench` becomes fastedit on `fe-shell`:

- `EditorView` as a `PaneItem` (the impl itself sits in `fe-gpui`, Q1).
- The file opener that reads files into editors in a pane (from `pane.rs`).
- Its dock panels, registered with the shell's dock: project tree, search,
  outline and references.
- Its pickers, shown through the shell's modal layer: quick open, language,
  toolbar, definitions and symbols.
- Navigation history and its back/forward toolbar buttons, the language
  servers (`lsp.rs`), the editor's status items, and vim's toggle saving.
- `Workbench` itself, renamed or kept (Q9), wraps one `fe_shell::Shell` per
  window. It registers all of the above. It keeps the public API that
  `crates/fastedit` uses (`main.rs`, `perf.rs`), re-exporting what moved, so
  the binary doesn't change:
  - `Workbench::new`, `file_read`, `reload_settings`, `set_title_bar`,
    `set_status_bar_visible`, `active_editor`, `pane`, and `focus_handle`;
  - `Pane::theme`;
  - `loader`;
  - `titlebar_options`, `set_trust_file` and `save_toggles_to`.

`crates/fastedit` stays the binary. The host app is a second binary in its
own repository that depends on `fe-shell`, plus `fe-workbench` if it wants
fastedit's panels and editor.

## 3. `PaneItem` trait sketch

These are signatures, not an implementation. Everything in this section is
**proposed**. Card S1 writes it in `crates/fe-workbench/src/pane_item.rs`,
and S7 moves it to `crates/fe-shell/src/item.rs`.

### 3.1 The trait

```rust
use std::{any::Any, path::Path, rc::Rc};

use fe_command::Value;
use fe_ui::{CrumbKind, Icon};
use gpui::{
    AnyView, App, Context, Entity, EntityId, Focusable, Render, SharedString, Task, Window,
};

/// A tab's identity: its item's entity ID. It stays with the tab as the tab
/// moves between panes, as `TabId` (`pane.rs:35`, the editor's `EntityId`)
/// does today, but for any item. It isn't saved: a restored item is a new
/// entity with a new ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub EntityId);

/// What a pane shows in a tab. The pane observes the item, so any
/// `cx.notify()` re-renders its tab, crumbs and status items and retitles
/// the window, as `Tab::_changed` does for an editor today.
///
/// Focus is the `Focusable` supertrait: activating a tab focuses
/// `focus_handle`, as `Pane::activate` focuses the editor's today.
pub trait PaneItem: Render + Focusable + Sized + 'static {
    /// The item's kind in saved layouts, unique among the kinds an app
    /// registers: "fastedit.editor", "host.chat".
    const KIND: &'static str;

    // Title and icon.

    /// The tab's label: today `file_name(tab.path)`.
    fn tab_title(&self, cx: &App) -> SharedString;

    /// The tab's icon, if any. fe-ui's tab draws none today.
    fn tab_icon(&self, _cx: &App) -> Option<Icon> {
        None
    }

    /// The window's title while this is the active pane's active item:
    /// today `Pane::title`, "main.rs ● — fastedit".
    fn window_title(&self, cx: &App) -> SharedString {
        self.tab_title(cx)
    }

    // Dirty state, save and close veto.

    /// It has changes that closing would lose. This drives the tab's dirty
    /// dot.
    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    /// Whether `other` shows the same document, so this item's unsaved
    /// changes live on there. For editors this is two views over one
    /// buffer (`EditorModel::shares_buffer_with`). The pane closes, or
    /// moves away, a dirty item without asking when another open item
    /// shares its document, as `Pane::shown_elsewhere` decides today.
    fn shares_document(&self, _other: &AnyView, _cx: &App) -> bool {
        false
    }

    /// Asked before anything closes the item, through
    /// `Shell::confirm_close` (§3.6): its tab, its pane, its window,
    /// quitting, or a drop on a pane that already shows its document. It
    /// isn't asked when an item that stays open shares its document. The
    /// default keeps a dirty item open, which is what `Pane::close_active`
    /// does until there are save prompts.
    fn may_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if self.is_dirty(cx) {
            CloseAnswer::Keep
        } else {
            CloseAnswer::Close
        }
    }

    /// Saves it, for Save All and quitting with unsaved items. `None`: it
    /// has nothing to save.
    fn save(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Task<gpui::Result<()>>> {
        None
    }

    // Toolbar and breadcrumbs: what `pane_toolbar.rs` reads from the editor
    // today.

    /// The file it shows, if any. The shell turns it into path crumbs
    /// relative to the window's root, as `path_crumbs` does today from
    /// `Tab::path`.
    fn path(&self, _cx: &App) -> Option<&Path> {
        None
    }

    /// Crumbs after the path's, drawn by the pane's toolbar from
    /// `fe_ui::Crumb`.
    fn breadcrumbs(&self, _cx: &App) -> Vec<ItemCrumb> {
        Vec::new()
    }

    // Status bar.

    /// Its status bar items while it's the active item. The host's items
    /// with the same id replace them, as `StatusBar::set_item` does today.
    fn status_items(&self, _cx: &App) -> Vec<StatusItem> {
        Vec::new()
    }

    // Split and clone.

    /// A second item over the same document, for Split Right and Split
    /// Down, and for opening an item a peer pane shows: today
    /// `EditorModel::split` in `Workbench::split` and `Pane::peer_model`.
    /// `None`: the new pane opens empty.
    fn split(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Entity<Self>> {
        None
    }

    // Serialise and restore, for layout persistence (§4.6).

    /// Its state in a saved layout. `None` leaves it out of the layout:
    /// the welcome text, an untitled buffer, a non-UTF-8 path.
    fn serialize(&self, _cx: &App) -> Option<Value> {
        None
    }

    /// Rebuilds an item from `serialize`'s output, possibly after reading
    /// a file. `None` drops the item from the restored layout. The app
    /// registers each kind with `fe_shell::register_item::<Self>(cx)`.
    /// The shell calls it once per saved document; the other items over
    /// that document come from `split` and `apply_state` (§4.6).
    fn restore(_state: Value, _window: &mut Window, _cx: &mut App) -> Task<Option<Entity<Self>>> {
        Task::ready(None)
    }

    /// Applies the per-view part of a saved state, such as a caret, to an
    /// item `split` made while restoring.
    fn apply_state(&mut self, _state: &Value, _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// What `PaneItem::may_close` answers. It's synchronous when it can be, so
/// closing stays immediate, as the tab and split tests expect.
pub enum CloseAnswer {
    Close,
    /// Stays open; the shell does nothing more. (Today's dirty-tab case
    /// logs a warning.)
    Keep,
    /// Asked the user (a save prompt): closes if the task says `true`.
    Ask(Task<bool>),
}

/// A crumb an item contributes: data for `fe_ui::Crumb`.
pub struct ItemCrumb {
    pub label: SharedString,
    pub kind: CrumbKind,
    pub icon: Option<Icon>,
    /// Run on a click, after the pane focuses the item, so focus returns
    /// there once a picker this opens is dismissed (`Pane::request` today).
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}
```

`StatusItem` is today's `crate::status_bar::StatusItem`, moved to `fe-shell`.

### 3.2 The pane's handle

A pane holds items of many types, so it holds them through an object-safe
handle. A blanket impl covers every `Entity<T: PaneItem>`:

```rust
pub trait ItemHandle: 'static {
    fn id(&self) -> ItemId;
    fn kind(&self) -> &'static str;
    fn to_any_view(&self) -> AnyView;
    /// For `downcast_ref::<Entity<EditorView>>()`, which borrows. That
    /// keeps the `Option<&Entity<EditorView>>` signatures of
    /// `Pane::active_editor` and `Workbench::active_editor`, whose callers
    /// include `lsp.rs` and `nav.rs`.
    fn as_any(&self) -> &dyn Any;
    fn boxed_clone(&self) -> Box<dyn ItemHandle>;
    // One forwarding method per `PaneItem` method, through `Entity::read`
    // or `Entity::update`.
    fn tab_title(&self, cx: &App) -> SharedString;
    fn may_close(&self, window: &mut Window, cx: &mut App) -> CloseAnswer;
    fn split(&self, window: &mut Window, cx: &mut App) -> Option<Box<dyn ItemHandle>>;
    // …
}

impl<T: PaneItem> ItemHandle for Entity<T> { /* forwards */ }

/// What `Tab` becomes (today `pane.rs:38`).
pub struct Tab {
    pub item: Box<dyn ItemHandle>,
    _changed: Subscription,
}

impl dyn ItemHandle {
    /// Through `AnyView::downcast`.
    pub fn downcast<T: PaneItem>(&self) -> Option<Entity<T>> { … }
    /// Through `as_any().downcast_ref::<Entity<T>>()`.
    pub fn downcast_ref<T: PaneItem>(&self) -> Option<&Entity<T>> { … }
}
```

`Tab::path` and `Tab::editor` go away. `pane.position(path)`, which finds an
open file, becomes the fastedit opener's own search: it walks the items,
downcasts each to `EditorView`, and compares `model().path()`.

### 3.3 `EditorView` implementing it

In S1 this impl lives in `fe-workbench`, which may implement its own trait
for a foreign type. In S7 it moves to `fe-gpui`, once the trait is in
`fe-shell`; the orphan rule forbids it in `fe-workbench` then (Q1). The
helpers it calls move into `fe-gpui` with it, since they need only the
editor crates:

- `built_in_items`, `language_label`, `vim_mode_label` and `PLAIN_TEXT` from
  `status_bar.rs`;
- `file_name` from `pane.rs`;
- `loader`, which needs `fe-buffer`, and `fe-text` once T250 lands.
  `fe-workbench` re-exports it for `crates/fastedit`.

```rust
impl PaneItem for EditorView {
    const KIND: &'static str = "fastedit.editor";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(self.model().path()).into()
    }

    fn window_title(&self, cx: &App) -> SharedString {
        let dirty = if self.is_dirty(cx) { " ●" } else { "" };
        format!("{}{dirty} — fastedit", self.tab_title(cx)).into()
    }

    // `tab_icon` keeps the default `None`, so the screenshot golden doesn't
    // change.

    fn is_dirty(&self, _: &App) -> bool {
        self.model().is_dirty()
    }

    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<EditorView>()
            .is_ok_and(|other| other.read(cx).model().shares_buffer_with(self.model()))
    }

    // Until S8, `may_close` keeps the default: a dirty editor stays open
    // with a warning, as now. S8 makes it `CloseAnswer::Ask` with a
    // "Discard unsaved changes?" prompt (Discard, Cancel), so that closing
    // a pane or window, or quitting, asks rather than refusing silently.

    fn save(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Task<gpui::Result<()>>> {
        // What `editor.save` (cmd-s) runs. `EditorModel::save` is
        // synchronous, so the task is ready at once.
        let saved = self.model_mut().save().map_err(Into::into);
        cx.notify();
        Some(Task::ready(saved))
    }

    fn path(&self, _: &App) -> Option<&Path> {
        self.model().path()
    }

    // `breadcrumbs` keeps the default. The symbol crumbs need the outline
    // that fastedit's OutlinePanel keeps, so fastedit pushes them per item,
    // as `Workbench::push_symbols` pushes `SymbolChain` today: proposed
    // `Pane::set_item_crumbs(ItemId, Vec<ItemCrumb>)`, shown only while that
    // item is active (today's `chain.editor == Some(editor.entity_id())`
    // check).

    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        // Today's `built_in_items`: vim mode, cursor, indent, encoding,
        // line ending, language.
        built_in_items(self)
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let model = self.model().split();
        let theme = self.theme().clone(); // `EditorView::theme`, which T246 adds
        Some(cx.new(|cx| EditorView::new(model, theme, cx)))
    }

    fn serialize(&self, _: &App) -> Option<Value> {
        // No path (welcome, untitled) or a non-UTF-8 one: not saved.
        let path = self.model().path()?.to_str()?;
        // The primary selection's head, as `pane_toolbar::caret` reads it.
        let primary = self.model().primary_selection();
        let caret = if primary.reversed { primary.range.start } else { primary.range.end };
        // `fe_command::Value` is serde_json's, built without its `json!`
        // macro: fe-gpui has serde_json as a dev-dependency only.
        Some(Value::from_iter([
            ("path", Value::from(path)),
            ("caret", Value::from(caret)),
        ]))
    }

    fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        // Reads `state["path"]` with `loader::read` off the main thread (as
        // `Pane::open_at` does), builds the view, and places the caret, clamped
        // to the text. A file that's gone is dropped. The theme is the
        // active one (T245).
        …
    }
}
```

### 3.4 A non-editor item: a chat pane in the host

The host app depends on `fe-shell`, not on fastedit's editor crates:

```rust
pub struct ChatPane {
    thread: ThreadId,             // the host's
    title: SharedString,
    draft: Entity<fe_ui::TextInput>,
    streaming: bool,
    focus: FocusHandle,
    // transcript, model, …
}

impl Focusable for ChatPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChatPane { /* transcript above, draft input below */ }

impl PaneItem for ChatPane {
    const KIND: &'static str = "host.chat";

    fn tab_title(&self, _: &App) -> SharedString {
        self.title.clone()
    }

    // An unsent draft counts as unsaved.
    fn is_dirty(&self, cx: &App) -> bool {
        !self.draft.read(cx).text().is_empty()
    }

    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.streaming && !self.is_dirty(cx) {
            return CloseAnswer::Close;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close this chat?",
            Some("A reply is still streaming, or the draft is unsent."),
            &[PromptButton::ok("Close"), PromptButton::cancel("Keep")],
            cx,
        );
        CloseAnswer::Ask(cx.spawn(async move |_, _| matches!(answer.await, Ok(0))))
    }

    fn breadcrumbs(&self, _: &App) -> Vec<ItemCrumb> {
        vec![ItemCrumb {
            label: self.title.clone(),
            kind: CrumbKind::Symbol,
            icon: None,
            on_click: None,
        }]
    }

    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        vec![StatusItem {
            id: "chat.model".into(),
            side: StatusSide::Right,
            priority: 100,
            text: self.model_name(),
            command: Some(Invocation::of("chat.select_model").expect("valid")),
        }]
    }

    // `split` keeps the default `None`: one view per thread, so a split
    // opens an empty pane.

    fn serialize(&self, _: &App) -> Option<Value> {
        Some(serde_json::json!({ "thread": self.thread })) // the host depends on serde_json itself
    }

    fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        // Loads the thread from the host's store.
        …
    }
}
```

The host registers it once with `fe_shell::register_item::<ChatPane>(cx)`
and opens one with `shell.active_pane().update(cx, |pane, cx|
pane.add_item(Box::new(chat), window, cx))`. Both are **proposed**. Tab drag,
splits, close and the status bar then work as they do for editors.

### 3.5 `Tab`, `DockPanel` and `Palette` stop being closed enums

- **`Tab`** holds `Box<dyn ItemHandle>` (§3.2), not
  `editor: Entity<EditorView>`.
- **`DockPanel`** (`dock.rs:34`, `Project | Search | Outline | References`)
  becomes registered panels:

  ```rust
  /// A panel in the dock. (Proposed.)
  pub trait DockItem: Render + Focusable + 'static {
      /// Distinct among the dock's panels: "project", "search", …
      fn id(&self) -> PanelId;
      /// The title row: today `DockPanel::name`.
      fn title(&self, cx: &App) -> SharedString;
      /// Its rail button: today `panel_icon`.
      fn icon(&self, cx: &App) -> Icon;
      /// The command whose key the rail's tooltip shows: today
      /// `DockPanel::show_command`.
      fn show_command(&self) -> Option<&'static str>;
      /// On the rail from the start; `false` joins it once first shown,
      /// as References does today (`references_shown`).
      fn always_on_rail(&self) -> bool {
          true
      }
      /// Focus it when shown, as `Dock::show_search` focuses the input.
      fn focus_on_show(&self) -> bool {
          false
      }
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct PanelId(pub &'static str);

  impl Dock {
      pub fn add_panel<P: DockItem>(&mut self, panel: Entity<P>, cx: &mut Context<Self>);
      pub fn show(&mut self, id: PanelId, window: &mut Window, cx: &mut Context<Self>);
      pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub fn active(&self) -> Option<PanelId>;
      pub fn panel<P: DockItem>(&self) -> Option<Entity<P>>; // replaces project(), search() …
  }
  ```

  The rail lists panels in registration order. The dock's own commands
  become `dock.toggle` plus `dock.show { panel }` (args per
  `command-model.md`). fastedit keeps its `dock.show_project`,
  `search.show` and `outline.show` IDs and maps them onto `Dock::show`, so
  the keymap and menus don't change. `nav.find_references` still runs its
  query first, then shows the References panel through `Dock::show`, as
  `DockOp::ShowReferences` does today. Leaving a hidden
  panel's focus is generic: focus goes to the active item, as
  `Workbench::leave_hidden_panel` does today.

- **`Palette`** (`workbench.rs:88`,
  `QuickOpen | Language | Toolbar | Definitions | Symbols`) becomes the
  shell's modal layer, which holds any GPUI `ManagedView` (focusable, emits
  `DismissEvent`):

  ```rust
  impl Shell {
      /// Shows `modal` over the window, dismissing the one showing, if any.
      /// It hides on its `DismissEvent` or when focus leaves it, as
      /// `Workbench::follow_palette` arranges today.
      pub fn show_modal<M: ManagedView>(
          &mut self,
          id: ModalId,
          modal: Entity<M>,
          window: &mut Window,
          cx: &mut Context<Self>,
      );
      pub fn modal(&self) -> Option<ModalId>;
      pub fn dismiss_modal(&mut self, window: &mut Window, cx: &mut Context<Self>);
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct ModalId(pub &'static str);
  ```

  fastedit's five pickers each get a `ModalId` ("quick_open", "language",
  "toolbar", "definitions", "symbols"). Code that checks
  `palette == Some(Palette::Symbols)` (in `workbench.rs` and `nav.rs`) checks
  `shell.modal() == Some(SYMBOLS)` instead. The host shows its own pickers
  the same way, and so does the command palette (§4.3).

### 3.6 Every close path goes through `may_close`

**Proposed**:

```rust
impl Shell {
    /// Asks `items` in order, skipping one that shares its document with an
    /// item that stays open. Resolves `true` only if every one answers
    /// `Close`, or `Ask` and then `true`. The first `Keep`, or an `Ask`
    /// that resolves `false`, stops the asking and resolves `false`, and
    /// nothing closes.
    pub fn confirm_close(
        &mut self,
        items: Vec<Box<dyn ItemHandle>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool>;
}
```

Every path that removes items calls it first. When every answer is `Close`,
nothing is awaited and the close happens in the same update, as today.

| Path | Today | With `confirm_close` |
|---|---|---|
| Close a tab (`workbench.close_tab`, the tab's ×) | `Pane::close_active`: a dirty tab no other tab shows stays, with a warning | that tab |
| Drop a tab on a pane that shows its document | `PaneGroup::move_tab`: the same check | that tab |
| Close a pane (`pane.close`) | `Workbench::close_active_pane`: checks only editor dirtiness | every tab in the pane |
| Close a window (`window.close`, the red button) | `window.remove_window()` (`fe-gpui/src/commands.rs`), and the platform closes it: nothing is asked | every item in the window. The button goes through `Window::on_window_should_close` (`gpui-pre` `src/window.rs:6658`): it returns `false`, runs `confirm_close`, and removes the window if that resolves `true`. |
| Quit (`app.quit`: cmd-q, the app menu) | `cx.quit()`: nothing is asked | every window's items, window by window. The first window to cancel stops the quit with every window still open. Then `cx.quit()`. |

**Known limit.** A quit the system starts (log out, restart, or Quit from the
Dock icon's menu) can't be cancelled in `gpui-pre` 0.3.7. It reaches the app
as AppKit's `will_terminate` and then `on_app_quit`, whose doc says "It is
not possible to cancel the quit event at this point" (`src/app.rs:2456`).
Such a quit still saves layouts, since quitting waits for the writer, but
unsaved items are lost, as they are today. Q10 asks about lifting this.

Card S8 does this, after the panes and the `Shell` exist (S7). Its tests use
a test item that is clean but vetoes (`Ask` resolving `false`, as the
streaming `ChatPane` in §3.4 would). They show that the item cancels each
path in the table: tab, pane, window by command, window by the close
callback, and quit. After each one, everything stays open. `Ask(true)` and
`Close` go ahead. `Keep` cancels. A quit with two windows where the second
cancels leaves both windows open.

## 4. Day-one host needs

### 4.1 Multiple windows

Each window gets one shell root, a `Shell` entity (**proposed**; today's
`Workbench` frame). `crates/fastedit/src/main.rs:265` opens exactly one
window today; the host opens several.

**Per window** (state in the window's `Shell`, or fastedit's layer over it):

- the pane tree, the active pane and every tab's item;
- the dock with its panels' state, so each window has its own project tree
  and search;
- the modal showing, if any;
- the status bar and the host's items on it (`StatusBar::set_item` is per
  bar);
- the title bar and the window title;
- the root folder and its trust resolution (`RootTrust`);
- the project settings layer: `fe_config::paths::project_settings_path(root)`
  is per root, so each window holds its own `SettingsStore` over the shared
  user layer (**proposed**; `main.rs` loads one store for the launch root
  today);
- fastedit's navigation history (`Nav`), outline, and language servers
  (`Lsp`, one `LspManager` per window since T143);
- a pending key chord. `KeyInput` is global, but a chord belongs to the
  window it started in and is abandoned when that window closes
  (`keymap.rs`'s `PendingChord`).

**Global** (GPUI `Global`s, one per app):

- trust decisions: `TrustState` with fe-config's `TrustStore`. `decide`
  already applies a decision "for every window at once" (`trust.rs`);
- the keymap and matcher (`KeyInput`, `current_keymap`), and the menu bar
  (`menu::install`; macOS has one per app);
- command registries: the app registry and each module's `Commands` global;
- the theme: fe-ui's active `UiTheme` global from T244;
- the user settings layer and its watcher;
- the item kinds registered for restore, and the layout writer;
- fastedit's vim switch (`fe_gpui::vim_enabled`).

**What changes for that.**

- Settings reloads fan out to every window. Today `main.rs` sends them to
  the one window handle and stops listening when it's gone (`main.rs`
  173–177). Keymap reloads are app-wide already: `fe_gpui::set_user_keymap`
  updates the global keymap and refreshes every window.
- `window.new` opens a window (**proposed** app command; fastedit binds it
  to File > New Window). Open Folder… gets an "in a new window" choice.
- Quitting when the last window closes stays as it is
  (`cx.on_window_closed`, `main.rs:255`).
- Tabs don't move between windows (Q6).

### 4.2 Non-editor panes

There are two ways in, and both are registered by the host:

- **As pane items** (§3): chat, agent logs, a diff review of several files,
  and later terminals. Each one gets tabs, splits (a fresh view through
  `PaneItem::split`, or an empty pane), drag between panes, the close veto,
  breadcrumbs, status items and saved layouts. To find an open one again,
  the host walks the panes' items and downcasts them (§3.2), as fastedit
  does by path.
- **As dock panels** (`DockItem`, §3.5): an agent log or a task list beside
  the editors, with a rail button and a show command.

Nothing in the shell assumes an item is an editor once S1 to S3 have landed.
The workbench's editor-only calls (`active_editor`, `open_editors`) stay in
fastedit's layer as downcasts.

### 4.3 Host commands, keymaps and the command palette

- **Commands** are `fe-command` registries. The host registers one or more
  with the shell (**proposed**
  `fe_shell::register_commands(cx, Registry<WindowCommands>)`, whose target
  is the window's `Shell`, `Window` and `App`, in the style of
  `WorkbenchCommands`' `PaneTarget` in `lib.rs`). It can also register into
  the app registry (`AppCommands`, target `&mut App`). The shell's root
  routes a `RunCommand` that nothing focused handled to them, as
  `Workbench::on_run_command` routes to fastedit's registries today. Their
  invocations reach `observe_invocations`' observers, so an agent bridge or
  recorder sees host commands too.
- **Keymaps** are `fe-keymap` TOML. `KeymapStore::load` takes one default
  source (`crates/fe-keymap/src/load.rs`). `[[bindings]]` tables concatenate,
  and a later binding wins, so the host passes
  `fe_keymap::DEFAULT_MACOS` followed by its own rows. The user's
  `keymap.toml` stays on top. Its catalog is the shell's composite `Catalog`
  (from `lib.rs`) over fastedit's registries and the host's. Menus come from
  the host's own `MenuBar` TOML, alongside `DEFAULT_MACOS_MENUS`.
- **The command palette** doesn't exist yet. `picker.rs`'s module doc
  anticipates it as another `PickerDelegate`. Card S9 adds
  `command_palette.toggle` (cmd-shift-p, workbench context), a
  `Picker<CommandPalette>` shown as a modal. It lists the `CommandMeta` of
  every registered registry (`Registry::commands`, with `title`, `id` and
  `context`). It shows the ones whose context predicate holds in the
  focused element's context stack, captured when the palette opens, each
  with its key from `fe_keymap::shortcuts` and `macos_label`. A pick refocuses
  the element that was focused and dispatches
  the pick's `Invocation` as a `RunCommand`, with `.source(Source::Palette)`
  and `.context(captured)`.
  `Source::Palette` exists already (`crates/fe-command/src/invocation.rs`).
  Commands that take args are left out for now (Q7).

### 4.4 Theming from the host

The host picks the theme. The shell follows it and never sees editor
colours:

- **Themes are fe-config data.** T238 (finalized) added GPUI-free themes to
  `fe-config` (`crates/fe-config/src/theme.rs`: `Theme { ui: UiColors,
  editor: EditorColors, syntax }`, `ThemeSet`, `load_themes`, the bundled
  fastedit Dark).
- **T244 to T247 make them live.**
  - T244 adds a `ThemeStore` to fe-config that selects and hot-reloads
    themes, and an fe-ui global for the active `UiTheme`.
  - T245 selects by a `theme` setting and builds `EditorTheme` from the
    theme's `editor`/`syntax` colours. Its
    `fn ui_theme(&fe_config::Theme) -> fe_ui::UiTheme` maps a theme to the
    UI colours, and its `apply` sets the global through
    `fe_ui::set_ui_theme`. Both live in `crates/fastedit/src/theme.rs`, a
    module of the binary.
  - T246 and T247 make panes, the frame, panels, pickers and the status bar
    follow the active `UiTheme` live.

  Today every component copies `UiTheme::default_dark()` once, at
  `workbench.rs:186`.
- **The mapping moves to the shell.** A host can't reuse a module of
  fastedit's binary. Card S4 moves `ui_theme`, and the UI half of `apply`,
  into `fe-shell` as **proposed** `fe_shell::apply_theme(cx, &Theme)`.
  `fe-shell` depends on both `fe-ui` and `fe-config`, so it can hold the
  mapping. `crates/fastedit` then calls it. Building `EditorTheme` stays in
  fe-gpui.
- **The host loads, the shell reads.** The host loads its themes with
  fe-config (`load_themes`, or T244's `ThemeStore` for hot reload) and calls
  `fe_shell::apply_theme`. The shell's code, by then moved from
  `fe-workbench`, keeps reading fe-ui's global, as T246 and T247 leave it.
  `EditorTheme` stays in fe-gpui, so the shell never sees editor colours. A
  host that also embeds the editor gets editor colours from the same `Theme`
  through T245's fe-gpui part.

### 4.5 Status items the host contributes

These work today: `Workbench::status_bar()` and
`StatusBar::set_item`/`remove_item` exist for hosts (`status_bar.rs`'s module
doc says "whatever items a host sets"). A `StatusItem` has an id, a side, a
priority, text and an optional `Invocation` that runs on a click. After the
move:

- **Window-wide host items** go through **proposed** `Shell::status_bar()`,
  as they go through `Workbench::status_bar()` now.
- **The active item's own items** come from `PaneItem::status_items` (§3.1).
  `StatusBar::set_editor` becomes "follow the active item", and the editor's
  built-ins move behind `EditorView`'s impl (S2).
- **A host item with a built-in id replaces it**, as now, so the host can,
  say, show its own language picker.

### 4.6 Layout persistence

This is new. `persist.rs` saves only `vim.enabled` and `status_bar.visible`
to `settings.toml`, and nothing saves the pane tree, splits, dock or open
tabs. Card S10 adds layout persistence, and S11 adds windows on one root.

**What's saved for each window.** One JSON entry (`fe_command::Value`) per
window:

- its window id (below), and when it was saved;
- the window's bounds;
- the pane tree: its axes, their members' ratios (`Members`' `ratios`), and
  its panes;
- each pane's items, with the active index. An item is saved as
  `{kind, document, state}`:
  - `kind` is `PaneItem::KIND`;
  - `state` is what `serialize` returned;
  - `document` is a group number. At save time, the shell gives items in the
    window that `shares_document` with each other the same number. Two split
    views of one file are two editors over one buffer, so they get one
    number.
- the active pane, by its path in the tree;
- the dock: whether it's visible, the active `PanelId`, and which panels
  have joined the rail;
- whether breadcrumbs show.

Status bar visibility stays in `settings.toml`.

**Where it's saved.** `fe_config::paths::state_dir()/layouts/<key>.json`,
one file per root folder:

- `<key>` is a stable hash of the canonical root's bytes (FNV-1a 64,
  written out rather than `DefaultHasher`), so non-UTF-8 roots work.
- The file is `{"version": 1, "windows": [entry, …]}`. A file with another
  version, or one that fails to parse, is ignored.

**Window ids, and several windows on one root.** Each window has an id: a
random `u64` given when the window opens, or the id of the entry it was
restored from.

- **Writing.** A window writes only its own entry. The writer reads the
  file, replaces that window's entry, and puts the file back whole, through
  a temporary file and a rename. It holds a lock file throughout, as
  `TrustStore::update` does (`crates/fe-config/src/trust.rs:95`). So two
  windows on one root, even in two fastedit processes, never drop each
  other's entries.
- **Closing.** Quitting keeps every window's entry, and so does closing the
  last window, which quits (`main.rs:255`). Closing a window while others
  stay open deletes its entry, so it doesn't come back.
- **Opening.** A folder launch reopens one window per entry, newest first,
  or one empty window if there are none (Q4). A new window on a root
  (`window.new`, or Open Folder in a new window) takes the newest entry that
  no open window holds, or starts empty.

**When it's saved.** After each layout change: a split, a close, a move, a
dock toggle, a tab opening or closing, or the end of a divider drag. Saves
go through `persist.rs`'s writer: one writer, and the last state of each
window wins. Quitting waits for the writer, as it waits for toggles and
trust writes today.

**Restoring.**

1. The shell rebuilds the window's pane tree.
2. For each document group, it restores the group's first item through its
   kind's `restore` (§3.1). Items restore asynchronously; until one lands,
   its tab shows the pane's placeholder.
3. Each other item in the group is that first item's `split`, given its own
   state with `apply_state`. So two restored views of one file share one
   buffer again, with their own carets, and the close and move rules (§3.6)
   treat them as they did before the restart.
4. A kind nobody registered, or a `restore` that returns `None` (a file
   that's gone), drops that tab, and every other tab of its group.
5. A pane left empty stays if it's the only one, and otherwise closes.

**Tests.**

- **S10:** a round trip of a window with the same file in two split panes,
  each with its own caret. After the restore, the two editors share a
  buffer (`shares_buffer_with`), an edit in one shows in the other, each
  keeps its own caret, and closing one of them while it's dirty asks
  nothing, because the other still shows the buffer.
- **S11:** two windows on one root, with different pane trees. Quit and
  relaunch on the folder: both windows come back, each with its own tree.
  Close one of the two windows and quit: a relaunch brings back only the
  other.

## 5. Staged extraction plan

The cards are drafted afterwards from this list. Names S1 to S12 are
placeholders until they get T numbers. Every card depends on T144 (LSP
go-to-definition) and T188 (the `visual.rs` split) landing, and lists both.
"Depends on" is a Regatta dependency, so rgt won't dispatch the card until
the named card is finalized. "Queued after" means ordered behind the named
card through another dependency in its chain.

Each card has to compile on its own, so each moves only code whose
dependencies are in `fe-shell` already or move with it. That fixes the
order:

1. The trait, inside `fe-workbench` (S1 to S3).
2. The crate, with the command plumbing and the theme mapping (S4).
3. The leaf modules, which need neither `Pane` nor `Workbench` (S5).
4. The `Shell` root (S6).
5. The panes, which render into it (S7).

| # | Card | Size | Depends on |
|---|---|---|---|
| S1 | **`PaneItem` trait in `fe-workbench`, `EditorView` behind it.** `pane_item.rs` gets `PaneItem`, `ItemHandle`, `ItemId`, `CloseAnswer` and `impl PaneItem for EditorView`. `Tab` holds `Box<dyn ItemHandle>`, and `TabId` becomes `ItemId`. That touches `pane.rs`, `pane_drag.rs`, `pane_group.rs` (`move_tab`'s checks), `workbench.rs` (`split`, `move_tab`, `open_editors`) and `references_panel/landing.rs` (`tab.editor`). `Pane::active_editor`, `Workbench::active_editor` and `open_editors` keep their signatures through `downcast_ref`, so `lsp.rs` and `nav.rs` don't change. Existing tests that read `tab.editor` or `tab.path` change only mechanically, to a downcast helper, and `tests/mod.rs` doesn't change. A new test module, declared by `#[path]` from `pane_item.rs` (as `persist.rs` declares `persist_tests.rs`), opens a non-editor test item, splits it, drags it, and vetoes its close. No behaviour change; T81's screenshot golden is unchanged. | m | T144, T188, T221, T231, T147, T235, T236, T237, T247, T265, T266 |
| S2 | **Status items and crumbs from the active item.** `StatusBar` follows the active item, and `built_in_items` sits behind `EditorView`'s `status_items`. Path crumbs come from `PaneItem::path`, and symbol crumbs are pushed per `ItemId` (`Pane::set_item_crumbs`). Files: `status_bar.rs`, `pane_toolbar.rs`, `pane.rs`, `pane_item.rs`, `workbench.rs`. | s | S1, T144, T188, T250 |
| S3 | **Registered dock panels and modals.** `DockPanel` becomes `DockItem`/`PanelId`/`Dock::add_panel`, and `Palette` becomes the modal layer (`show_modal`, `ModalId`). fastedit registers its four panels and five pickers. Command IDs, keys and menus are unchanged. Files: `dock.rs`, `workbench.rs`, `nav.rs`, `references_panel.rs`, `outline_panel.rs`, `picker.rs`. | m | S2, T144, T188 |
| S4 | **Create `crates/fe-shell` with the GPUI command plumbing and the theme mapping.** Its `[dependencies]` are exactly the five crates of §2.1. From fe-gpui (§2.3) it takes `RunCommand`, `AppCommands`, the app registry, `observe_invocations`, `menu.rs`, and `keymap.rs`'s interceptor behind `KeyOwner`, which fe-gpui implements. From `crates/fastedit/src/theme.rs` it takes `ui_theme`, which becomes `apply_theme` (§4.4). It adds `report`/`set_report_handler` (§2.1), and fastedit installs a handler that logs. fe-gpui depends on fe-shell and re-exports the moved items, so their callers don't change. `scripts/check-no-gpui.sh` adds `fe-shell` to `UI_CRATES` and fails if fe-shell's dependency tree reaches any excluded crate. | m | S3, T144, T188, T258, T260 |
| S5 | **Move the leaf modules into `fe-shell`.** These are the generic parts that need neither `Pane` nor `Workbench`. From `status_bar.rs`: `StatusItem`, `StatusSide` and `StatusBar`, which now shows a list of items that `Workbench` hands it. From `pane_toolbar.rs`: `Toolbar`, the leading buttons, `path_crumbs`, `shortcut`, `dispatch` and `view.toggle_breadcrumbs`. Also `picker.rs` without `quick_open.toggle`, `title_bar.rs`, `persist.rs`'s writer (generalised), and `Catalog`. `fe-workbench` imports them from fe-shell. | m | S4, T144, T188 |
| S6 | **The `Shell` root in `fe-shell`.** There is one per window. It composes the title bar, the dock (`dock.rs`'s generic part, with `DockItem`), the status bar and the modal layer, and it routes commands. It takes `trust.rs`, with `RootTrust` on `Shell`. Its centre is an element the host supplies, through **proposed** `Shell::set_center(AnyView)`; until S7, `Workbench` fills it with its `PaneGroup`. `Workbench` wraps one `Shell`. | m | S5, T144, T188 |
| S7 | **Move the panes into `fe-shell`.** It takes the generic parts of `pane.rs` (§2.2), plus `pane_group.rs` (whose `render` now takes `Context<Shell>`), `pane_drag.rs`, and `pane_item.rs` with `ItemHandle`/`register_item`. `Shell` owns the `PaneGroup`. `impl PaneItem for EditorView` and its helpers move to `fe-gpui` (§3.3, Q1). fastedit's file opener (reads, reveal, placeholder) stays in `fe-workbench`. The screenshot golden is unchanged. | m | S6, T144, T188 |
| S8 | **Close vetoes on every close path** (§3.6): `Shell::confirm_close` for a tab, a pane, a window (command and close button) and quit; `EditorView::may_close` asks Discard or Cancel. Tests: §3.6. | m | S7, T144, T188 |
| S9 | **Command palette over the registries** (§4.3): `command_palette.toggle` on cmd-shift-p lists the commands that apply in the focused context, with their keys, and dispatches the pick with `Source::Palette`. | s | S7, T144, T188 |
| S10 | **Layout persistence** (§4.6): `PaneItem::serialize`/`restore`/`apply_state` for `EditorView`; document groups, so restored views of one file share a buffer; one file per root with an entry per window id, merged under a lock; saved on change and at quit; restored on open. Tests: §4.6's round trip of two split views of one file. | m | S8, T144, T188 |
| S11 | **Multiple windows in fastedit** (§4.1): `window.new`, and Open Folder in a new window. Settings reloads fan out to every window, and there's a settings store per root. Windows on one root keep their own layouts by window id (§4.6). Tests: §4.6's two windows on one root. | m | S10, T144, T188 |
| S12 | **Host example**: `crates/fe-shell/examples/host.rs` builds a window from `fe-shell` alone. It has a chat-like `PaneItem`, a host command bound in a host keymap row, a host status item, a host dock panel, and a theme applied with `apply_theme`. It runs as a test (as fe-workbench's `screenshots` example does), so the gate builds it without fastedit's crates. | s | S9, S11, T144, T188 |

**Ordering against the open cards in the same files.** S1 is the first card
that edits `fe-workbench`'s shell files, and S4 the first that edits
`fe-gpui`'s command files. Every later card depends on S1 through the chain,
so ordering S1 and S4 orders them all. No extraction card puts
`crates/fe-workbench/src/lsp.rs` or `crates/fe-workbench/src/tests/mod.rs` in
its scope:

- new tests go in modules declared by `#[path]`;
- the existing tests stay where they are (§2.2);
- the `active_editor`/`open_editors` signatures stay.

| Open card | Shared files | How the extraction is ordered after it |
|---|---|---|
| T219 (vim command line) | `workbench.rs`, `status_bar.rs`, fe-gpui `command_tests.rs`/`keymap_tests.rs` | **Queued after**, through T221 (T221 depends on T219) and S1's dependency on T221. |
| T221 (vim `:w :q …`) | `workbench.rs`, `vim_line.rs` | **Depends on**: S1. |
| T231 (macros, replace mode, marks, folds) | `status_bar.rs`, `vim_line.rs`, fe-gpui `commands.rs` | **Depends on**: S1. S2 (`status_bar.rs`) and S4 (`commands.rs`) follow S1. |
| T145 (diagnostics) | `lsp.rs`, `tests/mod.rs`, fe-gpui `lib.rs` | **Queued after**, through T147 (T147 → T146 → T145) and S1's dependency on T147. |
| T146 (hover) | `lsp.rs`, `tests/mod.rs`, fe-gpui `commands.rs`/`lib.rs` | **Queued after**, through T147 (T147 depends on T146). |
| T147 (completion) | `lsp.rs`, `tests/mod.rs`, fe-gpui `commands.rs`/`lib.rs` | **Depends on**: S1. |
| T235 (LSP symlinks) | `lsp.rs`, `tests/mod.rs` | **Depends on**: S1. |
| T236 (LSP path aliases) | `lsp.rs`, `tests/mod.rs` | **Depends on**: S1. |
| T237 (LSP resolve retry) | `lsp.rs`, `tests/mod.rs` | **Depends on**: S1. |

The LSP cards don't share a file with the extraction cards (above). They are
ordered first because their code calls `Workbench::active_editor` and
`open_editors`, and S1 rewrites the bodies of both over items. Their specs
were written against today's `Tab { path, editor }`, so they should land
before it changes.

Other open cards touch the same files too, and the plan orders after them
the same way:

- **T246, T247 (themes):** `pane.rs`, `workbench.rs`, `lib.rs`, `dock.rs`,
  `status_bar.rs`, `picker.rs`, `quick_open.rs`, `toolbar_picker.rs`. S1
  depends on T247, and T247 depends on T246. T246 depends on T245, which
  depends on T223, so fe-gpui `lib.rs` is covered as well.
- **T250 (encodings):** `status_bar.rs`. S2 depends on it.
- **T258, T259, T260 (proposal buttons, commands and demo):** fe-gpui
  `lib.rs`, `commands.rs` and `Cargo.toml`. S4 edits all three, adding
  fe-shell to `Cargo.toml`. S4 depends on T258 and T260; T260 depends on
  T259.
- **T265, T266 (the find bar and its keys):** `pane.rs`, `lib.rs`,
  `tests/mod.rs`, fe-workbench's `Cargo.toml`, and fe-gpui's
  `command_tests.rs`/`keymap_tests.rs`. Those tests count the default
  keymap's bindings, which S9 adds to. S1 depends on both; T266 depends on
  T265, and T265 on T264.
- **T245 (theme setting):** `crates/fastedit/src/theme.rs` and fe-gpui
  `lib.rs`. S4 moves `ui_theme` out of that file. Queued after, through
  S1's dependency on T247 (T247 → T246 → T245).
- **T230 (vim scrolling):** fe-gpui `commands.rs`. Queued after through
  T231, which depends on T230.
- **T262 (T210's follow-ups, in the inbox):** `lsp.rs`, `tests/mod.rs` and
  `tests/lsp_quit.rs` only. No extraction card touches them, so it needs no
  ordering.

## 6. Open questions

1. **Where does `impl PaneItem for EditorView` live once the trait is in
   `fe-shell`?** The orphan rule rules out `fe-workbench` then: the trait
   and the type would both be foreign.
   **Recommended:** in `fe-gpui`, which depends on `fe-shell` from S4 anyway
   for `RunCommand`. The helpers the impl calls move into fe-gpui with it
   (§3.3). The alternative, a wrapper entity in `fe-workbench`
   around `Entity<EditorView>`, adds an entity layer and a second focus
   handle for no gain.
2. **May `fe-shell` depend on `log`?** §2.1 keeps it to the five crates, so
   the dozen or so log calls that move become `fe_shell::report`, and that
   goes to a handler the host installs.
   **Recommended:** keep the five. `report` and its handler are a few lines,
   and fastedit's handler calls `log`, so its logs read as before. Allowing
   `log` would add no lock entry. But the list would become "five plus
   exceptions", and the next exception would be easier to make.
3. **How does a host get its own icons?** `fe_ui::Icon` is a closed enum of
   bundled Lucide SVGs (`crates/fe-ui/src/file_icons.rs`).
   **Recommended:** add the host's icons to fe-ui's bundle as new variants,
   since it's this repository and the icons are Lucide's. Add an open
   `Icon::Path` only if the host needs icons from outside the bundle.
4. **Restore the saved layout on launch?** **Recommended:** yes for a folder
   launch (every window saved for the root) and for a new window on a
   folder (the newest window entry no open window holds), as §4.6 says. A file launch
   (`fastedit file.rs`) opens just that file, as today. The host decides for
   itself.
5. **One dock or several?** Today there is one dock, on the right.
   **Recommended:** keep one right dock through S3 and S6. Add a
   `DockPosition` (left, bottom) in its own card once the host asks for one.
   `DockItem` doesn't change for it.
6. **Can tabs move between windows?** **Recommended:** no, not for now.
   `Peers` and tab drag are per window, and nothing asks for it.
7. **Does the command palette list commands that take args?**
   **Recommended:** no. List commands whose `args_schema` is `None`, and add
   arg prompting with agent tooling. Also recommended: make the toolbar's
   "…" open the palette filtered to `pane.*` once S9 lands, retiring
   `toolbar_picker.rs`'s hard-coded `PANE_COMMANDS`. That needs its own
   small card.
8. **Where do layouts live?** **Recommended:** the state directory,
   `fe_config::paths::state_dir()`, one file per root with an entry per
   window (§4.6). Not
   `<root>/.fastedit/`: that would put machine state into repositories.
9. **Names.** **Recommended:**
   - Keep `Workbench` and `fe-workbench` for fastedit's layer. With the
     re-exports in §2.4, that leaves `crates/fastedit` and fe-workbench's
     tests unchanged.
   - Name the shell's root `Shell`.
   - Keep the `workbench.*` command IDs, the `workbench` key context and
     the `fastedit_menu` menu key context as they are, so no user keymap
     breaks.
   - Keep the `fastedit` action namespace of `RunCommand` and `MenuCommand`.
     It is internal: GPUI's own keymap is unused.
10. **Can a quit the system starts be cancelled?** No, not in `gpui-pre`
    0.3.7 (§3.6). AppKit's `applicationShouldTerminate:` isn't wired to the
    app, and `on_app_quit` can't cancel.
    **Recommended:** accept this for now. Layouts are still saved, and the
    quits we start ourselves (cmd-q, the app menu, closing the last window)
    all ask. Revisit it at a monthly GPUI bump. If the user wants it sooner,
    the way in is a small `[patch]` on the pinned `gpui-pre-macos` that
    routes `applicationShouldTerminate:` through a callback the app sets, as
    `on_window_should_close` does for windows. That patch would be its own
    card.
