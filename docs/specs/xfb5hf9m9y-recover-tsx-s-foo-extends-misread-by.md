---
task_uid: "01m4dwqwqmqg85v5xfb5hf9m9y"
task: "T193"
rev: 4
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "Regression (T192 F1) fe-syntax test tests::imports_recovery::a_generic_arrow_after_a_jsx_extends_attribute_keeps_its_require: in tsx, after const el = \u003cFoo extends=\"/\" /\u003e;, each of const f = \u003cT extends string\u003e(x:T)=\u003e require('real'); / const f = \u003cT extends string\u003e(x: T) =\u003e x; require('after'); / function g\u003cT extends U = V\u003e(x: T) { return require('inner'); } / class C\u003cT extends D\u003e { m() { return require('m'); } } / type X = Foo\u003cT extends string ? 1 : 2\u003e;, then import a from './a';, reports its require's module (if any) then ./a, in document order. The first case fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Regression (T192 F2) fe-syntax test tests::imports_recovery::apostrophes_outside_strings_keep_later_imports: const r = /'/;, const p = \u003cp\u003edon't\u003c/p\u003e; and const t = `don't`;, each followed by const el = \u003cFoo extends=\"/\" /\u003e; import a from './a';, report exactly ./a in tsx; each fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "Regression (T192 F2) fe-syntax test tests::imports_recovery::line_comments_end_at_every_line_terminator: // c, then CR, U+2028 or U+2029, then const el = \u003cFoo extends=\"/\" /\u003e; import a from './a';, reports exactly ./a in tsx. The U+2028 and U+2029 cases each fail at base. The CR case goes through Buffer::from_file_contents, as a loaded file does (a buffer can't hold a lone CR; loading turns it into LF), so it passes at base too and is kept as a check, not a regression (user's decision, 2026-10-08, accepting T193 review F1)"
    required: true
    manual: false
  - id: "C4"
    text: "Regression (T192 F3) fe-syntax test tests::imports_recovery::tag_names_with_combining_marks_and_joiners_keep_later_imports: \u003cFo\\u{301}o extends=\"/\" /\u003e and \u003cFo\\u{200d}o extends=\"/\" /\u003e, each followed by import a from './a';, report exactly ./a in tsx; both fail at base"
    required: true
    manual: false
  - id: "C5"
    text: "T192 F4, as two separate tests: tests::imports_recovery::a_cancel_stops_the_scan_in_a_long_gap (an affected attribute, then at least 1 MiB of // padding lines with no candidate, then another and a final import) and tests::imports_recovery::a_cancel_stops_the_scan_in_a_long_whitespace_run (\u003cA extends, then at least 1 MiB of spaces, then =\"/\" /\u003e; and a final import). Each region starts within the first 4 KiB. A callback that turns true on its 3rd poll gives Err(Cancelled) after at most 4 polls, and one never true gives exactly ./a"
    required: true
    manual: false
  - id: "C6"
    text: "fe-syntax test tests::imports_recovery::overlapping_lookaheads_are_read_in_linear_time: in tsx, const el = followed by 100,000 '\u003ca /* ' then */, 1 MiB of spaces and extends=\"/\" /\u003e; import a from './a';, and likewise 100,000 '\u003ca // ' on one line then a newline and extends=\"/\" /\u003e; import a from './a';, each report exactly ./a, and the test meets C10"
    required: true
    manual: false
  - id: "C7"
    text: "Existing tests pass: imports::tests::* (including T168 F7/F8, T181 F1 and T190 F1/F2's many_jsx_attributes_named_extends_keep_the_last_import, a_great_many_jsx_attributes_named_extends_are_read_in_linear_time, a_cancel_stops_the_recovery, a_cancel_stops_the_recovery_scan_between_occurrences, a_call_of_many_comments_and_strings_is_read_once and genuine_type_parameter_constraints_keep_their_imports), plus fe-nav's search::import_tests::*, search::tree_import_tests::* and followup_tests::*. The only changed expectations are the three forms tsc rejects in a_jsx_attribute_named_extends_in_any_spelling_keeps_later_imports (\u003cFoo extends foo=\"/\" /\u003e, \u003cFoo extends bar /\u003e, \u003cFoo extends a=\"/\" b=\"/\" /\u003e), now [\"./b\"], each with a comment and listed in the finish notes. a_great_many_jsx_attributes_named_extends_are_read_in_linear_time may change only as C10 allows"
    required: true
    manual: false
  - id: "C8"
    text: "Check: the scanner has no string, regex, template or JSX-text mode, and rg -n 'enum Token|enum Mode|history' crates/fe-syntax/src/imports.rs prints nothing. Only forms followed by =, \u003e or / are respelled. Recovery runs only for tsx with an erroring tree, with a single corrected parse whose tree alone is queried. There is no per-occurrence reparse and no occurrence or byte cap"
    required: true
    manual: false
  - id: "C9"
    text: "Check: cargo nextest -p fe-syntax -p fe-nav and clippy pass, fe-syntax builds with its language features off (the gate's syntax_features stage), and tests/imports_recovery.rs builds against base"
    required: true
    manual: false
  - id: "C10"
    text: "Load-robust (user's decision, 2026-10-08: gates kept failing at load average 100-180): imports::tests::a_great_many_jsx_attributes_named_extends_are_read_in_linear_time and tests::imports_recovery::overlapping_lookaheads_are_read_in_linear_time no longer depend on absolute wall-clock time or on the parse finishing under the production parse cap (for example, assert linearity with a test-only counter as ARGUMENT_VALIDATIONS does, and parse with no deadline). Each still fails when the recovery is made quadratic, shown in the finish notes. Both pass 30 of 30 runs under parallel load; the command, pass count and load average are in the evidence"
    required: true
    manual: false
