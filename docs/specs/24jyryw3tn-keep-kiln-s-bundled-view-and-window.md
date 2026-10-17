---
task_uid: "01m5415p89ec880224jyryw3tn"
task: "T126"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln test default_menus_catalog::a_dropped_app_menu_renames_no_other_menu passes: with a catalog rejecting app.*, apps named Notes, Window and View each get menus View [command_palette.toggle, window.toggle_fullscreen] then Window [window.minimize, window.zoom, window.close] on MacOs, Linux and Windows; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln passes with no existing test edited (default_keys.rs's an_app_named_like_a_bundled_menu_keeps_every_menu and the init_installs_the_menus_of_an_app_named_* tests included); cargo clippy -p kiln --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep kiln's bundled View and Window menus named as they are when the catalog drops the App menu, instead of renaming View for the app (T666 review F2) (Mac T736)

## Problem

kiln's bundled menus (`kiln/crates/kiln/src/setup/menus.toml`: App, View, Window) are named for the app
after parsing by `name_app_menu` in `kiln/crates/kiln/src/setup/defaults.rs`, called from
`Config::menu_bar_for` (`kiln/crates/kiln/src/setup.rs`). It assumes the bar's *first* menu is the app
menu: it folds a later menu named like the app into `menus[0]` and renames `menus[0]` to the app's name.
But `MenuBar::parse_in` drops an item whose command the catalog doesn't know (a warning, "unknown command
...; the item is ignored") and then drops a menu left empty. With a catalog that knows none of `app.*`
(`app.hide`, `app.hide_others`, `app.show_all`, `app.quit`), the App menu is gone, View is first, and
`name_app_menu` renames View, or merges Window into it.

Codex raised this as T666 review F2 (medium, not carried to a card). Confirmed at Mac main c5e93ed6 with
a scratch binary calling `Config::new("com.example.X", name).default_menus(true).menu_bar_for(os,
Some(&catalog))`, the catalog rejecting every `app.*` command with `CatalogError::UnknownCommand` and
accepting the rest:

| app name | os | menus now | should be |
|---|---|---|---|
| `Notes` | MacOs, Linux | `Notes` [command_palette.toggle, window.toggle_fullscreen], `Window` [minimize, zoom, close] | `View` [...], `Window` [...] |
| `Window` | MacOs, Linux | `Window` [command_palette.toggle, window.toggle_fullscreen, window.minimize, window.zoom, window.close] | `View` [2 items], `Window` [3 items] |
| `View` | MacOs, Linux | `View`, `Window` (right by chance) | the same |

## Change

In `name_app_menu`, find the bundled app menu by its fixed name `App` (the name `menus.toml` gives it)
instead of taking the first menu. If there's none (the catalog dropped it), leave the bar as parsed: no
rename and no folding. Otherwise fold and rename as now, folding a menu named like the app into that App
menu and never into another. Update `name_app_menu`'s doc comment to say so. Nothing else changes:
`menu_bar_for` with kiln's own catalog, an app's own menu table, and the `View`/`Window` app-name cases
covered by `kiln/crates/kiln/tests/default_keys.rs` stay as they are.

New test file `kiln/crates/kiln/tests/default_menus_catalog.rs`, plain `#[test]`s (no gpui context is
needed: `menu_bar_for` takes the catalog as an argument), with a local `CommandCatalog` that rejects
`app.*` and accepts everything else, checking each menu's name and its command items' commands in order
on `Os::MacOs`, `Os::Linux` and `Os::Windows`.
