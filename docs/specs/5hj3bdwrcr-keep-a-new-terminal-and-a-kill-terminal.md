---
task_uid: "01m5k7qwycbwwtbv5hj3bdwrcr"
task: "T292"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "With one session the panel shows a slim actions row (height TAB_HEIGHT, same background and border as the old strip) at the top right with a + IconButton running terminal.new and a kill IconButton running terminal.close; with 2+ sessions the same row shows above the list column (test actions_show_with_one_session in terminal_actions_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Clicking + from a single-session panel adds a second focused session and the list appears; clicking kill closes the active session, the last one hides the dock (tests plus_adds_and_kill_closes in terminal_actions_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "terminal.close's palette title is Kill Terminal (command id unchanged); command_catalog_tests.rs and terminal_tabs_tests.rs commands_are_in_the_palette updated and green"
    required: true
    manual: false
  - id: "C4"
    text: "The buttons dispatch through crate::pane_toolbar::dispatch(.., Source::Mouse, ..) like the old + button did (terminal_panel.rs:426), and their tooltips show the key from the terminal keys card where there is one"
    required: true
    manual: false
  - id: "C5"
    text: "Mac-only (GPUI view tests); no screenshot golden changes"
    required: true
    manual: false
---

# Keep a New Terminal (+) and a Kill Terminal button on the terminal panel even with one terminal, and title terminal.close Kill Terminal, as VS Code's terminal panel header does (Mac T1080)

User request 2026-10-21: "LEt's also file cards for multiple terminal tabs."

## Problem
VS Code's terminal panel always has its header actions: + (New Terminal) and the trash (Kill Terminal), even with a single terminal. fastedit's only + lives in the tab strip, which draws only from two sessions (terminal_panel.rs:562, button at :426), so with one terminal the mouse cannot create or kill one. terminal.close is titled "Close Terminal" (workbench_commands.rs:816); VS Code calls it Kill.

## Change
- Draw an actions row at the top of the panel always (not only with 2+ sessions), right-aligned: + (terminal.new) and kill (terminal.close). Use the existing IconButton/Button styles and pane_toolbar::dispatch. If the dock's title row (kiln_shell Dock) can take panel actions without touching kiln, prefer that over a new row; otherwise keep it inside TerminalPanel. Do not edit kiln/ in this card.
- Retitle the doc comment of terminal.close to "Kill Terminal"; keep the command id.
- No split button, no dropdown, no maximize (out of scope).

## Tests
Mac-only. New file terminal_actions_tests.rs wired with #[path] at the bottom of terminal_panel.rs.

Lend note (helium lead): lent by the Mac lead (y50) with the full task JSON. Criteria marked Mac-only are GPUI view tests: run them here if they run headless on Linux; otherwise say so in the finish notes (they are checked on the macOS gate). Chain order: Mac T1077 -> T1078 -> T1079 -> T1080 on one trunk.
