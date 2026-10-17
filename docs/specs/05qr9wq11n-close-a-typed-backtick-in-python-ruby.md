---
task_uid: "01m557xpsp1fr50s05qr9wq11n"
task: "T152"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-syntax::backtick_auto_pairs python_ruby_swift_and_json_close_backticks (tests/backtick_auto_pairs.rs, top level) passes: auto_pairs of python, ruby, swift and json is () [] {} \"\" '' and a backtick pair, in that order, and rust, lua, yaml, css, c and java still have no backtick pair; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "tests/auto_pairs.rs is edited only to move python, ruby, swift and json to the backtick arm of expected and to add the backtick to the json row of values, and its every_language_has_an_entry and values pass; every other language keeps its pairs"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax -p fe-editor passes with no existing test edited but tests/auto_pairs.rs; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Close a typed backtick in Python, Ruby, Swift and JSON, as their VS Code language configurations do, instead of typing one (Mac T791)

## Problem

`auto_pairs` (`crates/fe-syntax/src/comment.rs`) closes a typed backtick in JavaScript, TypeScript, TSX,
Go, Bash and Markdown only. VS Code's language configurations for Python, Ruby, Swift and JSON (the
`autoClosingPairs` in extensions/{python,ruby,swift,json}/language-configuration.json of the VS Code app)
all list `` ` `` ⇢ `` ` `` too (with `notIn: ["string"]`, or `["string", "comment"]` for JSON), so typing a
backtick there gives a pair and wraps a selection in backticks. Ruby's and Swift's backticks delimit
command strings and raw identifiers, Python's appear in docstrings and f-string reprs, and JSON files
hold Markdown-ish strings.

Reproduced at Mac main 3e3eaa03 (T773 landed): `auto_pairs("python")`, `"ruby"`, `"swift"` and `"json"`
are `() [] {} "" ''` with no backtick, so in fastedit a typed backtick there types one, and over a
selection replaces it.

## Change

Move `python`, `ruby`, `swift` and `json` from the `() [] {} "" ''` arm to the arm with the backtick, in
the same order (brackets, `"`, `'`, `` ` ``). Nothing else changes: other languages keep their pairs, and
the `notIn` string and comment conditions stay out of scope for every language, as now. Update
`crates/fe-syntax/tests/auto_pairs.rs`, which pins the table: its `expected` arms and the `json` row of
`values`. Test in a new `crates/fe-syntax/tests/backtick_auto_pairs.rs`. Prototyped on 3e3eaa03: all of
`cargo test -p fe-syntax` passed with the auto_pairs.rs edit.
