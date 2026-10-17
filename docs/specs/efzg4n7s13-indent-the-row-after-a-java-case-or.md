---
task_uid: "01m54rn9sd6beymhefzg4n7s13"
task: "T767"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test fe-syntax::java_case_indent a_new_row_after_a_java_case_label_is_a_level_in (new crates/fe-syntax/tests/java_case_indent.rs, cfg feature java, public suggest_indent only): the new row after 'case 1:' in a closed switch is Relative { basis_row: \u003ccase row\u003e, delta: 1 }, after 'default:' delta 1, and a statement row under a case is a level in from its label"
    required: true
    manual: false
  - id: "C2"
    text: "The only production change is one ((switch_block_statement_group) @indent.begin (#set! indent.immediate)) pattern in java.scm, with its header Changes note; indent.rs, language.rs and THIRD_PARTY.md do not change"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --all-features passes with no existing test edited, and cargo clippy -p fe-syntax --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Indent the row after a Java case or default label a level in, as VS Code does

## Problem

Enter at the end of a Java `case 1:` or `default:` row leaves the new row at the label's own column
instead of a level in, so the case's first statement is typed at the label's indentation. JavaScript,
TypeScript and C already indent it. Verified at Mac main c46c99e8 through `fe_syntax::suggest_indent` on a
blank row inserted after the label (the `after` helper of crates/fe-syntax/tests/indent_open_in_error.rs):

- Java `class A {\n    void f(int a) {\n        switch (a) {\n            case 1:\n        }\n    }\n}`, after row 3:
  `Relative { basis_row: 3, delta: 0 }` (column 12). Expected delta 1 (column 16).
- The same after a `default:` row that follows a case's statements: delta 0, expected 1.
- javascript, typescript and c with the same switch: delta 1.
- An existing statement row under a Java case (`g();` under `case 1:`) is also suggested at the label's
  column, so reindenting pulls it out of its case.

VS Code's Java language configuration (extensions/java/language-configuration.json, checked in the installed
VS Code on this Mac) has an onEnterRule with `beforeText` `^\s*(\bcase\s.+:|\bdefault:)$` and action
`indent`: the row after a case or default label is a level in. It has no rule dedenting after `break`.

Cause: crates/fe-syntax/queries/indents/java.scm begins on `(switch_block)` but has nothing for a case's
`(switch_block_statement_group)`, the node that holds a label and its statements. javascript.scm has
`(switch_case)` / `(switch_default)` with `indent.immediate` and c.scm has `(case_statement)` the same way.

## Change

In crates/fe-syntax/queries/indents/java.scm, after the `"}" @indent.end` line, add ours:

```scheme
; Ours: a case's statements are a level in from its label, and so is a new
; row after the label.
((switch_block_statement_group) @indent.begin
  (#set! indent.immediate))
```

and extend the header's `; Changes:` note to say the case pattern is ours. No `break` dedent (VS Code has
none for Java), no change to indent.rs, language.rs or THIRD_PARTY.md. Arrow cases (`case 1 -> {`) already
indent and are unchanged.

Prototyped at c46c99e8 on a copy: after row 3 above gives delta 1; inside a closed switch with
`case 1:` / `g();` / `default:` / `h();` / `break;`, the statement rows are suggested a level in from their
label, `default:` a level in from the `switch` row, the row after `break;` stays at `break;`'s column and the
closing `}` lines up with `switch`; all of `cargo test -p fe-syntax` passes, tests/indent.rs's 29 unchanged.

Regression test: new integration test file crates/fe-syntax/tests/java_case_indent.rs, gated on
`#![cfg(feature = "java")]`, with its own `after` and `at` helpers copied from
tests/indent_open_in_error.rs (public `suggest_indent` API only, so it builds at base). One test,
`a_new_row_after_a_java_case_label_is_a_level_in`: after row 3 of the text above it is
`Relative { basis_row: 3, delta: 1 }`; in
`class A {\n    void f(int a) {\n        switch (a) {\n            case 1:\n                g();\n            default:\n                h();\n        }\n    }\n}`
the new row after row 5 (`default:`) is `Relative { basis_row: 5, delta: 1 }` and row 4 (`g();`) in place is
`Relative { basis_row: 3, delta: 1 }`.
