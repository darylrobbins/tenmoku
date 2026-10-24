---
task_uid: "01m5n96y7bxkq3s89s98h406qg"
task: "T298"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "edit_keymap(source, KeymapEdit) -\u003e Result\u003cString, KeymapEditError\u003e is pure text-to-text on toml_edit::DocumentMut (already a dependency of fe-keymap's build; add it as a normal dependency, license MIT/Apache-2.0, already in the workspace): Add{keys, context, command, args} appends a [[bindings]] table with the fields written as in fastedit.toml (keys, command, context, args only when not null); Remove{keys, context} on a binding that exists in the user file deletes that table together with the comment lines directly above it; on a default-only binding appends an entry with command = \"\" for the same keys and context; Change{old_keys, context, new_keys} rewrites the keys of a user entry or, for a default, appends the unbind plus the new binding; ResetCommand{command} deletes every user entry for the command and every unbind entry that hides a default of that command. Tests add_appends_entry, remove_user_entry_deletes_with_its_comment, remove_default_appends_unbind, change_user_entry, change_default_unbinds_and_adds, reset_command, comments_and_other_entries_survive_byte_for_byte"
    required: true
    manual: false
  - id: "C2"
    text: "Each edit validates before writing: the keys parse as a KeySequence, the context as a ContextPredicate, the command as a CommandId; a result that fails to load as a KeymapLayer is an Err and nothing is written. Tests invalid_keys_rejected, invalid_context_rejected, result_always_loads"
    required: true
    manual: false
  - id: "C3"
    text: "write_keymap(path, edit) reads the file (a missing file starts empty, a header comment pointing at the docs is added only for a missing file), applies the edit and replaces the file atomically (temp file in the same directory, then rename; keep the file mode), re-reading once if it changed meanwhile. Tests write_creates_missing_file, write_is_atomic_and_keeps_mode, concurrent_change_is_not_lost"
    required: true
    manual: false
  - id: "C4"
    text: "Round trip: after any edit, loading defaults + the edited user file through Keymap::from_layers gives the intended effective bindings (test effective_keymap_after_each_edit). Headless; Linux-OK"
    required: true
    manual: false
---

# Add, change, remove and reset keybindings in the user keymap.toml with comments and layout kept: unbind a default with an empty-command entry, delete a user entry, and show the rebuilt keymap's issues (Mac T1099)

## Problem
The user keymap (`user_keymap_path()`, `keymap.toml`) can only be changed by hand. A Keyboard Shortcuts editor needs add / change / remove / reset to write that file. Semantics already exist in the engine (kiln-core `keymap.rs`): a later entry with the same keys and context wins, and `command = ""` removes every earlier binding with the same keys and context. VS Code writes keybindings.json the same way: an entry with a leading `-` command removes a default.

## Change
New `crates/fe-keymap/src/keymap_edit.rs`, declared and re-exported from `lib.rs`. `crates/fe-keymap/Cargo.toml`: move `toml_edit` from build-dependencies to also be a dependency (workspace version 0.25; MIT OR Apache-2.0; already used by fe-config and kiln-core). Use `fe_keymap::Keystroke`/`KeySequence` for validation and `ContextPredicate::parse`, `CommandId::parse` from kiln-command via the existing re-exports (add re-exports if missing, nothing else). Do not edit kiln-core. The `args` field is a TOML inline table or value; accept a `serde_json::Value`-free `toml::Value`.

## Tests
New `crates/fe-keymap/tests/keymap_edit.rs`, tempfile dirs for the write tests (add tempfile as a dev-dependency only if fe-keymap lacks it; it is a workspace dependency). Headless, Linux-OK.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.

## Implementation notes

- `edit_keymap(source, edit)` and `write_keymap(path, edit)` sit on fastedit's built-in keymap on this platform; `edit_keymap_on` / `write_keymap_on` take the default keymaps as text (`DefaultKeymap`, path and source) and the `Os`. Edits need the defaults: Remove and Change decide between a user entry and a default, and ResetCommand finds the unbinds hiding a command's defaults, on every platform, so the defaults are read per platform as needed.
- Edits are spliced into the source text at the spans `toml_edit::Document` reports, never by reserializing the document, so every byte outside the touched entries survives: a byte order mark, mixed line endings, a missing final newline. Added text uses the file's first line ending.
- Each `[[bindings]]` table is matched to its entry by parsing it alone through `KeymapLayer::parse_for`, so keys (`secondary`, keys by platform) and contexts compare exactly as the engine compares them. A table's text is a section per header it owns (`[[bindings]]`, and its own `[bindings.keys]` or `[bindings.args]`, which another table may come between), each from the comments above the header to the next header's comments, the file's trailing comments or its end; a delete removes every section and keeps what's between them.
- Remove deletes every user entry with the keys and context, then appends an unbind if a default still binds them, so the keys end up unbound either way; keys bound by nothing are `NotBound`. Change rewrites the winning user entry's keys: a string in place, or only this platform's key of keys by platform (inline, dotted or `[bindings.keys]`); for a default it appends the unbind and the binding with its command and args. ResetCommand matches user entries by their `command` text and unbinds against each platform's defaults, so entries limited to another platform go too.
- Deleting a table also deletes the comment lines directly above its header, up to a blank line; what's above that stays, and the blank lines after the table go when one is kept above it. Comments at the end of the file stay above an appended entry.
- The result is always checked, an edit that changes nothing included: a file that doesn't load is `Invalid` for every edit, and isn't repaired. `bindings` as an inline array is `Layout`.
- `write_keymap` follows kiln-core `write_settings`'s temp-file-then-rename scheme (private there, so copied): permissions kept, symlinks written through and checked to still resolve to the same file before the rename, permissions and contents checked unchanged, a missing file hard-linked in place. A change meanwhile, even under an edit with nothing to write, makes it read and edit again, once. It returns `KeymapWrite` with the rebuilt keymap, its `report()` issues and the file's warnings. `write_keymap_hooked` (doc-hidden) runs a hook before the file is replaced, for the concurrency tests.
- Scope widened to `crates/fe-keymap/Cargo.toml` (toml_edit as a dependency; fe-command moved from dev-dependencies to dependencies for `CommandId`/`ContextPredicate`, now re-exported) and `Cargo.lock` (unchanged).
