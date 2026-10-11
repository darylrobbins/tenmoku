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
| `kiln-core` | no | config layers and settings stores, app paths, platform conventions, keymaps and menu tables resolved per platform, theme data, the trust store, a headless text-edit model |
| `kiln-platform` | yes | platform capabilities, window options, prompts, fonts, dialogs, the key interceptor and its `KeyOwner` hook, command dispatch, native menus, the config service |
| `kiln-ui` | yes | theme resolution, icons and the widget kit |
| `kiln-shell` | yes | the `Root` view (window frame, in-window menu bar, modal layer) and the shell: panes, docks, palette, status, persistence, windows |
| `kiln` | facade | re-exports, the prelude, `kiln::gpui`, `application()` |

`kiln-command` and `kiln-core` stay GPUI-free, so headless tools, tests and
an app's AI backend can load the same commands, config and keymaps.

## 3. API: extend GPUI, never wrap it

- **Setup** is plain GPUI: `kiln::application().run(|cx| …)`, then
  `kiln::init(cx, Config::new(app_id, name))`, then `cx.open_window(
  kiln::window_options(&spec, cx), |window, cx| …)`, whose view is
  `cx.new(|cx| Root::new(content, window, cx))`. There is
  no kiln `App` type; `&mut App` and `Window` are reachable everywhere.
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
  in: kiln names none of an app's contexts itself.
- **Widgets.** Stateless ones are `RenderOnce` builders over a `Div`, so
  GPUI's `Styled`, `InteractiveElement` and `FluentBuilder` methods work on
  them; values are controlled. Stateful ones are `Entity<T>` views that emit
  events (`cx.subscribe_in`). Widgets read `cx.theme()` while rendering.
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
