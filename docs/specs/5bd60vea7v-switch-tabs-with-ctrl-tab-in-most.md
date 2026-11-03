---
task_uid: "01m6enyksfn069zb5bd60vea7v"
task: "T1314"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tab_switcher::tests::tap_goes_to_previous_tab: ctrl-tab pressed and ctrl released with no other key activates the previously used tab"
    required: true
    manual: false
  - id: "C2"
    text: "tab_switcher::tests::mru_order_follows_activation: after activating A, B, C, D the popup lists D, C, B, A, and a selection that wraps in both directions commits the right tab"
    required: true
    manual: false
  - id: "C3"
    text: "tab_switcher::tests::escape_and_close_leave_active_tab: escape closes the popup without switching; a tab closed or moved to another pane drops out of the list"
    required: true
    manual: false
  - id: "C4"
    text: "ctrl_tab_keys_resolve: ctrl-tab and ctrl-shift-tab resolve to the switcher commands in a workbench editor on macOS and Linux and not in a terminal; next_tab/previous_tab keys unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy -p fe-workbench -p fe-keymap --all-targets is clean"
    required: true
    manual: false
---

# Switch tabs with ctrl-tab in most-recently-used order: hold ctrl to cycle a popup list, release to commit, tap for the previous tab

## Problem

Tab switching is positional only: `workbench.next_tab` / `workbench.previous_tab` (crates/fe-workbench/src/workbench_commands.rs:123, `Pane::cycle`) on cmd-shift-] / cmd-shift-[ (ctrl-pagedown/up elsewhere), plus cmd-1..9. VS Code's ctrl-tab goes to the most recently used tab, and holding ctrl lists the tabs in MRU order. Approved 2026-11-02. Nothing records activation order today (`rg -i mru` finds nothing in pane.rs or kiln-shell pane.rs).

## Change

1. New `crates/fe-workbench/src/tab_switcher.rs`: per-pane MRU list of tab item ids, updated when a tab is activated or closed (hook `Pane::sync_tabs`, and `cx.defer` any check, as tab-close-routes says: a moved tab joins its target pane in the same effect cycle). Commands `workbench.tab_switcher_next` (ctrl-tab) and `workbench.tab_switcher_previous` (ctrl-shift-tab), context `workbench`.
2. First press opens a small popup (a modal list in the picker's place, rows = tab titles with the file icon, MRU order, selection starting on the second entry; ctrl-shift-tab starts on the last). Further presses move the selection with wrap. Releasing ctrl commits the selected tab through `Pane::activate_at`; escape closes without switching. A quick tap (ctrl released before any other key) therefore goes to the previous tab.
3. Release detection: a modal root with `on_modifiers_changed` (focused while open) commits when control is no longer held; kiln's `KeyOwner::modifiers_released` (kiln/crates/kiln-platform/src/keys.rs:119, implemented at crates/fe-gpui/src/keymap.rs:52) fires only for a modifier pressed and released alone, so do not rely on it. If neither route sees the release in a headless test, stop and block with the options; do not build a side matcher (keys-outside-editor-context, escape-chord-matcher-delay).
4. Defaults: bind `ctrl-tab` and `ctrl-shift-tab` on every platform in crates/fe-keymap/keymaps/fastedit.toml, context `workbench && !terminal` (a terminal keeps ctrl-tab for its programs). Add "Switch Tab" entries beside Next Tab / Previous Tab in crates/fe-keymap/menus/fastedit.toml. next_tab / previous_tab keep their keys and meaning.
5. The popup lists the active pane's tabs only. Terminal tabs and read-only tabs are included as ordinary tabs.

## Tests

New files only: crates/fe-workbench/src/tab_switcher_tests.rs (declared at the end of tab_switcher.rs with `#[cfg(test)] #[path]`) and crates/fe-keymap/tests/tab_switcher_keys.rs.
