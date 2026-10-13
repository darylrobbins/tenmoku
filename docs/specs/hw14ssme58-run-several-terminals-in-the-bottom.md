---
task_uid: "01m4s6vyr6xq9xpghw14ssme58"
task: "T63"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "terminal_tabs_tests.rs: terminal.new twice from one session gives three sessions with the newest active and focused, and the dock shown; terminal.next and terminal.previous wrap and focus the session they activate, and do nothing with one session"
    required: true
    manual: false
  - id: "C2"
    text: "terminal_tabs_tests.rs: terminal.close on a middle session activates its right neighbour, on the last its left; a shell exiting drops only its own session and the dock stays shown; closing or exiting the last session hides the dock as today"
    required: true
    manual: false
  - id: "C3"
    text: "terminal_tabs_tests.rs: the tab strip is drawn only with two or more sessions, labels sessions Terminal 1 and Terminal 2 until their shell sets a title, and a click on a tab activates and focuses that session; terminal.toggle still shows, focuses and hides the active session as terminal_panel_tests.rs checks"
    required: true
    manual: false
  - id: "C4"
    text: "terminal.new, terminal.close, terminal.next and terminal.previous are in the palette under the workbench context; no keymap or menu file changes, so no key vim uses in normal, visual or operator-pending mode is taken and terminal keys still reach the shell (terminal_panel_tests.rs terminal_keys passes unchanged)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes"
    required: true
    manual: false
---

# Run several terminals in the bottom panel: terminal.new, terminal.close, terminal.next and terminal.previous, with a tab strip once there are two (Mac T579)

Filed on argon from Mac T579 (card ~/coordination/argon-cards/T579.json rev 1). Mac task uid 01m4s6r69kkf04whmhwca4n02k. Depends on argon T62: both change fe-workbench terminal_panel.rs and workbench.rs. Title the host commit "... (Mac T579)".

## Problem

The bottom panel holds one terminal (`TerminalPanel::view: Option<Entity<TerminalView>>` in
`crates/fe-workbench/src/terminal_panel.rs`). A second shell (a dev server in one, git in another) means leaving
the app. VS Code, Zed and every IDE terminal run several sessions in the panel and switch between them; this is
day-one terminal work, on the panel argon built (Mac T382 and follow-ups).

## Change

`TerminalPanel` keeps an ordered list of sessions and an active index in place of `view`. Each session is a
`TerminalView` started as `start` starts one today (the panel's cwd, the terminal settings, the test `set_shell`
override) with its own `Exited` subscription.

- **`terminal.new`** "New Terminal": shows the bottom dock if hidden, starts a session after the last, makes it
  active and focuses it.
- **`terminal.close`** "Close Terminal": drops the active session (its drop hangs the shell up, as closing the
  window does today) and activates the one to its right, else its left. Closing the last behaves as the last
  shell exiting does today: the panel emits `TerminalPanelEvent::Exited` and the workbench hides the dock.
- **`terminal.next`** / **`terminal.previous`** "Next Terminal" / "Previous Terminal": activate the next or
  previous session, wrapping, and focus it. With one session or none they do nothing.
- A shell exiting drops only its own session (activating a neighbour as `terminal.close` does when it was the
  active one); `Exited` is emitted only when no session is left.
- `terminal.toggle` is unchanged in what the user sees: it shows and focuses the active session, starting one if
  there's none, and hides the dock when a session has focus.
- **Tabs:** with two or more sessions the panel draws a one-row strip above the active session: one tab per
  session labelled with the title its shell set, else `Terminal <n>` (n counted from 1 in start order), the active
  one highlighted in the theme's selection colour, each with a close button that closes that session; a `+` at
  the strip's end runs `terminal.new`. Clicking a tab activates and focuses it. With one session there's no
  strip, so a lone terminal looks as it does now. Build the strip from `fe_ui` pieces already used by the
  workbench's other panels; no new fe-ui component.
- `DockItem::title` is the active session's title, else "Terminal".

The four commands go in the terminal's command registry next to `terminal.toggle`, in the palette, under the same
`workbench` context so they run with focus in the editor or in a terminal. **No keys are bound in this card**
(the keymap files are held by other cards; a later card binds them), so no key vim uses in normal, visual or
operator-pending mode is taken, and with focus in a terminal every key still goes to the shell except the
bindings that exist today.

## Tests

New `crates/fe-workbench/src/terminal_tabs_tests.rs` (`#[path]` from `terminal_panel.rs`), on the
`terminal_panel_tests.rs` harness with `/bin/sh` sessions: new twice gives three sessions with the newest active
and focused; next and previous wrap; close of the middle activates the right neighbour, of the last the left;
a shell exiting (`exit` typed) drops only its session and the dock stays shown; the last exiting hides the dock;
the strip is drawn only with two or more sessions, labels `Terminal 1` and `Terminal 2` before a shell sets a
title, and a click on a tab activates it; the four commands are in the palette.

## Implementation notes

- The strip is built in the panel from a scrolling row of divs, kiln-ui's `IconButton` (each tab's close button,
  `Icon::X`) and `Button` (the `+`, as kiln-ui has no plus icon; it dispatches `terminal.new` as the pane toolbar's
  buttons dispatch theirs), not kiln-ui's `Tab`, which has no hook for the selection colour. The active tab is drawn
  in `list.row_selected`, the selection colour of the trees and lists.
- Each tab's element id and click handlers name its session by its view's entity id and look its index up when
  they run, so a press on a tab whose session then goes and a release on the tab drawn there after doesn't click,
  and a handler for a session that's gone does nothing. gpui's test windows redraw after every update, so the
  test covers the press-then-exit case; the stale-frame case is the same code path.
- Adding, activating or closing scrolls the active tab into view on the next draw; scrolling the strip by hand
  isn't undone until the active tab changes.
- Tab numbers count from 1 in start order and start again from 1 once no session is left.
- `terminal.next` and `terminal.previous` with another session to go to also show the dock if it's hidden, since
  they focus the session. A closed or exited session's neighbour takes focus only if the session it replaces had it.
- `terminal_panel::toggles` became `terminal_panel::op`, returning a `TerminalOp` for the five commands.