---

# Recover TSX's <Foo extends …> misread by TypeScript's own rule, in one linear pass

# Recover TSX's `<Foo extends …>` misread by TypeScript's own rule, in one linear pass

This card replaces the review-follow-up chain T168 → T181 → T190 → T192 → T193. The user decided
(2026-10-08) to stop patching it round by round: rework the recovery as a whole, with a test for
every case the reviews raised.

## Background

tree-sitter-typescript's tsx grammar reads `<Foo extends="/" />` (a JSX attribute named `extends`) as
a type parameter list with a constraint. The `/` then begins a regex, and the rest of the file is
swallowed, so `module_specifiers` (`crates/fe-syntax/src/imports.rs`) loses every later import. Each
round fixed what the last review found, and the next review found more:

- **T168 F8** (imports.rs:133): a cap of 16 reparses lost imports after the 17th affected element.
- **T168 F7** (imports.rs:279, javascript.scm:6): argument filtering was quadratic with many leading
  comments. Fixed in T181 (`a_call_of_many_comments_and_strings_is_read_once`).
- **T181 F1** (imports.rs:140): removing the cap made recovery a reparse per occurrence, Θ(N²).
- **T190 F1** (imports.rs:232): `extends` was recognised only before ASCII whitespace and `=`.
- **T190 F2** (imports.rs:227): cancellation was polled only at occurrences.
- **T192 F1** (imports.rs:310, :349; tests.rs:230): every `<Name extends` is respelled, genuine
  constraints included, and the original tree is merged in. In
  `const el = <Foo extends="/" />; const f = <T extends string>(x:T)=> require('real'); import a from './a';`
  `real` is lost. The original tree swallowed it, and the corrected one reads the respelled arrow as
  JSX text.
- **T192 F2** (imports.rs:268, :273, :292): the scanner's string mode treats apostrophes in regexes,
  JSX text and template text as quotes, so `const r = /'/; …`, `const p = <p>don't</p>; …` and
  ``const t = `don't`; …`` skip the attribute after them. Its line comments end only at LF, so `\r`,
  U+2028 and U+2029 miss it too.
- **T192 F3** (imports.rs:296, :299): a tag name with a combining mark (`<Fo\u{301}o …>`) or a joiner
  (U+200D) is split into several names, so the attribute after it is missed.
- **T192 F4** (imports/tests.rs:250, low): the cancellation test stops about 16 KiB into its gap and
  never reaches the long whitespace run it was meant to cover.

These have one cause. The scanner tries to re-tokenize JavaScript (strings, comments, history), and
it decides "attribute or constraint" by a guess the grammar has to undo. Probed on main (tree-sitter
0.27.0, tree-sitter-typescript 0.23.2): T192 F1's in-arrow `require`, all six F2 contexts and both F3
names lose `./a` today.

## The rule: TypeScript's own

