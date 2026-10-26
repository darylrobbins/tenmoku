---
task_uid: "01m5vdx0sbdmm58jy8vmffs412"
task: "T245"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "block_comment_tests::wraps_the_selection: with tokens /* */ and 'a b c' selecting 'b', editor.toggle_block_comment gives 'a /* b */ c' with 'b' still selected; again restores 'a b c' (also when the selection includes the delimiters or spaces inside them)"
    required: true
    manual: false
  - id: "C2"
    text: "block_comment_tests::caret_wraps_the_row: an empty selection wraps the row's text (not its indent) in the pair; a caret inside an existing /* */ on the row removes it"
    required: true
    manual: false
  - id: "C3"
    text: "block_comment_tests::no_block_tokens: with CommentTokens{block: None} (Python, shell) the command does nothing, changes no text and reports no change; HTML-style \u003c!-- --\u003e pairs work the same"
    required: true
    manual: false
  - id: "C4"
    text: "block_comment_tests::many_selections: two selections toggle independently in one undo step, and the line toggle (editor.toggle_comment) tests in comment_tests still pass"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap block_comment_key: shift-alt-a resolves to editor.toggle_block_comment on macos and linux in the editor context outside vim normal mode; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Toggle Block Comment (shift-alt-a), as VS Code's editor.action.blockComment (Mac T1178)

comment.rs already carries CommentTokens.block and uses it only as toggle_comment's fallback; there is no command that wraps a selection. Add editor.toggle_block_comment ("Toggle Block Comment") in comment.rs next to toggle_comment and use the host's comment_tokens(). Tests in a new file block_comment_tests.rs (declared from comment.rs via #[path] like comment_tests). Line-comment behaviour must not change.
