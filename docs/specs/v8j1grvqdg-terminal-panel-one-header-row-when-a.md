---
task_uid: "01m7nbbftqqyvsvcv8j1grvqdg"
task: "T415"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::terminal_single_header::one_terminal_draws_no_session_row_only_the_dock_strip passes: with one terminal there are no terminal-tab bounds, the bottom dock strip is the only header row and the terminal view starts directly under it"
    required: true
    manual: false
  - id: "C2"
    text: "tests::terminal_single_header::new_and_close_stay_reachable_with_one_terminal passes: with one terminal the dock strip's trailing controls hold New Terminal (dispatches terminal.new, same tooltip and accessible name) and Close (kills the sole session as terminal-kill did)"
    required: true
    manual: false
  - id: "C3"
    text: "tests::terminal_single_header::second_terminal_brings_the_session_row_back_and_closing_to_one_removes_it passes: two terminals show the row with tabs, rename and marks; closing back to one removes it without losing focus"
    required: true
    manual: false
  - id: "C4"
    text: "The existing terminal_actions_tests, terminal_tabs_tests, terminal_tab_list_tests, terminal_rename_tests, terminal_lane_tests, tests::terminal_b3, tests::terminal_plus and tests::bottom_panel_b3 pass, updated only where they assumed a session row with one terminal"
    required: true
    manual: false
  - id: "C5"
    text: "The terminal and terminal_session audit scenes are updated for the one-header-row layout (their probes and expectations follow the dock strip carrying New Terminal and Close), and the workbench.png golden is left unchanged because its terminal sits in a hidden bottom dock there; the screenshots example is macOS-only (Metal), so running those scenes in both themes and re-checking the golden is the Mac gate's job (KNOWN LIMIT, per the lend note 'the Mac gate re-checks')"
    required: true
    manual: false
---

# Terminal panel: one header row when a single terminal is open; the dock strip carries New Terminal and Close, and the session tab row appears from two terminals (Mac T1928)

## Problem

With one terminal open the bottom panel draws two header rows: the bottom dock's own tab strip
("Terminal", kiln-shell dock.rs) and TerminalPanel's session tab row ("Terminal 1",
terminal_panel.rs `tab_strip`, always built). The user wants a single row, so the website hero
(tenmoku.dev, recaptured once this lands) shows one terminal header. No setting hides either row.

## Change

- TerminalPanel draws no session tab row while it has exactly one session (a lane terminal counts
  as a session). From two sessions the row returns with its tabs, rename and marks; closing back to
  one removes it again, without losing focus.
- With one session, New Terminal (+) and Close (kill) stay reachable as PaneActions in the dock
  strip's trailing controls, with the same commands, tooltips and accessible names as
  terminal-new-button / terminal-kill.
- No new strings and no new setting. Change kiln-shell dock.rs only if panel actions are not
  already per-panel; otherwise drop it.

## Pins that move

Existing terminal tests assume a session row with one terminal (terminal_actions_tests,
terminal_tabs_tests, terminal_tab_list_tests, terminal_rename_tests, terminal_lane_tests,
tests/terminal_b3.rs, tests/terminal_plus.rs, tests/bottom_panel_b3.rs); update them, do not delete
coverage. Audit scenes terminal and terminal_session; golden workbench.png only if its terminal is
visible. New tests go in the new crates/fe-workbench/src/tests/terminal_single_header.rs, declared
from tests/mod.rs.

## Notes

Shares terminal_panel.rs with ready T1595/T1596/T1597/T1813 (different regions). Platform: nothing
macOS-specific (linux-ok). User request 11-17 (hero tweak, option "push now, recapture later").

## Implementation notes

- kiln-shell dock.rs is unchanged: `DockItem::actions` is already per panel. `TerminalPanel`
  returns `+` (`terminal-new-button`, `terminal.new`) and kill (`terminal-kill`, `terminal.close`)
  from it while it has fewer than two sessions, with the strip's names, and a debuggee tab's
  rerun (`terminal-debuggee-rerun-button`) before them when the lone session is one. From two it
  returns none and draws its own row as before. The dock's `icon-button:<id>` selectors are the
  strip buttons' own, so a test addresses either by the same id.
- The test hooks `drawn_tabs` and `drawn_tab_marks` still record every session's tab each frame,
  drawn or not, so the tests outside this card's scope that read them as the sessions' labels
  (task_run_tests, task_commands_tests, task_problems_panel_tests, test_commands_tests,
  tests/debug_session.rs) keep passing unchanged; the new `strip_drawn` says whether the row drew.
- One out-of-scope test edit: tests/debug_session.rs `click_rerun` clicked the strip's
  `terminal-debuggee-rerun` wrapper, which a lone debuggee tab no longer draws; it now clicks
  `icon-button:terminal-debuggee-rerun-button`, the strip's button or the dock's.
- With one session its tab's mark (spinner, error) is not on screen; tests/terminal_b3.rs reads it
  from the recorded tab (`lone_mark`) and checks it is drawn once a second session opens.
- Audit scenes (C5, rev 2): both are updated for the one-header-row layout. terminal_session (one
  session) gains a probe under the dock strip, (600, 377), which is the terminal's background now
  that the panel draws no row of its own (session from y = 360: 12 px mono on 18 px lines, so
  line 0's text ends near x = 430 and (600, 500) stays clear on line 7), and a step,
  `one_header`, that fails the scene unless the dock strip's trailing controls start with
  `terminal-new-button` and `terminal-kill`. terminal (three sessions) keeps its probes on the
  panel's strip (360–394) and gains `panel_header`, which fails it if the dock strip carries any
  of the panel's buttons. workbench.png is unchanged: its bottom dock is hidden.
- KNOWN LIMIT: the screenshots example is macOS-only (Metal; on Linux `main` prints "skipped"), so
  the scenes were not rendered here. Their new steps were type-checked and clippy-clean on Linux
  through a temporary module (not committed). Running both scenes in both themes and re-checking
  workbench.png is the Mac gate's job, per the lend note ("the Mac gate re-checks").
- docs/design/design-system.md §Terminal ("Terminals are tabs in the panel's own strip") is out of
  scope; it could gain "from two" in a follow-up.
- Review R1 F2: the panel strip's tooltips took `pane_toolbar::shortcut` (single keys only) while
  the dock's take the first sequence, chords too. The strip now uses `header_shortcut`, the dock's
  policy over the same contexts and keymap, so a user chord for `terminal.new` or `terminal.close`
  shows in both (`a_chord_for_new_or_close_shows_the_same_in_both_headers`).
