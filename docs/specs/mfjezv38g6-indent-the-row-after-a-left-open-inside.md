---
task_uid: "01m54me37z17y4cnmfjezv38g6"
task: "T761"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax test indent_open_in_error::an_unclosed_arrow_function_body_indents_the_next_row: after const f = () =\u003e { in javascript, typescript and tsx the new row is Relative { basis_row: 0, delta: 1 }, after row 1 of function g() { / const f = () =\u003e { it is delta 1, and typescript's const f = (): void =\u003e { gives delta 1"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test indent_open_in_error::an_unclosed_java_switch_block_indents_the_next_row: after an unclosed switch (a) { in a Java method the new row is Relative { basis_row: \u003cswitch row\u003e, delta: 1 }"
    required: true
    manual: false
  - id: "C3"
    text: "Other rows inside an @indent.auto ERROR still keep the previous row's indentation: cargo test -p fe-syntax passes with tests/indent.rs unchanged, and no file under crates/fe-syntax/queries changes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy -p fe-syntax --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Indent the row after a { left open inside a parse error in JavaScript, TypeScript and Java, as VS Code does

## Problem

`fe_syntax::suggest_indent` gives no indent to the row after a `{` left open when the parser turns the
unfinished construct into an ERROR node in JavaScript, TypeScript, TSX or Java. Verified at Mac main 7d20f49d
(new blank row after the given row, as Enter makes):

- JS/TS/TSX `const f = () => {` (row 0): `Relative { basis_row: 0, delta: 0 }`, column 0. Expected delta 1.
- JS `function g() {\n    const f = () => {` (row 1): column 4, expected 8.
- TS `const f = (): void => {`: column 0, expected 4.
- Java `class A {\n    void f(int a) {\n        switch (a) {` (row 2): column 8, expected 12.

The same openers indent when the parser keeps the node (`const f = function () {`, `const f = async () => {`,
`f(() => {` all give delta 1), and VS Code indents the row after any line ending in an unclosed `{`.

Cause: tree-sitter-javascript parses `const f = () => {` as `(ERROR "const" (identifier) "=" (formal_parameters)
"=>" "{")`, and Java's unclosed `switch (a) {` is an ERROR too. `javascript.scm` (line 93) and `java.scm`
(line 41) capture `(ERROR) @indent.auto`, so `Indents::walk` (crates/fe-syntax/src/indent.rs, the
`mark.auto || mark.ignore` check at the top of the loop) returns `Level::Keep` on reaching the ERROR, before the
error's still-open `{`, which `error_open_rows` already counts a level for, is looked at.

## Change

In `Indents::walk` (crates/fe-syntax/src/indent.rs), the auto/ignore `Level::Keep` return doesn't apply when `n`
is an error that has an opener still open at `row` (per `error_open_rows(n, self.text.row_start(row))`) on
`row`'s previous non-blank row (`self.prev_nonblank(row)`): the walk goes on and counts that opener's level.
Every other row inside an auto ERROR still keeps the previous row's indentation, as now. No query file changes.

New integration test file `crates/fe-syntax/tests/indent_open_in_error.rs` (gated on the `javascript`,
`typescript` and `java` features, using only the public `suggest_indent` API so it builds at base):

- `an_unclosed_arrow_function_body_indents_the_next_row`: for javascript, typescript and tsx, the new row after
  `const f = () => {` is `Relative { basis_row: 0, delta: 1 }`, and after row 1 of
  `function g() {\n    const f = () => {` it is `Relative { basis_row: 1, delta: 1 }`; typescript's
  `const f = (): void => {` gives delta 1.
- `an_unclosed_java_switch_block_indents_the_next_row`: after row 2 of
  `class A {\n    void f(int a) {\n        switch (a) {` it is `Relative { basis_row: 2, delta: 1 }`.

A prototype of this change (7 lines in `walk`) passed both tests and all of `cargo test -p fe-syntax`, including
`tests/indent.rs`'s 29 tests, unchanged.
