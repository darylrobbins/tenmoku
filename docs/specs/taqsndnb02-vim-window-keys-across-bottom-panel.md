---
task_uid: "01m7nk9h5crw1er1taqsndnb02"
task: "T417"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "dock_focus::dock_leave::dock_leave_tests::ctrl_w_h_and_l_move_between_bottom_panel_groups passes: with vim on and three bottom groups (Problems, terminal, Git log), \u003cC-w\u003el from Problems focuses the terminal, again the Git log; \u003cC-w\u003eh walks back; \u003cC-w\u003eh from the leftmost group focuses the sidebar shown on the left, and does nothing with it on the right; \u003cC-w\u003el from the rightmost group mirrors that"
    required: true
    manual: false
  - id: "C2"
    text: "dock_focus::dock_leave::dock_leave_tests::ctrl_w_k_from_any_group_goes_up_and_j_comes_back_to_it passes: \u003cC-w\u003ek from the right group goes to the editor (or back to the sidebar if a window key came from there), and \u003cC-w\u003ej from the editor then focuses that same group's tool window"
    required: true
    manual: false
  - id: "C3"
    text: "dock_focus::dock_leave::dock_leave_tests::ctrl_w_j_and_k_walk_groups_stacked_in_a_side_dock passes: with the bottom panel moved to the left edge and two groups stacked, \u003cC-w\u003ej and \u003cC-w\u003ek move between them and past the ends do what they did before"
    required: true
    manual: false
  - id: "C4"
    text: "the_window_keys_leave_the_git_log passes (crates/fe-keymap/tests/git_log_window_keys.rs): on macOS and Linux, in git_log_panel with the vim flag, ctrl-w h/j/k/l, their ctrl- forms, ctrl-w p and ctrl-w w resolve to the dock.focus_* and dock.focus_editor commands; not with vim off, not in its filter box (text_input), and bare ctrl-h/j/k/l and escape are not bound there"
    required: true
    manual: false
  - id: "C5"
    text: "The existing dock_focus tests, dock_leave_tests, fe-keymap's dock_leave_keys and runs_bindings tests, and fe-gpui's keymap tests (binding counts unchanged) pass unmodified; fe-gpui's keymap tests (crates/fe-gpui/src/command_tests.rs, keymap_tests.rs, vim/tests.rs) list the known host context strings verbatim, so their lists are updated for the git_log_panel additions to the window-key contexts, and the binding COUNTS they assert stay unchanged"
    required: true
    manual: false
---

# Vim window keys across bottom panel groups: <C-w>h/l move focus to the neighbouring group before leaving for the sidebar, <C-w>k still goes up to the editor, and the Git log gets the <C-w> window keys (Mac T1934)

# Vim's window keys between bottom panel groups: <C-w>h/l across groups, and the Git log gets the window keys

Part of the user's 2026-11-17 request for side-by-side bottom tool windows (the Git
graph beside a terminal). Card A gave kiln-shell's tabbed dock groups. This card
makes vim's window keys walk them.

## Today

`crates/fe-workbench/src/dock_leave.rs`: with vim on, `<C-w>h/j/k/l` in a bottom
tool window run `dock.focus_left/down/up/right`. `neighbour` sends left or right to
the sidebar shown on that side, up to `Back` (the sidebar if a window key came from
there, else the editor), and down to nothing. The keys are bound through the
default keymap (`crates/fe-keymap/keymaps/fastedit.toml`, around line 2557), in the
contexts `problems_panel`, `runs_panel`, `debug_panel` and `terminal_panel` (with
`vim && !editor && !text_input`), and kiln's shared matcher dispatches them. That is
the path to keep. Do not add a side matcher or an `on_key_down` in a panel: per
T1007, a private matcher misses prefixes and chord timing and gets rejected.

`git_log_panel` is missing from those `<C-w>` contexts, so the user's own case
(focus in the Git log beside the terminal) has no window keys today. Its list
already sets the `vim` flag (`commit_rows::key_context`), and its filter box drops
it.

## Change

- **Between groups** (`dock_leave.rs`, `neighbour` / `focus_from_dock`), with the
  bottom panel at the bottom:
  - `<C-w>h` from a tool window in group *g* focuses the active tool window of the
    nearest drawn group to its left.
  - From the leftmost group, `<C-w>h` does what it does today: the sidebar if it
    shows on the left, else nothing.
  - `<C-w>l` mirrors this to the right.
  - `<C-w>k` from any group goes up as today (`Back`), and `<C-w>j` stays nothing.
    The editor is above the whole panel, as in nvim, where `k` from either bottom
    split reaches the window above.
  - Moving focus makes that group the focused group, so `<C-w>j` from the editor
    later comes back to it.
- **With the panel moved to a side** (groups stacked top to bottom), `<C-w>k` and
  `<C-w>j` move between groups the same way, and past the ends they do what they do
  today. `<C-w>h/l` keep today's meaning there.
- **Git log**: add `git_log_panel` to the contexts of the ten `<C-w>` bindings
  (`h/j/k/l`, their ctrl- forms, `p`, `w`) in `fastedit.toml`. Only those ten: not
  the bare `ctrl-h/j/k/l` or `escape`, which the Git log list may use. This edits
  the contexts of existing bindings and adds none, so the binding counts that the
  fe-gpui keymap tests pin do not change.
- `<C-w>p` and `<C-w>w` are unchanged: they go to the editor.

## Not in this card

- New commands.
- Moving tool windows between groups by key: card B has commands for that.
- Groups in the sidebar.

## Pins

The existing `dock_leave_tests`, `dock_focus_tests`, and fe-keymap's `dock_leave_keys`
and `runs_bindings` tests pass unmodified. New workbench tests go in the existing
`crates/fe-workbench/src/dock_leave_tests.rs`, which is already declared from
`dock_leave.rs`, so `tests/mod.rs` is not touched. The new keymap test goes in the
new `crates/fe-keymap/tests/git_log_window_keys.rs`, its own test binary. Tests set
up the groups through kiln's `DockState` API directly, so this card needs card A but
not card B.

Scope note: T1931 (lent to helium) also edits `fastedit.toml`, in the hex viewer's
bindings, far from these. It is not claimed here, so there is no reservation clash.
Rebase if it lands first.

## Implementation notes

- `dock_leave.rs`: new `Neighbour::Group(by)`; `group_step` finds the focused tool window among `DockState::shown_panels()` and steps to the adjacent drawn group (left/right at the bottom, up/down at a side), moved with `follow_focus` + `focus_next_group`. Past the ends it falls through to the old rules.
- `workbench_chrome.rs`: `bottom_tool_focus(panel)` factored out of `focus_bottom_panel`.
- `fastedit.toml`: `git_log_panel` added to the ten `<C-w>` contexts.
- Moving between groups clears `bottom_from_sidebar` (the tool windows' focus-out handler), so `<C-w>k` after a group move goes to the editor.
