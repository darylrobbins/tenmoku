# The kiln framework: design

This is kiln's standalone copy of its design record, kept inside kiln so
that its code can point here. The repository kiln is developed in keeps the
original decision record, with that repository's own history; this copy
leaves the host app out.

## 1. What it is

A reusable application framework on top of GPUI (`gpui-pre` =0.3.7) that
hides per-platform differences and gives apps high-level widgets, written in
GPUI's own idioms. It covers app chrome (the window frame, menus, prompts),
a platform layer (keys, dialogs, fonts, config paths, live config reload),
the shell (panes, docks, the modal layer, the palette, close vetoes, layout
persistence, multiple windows) and a widget kit (basic controls, data
views, layout and forms).

- **Where.** kiln is a self-contained directory, developed inside a host
  application's repository for now and to be split out with its history
  (`git filter-repo --subdirectory-filter kiln`) and published. So no kiln
  crate may depend on an app's crate, kiln code is its own or permissively
  licensed (`CLEAN_ROOM.md`), and every crate sets its licence itself, so
  that publishing changes one line per crate.
- **Platforms.** macOS and Linux (X11 and Wayland) now. Windows is an
  addition: only `kiln-core/src/platform/` and `kiln-platform/src/` branch
  on the operating system (see §4), and the GPUI-free crates are checked
  to build for Windows.
- **Why.** Every GPUI app that runs on more than one platform answers the
  same questions: native menus on macOS but an in-window menu bar on
  Linux; a unified title bar, a window-manager frame or one the app draws;
  native alerts or an in-window prompt; cmd or ctrl in keymaps and labels;
  a file-dialog portal that may be missing; font fallback; where config
  lives. Left to each app, those answers end up as platform switches spread
  through its code, with a keymap and a menu table per platform to keep in
  step.

## 2. Crates

| Crate | GPUI | Holds |
|---|---|---|
| `kiln-command` | no | command IDs, invocations, context predicates, registries |
| `kiln-core` | no | config layers and settings stores, app paths, platform conventions, keymaps and menu tables resolved per platform, theme data, the trust store, the saved-layout store, a headless text-edit model |
| `kiln-platform` | yes | platform capabilities, window options, prompts, fonts, dialogs, the key interceptor and its `KeyOwner` hook, command dispatch, native menus, the config service |
| `kiln-ui` | yes | theme resolution, icons and the widget kit |
| `kiln-shell` | yes | the `Root` view (window frame, in-window menu bar, modal layer) and the shell: panes, docks, palette, status, persistence, windows |
| `kiln` | facade | re-exports, the prelude, `kiln::gpui`, `application()` |

`kiln-command` and `kiln-core` stay GPUI-free, so headless tools, tests and
an app's AI backend can load the same commands, config and keymaps.

## 3. API: default the composition, wrap nothing on the paint path

- **Setup** is plain GPUI: `kiln::application().run(|cx| …)`, then
  `kiln::init(cx, Config::new(app_id, name))`, then `cx.open_window(
  kiln::window_options(&spec, cx), |window, cx| …)`, whose view is
  `cx.new(|cx| Root::new(content, window, cx))`. This manual path stays as
  documented. On top of it kiln offers `kiln::app(id, name)` and attribute
  macros (§5), under one rule: *default the composition; wrap nothing on the paint
  path; every convention has a one-line override, and every sugar has a
  GPUI-shaped escape hatch.* The framework's owner approved this on
  2026-10-13, replacing the earlier "no kiln `App` type". `&mut App` and
  `Window` stay reachable everywhere. Library crates register commands and
  other contributions through an explicit `kiln::Module` passed to
  `.module(..)`; `inventory` auto-registration covers only the app crate's own
  items.
- **Extension traits** in `kiln::prelude`: `cx.theme()`, `cx.settings::<S>()`,
  `cx.set_theme_mode(..)`, `cx.binding_label(..)`, `window.run_command(..)`.
  `window.prompt(..)` stays GPUI's, made the same everywhere by `init`.
- **Commands are `gpui::Action`s** with kiln metadata (title, context,
  description), declared with `kiln::commands!` or `#[derive(Action)]` plus
  an `impl Command`, and handled with `.on_action(cx.listener(..))`. An
  action's GPUI name maps to its kiln command ID: `notes::NewNote` is
  `notes.new_note`. Every dispatch,
  from a key, a menu, the palette or an agent, carries an `Invocation`
  record (`command-model.md`). `RunCommand(Invocation)` stays only as the
  fallback for commands that live in a headless registry rather than as
  actions, such as a document model's.
