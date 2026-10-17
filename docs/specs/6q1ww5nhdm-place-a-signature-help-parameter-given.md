---
task_uid: "01m53hc4vhn9hsmq6q1ww5nhdm"
task: "T105"
rev: 3
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp test signature_help::label_tests::string_labels_are_found_as_words passes: max(a, b) gives [4..5, 7..8]; fn foo(o: Opts) gives [7..8]; fn pick(i: u8, item: T) gives [8..9, 15..19]; a non-word label ... is found; g(a, aa) with a, a, zz keeps 2..3, 5..6, 8..8; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp passes with signature_help_tests.rs unchanged; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Place a signature help parameter given as a string on the first whole-word match in the label, not inside the function's name (Mac T725)

Filed on argon from Mac T725 (card ~/coordination/argon-cards/T725.json rev 1; note zm). Mac task uid 01m53h5adqk0k6ezk6hs3kmgez. Base: argon trunk dd0ab81 (= mac/main e4f7677 synced; the code these touch is unchanged through mac/main 32c938f9). Title the host commit "... (Mac T725)".

## Problem

A signature help parameter whose label is a string (`ParameterLabel::Simple`) is placed with a raw
substring search: `crates/fe-lsp/src/signature_help.rs` `signature()`, `label[from..].find(&text)`. A short
parameter name is found inside the function's name or an earlier word, so the popover underlines the
wrong text and the active parameter lands on the name:

- `max(a, b)` with parameters `"a"`, `"b"` gives `[1..2, 7..8]`: the first parameter is the `a` of
  `max`. Right is `[4..5, 7..8]`.
- `fn foo(o: Opts)` with `"o"` gives the `o` of `foo` (`4..5`); right is `7..8`.

The spec says a string label "must be a substring of its containing signature label", which doesn't say
where; clients resolve it as a word. VS Code's `parameterHintsWidget.getParameterLabelOffsets` searches
`(\W|^)<label>(?=\W|$)`. The offsets form (`LabelOffsets`) is unaffected. Verified by reading at 32c938f9; the existing test
(`signature_help_tests.rs`, "Strings are found left to right": `g(a, aa)` with `"a"`, `"a"`, `"zz"` ->
`2..3, 5..6, 8..8`) only covers labels where the raw search happens to be right.

## Change

In `signature()`, for `ParameterLabel::Simple(text)`, search left to right from `from` for the first
occurrence that is bounded: no word char (`char::is_alphanumeric` or `_`) immediately before it unless
`text` starts with a non-word char, and none immediately after it unless `text` ends with one. If no
bounded occurrence exists at or after `from`, fall back to today's raw `find` from `from`, then to the
empty range at the label's end, as today. `from` advances to the range's end as today. Keep the
`LabelOffsets` arm, `convert`, `clamp` and `byte_of` as they are.

Test in a new unit module `crates/fe-lsp/src/signature_help_label_tests.rs`, included from
`signature_help.rs` with `#[cfg(test)] #[path = "signature_help_label_tests.rs"] mod label_tests;`, so it
can call `signature()` directly with `lsp_types::SignatureInformation` values. One test,
`string_labels_are_found_as_words`: `max(a, b)` -> `[4..5, 7..8]`; `fn foo(o: Opts)` -> `[7..8]`;
`f(x: Vec<x>, y)` with `"x: Vec<x>"`, `"y"` -> the whole first parameter and `y`; a non-word label
`"..."` in `g(a, ...)` -> its range; `g(a, aa)` with `"a"`, `"a"`, `"zz"` -> `2..3, 5..6, 8..8` (the
fallback); and `é(é, b)` with `"é"`, `"b"` -> the parameter `é`, not the name. It fails at the base on
`max` and `foo`. The existing `signature_help_tests.rs` passes unchanged.
## Lead's note (argon, 2026-10-17, after finish's regression check found no test at the base)

The regression check runs `string_labels_are_found_as_words` at the base with the test files carried over, and found 0
tests. The new signature_help_label_tests.rs is declared from signature_help.rs, which is production code, so at the base
nothing compiles it. Declare it from the existing test module instead (crates/fe-lsp/src/signature_help_tests.rs, which
signature_help.rs already includes under cfg(test)), or put the test in that file, so it compiles and fails at the
base. signature_help_tests.rs is now in scope and is a test-support path. signature_help.rs then needs no new mod line.

## Implementation notes

- The spec's `é(é, b)` case can't hold: `é` the name is itself a whole word, so it is the first bounded match. The test uses `éa(é, b)` (`é` inside the name) -> `4..6, 8..9`.
