---
task_uid: "01m4schcp8j4yg982tjmsvqpcx"
task: "T65"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test terminal_find_tests::show_type_close: terminal.find opens the row with focus in its input; typing a word the shell printed sets the active TerminalView's query and selects its newest match; escape (through the keymap) clears the query, closes the row and focuses the terminal"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test terminal_find_tests::steps_and_wraps: with three matches, enter steps to older ones and shift-enter to newer, both wrapping; the pane's find bar stays closed with its query unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test terminal_find_tests::toggles_and_errors: with regex on, fo+ matches foo; an invalid regex shows its error and highlights nothing; match case on stops Foo matching foo; terminal.next moves the query to the new active terminal"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Find in the terminal panel: terminal.find opens a find row over the active terminal, enter and shift-enter step, escape closes, with case and regex toggles (Mac T586)

Filed on argon from Mac T586 (card ~/coordination/argon-cards/T586.json rev 1). Mac task uid 01m4scfd3bx6dfjv798zm8cgtg. Depends on argon T64 (it calls set_find, find_next and find_matches). Its other dependency, Mac T579, is argon T63 (8893a13), already on argon main. Title the host commit "... (Mac T586)".

## Problem

The terminal panel (`crates/fe-workbench/src/terminal_panel.rs`) has no find: VS Code's and Zed's terminals
have one. T573 added the search to fe-terminal and the previous card the highlighting and stepping to
`TerminalView` (`set_find`, `find_next`, `find_matches`). This card is the panel's find row and its command.

Built on argon's trunk (T573 = argon T60, T579 = argon T63), not yet on the Mac's main.

## Change

- `crates/fe-workbench/src/terminal_panel.rs`: `TerminalOp::Find`, command `terminal.find` "Terminal: Find"
  in the panel's registry (context as the other terminal commands), run by `Workbench::run_terminal_op`
  (`crates/fe-workbench/src/workbench.rs`): it shows the dock if hidden and opens the find row of the panel
  with focus in its input. A non-empty selection in the active terminal within one row replaces the query.
  No key is bound here: binding `secondary-f` (and Linux `ctrl-shift-f`) in the `terminal` context belongs to
  a later key-binding card, since the keymap files are held by T462.
- New `crates/fe-workbench/src/terminal_find.rs` (declared from terminal_panel.rs): the row above the active
  terminal, under the tab strip: an `fe_ui::TextInput` "Find", and Aa (match case) and .* (regex)
  `ToggleButton`s drawn as `crates/fe-workbench/src/find_bar.rs` draws them. Match case off is `None` (smart
  case), on is `Some(true)`. A bad regex shows its `QueryError` text in the row; a query with no visible
  match shows "No results".
  - The row's element sets key context `find_bar` (`find_bar::KEY_CONTEXT`), so the existing bindings apply:
    enter `find.next`, shift-enter `find.previous`, escape `find.close`, `find.toggle_case`,
    `find.toggle_regex`. The row handles those commands itself (`find_bar::op`) and stops their propagation,
    so the pane's find bar never sees them; `find.toggle_whole_word` does nothing there.
  - Each change to the query or a toggle calls `set_find` on the active `TerminalView`, then
    `find_next(false)` from the cursor, so the newest match is selected. `find.next` steps up, toward older
    output, and `find.previous` down, as VS Code's terminal find does; both wrap (fe-terminal's find wraps).
  - `find.close`: `set_find(None)`, the row closes and focus returns to the active terminal.
  - `terminal.next`/`terminal.previous` move the open query to the new active terminal (and clear it on the
    old one); when the last session goes, the row closes.

## Tests

New file `crates/fe-workbench/src/terminal_find_tests.rs`, declared from terminal_panel.rs as
`terminal_tabs_tests` is, with the panel's test shell (`shell`) printing known lines.

## Implementation notes

- `terminal_find.rs` is declared from `terminal_panel.rs` with `#[path]`, so it's
  `terminal_panel::terminal_find`. The row reuses `find_bar::FindOp`: `terminal.find` runs
  `FindOp::Show` on the panel after the workbench starts a session and shows the dock.
- "No results" is worked out as the row renders, with `Terminal::find_all_visible`, not from
  `find_matches`, which the terminal only refreshes at its next prepaint.
- Typing a query clears the terminal's selection before `find_next(false)`, so each new
  query finds from the cursor rather than from the last match.
- `terminal.new` also moves an open query to the new session, as `terminal.next` does.
- Review round 1: the panel's focus handle is now its root's, which contains the find row and
  the sessions, so the dock's rail and `terminal.toggle` count focus in the row as the
  terminal's; focused itself (as by the dock showing it) the panel hands focus to the active
  session as it draws. A query set on a session that hasn't started is checked once it has
  (`TerminalFind::settle`), and a query error is kept when the query moves to another session.
  A selection seeds the query only when it covers one row on screen, so a soft-wrapped one
  doesn't.
