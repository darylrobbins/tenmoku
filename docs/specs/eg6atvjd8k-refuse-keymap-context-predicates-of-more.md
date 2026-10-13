---
task_uid: "01m4syn8bbzs8qckeg6atvjd8k"
task: "T81"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The new kiln/crates/kiln-command/tests/predicate_depth.rs has a test named exactly deep_predicates_are_refused (no other name): ContextPredicate::parse returns Err for 100,000 nested !, 100,000 nested parentheses, and 100,000-operand \u0026\u0026 and || chains; at base the binary aborts on stack overflow"
    required: true
    manual: false
  - id: "C2"
    text: "A test named exactly predicates_up_to_the_limit_still_parse: 256 operators parse and evaluate; 257 are refused with 'predicate too complex' at the 257th operator's offset"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing kiln-command, kiln-core and fe-keymap test passes unchanged"
    required: true
    manual: false
---

# Refuse keymap context predicates of more than 256 operators instead of overflowing the stack at launch (Mac T625)

Filed on argon from Mac T625 (card ~/coordination/argon-cards/T625.json rev 1). Mac task uid 01m4syj7ayw115nxqdvpge7rax. No dependencies. The criteria name exact test names: the pinned regression tests must use exactly those names. Title the host commit "... (Mac T625)".

## Problem

`ContextPredicate::parse` (kiln/crates/kiln-command/src/predicate.rs, `Parser`, around lines 173-220)
recurses once per `!` and once per `(` in `Parser::unary`. `&&` and `||` chains are built in a loop, but
as a left-deep tree as deep as the chain, and `eval`, `Display` and the derived `Drop`, `PartialEq` and
`Hash` all recurse through that tree. Nothing bounds the depth.

A keymap binding's `context` string comes from the user's `keymap.toml` (up to 1 MiB). A probe crate
against main (e6988aba) showed that `"!"` x 100,000 followed by `a` aborts the process with a stack
overflow inside `ContextPredicate::parse`, and `a && a && ...` with 100,000 operands parses, then aborts
in `eval`. Either one kills the app at launch and again on every hot reload until the file is fixed,
with no diagnostic. `predicate.rs` has never had a depth check.

## Change

Give `Parser` an operator budget: count every `!` (not the start of `!=`), `(`, `&&` and `||` it
consumes, and once the count passes `MAX_OPERATORS = 256` return a `PredicateError` at that operator's
offset with the message "predicate too complex". A tree can't be deeper than its operator count, so this
bounds the recursion in parsing, `eval`, `Display`, `Drop`, `PartialEq` and `Hash`. The longest context
in the shipped keymaps is about 115 chars with a handful of operators, far under the limit. Keymap loading
already reports a `PredicateError` as a diagnostic on the binding's `context` key
(kiln/crates/kiln-core/src/keymap.rs, around line 445), so no caller changes.

Regression test: a new integration test file kiln/crates/kiln-command/tests/predicate_depth.rs (the crate
has no tests/ dir yet), public API only (`kiln_command::ContextPredicate`). It must contain a test named
exactly `deep_predicates_are_refused`, which asserts that `ContextPredicate::parse` returns `Err` for `!`
x 100,000 then `a`, for `(` x 100,000 then `a` then `)` x 100,000, and for 100,000 `a` joined by ` && `
and by ` || `. At base this binary aborts on stack overflow. A second test named exactly
`predicates_up_to_the_limit_still_parse` checks that a predicate with exactly 256 operators parses and
evaluates, and that 257 is refused with the error's offset at the 257th operator.