- **Keys** keep kiln's own matcher, for a modal editing layer's priority
  (`KeyOwner`), chord replay, last-good hot reload with diagnostics, `.`
  and `==` in predicates, click bindings and headless per-platform tests.
  Bindings look like GPUI's (`KeyBinding::new("secondary-s", Save, None)`),
  contexts come from `.key_context(..)`, and keys kiln doesn't match fall
  through to GPUI's keymap. That keymap holds only what kiln doesn't
  match: the menus' display-only bindings, which kiln replaces on each
  keymap swap, and a widget's own GPUI bindings (such as `TextField`'s),
  which it leaves alone.
- **One keymap and one menu table per app.** `secondary-` is cmd on macOS
  and ctrl elsewhere; a binding can name keys per platform
  (`keys = { macos = "alt-left", default = "ctrl-left" }`) or be limited to
  some (`platform = "macos"`); menu items carry roles (`settings`, `quit`,
  `about`) that each platform places by its conventions. A menu item's
  shortcut is looked up in the app's own key contexts, which the app passes
  in: kiln names none of an app's contexts itself. Both are checked
  against every command kiln knows (`kiln::catalog`: the app's, kiln's,
  the shell's, and the picker's, whose keys the app binds) and the app's
  own catalogs.
- **The shell.** A window's `Shell` view lays out the app's centre view,
  the docks beside and below it, the status bar and the modal layer under
  the chrome, with the app's root key context, which the app passes in.
  Where an app keeps a piece's state in a view of its own (to read it
  without a context, or to add to what it does), kiln gives the state
  and a view trait as well as a ready view: `DockState` with `DockView`
  and `Dock`, and a `RootTrust` that any `TrustHost` view holds. The
  modal layer (`ModalLayer`, `ModalId`) shows any `ModalView`: a GPUI
  `ManagedView` that can be dismissed from outside, as `Picker` can.
- **View containers.** A `ViewContainer` is a dock panel (a `DockItem`
  with an id, title, icon and optional show command) that an app
  registers views into by id: `add_view(ViewSpec { id, title, order,
  default_collapsed, view }, cx)` puts each in the container's
  `ViewStack` under a collapsible header, top to bottom by `order`, views
  of equal order as they were added. A view added with an id that's
  there replaces it, moving to the new order but keeping its collapse and
  weight. Any panel can keep state of its own in a saved layout through
  `DockItem::save_state` and `restore_state`, which default to saving
  nothing and restoring nothing: `DockState::save` keeps each panel's
  under `"panels"` by its id, leaving the key out when no panel has
  state, and `DockState::restore` hands it back to the panels registered
  by then. A container's state is its stack's, `{"views": {"<id>":
  {"collapsed", "weight"}}}`, so a view's collapse persists through the
  workspace's layout. A saved view or panel that isn't there is ignored,
  and a layout saved before panels had state restores as it did. `Dock`
  hears its panels however they were registered, and notifies when one's
  saved state changes, so the workspace saves its layout as a view
  collapses or expands. A layout's shape, which decides those saves,
  leaves out only the window's bounds and pane items' states, never a
  panel's.
- **Closing asks first.** Every path that removes items (a tab, a tab
  dropped on a copy of its document, a pane, a window by command or by its
  close button, and quitting, window by window) asks them through
  `close::confirm`, skipping an item whose document stays open in another.
  When every item answers `Close`, the close happens in the caller's
  update. An item that asks the user makes the close a transaction: one at
  a time (a second request for the same close joins it; others wait,
  unless they have nothing to ask), each answer kept with the item's
  `close_revision`, and the scope read afresh after each answer, so an
  item that changed since it agreed, or came while the close waited, is
  asked again before anything closes. Hosts give each window's items with
  `close::set_window_items`; `app.quit` and `window.close` go through the
  same transactions.
- **The command palette** (`command_palette.toggle`, which `Shell`
  handles) lists the commands that apply in the context stack focus was
  in when it opened, with their keys: the app registry's, the typed ones,
  and those an app adds from its own registries
  (`command_palette::add_commands`), leaving out those with args. A pick
  gives focus back and dispatches the command there, from
  `Source::Palette`, in that context stack.
