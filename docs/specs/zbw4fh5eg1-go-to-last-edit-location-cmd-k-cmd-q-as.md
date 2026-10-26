---
task_uid: "01m5v8z3gaytev68zbw4fh5eg1"
task: "T1181"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "last_edit_tests::goes_to_the_last_change: after typing at row 2 and then moving the caret to row 9, editor.go_to_last_edit_location puts the caret where the edit ended (after the typed text) on row 2 and reveals it; a second call stays there"
    required: true
    manual: false
  - id: "C2"
    text: "last_edit_tests::follows_later_edits: an insert above the edit moves the remembered location with the text (anchor, not offset); a delete over it clamps to the deletion point"
    required: true
    manual: false
  - id: "C3"
    text: "last_edit_tests::ignores_non_user_edits: undo, redo, format, AI and disk reloads do not set the location; with no edit yet the command does nothing; the location is per editor model, and split views of one buffer share it"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap last_edit_key: secondary-k secondary-q resolves to editor.go_to_last_edit_location in the editor context; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Go to Last Edit Location (cmd-k cmd-q), as VS Code's workbench.action.navigateToLastEditLocation

Records the end of the last user edit as a buffer anchor in EditorModel and adds the command. Scope is the active editor's buffer (VS Code's also switches files; a cross-file follow-up can use the workbench nav history in crates/fe-workbench/src/nav.rs, not part of this card). The edit hook lives in EditorModel's edit path, so crates/fe-editor/src/lib.rs gets a field and one call; keep the hook O(1) per edit. The jump should push the previous caret onto nav history only if that is already the pattern for editor-local jumps; otherwise do not.

## Implementation notes

- `EditorModel::last_edit` is an `Rc<Cell<Option<Anchor>>>`, cloned by `split`, so a buffer's models share it. `note_edit` (in `last_edit.rs`) sets it to the primary head anchor of the selections an edit leaves when `origin` is `Origin::User`: O(1), one `Cell::set`.
- It records **before** the model catches up (review round 1, F2): catching up may measure chars, and a width may edit through another split; recording after would let the older edit overwrite that newer one. `an_edit_made_while_catching_up_is_the_last` covers ordinary and row edits.
- Call sites: `apply_through` (lib.rs: typing, paste, delete, newline, snippets) and `apply_row_edits` (rows.rs: the row commands, which bypass `apply_through`); completion acceptance (`completion.rs`) records its caret. Host edits made for the user straight to the buffer record through `BufferMut::note_edits_since(version)`, called inside the guard so its catch-up can't reorder them (review round 1, F1): find/replace (`find_bar.rs`), project search replace in open buffers (`search_replace.rs`), `EditorView::edit_and_reveal` (inlay hint insertion, conflict acceptance), vim's `dp` in a diff (`git_diff.rs`), and workspace edits the user asked for (`Workbench::apply_user_file_edits`, used by rename and picked code actions). `apply_file_edits` stays non-recording for format-on-save code actions, a server's `workspace/applyEdit` and a file rename's import updates.
- Still excluded: undo and redo, `reload`, the LSP formatter, save-time fixups (`pane_item.rs`), git's restores of a file's text, AI edits and proposals, and commands run as another actor (non-user `origin`).
- An anchor created by a user edit and then undone and redone maps back to where it was (the edit log restores it), so after redoing the last edit the place is after it again; `ignores_non_user_edits` pins this.
- Jumping unfolds a fold hiding the place, then sets one caret (`Effect::Changed`, which the view reveals). No `editor.*` jump pushes nav history (only workbench jumps, such as go to line, do), so this doesn't.
- `crates/fe-editor/tests/viewer.rs` lists every command a viewer doesn't support; the new id goes there. `crates/fe-gpui/src/tests.rs` needs a case for every editor binding (`bindings_are_all_tested`).
