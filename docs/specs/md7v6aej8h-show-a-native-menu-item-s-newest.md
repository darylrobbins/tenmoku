---
task_uid: "01m53hc4nvawm215md7v6aej8h"
task: "T104"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-platform test menus_native_fallback::an_older_native_shortcut_is_shown_when_the_newest_has_none passes: with secondary-s bound to pad.save and a user layer adding secondary-enter, ctrl-tab or secondary-fn-s for it, the Edit/Save menu action has exactly one GPUI binding, secondary-s in this platform's spelling, in the menus' key context; with secondary-enter and then secondary-shift-s added, it is secondary-shift-s; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-platform -p kiln-core passes with tests/menus.rs unchanged; kiln-core is unchanged; cargo clippy -p kiln-platform --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show a native menu item's newest shortcut that macOS can display, instead of none when the newest binding has no native key equivalent (Mac T726)

Filed on argon from Mac T726 (card ~/coordination/argon-cards/T726.json rev 1; note zm). Mac task uid 01m53h5as8q2t0er3827t15dak. Base: argon trunk dd0ab81 (= mac/main e4f7677 synced; the code these touch is unchanged through mac/main 32c938f9). Title the host commit "... (Mac T726)".

## Problem

`kiln_platform::menus::refresh` (`kiln/crates/kiln-platform/src/menus.rs`) shows a native menu item's
shortcut by taking `CommandItem::shortcut` (`kiln/crates/kiln-core/src/menu.rs`: the first, i.e. newest,
single-keystroke non-click entry of `kiln_core::shortcuts`) and binding it for display only if
`native_key_equivalent` accepts it. When the newest binding has no native key equivalent, the item shows
no shortcut at all, even though an older binding that still runs the command does.

Example: the app binds `secondary-s` to Save and the user adds `secondary-enter` (or `ctrl-tab`,
`secondary-fn-s`) for the same command without unbinding `secondary-s`. Both keys run Save, but the File
menu now shows Save with no shortcut, and macOS also loses the key equivalent. The existing test
`shortcuts_without_a_native_key_equivalent_are_not_shown` (`kiln/crates/kiln-platform/tests/menus.rs`)
unbinds `secondary-s` in each case, so it never meets this. Verified by reading at 32c938f9.

## Change

- `kiln/crates/kiln-platform/src/menus.rs` `refresh`: instead of `command.shortcut(&keymap)`, walk
  `kiln_core::shortcuts(&keymap, &command.command, &command.args, &command.shortcut_context)` in its order
  (precedence first), keep single-keystroke entries whose key isn't `Key::Click` (as
  `CommandItem::shortcut` does), and bind the first one that parses as a GPUI keystroke and passes
  `native_key_equivalent`. The `log::debug!` lines stay for the entries skipped, once per skipped entry.
  Nothing else in `refresh` changes; `CommandItem::shortcut` and kiln-core are unchanged (the in-window
  menu bar in kiln-shell keeps showing the newest key, which it can draw).
- New test file `kiln/crates/kiln-platform/tests/menus_native_fallback.rs` (copy the few helpers it needs
  from `tests/menus.rs`; don't edit that file). Test `an_older_native_shortcut_is_shown_when_the_newest_has_none`:
  with the base keymap binding `secondary-s` to `pad.save`, a user layer adding each of
  `secondary-enter`, `ctrl-tab` and `secondary-fn-s` for `pad.save` leaves exactly one GPUI binding for
  the Edit/Save menu action, `secondary-s` in this platform's spelling, in the menus' key context; and a
  user layer binding both `secondary-enter` and `secondary-shift-s` (newest) shows `secondary-shift-s`.
  It fails at the base. `tests/menus.rs` passes unchanged.
