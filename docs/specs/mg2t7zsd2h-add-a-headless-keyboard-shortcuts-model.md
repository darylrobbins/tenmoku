---
task_uid: "01m5n96xzd2beaf6mg2t7zsd2h"
task: "T297"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "ShortcutsModel::new(keymap: \u0026Keymap, commands: \u0026[CommandInfo{id, title, context}], os) yields one row per command, in title order, with its bindings (keys label from shortcut_label, when-context text, args shown as JSON when not null, source Default or User by Binding.layer) plus one extra binding-less row per command that has none; a command bound to two key sequences has two bindings in one row, as VS Code lists them as two rows of one command. Tests row_per_command_with_all_bindings, unbound_commands_listed, source_default_vs_user, args_are_shown"
    required: true
    manual: false
  - id: "C2"
    text: "search(query): a plain query matches command title, id and when-text case-insensitively, all terms anded; a query in quotes or starting with '@keybinding:' / typed as keystroke text parses with Keystroke and matches rows whose binding starts with or equals it (VS Code's 'ctrl+k' vs the exact-match toggle: exact: bool arg); '@source:user' and '@source:default' filter; '@command:id' filters by id. Tests search_by_title, search_by_id, search_by_keystroke_prefix_and_exact, search_by_source, unparsable_keystroke_falls_back_to_text"
    required: true
    manual: false
  - id: "C3"
    text: "conflicts(): bindings with identical keys whose contexts can both hold in some stack (use Keymap::report / KeymapIssue for what the engine already detects, do not reimplement matching) are marked with the other command's id; a binding hidden by a later one or by an unbind entry (command = \"\") is marked Hidden; chord prefixes that shadow a longer sequence are marked Shadowed. Tests identical_keys_same_context_conflict, different_contexts_do_not_conflict, unbind_hides_default, prefix_shadowing"
    required: true
    manual: false
  - id: "C4"
    text: "No GPUI, no new dependency; the real default keymap (fe_keymap::DEFAULT_LINUX) loads into the model with every binding appearing in exactly one row (test every_default_binding_is_listed). Runs on Linux"
    required: true
    manual: false
---

# Add a headless keyboard-shortcuts model that lists every command with its bindings and when-contexts from the effective keymap, searches by command name or keystroke, and reports conflicts, for the Keyboard Shortcuts editor (Mac T1098)

## Problem
Bindings are only visible by reading `crates/fe-keymap/keymaps/fastedit.toml`. VS Code's Keyboard Shortcuts editor (workbench.action.openGlobalKeybindings) lists commands with keybinding, when and source, searches by command or keystroke, and flags conflicts. This card is the headless model; the write-back (card 6), list view (card 7) and recorder (card 8) are separate.

## Change
New `crates/fe-keymap/src/shortcuts_model.rs`, declared and re-exported (`ShortcutsModel`, `ShortcutRow`, `ShortcutBinding`, `CommandInfo`, `ShortcutIssue`) from `crates/fe-keymap/src/lib.rs`. The keymap engine lives in kiln-core (`keymap.rs`: `Keymap::bindings()`, `Binding {keys, context, command, args, layer, location}`, `Keymap::report()` -> `KeymapIssue` in `report.rs`); the command list is passed in so fe-keymap does not gain a registry dependency (the workbench builds `CommandInfo` from the `CommandCatalog`/registry `CommandMeta` in a later card). Labels: `shortcut_label` for the current OS. Layer 0 is the defaults; any higher layer is User. Do not edit kiln-core.

## Tests
New `crates/fe-keymap/tests/shortcuts_model.rs`: inline keymap TOML parsed with `KeymapLayer::parse`, plus the DEFAULT_LINUX/DEFAULT_MACOS coverage test. Headless, Linux-OK.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.
