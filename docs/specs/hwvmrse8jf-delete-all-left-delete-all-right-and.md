---
task_uid: "01m5vxs7n01a4sfmhwvmrse8jf"
task: "T1184"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "delete_extent_tests::delete_all_left: editor.delete_all_left deletes from each caret back to column 0 of its row (indentation included), at column 0 deletes the previous row break instead, at the document start does nothing, deletes a non-empty selection as Backspace does, and merges carets whose ranges touch; one undo step"
    required: true
    manual: false
  - id: "C2"
    text: "delete_extent_tests::delete_all_right: editor.delete_all_right deletes from each caret to the end of its row, at the row end deletes the row break (joining the next row), at the document end does nothing, deletes a non-empty selection as Delete does; works with several carets on different rows and on one row"
    required: true
    manual: false
  - id: "C3"
    text: "delete_extent_tests::delete_word_right: editor.delete_word_right deletes to where editor.move_word_right would go (same word classes, so a run of punctuation or spaces counts as VS Code counts it), at the row end deletes the row break, and mirrors delete_word_left on a mirrored fixture; one undo step"
    required: true
    manual: false
  - id: "C4"
    text: "delete_extent_tests::graphemes_and_crlf: a combining mark, an emoji ZWJ sequence and a CRLF row break are never split by any of the three; carets after an edit land at the deletion point"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap delete_extent_key: on macOS cmd-backspace, ctrl-k and alt-delete resolve to the three commands in the editor context outside vim normal mode, cmd-backspace in an editor with a proposal still resolves to proposal.reject_all, and the Linux keymap binds only ctrl-delete to editor.delete_word_right (no default for the other two, as VS Code); Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Delete All Left, Delete All Right and Delete Word Right (cmd-backspace, ctrl-k, alt-delete on macOS), as VS Code's deleteAllLeft, deleteAllRight and deleteWordRight

Three deletions VS Code binds on macOS that fastedit lacks (only delete_word_left exists, bound to alt-backspace). New module delete_extent.rs registering editor.delete_all_left, editor.delete_all_right, editor.delete_word_right; each is a one-liner over the crate-private EditorModel::delete_with(around, typed) already used by delete_word_left (lib.rs ~1228), so lib.rs only gains the mod line and commands.rs the register call.

VS Code behaviour: deleteAllLeft (cmd-backspace) removes from the caret to the start of the line; with the caret at column 0 it removes the line break before it; deleteAllRight (ctrl-k on macOS) removes to the end of the line, or the line break at the end; both delete a non-empty selection like Backspace/Delete. deleteWordRight (alt-delete mac, ctrl-delete elsewhere) is the right-hand twin of delete_word_left. Overlapping or touching ranges from several carets merge (delete_with/apply_typed already do this; add a test, do not reimplement). Use graphemes, not bytes.

Keys: macOS only for the first two (VS Code has no Linux default; ctrl-k is its chord prefix there). Context "editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)" like join_lines. cmd-backspace already has a binding in context "editor && proposal" (proposal.reject_all, platform macos, keymap ~line 1533): put the new binding BEFORE it and keep the proposal one winning when a proposal is active (test it). Check ctrl-k on macOS does not disturb "ctrl-k escape" (that one is !macos). Add the three to the macos_only list in fe-workbench tests/default_keymaps.rs as needed (delete_word_right has a Linux binding, ctrl-delete, so it is not macOS-only).

Tests: headless EditorModel via the case_transform_tests.rs dispatch helper (Invocation::of("editor.<name>")); marked fixtures from tests::parse/render. No GUI.
