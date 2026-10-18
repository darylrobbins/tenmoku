---
task_uid: "01m55c0ycvgmajvc2yr55s7rcy"
task: "T143"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extension_bindings_sit_between_app_and_user: an extension binding on new keys runs its command; one on an app binding's keys and context wins over it; the user's keymap.toml binding on the same keys wins over the extension's"
    required: true
    manual: false
  - id: "C2"
    text: "extensions_cannot_unbind_or_remove: command = \"\" in an extension keymap is dropped with a warning and the app binding stays; a binding to an unknown command, a menu item with remove, and a menu item reusing an app item's ID are each dropped with a warning"
    required: true
    manual: false
  - id: "C3"
    text: "extension_menu_items_are_added: an item placed after an app item's ID appears there, a new menu goes before Window, and its shortcut label comes from the keymap"
    required: true
    manual: false
  - id: "C4"
    text: "extension_keys_reload_and_never_fail_fast: editing the extension keymap then sending a batch swaps the binding, a file with errors keeps its last good layer, and with fail_fast on its diagnostics don't panic"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check_kiln_boundary.py passes"
    required: true
    manual: false
---

# Lay extension keymaps between the app's and the user's, and add extension menu items, add-only (Mac T843)

Filed on argon from Mac T843 (card ~/coordination/argon-cards/zzn/T843.json rev 1; note zzn, kiln extensions stage 1: declarative, data only). Mac task uid 01m55bz527ebmtssk02snd8by5. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Respect scripts/check_kiln_boundary.py and give every new kiln file its SPDX header line. Argon deps: T141 (Mac T841), T142 (Mac T842) (T843 runs after T842 because both edit kiln/src/extensions.rs). Title the host commit "... (Mac T843)".

## Problem

Stage-1 extensions contribute keymap and menu layers (`contributes.keymaps`,
`contributes.menus`, extensions record §2, §4). kiln already layers several default keymaps
under the user's file (`KeymapStore::with_defaults`/`set_defaults`,
`kiln/crates/kiln-core/src/load.rs`), re-resolves keymap and menus as commands are contributed
(`kiln/crates/kiln/src/contributions.rs` `shipped` ~1363, `resolve` ~1499), and merges menu
layers (`MenuBar::extend`, `kiln/crates/kiln-core/src/menu.rs` ~222). Extensions' layers go
nowhere yet.

## Change

Order, lowest first: kiln's defaults, the app's defaults, each extension's layers (by id,
then file order), the user's `keymap.toml`. Menus: the app's table, then each extension's.

- `kiln/crates/kiln/src/contributions.rs` `shipped()`: after `config.own_keymap(..)`, append
  each loaded extension's keymap layers, parsed for this platform with
  `KeymapLayer::parse_for` and checked against the same catalog; then merge each extension's
  menu layers (`MenuBar::parse_in` with the app's `menu_shortcut_context`) into the bar with
  `MenuBar::extend`. Their diagnostics are kept apart from the app's, so `check_fail_fast` sees
  only the app's own, and are published with the extensions' (`ConfigDiagnostics`).
- `kiln/crates/kiln/src/extensions.rs`: before a layer is used,
  - a keymap entry with `command = ""` (an unbind) is dropped with a warning;
  - a binding to an unknown command is dropped with a warning (the catalog does this);
  - a menu item with `remove`, or whose ID the app's table has, is dropped with a warning:
    extensions add menu items, they don't take the app's away.
  An extension binding with the same keys and context as an app binding wins over it (later
  layer), as a user binding wins over both; `Keymap::report` names the hidden one.
- A rescan that changes an extension's keymap or menu files, or loads or drops an extension,
  re-runs `resolve` (the keymap store's defaults swapped through `SharedKeymap::update`, menus
  reinstalled); a layer with errors keeps its last good version.

Nothing in `kiln/` names fastedit (`scripts/check_kiln_boundary.py`); new kiln files start
with the SPDX line. Test: `kiln/crates/kiln/tests/extension_keys_menus.rs` (headless, a
`kiln::commands!` app as in `kiln/crates/kiln/tests/config.rs`, the extension in a
`tempfile` dir, reloads sent as batches).

## Out of scope

Extensions registering commands (stage 2); a user menu file.

## Implementation notes

- `init` now discovers extensions before it resolves the keymap and menus, so the
  first resolution lays their files in. `ExtensionSource` reads the keymap and menu
  files into a shared `extensions::Files` global; `shipped()` parses them
  (`extensions::keymap_layers`, `extensions::extend_menus`) and keeps the last good
  text of each file. Their diagnostics travel in `Shipped::extension_diagnostics`,
  outside `Shipped::diagnostics()`, so `check_fail_fast` never sees them; after a
  later resolution they replace their previous selves in `ConfigDiagnostics`, as
  `Layers::select` does for theme choices.
- "Whose ID the app's table has" is checked against the bar as merged so far, so an
  extension can't replace another extension's item either.
- T142 F1: every contributed file (themes, keymaps, menus) is canonicalized and
  checked to be inside its extension's directory when it's read; one that leads
  outside keeps its last good version, unread, with an error on the file.
- T142 F3: kiln-core's manifest check no longer refuses an extension over a
  contributed file that isn't there yet, when it's written without `..` under a
  directory inside the extension's; the reader reports it ("no such file") and the
  sibling files load. `missing/../x` and a missing file through a link out are still
  manifest errors.
