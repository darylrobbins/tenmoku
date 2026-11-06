---
task_uid: "01m6rgr28g31rs7284fwb210vv"
task: "T1392"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "percent_operator_verbs_are_placeholders: in \"%s: %5.2f %(k)d %%\" % args the three directives are string.placeholder and %% is escape"
    required: true
    manual: false
  - id: "C2"
    text: "format_method_fields_are_placeholders: \"{} {0} {name!r:\u003e{width}}\".format(...) gives one placeholder per field and {{ }} escapes"
    required: true
    manual: false
  - id: "C3"
    text: "logging_calls_colour_percent_verbs: log.info(\"%s took %d\", a, b) colours both verbs; foo.bar(\"%d\") does not"
    required: true
    manual: false
  - id: "C4"
    text: "fstrings_and_non_literals_are_untouched: f\"{x}\" % y, a variable on the left of %, and name.format(1) inject nothing and f-string spans equal python_fstring_highlights' results"
    required: true
    manual: false
  - id: "C5"
    text: "verbs::py_dialect_tests::chunking_never_changes_the_python_spans: both Python dialects give the same spans when scanned in chunks of 1..len bytes, and an unclosed { paints nothing"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass; python_fstring_highlights and the Go and C tests pass unchanged"
    required: true
    manual: false
---

# Colour Python % and str.format placeholders ("%s" % x, "%(k)d", "{} {0!r:>10}".format, logging-style calls) apart from the string

## Problem
Go (T443/T450) and, in the previous card, C/C++ colour printf verbs inside the string by injecting a hidden format language painted by a scanner in src/verbs.rs. Python is next. f-strings already colour their `{}` fields, conversions, specs and `{{ }}` escapes (queries/python.scm, tests/python_fstring_highlights.rs); nothing colours the other two Python formatting forms: `"%s: %d" % (name, n)` (also `%(name)s` and `%%`) and `"{} {0} {name!r:>{width}}".format(...)` with `{{ }}` escapes.

## Change
- verbs.rs: add `Dialect::PyPercent` (printf's phases plus an optional `%(key)` mapping key before the flags; length modifiers h l L are accepted and ignored; conversions diouxXeEfFgGcrsa; `%%` is an escape) and `Dialect::Brace` (replacement field `{` [arg: name, digits, `.attr`, `[idx]`] [`!r|s|a`] [`:` spec, whose own `{...}` nest one level] `}` painted as one string.placeholder; `{{` and `}}` as escape; an unclosed `{` paints nothing). Resumable by chunk like the others (the nesting depth is one extra counter).
- language.rs: hidden Defs `format-py-percent` and `format-brace` on the printf grammar (gated on the printf feature, as in the previous card); injection.rs maps layer name to dialect. Cargo: the `python` feature also enables `printf`.
- queries/python-injections.scm (new; the python Def gets `injections: Some(include_str!(..))`): inject format-py-percent into the string_content of a plain `string` (no interpolation: leave f-strings to python.scm) that is the left operand of a binary_operator with operator "%"; inject format-brace into the string_content of a string that is the object of a call whose function is an attribute named `format` (so `"{}".format(x)`). Also the logging-style calls `<x>.debug|info|warning|warn|error|exception|critical(<string>, ...)` and `<x>.log(<level>, <string>, ...)` inject format-py-percent: any receiver, because types are unknown; the only false positive is a `"%d"` first argument to a method with one of those names. Bytes strings (b"%s" % x) are included in the % form. A variable on the left of `%` or as `.format`'s receiver injects nothing.
- Keep python.scm's f-string patterns as they are: an f-string used with `.format` or `%` is not injected (the layer would double-paint its interpolations).

## Tests
New file crates/fe-syntax/tests/python_format_highlights.rs (`#![cfg(all(feature = "python", feature = "printf"))]`, spans() as in python_fstring_highlights.rs plus the take_missing_languages loop) and scanner unit tests in a new crates/fe-syntax/src/verbs_py_tests.rs declared `#[cfg(test)] #[path = "verbs_py_tests.rs"] mod py_dialect_tests;` from verbs.rs. Regenerate crates/fe-syntax/tests/golden/python.py.golden (add one % and one .format line to python.py if absent); the diff shows only added spans.

Judgment: one card, not two, because both forms share the dialect plumbing and the injection query file; if the worker finds the Brace scanner the larger half, split it off as a second card that depends on this one.
