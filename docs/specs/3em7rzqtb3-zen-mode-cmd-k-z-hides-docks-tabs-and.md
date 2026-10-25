---
task_uid: "01m5qnxrsqddc8543em7rzqtb3"
task: "T1147"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "zen_mode_tests::enter_hides_and_leave_restores: with the sidebar, terminal panel and status bar visible, workbench.toggle_zen_mode hides them and toggling again restores exactly the prior visibility, including a sidebar that was already hidden staying hidden"
    required: true
    manual: false
  - id: "C2"
    text: "zen_mode_tests::settings_control_what_hides: hide_status_bar=false keeps the status bar; hide_tabs=false keeps the tab strip; center_layout=false leaves the pane full width; center_layout=true gives 20% margins"
    required: true
    manual: false
  - id: "C3"
    text: "zen_mode_tests::escape_is_not_delayed_in_zen: in zen a single Escape closes an open find bar at once and does not leave zen; zen is left with secondary-k z or the command. Known limit: VS Code's escape escape exit is not bound, because kiln's matcher would delay every lone Escape by the chord timeout; follow-up T1173 adds it with a matcher change"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config zen_mode_settings: defaults are hide_tabs/hide_status_bar/center_layout all true, the template and schema list them; fe-keymap zen_mode_keys: secondary-k z resolves to workbench.toggle_zen_mode and no escape escape binding exists"
    required: true
    manual: false
  - id: "C5"
    text: "Catalog tests updated; scripts/check_kiln_boundary.py, cargo nextest run -p fe-workbench -p fe-config -p fe-keymap -p kiln-shell and clippy pass"
    required: true
    manual: false
---

# Zen Mode: cmd-k z hides docks, tabs and status bar and centers the editor; Esc Esc leaves; [workbench.zen_mode] settings

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Zen Mode (workbench.action.toggleZenMode, chord cmd-k z / ctrl-k z, Esc Esc to leave) is missing. Verified: no zen/focus mode command or setting in fe-workbench or fe-config.

## Current state
- Chrome visibility pieces exist separately: `Workbench::set_status_bar_visible` (`crates/fe-workbench/src/workbench_chrome.rs:91`, setting `[status_bar] visible`, `crates/fe-config/src/settings.rs:365`), the dock/sidebar and its rail (`dock.rs`, `workbench_commands.rs:455` `toggle_dock`), the terminal panel, and the per-pane toolbar (`pane_toolbar.rs`). The tab strip belongs to kiln-shell's pane (`kiln/crates/kiln-shell/src/pane.rs`); run `scripts/check_kiln_boundary.py` if a kiln hook is added.
- `WorkbenchSettings` is at `settings.rs:380` (only `preview_tabs`). Chords like `secondary-k z` are supported by the keymap (`secondary-k secondary-0`, fastedit.toml:311).

## Change
- Command `workbench.toggle_zen_mode` ("Toggle Zen Mode", View menu), chord `secondary-k z`. Entering saves the current visibility of the sidebar/dock, bottom panels (terminal, problems, references), activity rail, status bar, tab strip and breadcrumbs toolbar, then hides: all docks/panels and the rail always; status bar and tab strip per settings. Leaving restores exactly what was saved, including if the user toggled something while in zen (a toggle while zen is on is not undone by leaving).
- Esc Esc (`escape escape`, context `workbench && zen_mode`) leaves zen mode; a lone Esc must still reach the editor/find bar as today. Add a `zen_mode` context key.
- Settings `[workbench.zen_mode]` (subset of VS Code's `zenMode.*`): `hide_tabs` (true), `hide_status_bar` (true), `center_layout` (true): the active editor pane is centered with margins of 20% of the window width each side (VS Code's default centered-layout ratio). Add to `settings.rs`, `settings-template.toml`, `settings.schema.json` and a `crates/fe-config/tests/zen_mode_settings.rs`.
- Not included: full screen, hide line numbers, restore-on-restart, notification silencing.

## Tests
New `crates/fe-workbench/src/zen_mode_tests.rs` (headless GPUI; see memory fe-workbench-test-tricks and gpui-focus-key-ordering for chord/focus traps) and `crates/fe-keymap/tests/zen_mode_keys.rs`.

## Implementation notes
- **Files outside the listed scope.** `crates/fe-workbench/src/workbench.rs` gets the `zen` field, the `zen_mode` module (a child of `workbench`, for the private dock and status-bar fields) and a `LayoutKeys::zen_sidebar` cell. `crates/fe-workbench/src/workbench_root.rs`'s `reload_settings` hands each window its own `[workbench.zen_mode]` (`follow_zen_settings`, as with `preview_tabs`). The workbench starts with the app's settings and uses the window's from then on, for the next time zen turns on (review c4x3b3 F3). `crates/fe-workbench/src/pane.rs` leaves its tab row out while kiln's `PaneState::tab_bar_hidden()` is set. That row is drawn by fastedit's `Pane::render`, not by kiln-shell, so kiln's hook alone can't hide it.
- **What is saved, and what leaving restores.** The sidebar (Problems and References are sidebar panels), the terminal's dock and the status bar are each noted on entry. During zen, every notification from either dock (`note_zen_chrome`) marks a dock as changed if something other than zen showed or hid it. Every call to `set_status_bar_visible` (its toggle, a settings reload, a host) marks the status bar as changed, even if the value stays the same (review c4x3b3 F4). Zen's own writes skip that call. That holds even if it ends up back where zen left it. On leaving, an unchanged piece goes back to its noted state and a changed one stays as it is. So a sidebar shown during zen stays shown, one shown and then hidden again stays hidden, and one hidden before stays hidden. Nothing outside zen hides the activity rail or the tab strips, so leaving always shows them again. The breadcrumbs toolbar hides with the tab strip (`hide_tabs`), as VS Code's editor title area does. Its own toggle (`view.toggle_breadcrumbs`, saved in the layout) is never touched. The status bar is hidden with `set_status_bar_visible`, which doesn't write settings.toml. A terminal whose shell exited during zen isn't shown again.
- **Quitting during zen.** The saved layout records the sidebar as it was before zen, unless it was shown or hidden during zen; then it records it as it is now. Zen itself isn't restored on relaunch (out of scope).
- **The `zen_mode` key context.** While zen is on, the shell's centre (the panes, their editors and find bars) is wrapped in a `ZenCenter` view whose root carries `zen_mode`. No binding uses it yet. It's there for T1173's `escape escape`.
- **No `escape escape` (rev 2, review t6kq38 F1).** kiln's matcher treats any key that starts a longer binding as a pending chord. Bound, `escape escape` would hold every lone Escape in a pane for `CHORD_TIMEOUT`, and drop it if another key came first. So it isn't bound. Zen is left with `secondary-k z` or the command. `escape_is_not_delayed_in_zen` checks that a lone Escape closes Find at once, with no clock advance, and that a key typed right after it reaches the editor. T1173 adds the exit with a matcher change.
- **Centering** pads the whole pane group, with splits inside it, by 20% of the viewport's width on each side, as VS Code's centered layout wraps the editor grid.
