---
task_uid: "01m5qphvvsmrw1gcr7g713a39f"
task: "T324"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "dock_focus_tests::sidebar_side: with the sidebar on the left (dock.move_sidebar_left) and shown, vim on, ctrl-w h in the leftmost pane focuses the sidebar's active view (project tree, outline, search, changes, whichever is shown); with it on the right (the default), ctrl-w l in the rightmost pane does the same; the opposite edge does nothing"
    required: true
    manual: false
  - id: "C2"
    text: "dock_focus_tests::bottom: with the terminal dock at the bottom and shown, ctrl-w j in the bottom-most pane focuses the terminal; with it moved to the left or right it answers ctrl-w h / ctrl-w l at that edge instead; ctrl-w k from the top pane does nothing"
    required: true
    manual: false
  - id: "C3"
    text: "dock_focus_tests::hidden_and_inner: a hidden or collapsed dock is never opened by these keys (focus stays), and in a split with a neighbour the key still moves to the neighbour, not the dock"
    required: true
    manual: false
  - id: "C4"
    text: "dock_focus_tests::vim_off: with vim off pane.focus_left in the leftmost pane (ctrl-k ctrl-left) still does nothing, exactly as before; pane_resize_tests and vim_window_keys_tests pass unedited"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Let vim's focus keys leave the editor: with vim on, pane.focus_left/right/up/down from an edge pane focus the visible side or bottom dock on that side instead of doing nothing (Mac T1132)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The <C-w>h/j/k/l keys (T605) only walk editor panes: at the edge nothing happens, so a keyboard-only vim user can get into the tree only by cmd-shift-e (which also reveals/changes state) and has no key to reach the terminal or the outline.

## Current state

- fastedit.toml:1604-1662 binds ctrl-w h/j/k/l (and ctrl-w ctrl-h.. and the arrows) in `editor && vim.mode == normal` to pane.focus_left/down/up/right (workbench_commands.rs:75-100).
- `Workbench::focus_pane` (workbench_chrome.rs:522) activates `pane_group(cx).neighbour(&self.active, direction, cx)` and does nothing when there is none.
- The sidebar dock (`Workbench::dock`, workbench_chrome.rs:285) is on the right unless moved; the terminal dock is at the bottom unless moved (dock.move_sidebar_left/right, terminal.move_to_left/right/bottom; dock_layout.rs).
- Not VS Code behaviour (workbench.action.focusLeftGroup stops at the edge), so this is gated on vim being enabled and non-vim users see no change. No keymap edit: the existing bindings keep their command ids, so T605's tests stay valid and fastedit.toml is not touched.

## Change

- New dock_focus.rs (declared in lib.rs): `Workbench::focus_dock_beyond(direction, window, cx) -> bool`, which finds the dock whose position is on `direction`'s side (sidebar and terminal docks, honouring their current positions), returns false if it is hidden/collapsed/absent, and otherwise focuses that dock's currently shown view (the one dock.rs would focus on show; add a small `Dock::focus_active(window, cx)` in dock.rs if none exists).
- workbench_chrome.rs `focus_pane`: when `neighbour` is None and `fe_gpui::vim_enabled(cx)`, call it.
- Never opens a hidden dock: that stays the job of the show commands (`:Ex`, cmd-shift-e).

## Tests

New file dock_focus_tests.rs (declared from dock_focus.rs with #[cfg(test)] #[path]), driven with simulate_keystrokes "ctrl-w h" etc. in a vim-on workbench, checking `focus_handle().is_focused`. The way back is the next card.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- `Dock::focus_active` (dock.rs) focuses search's input, References,
  Problems, History or Chat, or, for the Explorer and Source Control, the
  top expanded view (Folders/Outline, Changes/Graph). A container with every
  view collapsed has nothing to focus, so focus stays (C3).
- The outline panel never tracked its focus handle ("takes no keys"), so
  focusing it put focus nowhere. Rather than widen scope to
  outline_panel.rs, the Explorer's Outline view is now a small `Tracked`
  wrapper in dock.rs that draws the outline inside an element tracking the
  outline's own handle, so focus lands in the sidebar.
- The terminal dock answers when it is visible and on that side; its
  panel's handle hands focus to the active session as it draws.
- C4: the test platform reads `secondary` as cmd, so `vim_off` presses
  `cmd-k cmd-left` (ctrl-k ctrl-left on Linux), first checking that the key
  walks between panes, so the edge assertion is meaningful (it fails with
  the vim gate removed).
- Carried T243 F3: Chat is now made with the dock, so History is the only
  lazily registered panel, and it is last in PANELS. A const assertion
  beside PANELS's order check now pins History last, so its append is its
  rail place. The stale `Dock::set_chat` module doc is gone.
