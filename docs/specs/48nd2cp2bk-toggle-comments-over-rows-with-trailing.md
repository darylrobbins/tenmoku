---
task_uid: "01m4sjp2y2dmxznf48nd2cp2bk"
task: "T86"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test comment::tests::token_only_row_uncomments_to_empty: with //, toggling \"  // \" gives \"  \" and \"//\" gives an empty row"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test comment::tests::block_wraps_trailing_whitespace: with only /* */, \"a  \" becomes \"/* a   */\" and toggles back to \"a  \" exactly, and \"/* a */  \" unwraps to \"a  \""
    required: true
    manual: false
  - id: "C3"
    text: "comment::tests and commands::comment_host_tests pass unchanged; cargo test -p fe-editor -p fe-gpui and cargo clippy on both --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Toggle comments over rows with trailing whitespace as specified: a token-only row uncomments to nothing and a block wraps the whole row (T67 review F2) (Mac T459 follow-up)

Follow-up from T67 (Mac T459, toggle comment, landed as 9ffbe5f), filed on helium. Review 01m4scdxez6zgbtheavezccrs6
(`rgt review show T67`), finding F2. (F1, two toggles before a frame, was fixed by T69/5298ee4: `ViewHost` takes
comment tokens from `loaded_language`. It is covered by
`commands::comment_host_tests::toggles_back_before_the_next_frame`.)

## Problem

`EditorModel::toggle_comment` (`crates/fe-editor/src/comment.rs` :43) works on each row's range with trailing
whitespace trimmed (`code = trimmed.trim_end()`, :55). That breaks both whitespace rules in T67's spec:

- **Line token.** The spec says a row "loses the token and one space after it if there is one". But for a row
  `// `, the space was trimmed off before the check (:69). Uncommenting leaves ` ` instead of an empty row.
- **Block pair.** The spec says "the text after its indentation becomes `open + " " + text + " " + close`". Today
  `a  ` becomes `/* a */  `, not `/* a   */`.

## Change

In comment.rs only:

- Each row keeps two ends: the full row end, and the end with trailing whitespace trimmed. The trimmed end is used
  only to skip blank rows and to detect an existing block wrap (the row's trimmed text starts with `open` and
  ends with `close`).
- **Line uncomment:** remove the token, plus one following space if the char right after the token in the full
  row is a space.
- **Block comment:** insert `open + " "` after the indentation and `" " + close` at the full row end, so trailing
  whitespace ends up inside the delimiters.
- **Block uncomment:** remove the delimiters where the trimmed range finds them, with one inner space each side
  where present, as today. So `/* a   */` restores `a  ` exactly, and `/* a */  ` still unwraps to `a  `.

Line comment insertion and indentation alignment don't change. Existing `comment::tests` pass unchanged.

## Tests

Add the tests to the existing `crates/fe-editor/src/comment_tests.rs` (already declared from comment.rs at base as
`mod tests`), leaving its existing tests unchanged, using `crate::tests::{parse, render}`. Not a new file: Regatta's
regression check applies only test files to base, so a test module declared in comment.rs can't run there.

- `token_only_row_uncomments_to_empty`: with `//`, toggling `  // ` gives `  `. A row `//` gives an empty row.
- `block_wraps_trailing_whitespace`: with only `/* */`, `a  ` becomes `/* a   */` and toggles back to `a  `.
  `/* a */  ` unwraps to `a  `.
