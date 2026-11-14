---
task_uid: "01m7c01cy9181dhndtc4mqza4e"
task: "T630"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features python,kotlin --test named_argument_highlights passes: python_keyword_arguments_are_properties, kotlin_named_arguments_are_properties"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features kotlin --test wasm_parity kotlin_extension_is_as_built_in passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --features python,kotlin highlights_match_golden_files passes with python.py.golden and kotlin.kt.golden regenerated"
    required: true
    manual: false
---

# Highlight Python keyword arguments and Kotlin named arguments as property (Mac T1819)

# Highlight Python keyword arguments and Kotlin named arguments as properties

Decision 2026-11-13: a name that labels a value is `property`. Today Python's `f(key=1)` name is
`variable.parameter` or plain via `@variable`, and Kotlin's `f(name = 1)` the same. Add overrides:

- `crates/fe-syntax/queries/python.scm` (exists; append after the f-string patterns, keep its header's "Ours"
  note true): `(keyword_argument name: (identifier) @property)`. A definition's parameter
  (`def f(key=1)`, a `default_parameter`) stays `variable.parameter`; only call-site keyword arguments move.
- `crates/fe-syntax/queries/kotlin.scm` (exists; append, and extend its header with what the new patterns are):
  `(value_argument (simple_identifier) @property "=")`-style pattern for a named argument; check the node names in
  tree-sitter-kotlin-sg 0.4.1's `node-types.json` (an unknown name fails the whole query; the grammar's argument
  node is `value_argument` with an optional leading `simple_identifier` before `=`). A positional argument is not
  touched.
- Mirror: `extensions/grammars/kotlin/highlights.scm` is the crate query then `kotlin.scm` byte for byte
  (`wasm_parity.rs` `kotlin_extension_is_as_built_in`); regenerate it with the same two strings, no `wasm_parity.rs`
  edit. Python has no extension. Wasm unchanged: no `pack.toml` edit, no `NOT_RELEASED` entry; the kotlin extension
  directory ships through the grammar registry's own release.

Regenerate `tests/golden/python.py.golden` and `tests/golden/kotlin.kt.golden` (`UPDATE_GOLDEN=1 cargo nextest run
-p fe-syntax --features python,kotlin golden`) and review: only keyword-argument names move.

**Test.** New file `crates/fe-syntax/tests/named_argument_highlights.rs`,
`#![cfg(all(feature = "python", feature = "kotlin"))]`, `spans` helper like `tests/code_keys.rs`.
`python_keyword_arguments_are_properties` (`f(a, key=1, **kw)`: `key` property; `def f(key=1): pass`: `key`
variable.parameter), `kotlin_named_arguments_are_properties` (`f(1, name = "x")`: `name` property; positional
`f(x)`: `x` unchanged). Base fails. Also
`cargo nextest run -p fe-syntax --features kotlin --test wasm_parity kotlin_extension_is_as_built_in`.

Conflicts: no other open task lists python.scm/kotlin.scm; shares goldens with nobody before G1 (G1 depends on
this card). Linux-OK.
