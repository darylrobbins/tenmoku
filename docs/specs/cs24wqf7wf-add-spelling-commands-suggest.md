---
task_uid: "01m6ezvj21m70pevcs24wqf7wf"
task: "T423"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test spell_commands_tests::suggest_replaces_the_word (cargo nextest run -p fe-workbench) passes: with the caret in a squiggled word, spell.suggest opens a picker of the speller's suggestions (case-matched), Enter replaces the word as one undoable edit and the squiggle goes; with the caret on a correct word or none it toasts 'No misspelling here'"
    required: true
    manual: false
  - id: "C2"
    text: "Test spell_commands_tests::add_to_dictionary_is_global (cargo nextest run -p fe-workbench) passes: spell.add_word writes the word to the dictionary file, removes its squiggles in every open editor and toasts 'Added \u003cword\u003e to dictionary'"
    required: true
    manual: false
  - id: "C3"
    text: "Test spell_commands_tests::ignore_is_for_the_session (cargo nextest run -p fe-workbench) passes: spell.ignore_word hides the word everywhere until the app restarts and writes nothing to disk"
    required: true
    manual: false
  - id: "C4"
    text: "Test spell_commands_tests::toggle_writes_the_setting (cargo nextest run -p fe-workbench) passes: spell.toggle flips editor.spell_check through the same settings write path other toggles use, and the squiggles follow"
    required: true
    manual: false
  - id: "C5"
    text: "Test spell_commands_tests::next_and_previous_misspelling (cargo nextest run -p fe-workbench) passes: spell.next and spell.previous move the caret to the next/previous squiggled word, wrapping, toasting 'No misspellings' when none"
    required: true
    manual: false
  - id: "C6"
    text: "Test spell_bindings::quick_fix_key_and_menu (cargo nextest run -p fe-keymap) passes: spell.suggest is bound to alt-cmd-. on macOS and ctrl-alt-. elsewhere in the editor context with no conflicting binding, and the other commands have palette and Edit-menu entries 'Spelling' with no keys; default_menus_parse still passes"
    required: true
    manual: false
  - id: "C7"
    text: "command_catalog_tests and typed_command_ids_tests gain one row per new command (spell.suggest, spell.add_word, spell.ignore_word, spell.toggle, spell.next, spell.previous) and pass"
    required: true
    manual: false
---

# Add spelling commands: suggest corrections for the word at the caret, add it to the dictionary, ignore it for the session, and toggle spell check (Mac T1335)

## Problem
Squiggles (f9c) need a way to fix them. The editor's lightbulb/code-action list belongs to LSP, so spelling gets its own small commands rather than merging into it.

## Change
Six typed commands in workbench_commands.rs, implemented in new spell_commands.rs (picker in the style of crates/fe-workbench/src/picker.rs, as f8c's task picker). Word at the caret = the cached misspelling whose range contains the caret (or ends at it). alt-cmd-. / ctrl-alt-. sits beside cmd-. / ctrl-. (lsp.code_actions) and is unused in keymaps/fastedit.toml (checked 2026-11-02; re-check at build). Runs after f8c because both edit workbench_commands.rs, the catalogs and the keymap/menu TOML.

## Implementation notes

- Menus have no submenus, so "Spelling" is a separated group in Edit whose titles start `Spelling:`. It holds all six commands, spell.suggest with its key.
- spell.toggle saves through `persist` as Toggle Status Bar does (`save_toggles_to`'s writer), and applies at once. It flips what's in effect, and writes the user's file: a project's `editor.spell_check` still wins at the reload that follows.
- spell.next and spell.previous go by the squiggles an editor has, so by rows checked so far: the view and 50 rows either side, as T422 checks them.
- An accepted word reaches every open window, each of which has its own copy of the dictionary. A window opened later gets it from `SessionWords`, an app-wide global: when it's handed a dictionary, and otherwise before its first check.
- A write of spell.add_word that fails says so in a toast rather than "Added", and the word counts for the session, as `Dictionary::add_word` has it.
