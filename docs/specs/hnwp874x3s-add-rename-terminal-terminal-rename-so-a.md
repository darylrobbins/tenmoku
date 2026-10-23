---
task_uid: "01m5k7qwnqax1vfmhnwp874x3s"
task: "T291"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Command terminal.rename (title Rename Terminal) asks for a name pre-filled with the active session's current label; Enter applies it, Escape leaves the name alone; an empty answer clears the custom name (test rename_applies_cancels_and_clears in terminal_rename_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "A custom name wins over the shell's OSC title for that session only, survives the shell setting a new title, and goes with the session when it closes; other sessions keep their own labels (test custom_name_wins_over_shell_title in terminal_rename_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "Double-clicking a list row asks to rename that session (test double_click_renames_its_row in terminal_rename_tests.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "terminal.rename is in the palette and in the typed-command catalog list in command_catalog_tests.rs (test commands_are_in_the_palette in terminal_tabs_tests.rs still passes); no key is bound (VS Code binds none outside the list)"
    required: true
    manual: false
  - id: "C5"
    text: "Mac-only (GPUI view tests); no screenshot golden changes"
    required: true
    manual: false
---

# Add Rename Terminal (terminal.rename) so a terminal's list row shows a name the user chose instead of the shell's title, as VS Code's Terminal: Rename does (Mac T1079)

User request 2026-10-21: "LEt's also file cards for multiple terminal tabs."

## Problem
A terminal's tab label is the title its shell set, else "Terminal <n>" (TerminalPanel::label, crates/fe-workbench/src/terminal_panel.rs:360). VS Code's workbench.action.terminal.rename lets the user name a terminal; the name replaces the process title in the list until the terminal is killed.

## Change
- Add `name: Option<SharedString>` to Session (terminal_panel.rs:53) and have label() return it first.
- Add a typed command `terminal.rename` ("Rename Terminal") beside terminal.close in workbench_commands.rs (:816) and a TerminalPanel/Workbench method like close_terminal in workbench_chrome.rs (:183). Ask for the name with whatever single-line prompt the workbench already uses for names (find how the project tree's rename and other name prompts do it; reuse it, do not build a new input). Pre-fill with the current label; empty clears the custom name.
- Double-clicking a list row (from the previous card) runs it for that row's session.
- Update the command list in command_catalog_tests.rs (:312-320 region).

## Tests
Mac-only. New file terminal_rename_tests.rs wired with #[path] at the bottom of terminal_panel.rs; reuse helpers from terminal_tabs_tests.rs.

Lend note (helium lead): lent by the Mac lead (y50) with the full task JSON. Criteria marked Mac-only are GPUI view tests: run them here if they run headless on Linux; otherwise say so in the finish notes (they are checked on the macOS gate). Chain order: Mac T1077 -> T1078 -> T1079 -> T1080 on one trunk.
