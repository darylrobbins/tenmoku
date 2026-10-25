---
task_uid: "01m5qphwctnzgw3fnvxn7qfy0e"
task: "T325"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap tests/dock_leave_keys.rs: with vim on, in each of project_panel, outline_panel, ReferencesPanel, problems_panel, search_results and git_panel (not its commit box), ctrl-w h/j/k/l and ctrl-w ctrl-h/j/k/l resolve to dock.focus_left/down/up/right, ctrl-w p and ctrl-w w to dock.focus_editor; with vim off none resolves; none is bound in an editor or text_input context"
    required: true
    manual: false
  - id: "C2"
    text: "dock_leave_tests::to_editor: with the sidebar on the right, ctrl-w h from the project tree focuses the editor pane that was last active (not always the first), and ctrl-w l from the tree does nothing; with it moved left the keys swap; ctrl-w p returns to the editor from either dock"
    required: true
    manual: false
  - id: "C3"
    text: "dock_leave_tests::between_docks: with the terminal at the bottom and shown, ctrl-w j from the sidebar focuses the terminal and ctrl-w k from the terminal returns to the sidebar's shown view; ctrl-w k from the sidebar does nothing"
    required: true
    manual: false
  - id: "C4"
    text: "dock_leave_tests::escape: escape in a vim-on panel (no field open) focuses the editor; with the tree's name field, the search input or the commit box focused, escape keeps its current meaning; typing ctrl-w in the search input is untouched"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-workbench -p fe-gpui and cargo clippy -p fe-keymap -p fe-workbench -p fe-gpui --all-targets -- -D warnings pass; no existing test is edited, save adding the new bindings' contexts to the lists of the workbench's contexts in fe-gpui's keymap_tests, command_tests and vim tests, as T321 did"
    required: true
    manual: false
---

# Leave a dock panel with vim's window keys: ctrl-w h/j/k/l and ctrl-w p/w move from the tree, outline and other lists to the editor or the other dock, and escape returns to the editor (Mac T1133)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" Once the previous card lets <C-w>h/l enter a dock, the keyboard user needs the same keys to leave it. Today nothing in a panel's contexts handles ctrl-w (and off macOS an unbound ctrl-w in `workbench` closes the tab: fastedit.toml:1600-1603 comment), so pressing it in the tree would close the active editor tab.

## Current state

- Panel contexts: project_panel (fastedit.toml:790-900), outline_panel (new, sibling card), ReferencesPanel (:903), problems_panel (:955), search_results (:694), git_panel (:1592; its commit box is `git_panel && editor`).
- No command focuses "the editor" or "the other dock" from a panel; each *_tests helper does `window.focus(...)` by hand. `leave_hidden_panel` (workbench_chrome.rs:~300) focuses the active editor when a hidden panel has focus: reuse its target.
- T605's rule and T523's still hold: no key is taken from an editor context. Binding <C-h>/<C-j>/<C-k>/<C-l> bare in the editor's normal mode would take vim motions, so it is deliberately not done here; they are bound only in the panel contexts (below) where they are free.

## Change

- Commands dock.focus_left/right/up/down (the neighbour of the focused dock in that direction: editor pane group, the other dock, or none) and dock.focus_editor (the last active editor pane), in new dock_leave.rs on Workbench with the registrations in workbench_commands.rs.
- Bind them in each panel context with `&& vim` (and `&& !text_input` where a field can sit inside): ctrl-w h/j/k/l, ctrl-w ctrl-h/j/k/l, ctrl-w p, ctrl-w w, and bare ctrl-h/j/k/l. escape → dock.focus_editor in the list contexts when no field is open. Prefer one shared context added to all dock panels over six copies if kiln's dock lets the workbench add a key context to the panel container; otherwise write the six blocks.
- Neovim parity: nvim-tree and neo-tree sit in ordinary windows, so <C-w> works there as in any window; nvim's <C-w>p is "previous window", here the editor pane last active.

## Tests

New files dock_leave_tests.rs (declared from dock_leave.rs) and fe-keymap tests/dock_leave_keys.rs. A real-app check is out of reach in this environment (no GUI-driving permission), so all criteria are headless simulate_keystrokes tests.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- One binding per key, not six blocks: the lists share the predicate
  `(project_panel || outline_panel || ReferencesPanel || problems_panel || search_results || git_panel) && vim && !editor && !text_input`;
  the `<C-w>` keys add `terminal_panel` to it. kiln's dock can't take a key
  context from the workbench, and the bottom dock is kiln's, so there is no
  shared container frame.
- The Changes panel's context had no `vim` flag; it gets one (git_panel.rs),
  which the commit box (an `editor`) never matches. The terminal panel gets a
  `terminal_panel` context with the flag around its sessions (terminal_panel.rs);
  vi copy mode's `holds_keys` still keeps the keymap out. With vim on, ctrl-w
  in the shell now waits for the chord's next key (or the chord timer) before
  reaching it, as in the editor's normal mode.
- Up from the terminal's dock at the bottom goes back to the sidebar if a
  window key brought focus from it (`TerminalPanel::from_sidebar`, cleared
  when focus leaves the panel), else to the editor; workbench.rs was held by
  another task, so the flag lives on the panel. Left or right from that dock
  reaches the sidebar on that side.
- `mod dock_leave` is declared from dock_focus.rs, lib.rs being out of scope.
- fe-gpui's three lists of the workbench's binding contexts (keymap_tests,
  command_tests, vim tests) gain the two new predicates, as T321's did.
- Bare ctrl-k in a list off macOS waits behind the workbench's ctrl-k chords
  and runs on the chord timer.
