---
task_uid: "01m4tjjp6sg8czbb1pw5fjtbeq"
task: "T117"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-core test menu_click_shortcut::a_later_click_binding_does_not_become_the_menu_shortcut: with f12 then cmd-click bound to nav.goto_definition, the Go menu item's shortcut is f12"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-core test menu_click_shortcut::a_command_bound_only_to_a_click_has_no_menu_shortcut: with only cmd-click bound, the item's shortcut is None"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-core test menu_click_shortcut::the_newest_key_binding_is_still_the_menu_shortcut passes, and the existing menu tests (kiln-core, fe-keymap tests/menu.rs and menu_go_nav.rs, kiln tests/setup.rs) pass unchanged"
    required: true
    manual: false
---

# Never show a mouse click as a menu item's shortcut in kiln-core: CommandItem::shortcut skips click bindings, so f12 shows instead of a later cmd-click (Mac T656)

## Problem

`CommandItem::shortcut` (kiln/crates/kiln-core/src/menu.rs) says it returns "the key a menu shows, which
can't be a chord", but it only filters `shortcuts(..)` on `keystrokes().len() == 1`, so a mouse binding such
as `cmd-click` (`Key::Click`) qualifies. `shortcuts` lists the newest binding first (later layer, later
entry), so as soon as a click binding for a menu command comes after its key binding, the drawn menu bar
(kiln-shell menu_bar.rs) labels the item "⌘Click" / "Ctrl+Click", and the native macOS menu
(kiln-platform menus.rs `refresh`, which takes the same `shortcut`) finds no native key equivalent for it and
shows no shortcut at all, though f12 still works. fastedit avoids it today only by entry order: the comment
above `secondary-click` in crates/fe-keymap/keymaps/fastedit.toml says "Above f12, so menus and tooltips show
f12". Any user keymap layer that binds a click to a menu command (or a host layer appended later) brings it
back.

Checked 2026-10-13 at Mac e6988aba (menu.rs is identical on helium main, blob eb46260a): with a macOS keymap
`f12 -> nav.goto_definition` then `cmd-click -> nav.goto_definition`, the Go menu item's `shortcut(&keymap)`
is `Some(cmd-click)` (expected `f12`); with only `cmd-click` bound it is `Some(cmd-click)` (expected `None`).

## Change

In kiln/crates/kiln-core/src/menu.rs `CommandItem::shortcut`: take the first sequence of exactly one
keystroke whose key is not `Key::Click`, e.g. `.find(|keys| matches!(keys.keystrokes(), [key] if key.key != Key::Click))`,
and say so in its doc comment. `shortcuts` itself, the command palette and the toolbar tooltips are not
changed. The existing menu tests (kiln-core in-crate, fe-keymap tests/menu.rs and tests/menu_go_nav.rs,
kiln tests/setup.rs) pass unchanged. Choosing a native-capable key in kiln-platform menus.rs when the newest
key has no native equivalent (e.g. `cmd-enter`) is a separate issue and out of scope.

## Tests

New file kiln/crates/kiln-core/tests/menu_click_shortcut.rs (public API only: `KeymapLayer::parse_for`,
`Keymap::from_layers`, `MenuBar::parse_for` with `Os::MacOs`, one Go menu item `nav.goto_definition`):

- `a_later_click_binding_does_not_become_the_menu_shortcut` (regression, fails at base): bindings
  `f12` then `cmd-click` give `Some("f12")`.
- `a_command_bound_only_to_a_click_has_no_menu_shortcut` (regression, fails at base): only `cmd-click` gives
  `None`.
- `the_newest_key_binding_is_still_the_menu_shortcut` (control, passes at base): `cmd-click`, `f12`, `cmd-b`
  in that order give `Some("cmd-b")`.