- **Saved layouts.** kiln-core's `layout::LayoutStore` keeps a file per
  root folder under the app's state directory, an entry per window, each
  window id claimed by one live window at a time with an OS file lock
  (released if the process dies), and every write a locked
  read-merge-write of the window's own entry. kiln-shell's `layout`
  saves a window's pane tree (ratios, items as `{kind, document, state}`,
  the active pane), its dock and bounds, and restores them: each
  document's first item through its registered kind, the others as its
  splits with their own state, so restored views share a document again.
  A `WindowLayout` saves as the layout's shape changes and in full at
  quit, keeps its entry when the last window closes, deletes it when
  others stay open, and releases its claim after its last write. A
  launch on a folder opens a window per entry no live window holds
  (`layout::launch`), and a new window on it takes the newest such entry
  (`layout::new_window`); a launch on a root another process has open
  gets fresh windows.
- **Widgets.** Stateless ones are `RenderOnce` builders over a `Div`, so
  GPUI's `Styled`, `InteractiveElement` and `FluentBuilder` methods work on
  them; values are controlled. Stateful ones are `Entity<T>` views that emit
  events (`cx.subscribe_in`). Widgets read `cx.theme()` while rendering,
  and so does the shell (its docks, panes, borders and pickers), so a theme
  change reaches the whole window at once. Constructors that still take a
  flat `UiTheme` ignore it; it goes in a later API cleanup.
- **GPUI internals kiln relies on.** gpui-pre is pinned (`=0.3.7`), and a
  bump is its own change. `kiln::commands!` implements `gpui::Action` by
  hand rather than through GPUI's derive, so that an app needs no `gpui`
  dependency of its own: the derive writes bare `gpui::` paths. That impl
  names `gpui::MacroActionData`, `MacroActionBuilder` and
  `gpui::private::inventory`, which are public but not stable. A bump that
  changes them fails to compile in kiln-platform, and
  `the_macro_makes_gpui_actions` and `the_macro_needs_no_gpui_of_its_own`
  test the result.

## 4. Boundary rules

Checked on every change while kiln lives in a host repository, by that
repository's boundary check, so kiln can leave it as it is:

- **No app in kiln.** No kiln crate depends on, names or includes (`#[path]`,
  `include_str!`, `include_bytes!`) anything outside `kiln/`. An app's own
  names, such as its root key context, come in as parameters; kiln's code
  outside tests holds no app's context names as string literals.
- **OS branches.** Only `kiln-core/src/platform/` and `kiln-platform/src/`
  may use `cfg!(target_os = ..)`, `cfg(unix)`, `cfg(windows)` or the like.
  Everything else asks `kiln_core::Os`, so it can be tested as each
  platform on any host.
- **Licences.** Every `.rs` file starts with an `SPDX-License-Identifier`
  line, and every crate sets its own `license` field (not inherited from a
  workspace); they all agree.
- **GPUI-free core.** `kiln-command` and `kiln-core` don't depend on GPUI.
- **Builds alone.** kiln, copied out into a workspace of its own with only
  the workspace entries its crates reference, builds and passes its tests
  offline against the host's lock file.

## 5. The app layer

`kiln::app(id, name)` and the attribute macros compose what an app would
otherwise write by hand over `init`, `Config` and `Workspace`. They follow
the rule of §3: *default the composition; wrap nothing on the paint path;
every convention has a one-line override, and every sugar has a GPUI-shaped
escape hatch.* `crates/kiln/examples/notes` is the smallest app on them, and
`README.md` shows it first.

### Conventions and their overrides

`app(id, name)` returns an `AppBuilder`. Until `run()` (or `start(cx)`, which
returns the windows it opened) it does nothing.

| Convention | Override |
|---|---|
| The short name is the name lowercased, each run of other characters than letters and digits a `-`, none at either end: `Kiln Chat` is `kiln-chat` | `.short_name(..)` |
| The app's directories are `AppPaths::new(&AppId::new(id, short, name), PathStyle::Native)`, so `<SHORT>_CONFIG_DIR` and `<SHORT>_STATE_DIR` move them | `.paths(..)` |
| The user's `keymap.toml` and `theme.toml` in the config directory, watched | `.config(\|c\| c.user_keymap(..).user_theme(..))` |
| `.settings::<S>()` reads `settings.toml` in the config directory, and no project file | `.config(..)` with `Config::settings`, which replaces the store |
| Each `#[kiln::settings]` section reads its table of that `settings.toml` | `.config(..)` with `Config::settings_file` |
| Closing the last window quits, except on macOS | `.quit_on_last_window(bool)` |
| Windows come from `WindowSpec::main` with the app's name and ID | `.window(\|spec\| ..)` |
| Each window is a `Workspace` of `ItemPane`s under the key context `workspace`, titled with the app's name, with the items, panels and status sources the app adds, then those contributed | `.key_context(..)`; `.root(..)` for a view of the app's own in a `Root` |
| The workspaces' layout is saved in the state directory's `layouts` (the state directory standing for the root folder); on start every saved window no live one holds opens again, else one fresh window | `.restore_layout(false)` |
| kiln's own keys go under the app's keymap, and its menus if the app has no table | `.config(..)` with `Config::default_keys` or `Config::default_menus` |
| In a debug build, a mistake in the keymap or menus the app ships panics at start | `.config(..)` with `Config::fail_fast` |
| A command's keys and menu item come from its attribute (`key`, `menu`, `order`) | `.keys(keymap!{..})` and `.menu_bar(menus!{..})`: typed tables, a typo a compile error; or `.keymap(..)` and `.menus(..)` files |

