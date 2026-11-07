---
task_uid: "01m6rmda61bchh02w349889e1b"
task: "T477"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test pane_pin_tests::pinning_moves_the_tab_to_the_pinned_group: pinning the third of four tabs puts it first; pinning another puts it after the first pinned; unpinning puts it right after the pinned group; reordering cannot cross the boundary"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test pane_pin_tests::bulk_closes_skip_pinned_tabs: close_others, close_all_editors and close_saved leave pinned tabs, while close_tab on a pinned tab closes it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test pane_pin_tests::pinned_flags_round_trip_the_layout: save then restore keeps order and pinned flags"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test layout_compat_tests::a_layout_without_pinned_flags_loads_unpinned: a layout saved by the previous build restores all tabs unpinned (add the case to this existing file)"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test pane_pin_tests::pin_commands_are_in_the_catalog: workbench.pin_tab and workbench.unpin_tab are registered and run on the active tab"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test pane_pin_tests::a_pinned_tab_lays_out_at_icon_width: the strip's tab width for a pinned tab is the icon width, with the title in its tooltip"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Pin tabs: pinned tabs stay left at icon width, survive Close Other Tabs, Close All Editors and Close Saved, and are restored with the layout (Mac T1400)

Lent from Mac T1400 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

VS Code lets you pin a tab: a pinned tab shrinks to its icon, stays at the left of the strip, and Close Other Tabs, Close All and Close Saved leave it alone (a direct Close Tab on a pinned tab still closes it, as in VS Code). Tenmoku has no pinning. Verified missing: no `pin` anywhere in `fe-workbench`/`fe-gpui` tab code. Close Other Tabs (`Pane::close_others`, `close_others_tests.rs`), Close All Editors, Close Saved and middle-click close (`middle_click_close_tests.rs`) all exist now, so the dependency noted in the T1145 memory is cleared.

Linux-OK: pane model tests are headless; the compact pinned look is checked by a layout test, no screenshot.

## Change

- `crates/fe-workbench/src/pane.rs`: per-tab `pinned` flag; `Pane::pin(index)`/`unpin(index)`; pinning moves the tab to the end of the pinned group, unpinning moves it to just after it; a drag or reorder cannot cross the boundary. `close_others`, `close_all` and `close_saved` skip pinned tabs; `close_tab` on a pinned one closes it. Tab order and pinned flags are saved in the window layout and restored (`layout_compat_tests.rs`: an old layout with no flag loads unpinned).
- Commands `workbench.pin_tab` / `workbench.unpin_tab` (palette: Pin Tab, Unpin Tab; no tab context menu was found, so palette only, plus a tab-strip pin click only if a menu exists by then). Keybinding: none by default.
- The strip draws a pinned tab at icon width with a small pin glyph (use the icon seam in kiln-ui, T1217 style, no new asset format if Lucide has `pin`); title in tooltip.
- Vim: no mapping. `:tabo` follows `close_others`, so it skips pinned tabs too.

## Out of scope

Pinned tab rows (second strip row), pin from the project tree.


## Helium notes
- New bindings or commands have broken count tests before (fe-gpui binding counts / DEDICATED, the git_keys chord, crates/fastedit/src/vim_remaps.rs for lazyvim.vimrc). Run the affected crates' tests early and respec any count-pinning test file into scope before the first finish (`rgt task respec --scope`, which replaces the list).

## Implementation notes

- The pinned flag lives with the item order, in kiln-shell's `PaneState` (a set of pinned item IDs whose items lead): `PaneState::pin`/`unpin`, and `PaneView::pin_tab`/`unpin_tab`. `reorder_item` keeps an item on its side of the boundary and `PaneState::insert` (so `insert_item`, a tab dropped from another pane) puts it after the pinned group, which covers drags, vim tab moves and opens placed after a pinned active tab. A tab moved to another pane, or split off, arrives unpinned. fe-workbench's `Pane::pin`/`unpin`/`is_pinned` read it.
- Layout: a pinned item's entry gains `"pinned": true`; unpinned items save as before, so an older build's entry and an unpinned window's entry are unchanged. Restore pins flagged items in saved order after `add_items`. The pin commands save the layout at once (`save_layout_in_full`).
- Strip: `kiln_ui::Tab::pinned` draws the tab at `Tab::PINNED_WIDTH` (icon, Lucide `pin` glyph or the dirty dot in its corner, no ×; middle click still closes) with the label and description in a tooltip. `pin.svg` is Lucide 1.52.0's, unmodified, ISC only (not Feather-derived), added to kiln/THIRD_PARTY.md.
- Bulk closes: `Pane::close_matching` (Close Other Tabs, `:tabo`, Close Saved) never picks a pinned tab; `close_many::scope` leaves pinned tabs out of Close Editors in Group and Close All Editors, as VS Code's do.
- Replacing a tab in place (a large-file viewer converted to an editor, an SVG viewer swapped for an editor and back, a diff view rebuilt) goes through `PaneState::insert_replacing`: the new item goes right before the old one, pinned if it was, without the insert clamp, and the old one is taken out by its ID (`Pane::replace_in_place`, `git_diff::replace_diff`).
- A bulk close that waits on a prompt, or behind another close, commits only the tabs still unpinned (`Pane::take_unpinned_ids`), matching its live items, so a tab pinned meanwhile stays open, unasked, with its changes.
- Close All Editors and Close Editors in Group are `CloseKind::Bulk` requests (kiln-shell `close`), never one close with a Close Tab over the same tabs, so a Close Tab on a tab pinned while a bulk close asks waits for it and then closes the tab on its own answer. A request queued behind another close reads its items live on its first pass, not the snapshot it was made with, so a dirty tab pinned while a bulk close waits isn't asked about.
