---
task_uid: "01m543nc5zte8xzqfbdd6qt52w"
task: "T134"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests menu_bar_identity an_open_menu_removed_before_a_sibling_closes, an_open_menu_that_moves_stays_open_on_it and an_item_moved_before_the_highlight_keeps_it_on_its_command pass: an open menu removed while a sibling survives closes with focus back and Enter running nothing; a surviving open menu that moves stays open at its new index; a highlighted command that moves keeps the highlight and Enter runs it"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-shell passes with no existing test edited (tests/menu_bar.rs unchanged)"
    required: true
    manual: false
---

# Keep the in-window menu bar's open menu and highlight on the same menu and command when the installed menus change, closing it when its menu goes (T673 review F7) (Mac T738)

## Problem

T673 review F7 (codex, review 2m8723, medium): the in-window menu bar (Linux) tracks the open menu and its
highlighted item by position only. `kiln/crates/kiln-shell/src/menu_bar.rs` `OpenMenu { menu: usize, item:
Option<usize> }`; `MenuBarView::reconcile` (around line 168) closes the menu only when `bar.menus.get(open.menu)`
is gone and drops the highlight only when that index is no longer a command; `Render for MenuBarView`
(around lines 270-282) repeats the positional check. When the installed menus change while one is open (a
command contributed or removed after init, `kiln_platform::menus::install`), the open menu and highlight
silently land on a different menu or command.

Reproduced at Mac main 5ce93c05 with the shell's test app (`tests/support`: menus Pad [Clear, separator,
Select All], Edit [Select All], Window [Close Window]):

- F10 opens Pad with Clear highlighted, `open_menu() == Some((0, Some(0)))`. Install the same bar without
  Pad: `open_menu()` stays `Some((0, Some(0)))`, now Edit's Select All, and Enter runs `pad.select_all`.
  Expected: Pad is gone, so the menu closes, focus goes back to the pad and Enter runs nothing.
- F10, Down highlights Pad's Select All, `(0, Some(2))`. Install a bar whose Pad lists Select All first
  (Select All, Clear, separator): index 2 is now the separator, the highlight is dropped and Enter runs
  nothing. Expected: the highlight follows Select All to index 0 and Enter runs `pad.select_all`.

The existing regression (`tests/menu_bar.rs` `a_menu_removed_while_open_closes`) only removes the whole bar.

## Change

- `menu_bar.rs`: `OpenMenu` also records the open menu's name and the highlighted item's `CommandId`, set
  wherever `menu`/`item` are set (`open`, the Up/Down keys, panel hover). `reconcile` re-finds both by
  identity in the new bar: the open menu is the first menu with that name (none: close and give focus back,
  as now); the highlight is the first command item in it with that id (none: no highlight). It updates the
  stored indices when they moved and notifies.
- `Render for MenuBarView` draws the open menu and highlight by that same identity lookup (share one helper
  with `reconcile`), and defers `reconcile` whenever the stored indices differ from that lookup (not only when
  the menu is gone), so `open_menu()` reports the new positions after the next frame, as the close does now. Left/Right, Enter and clicks
  then act on the reconciled indices.
- New integration test file `kiln/crates/kiln-shell/tests/menu_bar_identity.rs` (`mod support;`, its own
  `open_menu`/`ran` helpers copied from tests/menu_bar.rs, which stays unchanged), with three tests at the
  file's top level, each failing at the base:
  - `an_open_menu_removed_before_a_sibling_closes`: the first case above (menu closes, focus back, Enter
    runs nothing).
  - `an_open_menu_that_moves_stays_open_on_it`: F10 Right opens Edit `(1, Some(0))`; remove Pad; Edit stays
    open at `(0, Some(0))` and Enter runs `pad.select_all` (at base it stays `(1, Some(0))`, now Window's
    Close Window).
  - `an_item_moved_before_the_highlight_keeps_it_on_its_command`: the second case above, asserting
    `open_menu() == Some((0, Some(0)))` before Enter.

No change to kiln-platform, kiln-ui or the other shell files.
