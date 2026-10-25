---
task_uid: "01m5qnx17pxxvnmc5d6qjtn8z7"
task: "T1145"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "closed_editors_tests::close_then_reopen: closing a file tab and running workbench.reopen_closed_editor opens it again in its pane with the same selections and scroll top"
    required: true
    manual: false
  - id: "C2"
    text: "closed_editors_tests::stack_order_and_cap: closing A, B, C reopens C, then B, then A; the 21st closed file drops the oldest entry"
    required: true
    manual: false
  - id: "C3"
    text: "closed_editors_tests::already_open_and_missing: an entry whose file is open again just activates the tab; an entry whose file was deleted is skipped for the next; an empty stack is a no-op"
    required: true
    manual: false
  - id: "C4"
    text: "closed_editors_tests::every_close_route_pushes: every close route fastedit has today (Close Tab, the tab's close button, Close Pane) pushes the closed file tabs; untitled, diff and replaced-preview tabs and a tab moved to another pane are not pushed. Known limit: fastedit has no middle-click tab close and no Close Others yet; the push sits at Pane::sync_tabs so those routes push once they are added (follow-up cards)"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap test reopen_closed_key: secondary-shift-t resolves to the command in workbench context, not in a focused terminal; File menu lists 'Reopen Closed Editor'; catalog tests updated; nextest and clippy pass"
    required: true
    manual: false
---

# Reopen Closed Editor (cmd-shift-t) with a stack of closed files that restores selections and scroll, as VS Code's workbench.action.reopenClosedEditor

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Reopen Closed Editor (workbench.action.reopenClosedEditor, cmd-shift-t / ctrl-shift-t, File menu) is missing. Verified: no closed-tab history anywhere in fe-workbench (`Pane::close_active` at `crates/fe-workbench/src/pane.rs:1048` just drops the item), and cmd-shift-t / ctrl-shift-t are unbound in `crates/fe-keymap/keymaps/fastedit.toml`.

## Current state
- Tabs close through `Pane::close_active` (pane.rs:1048) and the close paths in `crates/fe-workbench/src/workbench_chrome.rs:623` (`close_active_pane`) plus the tab-strip close button / middle-click / close-others commands in kiln-shell's `PaneState` (check which route they all end in).
- Files open at a position through `Pane::open_at` with a `Reveal` (pane.rs struct fields `reveal`, `intents`).
- Commands live in `crates/fe-workbench/src/workbench_commands.rs` (`#[command(id = "workbench.close_tab")]` at line 37).

## Change
- New `crates/fe-workbench/src/closed_editors.rs`: a bounded stack (20, as VS Code) of closed file editors in the workbench: path, caret/selections and scroll top at close time, and the pane it was in. Push from every close route for file-backed editors (not untitled, diff or preview items); closing a file whose latest entry exists again only keeps one entry per path.
- New command `workbench.reopen_closed_editor` ("Reopen Closed Editor"): pops the latest entry whose file still exists, opens it in its old pane if that pane still exists else the active pane, restores selections and scroll; if the path is already open it just activates it and discards the entry; a missing file is skipped and the next entry tried; empty stack does nothing.
- Keymap: `secondary-shift-t` (cmd-shift-t on macOS, ctrl-shift-t elsewhere), context `workbench && !terminal` (check the terminal context so the terminal's own ctrl-shift-t isn't taken); File menu item "Reopen Closed Editor" after Close Tab in `crates/fe-keymap/menus/fastedit.toml`.

## Tests
New `crates/fe-workbench/src/closed_editors_tests.rs` (GPUI headless; see memory fe-workbench-test-tricks for hooks), keymap test in `crates/fe-keymap/tests/reopen_closed_key.rs`; catalog tests updated.
