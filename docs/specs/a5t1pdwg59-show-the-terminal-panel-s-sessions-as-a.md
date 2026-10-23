---
task_uid: "01m5k7qwc996s35ga5t1pdwg59"
task: "T290"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Mac-only: with 2 or more sessions the panel draws a vertical list on the RIGHT of the active terminal (fixed ~160px wide, left border, full panel height, scrolls vertically when rows overflow); with 1 session no list shows (test list_shows_from_two_sessions in terminal_tab_list_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Each row has the Icon::FileTerminal icon, the label (shell-set title, else Terminal \u003cn\u003e) and an Icon::X kill button; the active row uses colors.list.row_selected, the others hover with colors.list.row_hover (test rows_show_icon_label_and_active in terminal_tab_list_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "Clicking a row activates and focuses its session; clicking its kill button drops that session and not the active one unless it is (tests clicks_follow_sessions in terminal_tabs_tests.rs, adapted, and kill_button_drops_its_own_session in terminal_tab_list_tests.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "Killing the last session hides the dock (existing Exited path) and the next toggle starts a fresh Terminal 1 (test killing_the_last_hides_the_panel in terminal_tab_list_tests.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "The active row scrolls into view (the_active_tab_scrolls_into_view in terminal_tabs_tests.rs, adapted to vertical); TerminalPanel's test accessors drawn_tabs, tab_bounds, strip_bounds keep their meaning for the list; the old horizontal strip and its + button are removed here (a later card restores +)"
    required: true
    manual: false
  - id: "C6"
    text: "Existing new_and_cycle, close_activates_a_neighbour, exit_drops_only_its_session, tab_strip and commands_are_in_the_palette in terminal_tabs_tests.rs pass; no screenshot golden changes (the terminal is in none)"
    required: true
    manual: false
---

# Show the terminal panel's sessions as a vertical list on its right with a terminal icon, the shell's title and a kill button per row, as VS Code's terminal tabs do, in place of the strip above (Mac T1078)

User request 2026-10-21: "LEt's also file cards for multiple terminal tabs."

## Problem
fastedit already holds several terminals (TerminalPanel.sessions: Vec<Session>, crates/fe-workbench/src/terminal_panel.rs:53-63; each its own TerminalView/pty, scrollback and cwd; commands terminal.new/close/next/previous). But the tabs are a horizontal strip ABOVE the terminal (tab_strip, terminal_panel.rs:374, drawn only when sessions.len() > 1 at :562) with no icons. VS Code's terminal tabs are a vertical list on the right of the terminal, shown from two terminals, each row an icon, the name (shell/process title) and, on hover and for the active row, a kill (trash) button.

## Change
- Replace tab_strip with a right-hand vertical list in render() (:549-585): the terminal row becomes flex_row, terminal view flex_1, list flex_none. Reuse the colours the strip uses (colors.list.row_selected for active, colors.list.row_hover on hover, colors.border.default for the separator). Keep ScrollHandle but scroll vertically (overflow_y_scroll).
- Row: Icon::FileTerminal (what the panel already uses as its icon, :529), label from label() (:360), IconButton Icon::X (existing close handler, name sessions by view EntityId so a stale click lands on nothing). Show the kill button on the active row and on hover, as VS Code does.
- Keep the existing session model, commands and Exited path untouched. Trash icon: the Icon set (kiln/crates/kiln-ui/src/file_icons.rs) has no trash glyph; use Icon::X unless one exists, do not add icons in this card.
- Split terminals (side by side in one group) are explicitly NOT in this card or this series.

## Tests
All Mac-only GPUI view tests. Adapt terminal_tabs_tests.rs helpers (drawn_tabs, tab_bounds) rather than duplicating; new tests go in the new file terminal_tab_list_tests.rs wired with #[path] at the bottom of terminal_panel.rs.

Lend note (helium lead): lent by the Mac lead (y50) with the full task JSON. Criteria marked Mac-only are GPUI view tests: run them here if they run headless on Linux; otherwise say so in the finish notes (they are checked on the macOS gate). Chain order: Mac T1077 -> T1078 -> T1079 -> T1080 on one trunk.
