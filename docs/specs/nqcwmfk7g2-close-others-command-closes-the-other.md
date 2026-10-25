---
task_uid: "01m5stcmatr67dqanqcwmfk7g2"
task: "T343"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "close_others_tests::closes_other_clean_tabs: with tabs A B C D and C active, workbench.close_other_tabs leaves only C open and active in that pane; tabs in other panes are untouched"
    required: true
    manual: false
  - id: "C2"
    text: "close_others_tests::dirty_tabs_stay: dirty tabs among the others stay open exactly as Close Tab leaves a dirty tab (pane.rs close_active: no prompt, kept), unless another tab shows the same buffer; the active tab stays even if dirty; a single-tab pane is a no-op"
    required: true
    manual: false
  - id: "C3"
    text: "close_others_tests::pushes_closed_editor_stack: the closed file tabs are pushed onto T1145's closed-editor stack (reopened by workbench.reopen_closed_editor, most recently closed first)"
    required: true
    manual: false
  - id: "C4"
    text: "Catalog: command_catalog_tests.rs and typed_command_ids_tests.rs list workbench.close_other_tabs 'Close Other Tabs' (workbench); tests/pane_toolbar.rs expects it in the pane '...' list after Close Tab; no key is bound"
    required: true
    manual: false
  - id: "C5"
    text: "nextest and clippy pass in the root workspace and in kiln/"
    required: true
    manual: false
  - id: "C6"
    text: "close_others_tests::shares_core_with_tabo: workbench.close_other_tabs and T1143's Workbench::vim_tab_only (:tabo, :tabo!) call one shared close-others function (no second loop); :tabo still reports E445 when it kept a dirty tab and :tabo! still closes them; T1143's vim tab tests pass unchanged"
    required: true
    manual: false
---

# Close Others command closes the other tabs of the pane, as VS Code's workbench.action.closeOtherEditors (Mac T1172)

## Problem
VS Code's Close Others (`workbench.action.closeOtherEditors`, in the tab context menu and palette) is missing. T1143's vim `:tabo` also needs it and can call this command.

## Current state
- `workbench.close_tab` (workbench_commands.rs:37) calls `Pane::close_active` (crates/fe-workbench/src/pane.rs ~1048), which goes through kiln-shell `close_active_item` / `close::confirm`: a dirty tab stays open (until save prompts) unless another tab shows its buffer.
- fastedit has no tab context menu. Pane commands are listed in the toolbar "..." picker (`PANE_COMMANDS` in toolbar_picker.rs) and the File menu (menus/fastedit.toml lists Close Tab, Close Pane).
- Key: none added. (VS Code may bind cmd-alt-t on macOS only; unverified here. If confirmed, add `secondary-alt-t` on macOS in a follow-up; it is unbound in fastedit.toml today.)

## Change
- Command `workbench.close_other_tabs` ("Close Other Tabs") next to close_tab, on the active pane: `Pane::close_others` closes every tab but the active one, each through the same veto path as Close Tab, so dirty ones stay. If kiln-shell needs a `close_other_items` helper on `PaneState`, add it in kiln/crates/kiln-shell/src/pane.rs.
- Listed in `PANE_COMMANDS` after Close Tab; no File menu entry and no key.
- Closing via this route reaches `Pane::sync_tabs`, so it pushes onto T1145's closed-editor stack; T1143's `:tabo` can call this command.

## Tests
New `crates/fe-workbench/src/close_others_tests.rs` (add `mod` in lib.rs); update the catalog tests and tests/pane_toolbar.rs lists. Serialize with the middle-click card (both touch kiln-shell pane.rs): run it after that one.

## Shared with :tabo (rev 2)
Helium's lent card y57 (= Mac T1143, helium T329, commit 4b08c288) already closes a pane's other tabs for vim `:tabo` as `Workbench::vim_tab_only` in crates/fe-workbench/src/vim_tabs.rs: dirty tabs stay open with E445 unless `:tabo!` forces them closed. This card waits for T1143 so that code is on main, then factors one shared core out of it (e.g. `Pane::close_others(force: bool)` returning how many dirty tabs it kept) that both `workbench.close_other_tabs` (force = false, no message) and `vim_tab_only` (E445 when any were kept; `!` forces) call. Do not add a second close-others loop. Keep the vim tests from T1143 passing unchanged.

Helium lead note: :tabo is already `Workbench::vim_tab_only` in crates/fe-workbench/src/vim_tabs.rs (helium T329). Extract/call that same close-others code from workbench.close_other_tabs: ONE shared function, no second implementation. Depends on helium T329 and the helium copy of Mac T1171.


Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y60).
