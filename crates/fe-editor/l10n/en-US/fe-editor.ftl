# fe-editor's strings (docs/decisions/xte5e7hqsw-i18n-design.md).

## Command titles: `command-<id>`, with the id's `.` as `-`, as kiln-command
## keys them (section 4), and a command's description as its message's
## `.description`. A command registers with its title resolved when the
## registry is built; `label()` and `detail()` resolve them again in the
## active locale wherever they're shown.

command-editor-add_caret_at_next_occurrence = Add Caret at Next Occurrence
command-editor-add_carets_at_all_occurrences = Add Carets at All Occurrences
command-editor-add_cursor_above = Add Cursor Above
command-editor-add_cursor_below = Add Cursor Below
command-editor-backspace = Delete Backward
command-editor-column_select_down = Column Select Down
command-editor-column_select_left = Column Select Left
command-editor-column_select_right = Column Select Right
command-editor-column_select_up = Column Select Up
command-editor-convert_indentation_to_spaces = Convert Indentation to Spaces
command-editor-convert_indentation_to_tabs = Convert Indentation to Tabs
command-editor-copy = Copy
command-editor-copy_rows_up = Copy Rows Up
command-editor-cursor_redo = Cursor Redo
command-editor-cursor_undo = Cursor Undo
command-editor-cut = Cut
command-editor-delete = Delete Forward
command-editor-delete_all_left = Delete All Left
command-editor-delete_all_right = Delete All Right
command-editor-delete_duplicate_lines = Delete Duplicate Lines
command-editor-delete_rows = Delete Rows
command-editor-delete_subword_left = Delete Subword Left
command-editor-delete_subword_right = Delete Subword Right
command-editor-delete_word_left = Delete Word Left
command-editor-delete_word_right = Delete Word Right
command-editor-detect_indentation = Detect Indentation from Content
command-editor-duplicate_rows = Duplicate Rows
command-editor-expand_selection = Expand Selection
command-editor-fold = Fold
command-editor-fold_all = Fold All
command-editor-fold_level_1 = Fold Level 1
command-editor-fold_level_2 = Fold Level 2
command-editor-fold_level_3 = Fold Level 3
command-editor-fold_level_4 = Fold Level 4
command-editor-fold_level_5 = Fold Level 5
command-editor-fold_level_6 = Fold Level 6
command-editor-fold_level_7 = Fold Level 7
command-editor-fold_recursively = Fold Recursively
command-editor-go_to_last_edit_location = Go to Last Edit Location
command-editor-indent = Indent
command-editor-insert = Insert Text
    .description = Replaces every selection with `text`, as typing does. A single char typed closes what it opens, in the host's language, unless auto-closing is off, and one typed as its row's first non-blank that closes a block re-indents the row to the block's.
command-editor-insert_line_above = Insert Line Above
command-editor-insert_line_below = Insert Line Below
command-editor-join_lines = Join Lines
command-editor-move = Move
    .description = Moves every caret `count` times (at most 10,000) by `motion`, extending the selections if `extend`.
command-editor-move_document_end = Move to Document End
command-editor-move_document_start = Move to Document Start
command-editor-move_down = Move Down
command-editor-move_last_selection_to_next_find_match = Move Last Selection to Next Find Match
command-editor-move_left = Move Left
command-editor-move_line_end = Move to Line End
command-editor-move_line_start = Move to Line Start
command-editor-move_right = Move Right
command-editor-move_rows_down = Move Rows Down
command-editor-move_rows_up = Move Rows Up
command-editor-move_subword_left = Move Subword Left
command-editor-move_subword_right = Move Subword Right
command-editor-move_up = Move Up
command-editor-move_word_left = Move Word Left
command-editor-move_word_right = Move Word Right
command-editor-newline = Insert Newline
command-editor-outdent = Outdent
command-editor-page_down = Page Down
command-editor-page_up = Page Up
command-editor-paste = Paste
command-editor-redo = Redo
command-editor-save = Save
command-editor-select_all = Select All
command-editor-select_all_occurrences = Select All Occurrences
command-editor-select_document_end = Select to Document End
command-editor-select_document_start = Select to Document Start
command-editor-select_down = Select Down
command-editor-select_left = Select Left
command-editor-select_line_end = Select to Line End
command-editor-select_line_start = Select to Line Start
command-editor-select_next_occurrence = Select Next Occurrence
command-editor-select_page_down = Select Page Down
command-editor-select_page_up = Select Page Up
command-editor-select_right = Select Right
command-editor-select_subword_left = Select Subword Left
command-editor-select_subword_right = Select Subword Right
command-editor-select_up = Select Up
command-editor-select_word_left = Select Word Left
command-editor-select_word_right = Select Word Right
command-editor-set_end_of_line_crlf = Change End of Line Sequence to CRLF
command-editor-set_end_of_line_lf = Change End of Line Sequence to LF
command-editor-shrink_selection = Shrink Selection
command-editor-single_selection = Single Selection
command-editor-sort_lines_ascending = Sort Lines Ascending
command-editor-sort_lines_descending = Sort Lines Descending
command-editor-split_selection_into_lines = Split Selection into Lines
command-editor-tab = Insert Tab
command-editor-toggle_block_comment = Toggle Block Comment
command-editor-toggle_comment = Toggle Line Comment
command-editor-toggle_soft_wrap = Toggle Soft Wrap
command-editor-transform_lowercase = Transform to Lowercase
command-editor-transform_titlecase = Transform to Title Case
command-editor-transform_uppercase = Transform to Uppercase
command-editor-transpose_letters = Transpose Letters
command-editor-trim_trailing_whitespace = Trim Trailing Whitespace
command-editor-undo = Undo
command-editor-unfold = Unfold
command-editor-unfold_all = Unfold All
command-editor-unfold_recursively = Unfold Recursively

## Errors a command reports, with the error that caused it.

editor-undo-failed = undo failed: { $error }
editor-redo-failed = redo failed: { $error }
editor-save-failed = save failed: { $error }

## `editor.move`'s args: a count over the most moves one invocation makes.

editor-move-count-too-large = count { $count } is more than { $max }

## Why a save didn't write.

editor-save-changed-on-disk = the file changed on disk since it was read
editor-save-no-path = buffer has no path
# The path to save to is a directory: what Unix's EISDIR says.
editor-save-is-a-directory = Is a directory
editor-save-not-a-regular-file = not a regular file
# Following the path's symlinks went round in a loop: what Unix's ELOOP says.
editor-save-symlink-loop = too many levels of symbolic links

## Why a hunk of a proposed changeset couldn't be applied or reverted;
## `$path` is the file's.

editor-changeset-conflict = { $path } changed since it was proposed
editor-changeset-no-such-file = { $path } is not in the changeset
editor-changeset-not-pending = that hunk of { $path } is not pending

## A viewer's copy of a selection larger than it copies, in bytes.

editor-viewer-copy-too-large = { $len ->
    [one] the selection is { $len } byte, more than the { $limit } a copy takes
   *[other] the selection is { $len } bytes, more than the { $limit } a copy takes
}
