---
task_uid: "01m5n96yqg5tgsdesv9edtf6de"
task: "T300"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Double-click on a row, Enter on a selected row, or the row's pencil button (selector shortcuts-edit-\u003ccommand\u003e) opens an inline recorder (selector shortcuts-recorder) in its Keybinding cell; the Add button on a binding-less row and 'Add Keybinding' in the context menu open it empty. While it is open, every keystroke including ones bound to commands (cmd-w, cmd-q) is captured and shown instead of running, converted with kiln's keystroke conversion (kiln-platform keys.rs `keystroke`), up to a chord of 2 keystrokes: after the first, the next keystroke within 1.5 s extends it, and the recorder shows the label as shortcut_label does. Tests recording_captures_bound_keys_without_running_them, chord_of_two, third_key_starts_a_new_sequence, label_matches_shortcut_label"
    required: true
    manual: false
  - id: "C2"
    text: "While recording, the cell lists the commands that already have that key sequence in the row's context ('Also bound to X, Y'), from ShortcutsModel conflicts; it never blocks saving. Test conflict_preview_lists_other_commands"
    required: true
    manual: false
  - id: "C3"
    text: "Enter saves: a new row calls Add {keys, context: the command's context, command}, an existing binding calls Change; both through write_keymap; Escape cancels with no write; a bare modifier press records nothing; the 'When' for a new binding defaults to the command's own key context (CommandInfo.context), editable by a second inline field that takes a ContextPredicate and shows the parse error under it (an invalid one disables Enter). Tests enter_adds_binding, enter_changes_binding, escape_writes_nothing, invalid_when_blocks_save, new_binding_defaults_to_command_context"
    required: true
    manual: false
  - id: "C4"
    text: "After saving, the tab refreshes from the reloaded keymap and the row shows the new binding and the 'User' source; the file text keeps its comments (assert). Test saved_binding_shows_after_reload"
    required: true
    manual: false
  - id: "C5"
    text: "GPUI view tests headless on Linux; the recorder's key context uses kiln-platform's HOLDS_ALL_KEYS (T315) so platform-modifier keys (cmd-w, cmd-q) are captured; no golden"
    required: true
    manual: false
---

# Add and change a keybinding in the Keyboard Shortcuts tab by recording keystrokes in an inline box, as VS Code's Define Keybinding does: chords, a conflict preview, Enter to save and Escape to cancel (Mac T1101)

## Problem
Card 7 can remove and reset bindings but not create them. VS Code's Define Keybinding box records the pressed keys, previews conflicts and saves on Enter.

## Change
- New `crates/fe-workbench/src/shortcuts_recorder.rs`: a recorder element/state owned by `ShortcutsEditor` (edit that file to host it and add the row actions). Capturing keys that the keymap would otherwise dispatch needs the platform keystroke interceptor in `kiln/crates/kiln-platform/src/keys.rs` (`on_keystroke`, `holds_keys`); find the existing way a focused view asks to receive all keys raw (e.g. text inputs and the terminal), and use it; if none can pass chords like cmd-w to the view, stop and file the missing kiln seam as a follow-up rather than editing kiln-platform in this card (scope excludes it).
- Saving uses `keymap_edit::{edit_keymap, write_keymap}` from card 6 with `Add` / `Change`.
- No new commands and no keymap defaults change.

## Tests
New `crates/fe-workbench/src/shortcuts_recorder_tests.rs` using the real GPUI keystroke simulation (`simulate_keystrokes` as other workbench tests do). Linux-OK. Note GPUI reports shifted punctuation by the typed character on Linux (see the header of fastedit.toml); assert labels via the same conversion, not literals, where they differ per OS.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.

## Implementation notes

- The recorder is `crates/fe-workbench/src/shortcuts_recorder.rs`, a child module of `shortcuts_editor.rs` (`shortcuts_editor::recorder`) so it can use the tab's private state; its tests are a child of `shortcuts_editor_tests.rs` (`shortcuts_editor_tests::recorder_tests::*`) so they can share its workbench, config directory and keymap-store helpers. Scope was widened to `shortcuts_editor_tests.rs` for that, and because the new Add Keybinding menu item changes the menus its tests assert.
- Key capture: the box's key context is `shortcuts_recorder` plus kiln-platform's `HOLDS_ALL_KEYS` (T315). `recording_captures_bound_keys_without_running_them` binds `cmd-alt-9` (the platform modifier) in the user keymap and checks it doesn't run while recording but runs again afterwards; it fails with `HOLDS_ALL_KEYS` removed.
- Enter and Escape on their own save and cancel, as in VS Code, so they can't be recorded unmodified; with a modifier (`ctrl-enter`) they are recorded. Key repeats are ignored. The chord clock is the GPUI executor's (`now()`), so tests advance it.
- Conflict preview: the tab's own conflict marks (`ShortcutsModel` with the tab's `clashes` contexts), computed on the keymap the rows show with the changed binding taken out and the new binding added last. Only `ShortcutIssue::Conflict` commands are listed ("Also bound to …"), not the longer chords the keys would start (`secondary-w` starts about 30 `pane.*` chords).
- A changed binding keeps its When (`KeymapEdit::Change` has no context to change); the When field shows only when adding. A binding a later entry hides has no pencil and isn't changed, as Remove refuses it; Add Keybinding still works on its line.
- The pencil and Add buttons are text glyphs (`✎`, `+`): kiln-ui's icon set has no pencil or plus icon, and kiln-ui is out of scope.
- The recorder closes on Escape, Enter, a click on another line, a line's menu, when its line is no longer listed after a refilter or reload, or when focus leaves both its box and its When field (review F1: scrolling its line out of the `uniform_list` takes the box, and with it `HOLDS_ALL_KEYS`, away; test `scrolling_the_line_away_closes_the_recorder`).
- The preview of a changed binding keeps its args (review F2), so moving a `vim.move { motion = "left" }` binding onto Left doesn't clash with the default that runs the same motion; test `conflict_preview_keeps_a_changed_bindings_args`.
