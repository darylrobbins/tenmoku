---
task_uid: "01m557xc2qhbada6bxr97afvjd"
task: "T139"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-editor::toggle_comment_blank_rows blank_rows_alone_are_commented_as_vs_code_does (tests/toggle_comment_blank_rows.rs, top level) passes: with tokens // and /* */, toggle_comment at a caret in an empty text gives \"// \" with the caret after it, a caret at the end of the empty middle row of a\\n\\nb gives a\\n// \\nb, a caret after \"  \" gives \"  // \", a selection over the whitespace rows \"  \" and \"\\t\" comments both at their ends, and a selection over a, an empty row and b gives // a\\n\\n// b; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Rows with text toggle as before and a block-only language still does nothing on blank rows: the fe-editor unit tests comment::tests::* and tests::commands pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor passes with no existing test edited; cargo clippy -p fe-editor --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Toggle Line Comment on blank rows alone comments them at their ends, as VS Code does, instead of doing nothing (Mac T787)

## Problem

`EditorModel::toggle_comment` (`crates/fe-editor/src/comment.rs`) collects the non-blank rows the
selections touch and returns early when there are none, so Toggle Line Comment on an empty row, or on a
selection of only whitespace rows, does nothing. VS Code's Toggle Line Comment (`LineCommentCommand`,
`_analyzeLines` in src/vs/editor/contrib/comment/browser/lineCommentCommand.ts) ignores blank rows only
among rows with text: when every touched row is blank it clears the ignore flag on all of them and
comments each at its end (`commentStrOffset = lineContent.length`), with the space after the token, and a
caret at the row's end moves after the inserted `// `.

Reproduced at Mac main 3e3eaa03 with a new integration test: with tokens `//` and `/* */`, a caret in
an empty text toggles to `""` (VS Code: `// `, caret after it), a caret on the empty middle row of
`a\n\nb` leaves it as is (VS Code: `a\n// \nb`), and `  ` stays `  ` (VS Code: `  // `). Typing
cmd-/ on a fresh line to start a comment, the common case, does nothing in fastedit.

## Change

In `toggle_comment`, when no touched row has text and `tokens.line` is `Some`, insert `"{token} "` at
the end of each touched row's content (after its whitespace, before its line break) through
`edit_rows_in_place`, as one transaction like the other paths. A caret at a row's end ends after the
inserted text; one inside the whitespace stays where it is. Rows with text keep today's behaviour,
blank rows among them included (still skipped). A language with only a block pair keeps doing nothing on
blank rows. Update the method's doc comment. Test in a new `crates/fe-editor/tests/toggle_comment_blank_rows.rs`.