`.commands(..)`, `.keymap(..)`, `.menus(..)`, `.theme_mode(..)` and
`.theme_extensions(..)` are `Config`'s own, and `.config(..)` changes anything
else on it. A workspace window needs an item kind (`.item::<T>()`), a panel
(`.panel(..)`) or setup (`.workspace(..)`), or a contributed one; without any
of them, and without `.root(..)`, `start` panics saying so.

### The macros

Each macro leaves the item as written, adds what is listed, and records
itself with `inventory`; `app()` adds the records compiled into the app's own
binary. Each has a compile-fail test for the mistakes it can name.

| Macro | Expands to | By hand |
|---|---|---|
| `#[kiln::command(key, menu, order, id, context)]` on a fn | the fn; a unit-struct GPUI action named for it (`new_note` is `NewNote`, ID `<crate>.new_note`, titled by the fn's first doc line); a `CommandRecord` | `kiln::commands!`, then `Contributions::add_command(cx, CommandSpec::new::<A>(title).keys(..).menu(..))` and `cx.on_action(kiln::workspace_handler::<A>(f))` in a `Module` |
| `#[kiln::command(context = "..")]` on a view's `impl`, its methods marked `#[command(..)]` | the impl without the marks; `ViewCommands`, whose `attach(div(), cx)` sets the key context and a handler of each command; and, for each marked method, what a fn gets: a unit-struct action (`rename` is `Rename`, ID `<crate>.rename`, titled by the method's first doc line) and a `CommandRecord` carrying its context, `key`, `menu` and `order` | `kiln::commands!` for the actions, `Contributions::add_command(cx, CommandSpec::new::<A>(title).keys(..).menu(name, order).context(..))` for each in a `Module`, and on the view's root element `.key_context(..)` with an `.on_action(cx.listener(..))` for each |
| `#[kiln::model]` / `#[kiln::model(persist, file = "..")]` on a type | the type; a `ModelRecord` that makes it from `Default` at start, and with `persist` keeps it in `<state>/<name>.json` | `Contributions::add_model(cx, model::register::<M>)` or `persist::<M>(cx, name)` |
| `#[kiln::settings(section = "..")]` on a struct of named fields | the struct without its `#[setting]`s, with serde, `PartialEq`, `JsonSchema`, a `Default` of each field's `default = ..`, and `SettingsSection` | those impls by hand, then `Contributions::add_settings` and `Config::section` |
| `#[kiln::status(side, order, id)]` on `fn(&App) -> T` | the fn; a `StatusRecord` that calls it each time the bar draws | `Workspace::add_status_source(source, cx)` in each window |
| `#[kiln::panel(title, icon, dock, toggle_key, order, id, new)]` on a struct that implements `kiln::View` | the struct with a hidden `__kiln` field holding its focus handle; `Render` around `View::render`; `Focusable`; `DockItem`, with a toggle command, key and View-menu item; a `PanelRecord` | `Render`, `Focusable` and `DockItem` written out, then `Workspace::add_panel(position, panel, window, cx)` in each window |
| `#[kiln::item(kind = "..")]` on a struct that implements `kiln::Item` and `kiln::View` | the struct; `Render` and `Focusable`; `PaneItem` and `ItemState`, saving `Item::state` and restoring through `Item::new`, under the kind `<crate>.<name>`; an `ItemRecord` | `PaneItem` and `ItemState` by hand, then `layout::register_item::<T>` |

`WorkspaceExt::open::<T>(state, window, cx)` shows the item of a state in the
active pane (its tab if one has an equal state, else a new one), and
`active_state::<T>(cx)` reads it. Every macro's doc comment in `kiln` shows
its expansion and the same code written by hand, compiled as a doctest, so
the two cannot drift.

