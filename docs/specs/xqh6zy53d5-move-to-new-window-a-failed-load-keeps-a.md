---
task_uid: "01m7nhcbxzwamjx3xqh6zy53d5"
task: "T382"
rev: 7
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "A regression test in crates/fastedit/src/main.rs shows that when the file's load fails after the user edited or opened something in the destination window, the window and its content stay (no silent removal); it is checked by hand against the old unconditional-removal behaviour (decision file), because the regression check cannot base-verify an inline main.rs test; follow-up card covers moving it"
    required: true
    manual: false
  - id: "C2"
    text: "A regression test shows that moving a binary file's tab to a new window opens it in a hex viewer there, keeps the destination and closes the source tab; the regression check names this F4 hex test and it fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "An untouched destination whose load fails is still removed and the source tab stays, as T1606's existing tests in tests::move_to_new_window check"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench move_to_new_window and the fastedit crate's tests pass"
    required: true
    manual: false
---

# Move to New Window: a failed load keeps a destination window the user has used, and binary files moved into a hex viewer count as loaded (T1606 F3, F4) (Mac T1930)

## Problem

Follow-up of T1606 (Move to New Window), approved by the user with these review findings open
(rgt review show T1606):

- F3 (high), crates/fastedit/src/main.rs `open_window_on`: when loading the file fails, the
  destination window is removed unconditionally. If the user typed into it, opened other tabs or
  otherwise used it while a slow load (e.g. a large file or a slow decode) was pending, that work is
  lost without a prompt.
- F4 (medium), crates/fe-workbench/src/open.rs ~197: binary files now open in HexViewer tabs (T1895),
  but the load-completion check (`file_loaded` / `load_of`) doesn't recognise a hex viewer tab, so
  moving a binary file's tab to a new window counts as failed: the destination closes and the
  source tab stays.

## Change

- F3: remove the destination automatically only while it still holds nothing but the untouched move
  attempt; otherwise keep it (the failed tab shows its error as usual) or go through the normal
  close confirmation.
- F4: a successfully opened hex viewer counts as loaded.

## Tests

Regression tests first (they fail at base): a dirty edit made in the destination while the load is
pending, then the load fails -> the window and the edit stay; a binary file moved to a new window ->
success, destination kept, source tab closed. Put them in the existing
crates/fe-workbench/src/tests/move_to_new_window.rs where the API allows; a test that needs the
binary's `open_window_on` goes beside it in crates/fastedit. Name them with --regression-test via
respec once written.

## Implementation notes

- F3: `open_window_on` removes the destination after a failed load only while `fe_workbench::holds_only_untouched` holds: a single pane whose every tab is the moved file's own, unedited (a fresh window has no other tab, so a pathless one is the user's; an unreadable file leaves none), no other file is being read (`Pane::reading`), no terminal session was started there (`open::untouched_tests`), and no debug session is starting or has begun (`open::untouched_tests::a_debug_session_makes_a_destination_used`). Any other tab, pane, pending open or edit keeps the window.
- F4: a successfully opened hex viewer counts as loaded.

## Tests

Regression tests first (they fail at base): a dirty edit made in the destination while the load is
pending, then the load fails -> the window and the edit stay; a binary file moved to a new window ->
success, destination kept, source tab closed. Put them in the existing
crates/fe-workbench/src/tests/move_to_new_window.rs where the API allows; a test that needs the
binary's `open_window_on` goes beside it in crates/fastedit. Name them with --regression-test via
respec once written.

## Implementation notes

- F3: `open_window_on` removes the destination after a failed load only while `fe_workbench::holds_only_untouched` holds: every tab is the moved file's or has no path (a failed load's error, the welcome text) and no editor is dirty, in a single pane. Any other tab, pane or edit keeps the window.
- F4: `load_of` counts a hex viewer tab as `Load::Loaded`.
- Regression test (named): `tests::move_to_new_window::a_binary_file_in_a_hex_viewer_counts_as_loaded` (fe-workbench, no key input).
- `a_moved_file_opens_alone_in_a_fresh_window_at_its_place` also moves a binary file through `open_window_on`: it lands, the window stays and has a hex viewer tab.
- The F3 test, `tests::a_destination_the_user_used_stays_when_the_load_fails`, is inline in `crates/fastedit/src/main.rs` as an ordinary test, not a named regression test: it doesn't exist at base, so the base run finds no tests. Verified by hand that it fails with the old unconditional-removal behaviour: with the check forced to `if untouched || true` it panics at an `assert_eq!` (main.rs:1652), and passes with the fix.
