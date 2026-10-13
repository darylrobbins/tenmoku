---
task_uid: "01m4vbhypyse78kkxw2fje2z08"
task: "T666"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/default_keys.rs: with Config::default_keys(true) and an app keymap without them, the picker keys, text_input.backspace and delete_word_left, command_palette.toggle (secondary-shift-p) and app.quit (secondary-q) resolve on macOS and Linux, and menu_bar.open (f10) only off macOS"
    required: true
    manual: false
  - id: "C2"
    text: "default_keys.rs: an app binding on the same key overrides the bundled one and the user's keymap overrides both; with Config::new's default (default_keys off) the keymap holds only the app's and user's bindings, as today"
    required: true
    manual: false
  - id: "C3"
    text: "default_keys.rs: Config::default_menus(true) with no menu table installs the bundled app, View and Window menus named after the app, every item's command known; an app menu table replaces them; the bundled keymap and menus parse with no diagnostics as on macOS, Linux and Windows"
    required: true
    manual: false
  - id: "C4"
    text: "kiln tests/fail_fast.rs: with Config::fail_fast(true), init panics listing each diagnostic with its file when the app's own keymap or menus have an error or a warning (an unknown command); a user keymap file with an error does not panic and keeps its last good layer; fail_fast off (the default) returns the diagnostics as today"
    required: true
    manual: false
  - id: "C5"
    text: "Existing kiln, kiln-core and kiln-platform tests, and fastedit's keymap and menu tests, pass unedited"
    required: true
    manual: false
---

# Give kiln a bundled default keymap and menu table and an opt-in fail-fast on the app's own config errors

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1, workspace and defaults; the design record
is `docs/decisions/kiln-app-layer.md`, from the docs card, which may not be on main yet).

Every kiln app copies the same keys kiln-shell's picker and palette need: `kiln/crates/kiln/examples/host/keymap.toml`
lines 36-81 bind `command_palette.toggle`, `app.quit`, `menu_bar.open` (F10, not macOS) and the
`picker.*`/`text_input.*` keys, under the comment "kiln-shell's picker leaves them to the app". An app with no
menu table gets no menus at all. And a typo in an app's own bundled keymap or menus is only a logged warning at
run time (`kiln-core/src/keymap.rs:506`, `kiln-core/src/menu.rs:733`), so a developer finds it late.

`KeymapStore` (`kiln/crates/kiln-core/src/load.rs:59`) holds a single default layer, so kiln can't put a layer of
its own under the app's.

## Change

All three are **opt-in on `Config`**, off by default, so `kiln::init` behaves as today for fastedit (its keymap
tests count its bindings) and for the existing examples. The `kiln::app` builder (a later card) turns them on.

1. **Base layers in `KeymapStore`.** Add `KeymapStore::with_defaults(layers: Vec<KeymapLayer>, diagnostics,
   user, catalog)`: the layers in order (later wins), then the user's file. `with_default` stays and calls it.
2. **`Config::default_keys(bool)`.** A bundled keymap, `kiln/crates/kiln/src/setup/keymap.toml`, parsed for the
   platform and checked against the catalog like the app's, laid *under* the app's keymap: the bindings the host
   example's `keymap.toml` lines 36-81 hold, with `command_palette.toggle` on `secondary-shift-p` in no context
   (global) rather than the host's `chat`. The app's keymap and the user's override it key by key.
3. **`Config::default_menus(bool)`.** A bundled menu table, `kiln/crates/kiln/src/setup/menus.toml`, installed only
   when the app gives no menu table: an app menu named after the app (Hide, Hide Others and Show All on macOS
   only, then Quit), a View menu (Command Palette, Toggle Full Screen) and a Window menu (Minimize, Zoom, Close
   Window), naming only kiln's own commands (`app.*`, `window.*`, `command_palette.toggle`). The app menu's
   name and its Quit title carry the app's name (`Config`'s `app_name`), put into the table before it's parsed.
4. **`Config::fail_fast(bool)`.** When on, `init` panics if the app's own keymap or menus, or the bundled
   defaults, have any diagnostic (error or warning), with the message listing each diagnostic with its file and
   line. The user's files never panic: they keep today's last-good behaviour.

Put the new code in `kiln/crates/kiln/src/setup/defaults.rs`, declared from `setup.rs` (`mod defaults;`), so
`kiln/src/lib.rs` isn't touched. Document each method with what it does by default and that `kiln::app` turns
it on.

New tests go in `kiln/crates/kiln/tests/default_keys.rs` and `kiln/crates/kiln/tests/fail_fast.rs`.

## Implementation notes

- `KeymapStore::with_default` passes its one layer (empty if `None`) to `with_defaults`, so a store built
  the old way still numbers the user's layer 1 in `Binding::layer`.
- `Config::keymap_for` and `Config::menu_bar_for` honour `default_keys` and `default_menus` too, so an app's
  tests can check the keymap and menus as on another platform with the bundled ones in.
- The bundled files' diagnostics name `<kiln keymap>` and `<kiln menus>`. The menus' `{app}` placeholder in
  titles is replaced with the app's name escaped for a TOML basic string, so a name with `"` or `\` still
  parses. The app menu itself is parsed under a fixed name and renamed after, not before (review F1): menus
  are known by name, so an app called `View` or `Window` would repeat a bundled menu's name, which the parser
  rejects. The bundled menu it shares its name with is folded into the app menu after a separator instead.
- The fail-fast check runs in `init` after both the keymap and the menus are parsed and before either is
  installed, over their diagnostics only: the user's keymap is read later, by the store, and never checked.
- The bundled Window menu shows Minimize and Zoom on every platform, as the spec lists them; only the
  Hide/Hide Others/Show All trio is macOS-only.