### The Module rule

`inventory` registration finds only what is linked into the app's own binary,
example or integration test. A library crate's `#[kiln::command]` (and its
models, panels, items, settings and status sources) is silently missing when
nothing references the crate, so nothing registers across crates by itself.
Instead a library crate declares `kiln::module!(pub fn module);`, which adds
what its macros recorded, and the app passes it on:

```rust
kiln::app("com.example.Notes", "Notes").module(outline::module).run();
```

A `Module` is any `FnOnce(&mut App)`, so a library crate may also write one
by hand with `Contributions::add_*`. `start` runs modules first, before
`init`, so what they add is set up with the app's own. In a debug build a
module that adds nothing logs a warning naming it, and so does a library
crate whose records are linked in but that no module claimed.

### `Cx` and `cx.watch`

`Cx<V>` is a view's `Window` and `Context<V>` together. Both are public
fields (`cx.window`, `cx.cx`) and it derefs to `Context<V>`, so every GPUI
call on a context works on it. `View::render(&mut self, cx: &mut Cx<Self>)`
takes one, and so do command handlers and `cx.handler(|view, cx| ..)`, a
listener for `on_click` and the like.

`cx.watch::<M>()` reads the model `M` and has the view redraw each time `M`
changes until the view is released. It observes `M` once per view however
often it is called, so no `Subscription` is stored or relayed with
`cx.notify()`. `cx.setting::<S>()` does the same for the app's settings, so
a reload redraws the view. `cx.model::<M>()` reads without watching,
`cx.update_model(..)` changes a model and notifies its observers,
`cx.workspace()` is the window's workspace, and `cx.service::<T>()` reads
what `kiln::provide(cx, value)` stored.

A handwritten `Render::render` gets a `Cx` from `kiln::with_cx(window, cx,
|cx| ..)`. A model is an entity under the hood (`model::entity::<M>(cx)`),
so `cx.subscribe`, `cx.observe` and explicit `Subscription`s still work
where an app wants them.

### Dropping to GPUI

Nothing the layer builds is out of reach, and no layer-owned type sits
between an app and the paint path.

- **`Cx` is two fields.** `cx.window` is GPUI's `Window` and `cx.cx` its
  `Context<V>` (an `App` by deref), for any call the layer has no name for.
- **A view is any `Render + Focusable`.** `.panel(..)` takes any `DockItem`,
  `.item::<T>()` any `PaneItem`, and `Workspace::add_panel` any
  `Entity<impl DockItem>`; `View`, `#[kiln::panel]` and `#[kiln::item]` only
  write those impls. A view written as plain GPUI keeps its own `Render`,
  and `Self::attach(div(), cx)` gives it the commands of its impl.
- **The workspace is open.** `Workspace::shell()` is the `Entity<Shell>`,
  `group()` and `group_mut()` the `PaneGroup`, and `dock(position)` the
  `Entity<Dock>`: everything the app layer called is a public call, and
  `.workspace(|workspace, window, cx| ..)` runs on each window's workspace
  before its layout is restored.
- **The window is yours.** `.root(|window, cx| view)` opens a `Root` around
  any `AnyView`, and the workspace's calls go unused. `Started::window` and
  `Started::windows` are `AppWindow`s: `window.root::<Workspace<ItemPane>>(cx)`
  gets the root view back, and `update(cx, ..)` is GPUI's `update_window`.
- **The whole layer is optional.** `kiln::init` with `Config` and `Root`
  (README, "Underneath") is the same setup written out, and `AppBuilder`'s
  docs print the one as the other. Macros and manual registration mix:
  `.contributions(|cx| Contributions::add_panel(cx, ..))` adds one by hand.

### What kiln deliberately does not do

- **No element macro.** A view's `render` returns GPUI's elements, built with
  GPUI's `div()` and its styling methods. There is no `rsx!` or `view!`.
- **No signals.** State is GPUI entities; `cx.watch` is an observation of
  one, not a new reactive primitive with its own arena or dependency graph.
- **No DSL and no code generation outside the macros above.** Keymaps and
  menus are TOML files or typed tables; nothing is compiled from a separate
  language.
- **Nothing on the paint path.** Kiln wraps no element, layout or paint
  call. `Cx` is built around a `render` call and costs two references;
  `#[kiln::status]` functions run on every draw of the bar, so they must be
  O(1) reads.
- **No hidden global app type.** `&mut App` and `Window` stay reachable
  everywhere, and every convention above is a default, not a requirement.
