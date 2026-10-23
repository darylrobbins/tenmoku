---
task_uid: "01m5k7qw376ky6qqtq9r8hsa3w"
task: "T289"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "keymap.toml binds terminal.new to macos = \"ctrl-shift-`\", default = \"ctrl-~\" in context \"workbench\", and terminal.next / terminal.previous to macos \"cmd-shift-]\" / \"cmd-shift-[\", default \"ctrl-pagedown\" / \"ctrl-pageup\" in context \"terminal\"; the default keymap parses with no diagnostics on both platforms"
    required: true
    manual: false
  - id: "C2"
    text: "Linux-OK: crates/fe-keymap/tests/terminal_tab_bindings.rs test ctrl_backtick_toggles_in_editor_and_terminal: ctrl-` resolves to terminal.toggle in stacks [workbench, editor] and [workbench, terminal] on macOS and Linux"
    required: true
    manual: false
  - id: "C3"
    text: "Linux-OK: same file, test ctrl_shift_backtick_creates_a_terminal: macOS \"ctrl-shift-`\" and Linux \"ctrl-~\" resolve to terminal.new in [workbench, editor] and [workbench, terminal]"
    required: true
    manual: false
  - id: "C4"
    text: "Linux-OK: same file, test terminal_cycle_keys_beat_editor_tabs: in [workbench, terminal] macOS cmd-shift-] / cmd-shift-[ resolve to terminal.next / terminal.previous and Linux ctrl-pagedown / ctrl-pageup likewise; in [workbench, editor] they still resolve to workbench.next_tab / workbench.previous_tab"
    required: true
    manual: false
  - id: "C5"
    text: "Mac-only: crates/fe-workbench/src/terminal_keys_tests.rs (wired with #[path] at the bottom of terminal_panel.rs next to terminal_tabs_tests) test ctrl_backtick_shows_then_hides: a real ctrl-` keystroke from the focused editor shows the dock with a focused session, a second keystroke hides it and refocuses the editor"
    required: true
    manual: false
  - id: "C6"
    text: "Mac-only: same file, test new_and_cycle_keys: ctrl-shift-` adds a focused session; cmd-shift-[ and cmd-shift-] with a terminal focused change the active session and do not change the editor's tabs"
    required: true
    manual: false
  - id: "C7"
    text: "Existing terminal_tabs_tests.rs, menu_terminal.rs and command_catalog_tests.rs stay green; no screenshot golden changes"
    required: true
    manual: false
---

# Bind ctrl-shift-backtick to New Terminal and cmd-shift-] and [ (ctrl-pagedown and ctrl-pageup elsewhere) to the next and previous terminal, as VS Code does (Mac T1077)

User request 2026-10-21: "LEt's also file cards for multiple terminal tabs. LEt's also add the VS code key shortcut to open the terminal paned"

## Problem
VS Code's View: Toggle Terminal is ctrl+` on every platform, Terminal: Create New Terminal is ctrl+shift+`, and focusNext/focusPrevious are cmd+shift+] / cmd+shift+[ on macOS while a terminal has focus. fastedit already binds ctrl-` to terminal.toggle (crates/fe-keymap/keymaps/fastedit.toml:604, tested only as a menu hint in crates/fe-keymap/tests/menu_terminal.rs:50, never as a keystroke). terminal.new, terminal.next and terminal.previous (workbench_commands.rs:809-834) have no key. cmd-shift-]/[ are workbench.next_tab/previous_tab (fastedit.toml:453, :458, context "workbench"), so inside a terminal they would cycle editor tabs if bound only there.

## Change
- In fastedit.toml add, next to the terminal.toggle binding (:604): keys = { macos = "ctrl-shift-`", default = "ctrl-~" } command terminal.new, context "workbench". The default spelling follows the file header (fastedit.toml:10-11: shifted punctuation is named by what it types; compare :527 ctrl-_).
- Add terminal.next / terminal.previous in context "terminal" (like terminal.find at :611-614, which already outranks workbench bindings): keys = { macos = "cmd-shift-]", default = "ctrl-pagedown" } and { macos = "cmd-shift-[", default = "ctrl-pageup" }. Update the comment above explaining why they are terminal-only.
- Verified nothing else binds ctrl-`, ctrl-~ or ctrl-shift-` (vim and the editor keymap included). Do not change terminal.toggle semantics (workbench_chrome.rs:156-174 already matches VS Code: show/start, focus if unfocused, hide if focused).
- Do not add menu items (menu_terminal.rs asserts the exact menu order).

## Tests
Keymap tests are Linux-OK (build the keymap per Os as crates/fe-keymap/tests/terminal_find_bindings.rs does). View tests are Mac-only GPUI tests, new file terminal_keys_tests.rs, reusing the helpers and `keys(..)` approach of terminal_find_keys_tests.rs. If the ctrl-backtick view test fails on current main, that is a real bug in the existing binding: fix it in this card.

Lend note (helium lead): lent by the Mac lead (y50) with the full task JSON. Criteria marked Mac-only are GPUI view tests: run them here if they run headless on Linux; otherwise say so in the finish notes (they are checked on the macOS gate). Chain order: Mac T1077 -> T1078 -> T1079 -> T1080 on one trunk.
