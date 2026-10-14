---
task_uid: "01m4vbhzm7jzm9bxhvxe5sc39c"
task: "T667"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/app_builder.rs: kiln::app(dev.kiln.Chat, Kiln Chat) derives the short name kiln-chat and the same AppPaths as AppPaths::new(\u0026AppId::new(id, kiln-chat, name), PathStyle::Native); .short_name and .paths override them"
    required: true
    manual: false
  - id: "C2"
    text: "app_builder.rs: with .paths over temp dirs holding settings.toml, keymap.toml and theme.toml, start loads the user's setting (from .settings::\u003cS\u003e()), the user's binding and the user's theme"
    required: true
    manual: false
  - id: "C3"
    text: "app_builder.rs: .config(..) runs last on the built Config and can replace any convention (a different user keymap path is the one read)"
    required: true
    manual: false
  - id: "C4"
    text: "app_builder.rs: with quit_on_last_window(true) closing the last window quits (observed through close::set_quit), with false it does not; the default is true except on macOS"
    required: true
    manual: false
  - id: "C5"
    text: "app_builder.rs: .root(..) opens one window whose root view is kiln's Root around the given view, titled with the app's name and carrying the app id; kiln::app (the fn) and kiln::app::Quit (the commands module) both resolve"
    required: true
    manual: false
---

# Add kiln::app(id, name), a builder that applies kiln's path, user-file, settings and quit conventions, each with a one-line override

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`, from the docs card, which may not be on main yet). The approved rule:
*default the composition; wrap nothing on the paint path; every convention has a one-line override, and every
sugar has a GPUI-shaped escape hatch.*

Every kiln app wires the same things by hand before its first window:

- `kiln/crates/kiln/examples/host/setup.rs:37-56` builds `AppId::new(APP_ID, "kiln-chat", APP_NAME)` and
  `AppPaths::new(&id, PathStyle::Native)`, then attaches the user's `keymap.toml` and `theme.toml`;
- settings need `Config::settings(user, project)` with paths the app works out itself;
- both examples repeat "quit when the last window closes" (`host/main.rs:32-37`, `gallery/main.rs:27-32`);
- `main` calls `kiln::application().run`, `kiln::init`, `kiln::window_options` and `Root::new` itself.

## Change

Add `kiln::app(id, name) -> AppBuilder` in a new `kiln/crates/kiln/src/app_builder.rs`, re-exported from
`kiln/src/lib.rs` as the function `kiln::app` (it lives in the value namespace beside the existing
`pub mod app` of app commands, so `kiln::app::Quit` keeps working). `kiln::init` and `Config` stay as the manual
path underneath; the builder only composes them.

Conventions, each with its override:

| Convention | Override |
|---|---|
| Short name from the display name: lowercased, runs of non-alphanumerics become `-`, trimmed (`"Kiln Chat"` → `kiln-chat`) | `.short_name("..")` |
| `AppPaths::new(&AppId::new(id, short, name), PathStyle::Native)`, so `<SHORT>_CONFIG_DIR`/`_STATE_DIR` work | `.paths(AppPaths)` (tests use it with temp dirs) |
| The user's `keymap.toml` and `theme.toml` in the config dir, watched | through `.config(..)` |
| `.settings::<S>()` reads `settings.toml` in the config dir (no project file) | `.config(..)` with `Config::settings` |
| Quit when the last window closes, except on macOS | `.quit_on_last_window(bool)` |
| One window from `WindowSpec::main(name)` | `.window(\|spec\| spec…)` |

Forwarders to `Config`: `.commands(register)`, `.keymap(path, source)`, `.menus(path, source)`,
`.theme_mode(..)`, `.theme_extensions(..)`. `.config(|config| config…)` is the escape hatch: it runs last, on
the `Config` the conventions built, and can change anything.

The window: `.root(|window, cx| -> AnyView)` opens it as kiln's `Root` around that view, titled with the app's
name, through `kiln::window_options` (so the app id is on it). In this card `.root(..)` is required; a later card
makes a `Workspace` the default window and turns on the bundled default keys, default menus and debug
fail-fast.

`run(self)` is `kiln::application().run(..)` with `start` inside, then `cx.activate(true)`. `start(self, cx:
&mut App) -> Started` does the work inside an existing app and returns the init diagnostics and the opened
window handle, so tests drive it headless. `app_paths(&self) -> &AppPaths` reads the resolved paths.

Rustdoc on `kiln::app` lists every convention and its override, and shows the manual `kiln::init` equivalent.

New tests go in `kiln/crates/kiln/tests/app_builder.rs`.
