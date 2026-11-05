---
task_uid: "01m6j9a0fg3qd2qdpxjpvdsa95"
task: "T444"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "python_fstring_highlights::braces_are_placeholders: in f\"Hello {name}!\" the { and } are string.placeholder, name is variable, and \"Hello \" and \"!\" are string"
    required: true
    manual: false
  - id: "C2"
    text: "python_fstring_highlights::conversion_and_format_spec_are_placeholders: in f\"{path!r}: {share:6.1%} {x:\u003e{width}}\" the !r, :6.1% and :\u003e parts are string.placeholder, and width inside the nested spec is variable"
    required: true
    manual: false
  - id: "C3"
    text: "python_fstring_highlights::expression_keeps_its_colours: in f\"{a.b(1) + c}\" b is a call or attribute colour, 1 is number, + is operator, and nothing inside the braces is string or embedded"
    required: true
    manual: false
  - id: "C4"
    text: "python_fstring_highlights::escaped_braces_and_string_kinds: {{ and }} are escape and not placeholders; rf\"{x}\\d\", a triple-quoted f-string and a nested string f\"{d[\"k\"]}\" behave the same way"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass; the python.py.golden diff only changes f-string spans"
    required: true
    manual: false
---

# Colour Python f-string placeholders like VS Code: the braces, !r conversions and :format specs as string.placeholder, the expression inside in its own colours, {{ and }} as escapes (Mac T1377)

The user (2026-11-03): "Python f-strings don't highlight their placeholders either." Python uses tree-sitter-python 0.25's upstream HIGHLIGHTS_QUERY (crates/fe-syntax/src/language.rs, python Def, earlier_patterns_win false). It parses f-strings into `interpolation` nodes and captures `{`/`}` as punctuation.special, the inner expression with its normal captures, and the whole interpolation as `embedded`; so in crates/fe-syntax/tests/golden/python.py.golden, `f"skipping {path!r}: {error}"` gives punctuation.special braces, variable `path`, and `!r` as `embedded`. Themes give punctuation.special and variable near-plain colours and don't colour `embedded`, so placeholders don't stand out. All bundled themes define `string.placeholder` (e.g. vscode-2026-dark #569CD6, tenmoku-ember #d9965a), which is VS Code's placeholder colour.

Do: add our own crates/fe-syntax/queries/python.scm layered after the upstream query in the python Def's highlights slice (or replacing upstream if layering can't override; check how the highlighter resolves overlapping captures with earlier_patterns_win false and pattern order across the slice), so that in an f-string:
- the interpolation's `{` and `}` are string.placeholder;
- a `type_conversion` (`!r`, `!s`, `!a`) and a `format_specifier` (`:>10`, `:6.1%`, `:{width}`) are string.placeholder, with any nested interpolation inside a format spec keeping its own expression colours;
- the expression inside keeps its normal colours (variable, attribute, call, operator, number, string for a nested string), not string and not embedded;
- `{{` and `}}` (escaped braces) are escape;
- the literal text around stays string; byte strings, raw f-strings (rf"…") and triple-quoted f-strings behave the same.
Also update crates/fe-syntax/tests/golden/python.py.golden for the changed spans. Out of scope: `"%s" % x` and `"{}".format(x)` placeholders (a follow-up can reuse T1376's printf injection and a format-string grammar).

Tests in the new file crates/fe-syntax/tests/python_fstring_highlights.rs (`#![cfg(feature = "python")]`), using the spans() helper pattern from crates/fe-syntax/tests/rust_doc_comment_colours.rs. Headless; Linux-OK.
