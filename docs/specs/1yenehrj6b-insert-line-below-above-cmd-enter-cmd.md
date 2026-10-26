---
task_uid: "01m5vdx7bvdc2ea51yenehrj6b"
task: "T246"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "row_insert_tests::below_opens_an_indented_row: with the caret mid-row on '    foo(a, b)', editor.insert_line_below adds an empty row under it carrying the row's indent (auto-indent rules apply after '{'), the caret on that row; the row text is untouched; one undo reverts it"
    required: true
    manual: false
  - id: "C2"
    text: "row_insert_tests::above_and_many_carets: editor.insert_line_above puts the new row above with the same indent; carets on the same row make one row and carets on different rows one each; a selection spanning rows uses its last row for below and its first for above"
    required: true
    manual: false
  - id: "C3"
    text: "row_insert_tests::copy_up_keeps_selection_on_the_upper_copy: editor.copy_rows_up duplicates the touched rows above themselves and leaves the selection on the upper copy, the reverse of editor.duplicate_rows; a final row without a newline copies correctly; one undo step"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap row_insert_keys: cmd-enter, cmd-shift-enter, shift-alt-up resolve on macos (ctrl-enter, ctrl-shift-enter, ctrl-shift-alt-up elsewhere) in the editor context outside vim normal mode; cmd-enter stays proposal.accept_all in 'editor \u0026\u0026 proposal' and git.commit_box.commit in git_panel"
    required: true
    manual: false
  - id: "C5"
    text: "Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Insert Line Below / Above (cmd-enter, cmd-shift-enter) and Copy Lines Up (shift-alt-up), as VS Code's editor.action.insertLineAfter, insertLineBefore and copyLinesUpAction (Mac T1179)

Everyday row insertion VS Code has and fastedit lacks. The existing row commands (crates/fe-editor/src/rows.rs) cover indent, move, duplicate down and delete; add the rest in a new module row_insert.rs registered with one line in commands.rs, so rows.rs is not touched. Reuse the auto-indent hook for the new row's indent. Keybinding context matches editor.duplicate_rows (editor && (!vim.enabled || insert || replace)). Check the proposal and git_panel cmd-enter contexts still win in their own contexts.
