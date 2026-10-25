---
task_uid: "01m5stck63xms5p6yj3y5z4ahz"
task: "T341"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell test tab_middle_click::middle_click_closes_tab: a middle click (MouseButton::Middle) on an inactive tab and on the active tab of an ItemPane closes that tab and no other, as the x button does (closing the active tab activates the neighbour Close Tab would); a left click and a right click do not close"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell test tab_middle_click::dirty_tab_keeps_open: a middle click on a dirty tab goes through the same may_close veto as the x button and Close Tab (stays open, asks if the item asks)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test middle_click_close_tests::pushes_closed_editor_stack: middle-clicking a file tab closes it and workbench.reopen_closed_editor (T1145) reopens it with its selections and scroll"
    required: true
    manual: false
  - id: "C4"
    text: "nextest and clippy pass in the root workspace and in kiln/"
    required: true
    manual: false
---

# Middle-click on a tab closes it, as VS Code does (Mac T1171)

## Problem
VS Code closes a tab on middle-click (the default `workbench.editor.tabActionCloseVisibility` tab behaviour; middle button on a tab). fastedit has no middle-click close: `kiln_ui::Tab` (kiln/crates/kiln-ui/src/tab_bar.rs) only has `on_click` (activate / double-click pin), `on_close` (the x button), `on_drag` and `on_drop`.

## Current state
- `Tab::render` (tab_bar.rs ~line 349) wires `on_click`; gpui has `StatefulInteractiveElement::on_aux_click` for non-primary buttons.
- `PaneState::tab_bar` (kiln/crates/kiln-shell/src/pane.rs ~line 562) builds each tab with `on_click`/`on_close`; the x handler does `activate_item(ix)` then `close_active_item`, which asks `PaneItem::may_close` through `close::confirm` (a dirty item stays open).

## Change
- `Tab::on_middle_click` (a handler, fired from `on_aux_click` when `event.is_middle_click()` / the middle button); wired in `Tab::render`.
- `PaneState::tab_bar` gives it the same listener as `on_close`, so middle-click is exactly the x button: same veto, same neighbour activation.
- The x button's own click must not also middle-close (it already stops propagation).
- Closing this way ends in `Pane::sync_tabs`, so it pushes onto T1145's closed-editor stack with no extra code.

## Tests
New `kiln/crates/kiln-shell/tests/tab_middle_click.rs` (model on tab_icon_close.rs) and `crates/fe-workbench/src/middle_click_close_tests.rs` (add `mod` in lib.rs next to closed_editors_tests).

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y60).
