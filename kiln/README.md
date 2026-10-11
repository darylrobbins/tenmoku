# kiln

kiln is an application framework on [GPUI](https://www.gpui.rs/), the UI
framework from the Zed editor. It hides what differs between platforms and
gives apps high-level widgets, and it's written in GPUI's own idioms.

The name `kiln` is a placeholder. The final name is chosen before the
framework is published, and the rename is done in one change.

What it takes care of, so that apps don't each branch on the platform:

- **Menus.** Native menus on macOS and an in-window menu bar elsewhere, built
  from one menu table, with roles (`settings`, `quit`, `about`) that each
  platform places by its own conventions.
- **Keys.** One keymap per app: `secondary-` is cmd on macOS and ctrl
  elsewhere, a binding can name different keys per platform, and the same
  binding labels its menu item and shortcut hints.
- **Windows.** The title bar and frame for each platform: unified on macOS,
  drawn by the window manager on X11, and drawn by the app on Wayland
  desktops without server-side decorations.
- **The rest.** Prompts (native where they exist, in-window elsewhere),
  file dialogs with a fallback when the portal is missing, fonts, config
  paths, and live reload of the user's settings, keymap and theme.

It targets macOS and Linux (X11 and Wayland) now. It's structured so that
Windows is an addition: only two directories may branch on the operating
system (see [Boundary rules](#boundary-rules)).

## Crates

| Crate | GPUI | Holds |
|---|---|---|
| `kiln-command` | no | command IDs, invocation records, context predicates, headless command registries |
| `kiln-core` | no | config layers and typed settings stores, app paths, platform conventions, keymaps and menu tables resolved per platform, theme data, the trust store, a headless text-edit model |
| `kiln-platform` | yes | platform capabilities, window options, prompts, fonts, dialogs, key input and its `KeyOwner` hook, command dispatch, native menus, the config service |
| `kiln-ui` | yes | theme resolution (`cx.theme()`), icons (bundled Lucide plus an app's own) and the widget kit |
| `kiln-shell` | yes | the app shell: `Root` (title bar, window frame, in-window menu bar), the in-window prompt, `Shell` (a window's chrome, docks, status bar and modal layer under the app's root context), panes of `PaneItem`s (`ItemHandle`) split in a `PaneGroup` with tab drag and drop, docks of `DockItem` panels, the modal layer, workspace trust, status items, the picker, breadcrumbs and a background settings writer; close vetoes, the command palette, layout persistence and multiple windows are moving in |
| `kiln` | facade | re-exports of the above, `kiln::prelude`, `kiln::gpui`, `application()`, `init()` and `Config` |

An app depends on `kiln` alone: it re-exports GPUI as `kiln::gpui`, brings
GPUI's Linux backends, and `kiln::commands!` needs no `gpui` dependency of
the app's own.

`kiln-command` and `kiln-core` never depend on GPUI, so headless tools,
tests and agents can load the same commands, config and keymaps an app
uses.

The design is written up in [`docs/framework.md`](docs/framework.md), and the
command model in [`docs/command-model.md`](docs/command-model.md).

## Using it

There is no kiln `App` type. Setup is plain GPUI plus `kiln::init`, the
extension traits in `kiln::prelude`, and typed commands that are real GPUI
actions. This is how the gallery example starts, slightly shortened:

```rust
use kiln::{Config, WindowSpec, gpui::{App, AppContext as _}, prelude::*};

// Typed commands: unit-struct GPUI actions, titled by their doc line. Their
// kiln IDs are `notes.new_note` and `notes.delete_note` (NewNote::id()).
kiln::commands!(notes, [
    /// New Note
    NewNote,
    /// Delete Note
    #[command(context = "notes_list")]
    DeleteNote,
]);

fn main() {
    kiln::application().run(|cx: &mut App| {
        // Platform, commands, keys, menus, prompts, themes and the user's
        // config files, before the first window.
        kiln::init(
            cx,
            Config::new("com.example.Notes", "Notes")
                .commands(register) // the `register` kiln::commands! wrote
                .keymap("keymap.toml", include_str!("keymap.toml"))
                .menus("menus.toml", include_str!("menus.toml")),
        );
        let spec = WindowSpec::main("Notes");
        cx.open_window(kiln::window_options(&spec, cx), |window, cx| {
            let notes = cx.new(|cx| Notes::new(window, cx));
            // The platform's chrome around the app's view, and each
            // window's setup (focus, the system appearance).
            cx.new(|cx| kiln::Root::new(notes, window, cx).title("Notes"))
        })
        .unwrap();
    });
}
```

- **Handlers are GPUI's.** A view handles a command with
  `.on_action(cx.listener(Self::new_note))` under its `.key_context("notes")`.
- **Every dispatch carries an `Invocation`:** who asked (a user, an agent, a
  script), how (a key, the mouse, a menu, the palette), the command's args,
  and the context it was dispatched in. A handler reads it with
  `kiln::current_invocation(cx)`, and observers can record every one.
  `window.run_command(Invocation::of("notes.new_note")?, cx)` runs a command
  by ID, as a menu item or an agent does.
- **Keymaps and menus are TOML,** one file each for every platform:

  ```toml
  [[bindings]]
  keys = "secondary-n"            # cmd-n on macOS, ctrl-n elsewhere
  command = "notes.new_note"

  [[bindings]]
  keys = { macos = "alt-backspace", default = "ctrl-backspace" }
  command = "notes.delete_word"
  context = "text_input"
  ```

- **The prelude's extension traits** put kiln on GPUI's own types:
  `cx.theme()`, `cx.settings::<S>()`, `cx.set_theme_mode(..)`,
  `cx.binding_label(&NewNote::id())`, `window.run_command(..)` and
  `window.follow_system_appearance(cx)`. `window.prompt(..)` stays GPUI's;
  `init` makes it look the same everywhere.
- **Settings.** `Config::settings::<S>(user, project)` loads an app's own
  settings type (serde, with `#[serde(crate = "kiln::serde")]`), layered over
  its defaults and reloaded when the files change; read it with
  `cx.settings::<S>()`. `Config::user_keymap` and `Config::user_theme` follow
  the user's keymap and theme the same way.
- **Widgets.** Stateless ones (`Button`, `Checkbox`, `ListRow` …) are
  `RenderOnce` builders over a `div`, so GPUI's styling methods work on them.
  Stateful ones (`TextField`, `List`, `Tree`, `SplitPane` …) are views
  created with `cx.new(..)` that emit events for `cx.subscribe_in`. Widgets
  read `cx.theme()` while rendering, so a theme switch applies everywhere.

`kiln::Root` wraps the app's content view in what the platform leaves to
the app: macOS's unified title bar, an in-window menu bar where there are
no native menus (F10 opens it), and a window frame drawn by the app where
the compositor draws none (GNOME on Wayland). A modal layer is still to
come.

## The gallery

`crates/kiln/examples/gallery` is an app with no code but kiln's: every
widget on a page of its own, a theme switcher, and kiln's commands, keymap,
menus, prompts and settings working together.

```sh
cargo run -p kiln --example gallery
KILN_GALLERY_THEME=light cargo run -p kiln --example gallery   # or dark, system
```

It reads `settings.toml` (its `title`), `keymap.toml` and `theme.toml` from
its config directory (`~/.config/kiln-gallery` on Linux, or
`$KILN_GALLERY_CONFIG_DIR`) and applies them as they're saved.
`tests/gallery.rs` drives it headless.

## Building and testing

kiln is a Cargo workspace (`crates/*`), in edition 2024 with a minimum
Rust of 1.98. The repository it lives in pins the toolchain to 1.98.1, the
one gpui-pre 0.3.7's Zed revision uses; bump the two together.

From kiln's own checkout (inside a host repository's workspace, run them
from its root with `-p kiln-…` instead):

```sh
cargo build --workspace --all-targets
cargo test --workspace                         # or: cargo nextest run --workspace
cargo nextest run --workspace --profile ci     # with retries for the real-watcher tests
cargo deny check                               # licences, advisories, sources (deny.toml)
```

On Linux, GPUI needs its libraries' development packages. On Debian or
Ubuntu:

```sh
sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libvulkan-dev libfontconfig-dev libfreetype-dev libssl-dev libx11-xcb-dev \
  libxcb1-dev libxcb-xkb-dev libasound2-dev libzstd-dev clang cmake
```

The GPUI tests run headless and need no display; the gallery needs one
(Xvfb with `mesa-vulkan-drivers` will do).

A few tests use the operating system's real file watcher, which can deliver
late under heavy load. `.config/nextest.toml` gives them retries in its `ci`
profile and runs them one at a time.

## GPUI version

kiln pins `gpui-pre` (a published snapshot of Zed's GPUI) to exactly
`=0.3.7`. A bump is a change of its own, tested on every platform. kiln
re-exports it as `kiln::gpui`, so an app uses the same version.

kiln relies on a few GPUI items that are public but not a stable API:

- `kiln::commands!` implements `gpui::Action` by hand rather than through
  GPUI's derive, because the derive writes bare `gpui::` paths, which
  resolve only where an app depends on `gpui` itself. The impl names
  `gpui::MacroActionData`, `gpui::MacroActionBuilder` and
  `gpui::private::inventory`. A bump that changes them fails to compile in
  kiln-platform; `the_macro_makes_gpui_actions` and
  `the_macro_needs_no_gpui_of_its_own` test the result.
- GPUI 0.3.7 doesn't say whether the focused element was drawn in the last
  frame, so kiln-platform's `focus::is_drawn` asks with a probe action that
  only a global listener hears. A GPUI that exposes the dispatch tree, or a
  synchronous focus signal, would replace it (and lift the chord limit noted
  in `keys.rs`).
- `TextField` adapts GPUI's `examples/input.rs` (see
  [`THIRD_PARTY.md`](THIRD_PARTY.md)), so a bump should be checked against
  that example's changes.

## Boundary rules

These are checked on every change, so that kiln can be split out of the
repository it's developed in:

- Only `crates/kiln-core/src/platform/` and `crates/kiln-platform/src/` may
  branch on the operating system (`cfg!(target_os = ..)` and the like).
  Everything else asks `kiln_core::Os`, so it can be tested "as Linux" or
  "as macOS" on any host.
- No kiln crate depends on an app's crate, names one, or includes a file
  from outside this directory. An app's own names (its key contexts, its
  command namespaces) come in as parameters.
- Every `.rs` file starts with an `SPDX-License-Identifier` line, and every
  crate sets its own `license` field; they all agree.
- `kiln-command` and `kiln-core` don't depend on GPUI.

## Licence

kiln is not yet open source. Every crate's licence is
`LicenseRef-Proprietary`, set in each crate's own `Cargo.toml` rather than
inherited, and each source file has a matching SPDX header. Publishing it
is a later change: one `license` line per crate, the SPDX headers, and the
licence texts added beside this file.

Third-party code and assets in kiln are listed in
[`THIRD_PARTY.md`](THIRD_PARTY.md), with the notices in [`NOTICE`](NOTICE).
What may and may not be read or copied while working on kiln is in
[`CLEAN_ROOM.md`](CLEAN_ROOM.md); dependency licences are checked by
`cargo deny` against [`deny.toml`](deny.toml).