In a .tsx file, tsc decides this ambiguity with a fixed lookahead
(`isParenthesizedArrowFunctionExpressionWorker`, `LanguageVariant.JSX`). `<`, an identifier and
`extends`, followed by `=`, `>` or `/` as the next token, is JSX. Anything else is a type parameter
list. Checked with tsc 5.6.3: `<Foo extends="/" />`, `<Foo extends />`, `<Foo extends>text</Foo>`,
`<Foo extends\u{a0}="/" />` and the commented forms are JSX. `<Foo extends foo="/" />`,
`<Foo extends bar />` and `<Foo extends a="/" b="/" />` parse as a generic arrow with errors
("',' expected"). They are invalid TSX.

A genuine constraint or conditional type is always followed by a type, never by `=`, `>` or `/`. So
under this rule a genuine constraint is never respelled. That fixes T192 F1 by construction.

## Change: `crates/fe-syntax/src/imports.rs`

- **When.** Recovery runs only for `tsx` (javascript has no type parameters, and typescript has no
  JSX), and only when the parsed tree `has_error()`. Otherwise nothing changes.
- **Candidates, context-free.** Find each `<`. After it, skip trivia, then read a name, skip trivia,
  read `extends` as a whole word, skip trivia, and look at the next char. If it is `=`, `>` or `/`,
  respell that `extends` as `extendz`: same length, so no offset moves.
  - Trivia is whitespace (Unicode `White_Space`, U+FEFF), `/* … */`, and `//` up to the next LF, CR,
    U+2028 or U+2029.
  - A name char is `$`, `_`, `\`, an alphanumeric, or any non-ASCII char that isn't whitespace. This
    is deliberately a superset of `ID_Continue`, so combining marks, ZWNJ/ZWJ and escapes never split
    a name (T192 F3).
  - There are no string, regex, template or JSX-text modes. A candidate inside one of those is
    respelled too. That is harmless: replacing a letter inside a string, comment, regex, template or
    JSX text changes no token boundary, and reported ranges index the original text. This removes
    T192 F2 by construction.
- **Linear, polled.** Total work is O(n) for any input, overlapping lookaheads included. For example,
  many `<a /*` openers before a single `*/`, or many `<a //` on one line, must each be read once.
  Memoise the next `*/` and next line terminator, or the trivia end, so no byte is rescanned per `<`.
  Poll `cancelled` at least every `SCAN_POLL_BYTES` bytes, inside trivia skips and comment searches
  as well.
- **One corrected parse.** If anything was respelled, parse the corrected text once with `poll`, and
  query only that tree. The original tree's reading after a misread is wrong, and merging it in is
  how T192 F1 lost `real`. If nothing was respelled, query the original tree as now.
- **Delete** the `Token`/`Mode`/`history` machinery and the merge. Keep `is_module`, `is_recovered`,
  `after_division`, `inside_quotes`, the `lone` cache and the query files as they are.

## Decided (lead default, 2026-10-08; the user may revisit)

Forms that tsc rejects (`<Foo extends foo="/" />`, `<Foo extends bar />`,
`<Foo extends a="/" b="/" />`) are invalid code. Like T168's rule for invalid code, they keep only
what the grammar's recovery gives (`./b` in the existing test, not `./a`). Change those three cases in
`imports::tests::a_jsx_attribute_named_extends_in_any_spelling_keeps_later_imports`. Keep each case,
with a comment naming tsc's reading, and list them in the finish notes. Every other existing
expectation stays.

## Tests

- New file `crates/fe-syntax/src/tests/imports_recovery.rs`, declared by `mod imports_recovery;` in
  `crates/fe-syntax/src/tests.rs`. Both files match the overlay's test globs. It may use only items
  that exist at base: `crate::{Syntax, language_by_name, module_specifiers, ImportsError}` and
  `fe_buffer::Buffer`, with a local `specifiers(language, text)` helper like the one in
  `imports/tests.rs`. Every case is TSX and ends in a real `import a from './a';` that must be
  reported.
- Run each regression test at base before implementing. Each must fail there.

## Don't

- Don't add string, regex, template or JSX-text tracking to the scanner, or any other re-tokenizing.
- Don't reparse per occurrence, loop to a fixpoint, or add any occurrence or byte cap.
- Don't respell any form tsc reads as type parameters, and don't merge results from the original
  tree.
- Don't change fe-nav or the query files.
