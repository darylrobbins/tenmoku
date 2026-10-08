---
task_uid: "01m4bfbyhdxj6my913m2yhxxep"
task: "T168"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test imports::tests::specifiers_of_imports_exports_and_requires: over parsed javascript, typescript and tsx, module_specifiers gives, in document order, the range inside the quotes of import … from 'm', import 'm', export … from 'm', export * as n from 'm', import type / export type, import X = require('m') (typescript and tsx only) and require('m') with one string argument; it gives none for import('m'), import.meta, require(x), require(`m`), or text in comments, strings, templates, regexes or JSX text; an unsupported language is Unsupported and an unparsed syntax is NotParsed"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test imports::tests::a_cancel_stops_the_query: on a source of 8 × 1024 require calls, a cancelled callback that turns true partway stops the query with Cancelled, after polling it more than once but before every match is read"
    required: true
    manual: false
  - id: "C3"
    text: "Regressions (T152 F10, F11, F12, F14, F17), in search_import_tests.rs using only tests::imports: parenthesized_arrow_return_types_keep_later_imports (F10), templates_in_jsx_type_arguments_keep_later_imports (F11: a multiline template, an escaped multiline string and a nested template type, in tsx), generic_arrows_across_long_comments_keep_later_imports (F12: a comment longer than 4 lines), deep_ternaries_keep_their_case_scope (F14: 65 same-depth and 17 parenthesized nested ternaries, then /import a from 'util'/, report no util) and spaced_conditionals_keep_later_imports (F17: spaced and commented forms). Each reports exactly ./a (F14: only the real import after it), and each fails at base"
    required: true
    manual: false
  - id: "C4"
    text: "Corpus: tests::javascript_imports_are_their_module_paths, followup_tests::* and import_tests::* pass. Every valid JavaScript, TypeScript or TSX case keeps its expectation. Expectations change only for require(…) inside comments (no longer reported) and for invalid code whose module the grammar's recovery now reports; each changed case is kept with a comment and listed in the finish notes"
    required: true
    manual: false
  - id: "C5"
    text: "fe-nav test search::tree_import_tests::a_request_too_large_to_parse_ranks_by_its_prefix_imports: a typescript request over its parse limit that starts with import { helper } from './util' ranks util.ts's helper Imported, ahead of another file's helper; search::tree_import_tests::a_cancel_during_the_import_query_is_answered: a request with 8 × CANCEL_EVERY require calls, cancelled after the query has begun, answers Err(Cancelled)"
    required: true
    manual: false
  - id: "C6"
    text: "Check: the hand-written JavaScript lexer is gone. rg -n 'fn js_imports|struct Lex\\b|enum Token|struct Lookahead|fn line_states|fn string_end' crates/fe-nav/src/search.rs prints nothing, and search.rs is at least 1,000 lines shorter than at base. The ranking loop's per-definition cancel polling and the final definitions check stay, with T152's import_tests::a_cancel_while_matching_imports_is_answered_within_a_poll, a_cancel_while_looking_at_definitions_is_answered_within_a_poll and a_cancel_after_the_checks_before_ranking_is_answered passing. The line-based import scan for other languages is unchanged"
    required: true
    manual: false
  - id: "C7"
    text: "Check: cargo test -p fe-syntax -p fe-nav and clippy pass, and fe-syntax builds with its language features off (the gate's syntax_features stage). search_followup_tests.rs and search_import_tests.rs still build against base"
    required: true
    manual: false
---

# Find JavaScript and TypeScript imports with tree-sitter queries instead of the hand-written lexer

This card follows T152 ("Follow-ups from review of T136"). It closes T152's open review findings F10,
F11, F12, F14 and F17, and the part of T152's C1 that T152 moves to Known limits.

## Problem

fe-nav ranks a definition `Imported` when the request file imports the definition's module
(`SearchNavigation::definitions`, `crates/fe-nav/src/search.rs`). For JavaScript, TypeScript and TSX,
`js_imports` finds module paths with a hand-written tokenizer. T152 grew it to about 1,400 lines
(`Lex`, `Scope`, `Mode`, `Tokens`, `Lookahead`, `line_states`, `string_end`, …) to track regexes,
strings, templates, JSX and TS type contexts. Each of T152's three review rounds found new contexts
that it misreads. Valid code then either swallows a later real import or exposes a phantom one. The
open examples (T152 review, all `medium`) are below. Each is valid code followed by
`import a from './a'`, which must report `./a`:

- F10 (ts): `const f = (x): (boolean) => x < y && z > {} / 2; import a from './a';`
- F11 (tsx): ``const el = <Foo<`a\n>`> />; import a from './a';``, plus an escaped multiline quoted type
  argument, and a nested template literal type.
- F12 (ts): `const f = <T\n/* one\n two\n three\n four\n*/ = unknown>(x: T) => x; import a from './a';`
- F14 (js): 65 same-depth nested ternaries, and 17 parenthesized nested ternaries, inside a `case`,
  followed by `/import a from 'util'/`. This must report no `util`.
- F17 (js): `x ? (y) : void {} / 2; import a from './a';`, plus the same with a comment after `?`.

A lexer that only looks at tokens can't resolve these. They need a parser.

## Change

fe-nav already parses the request text with tree-sitter in `definitions`: `syntax`, through
`Syntax::parse_until` within the deadline. fe-syntax already ships the javascript, typescript and tsx
grammars, and runs our own queries over a `Syntax`'s tree (`outline.rs`, `queries/outline/`,
`queries/definitions/`; T64, T102, T122). So run a query for the module specifiers instead of lexing.

The grammars were probed before this card was written: tree-sitter 0.27.0, tree-sitter-javascript
0.25.0, tree-sitter-typescript 0.23.2, the query below. All five examples above report exactly `./a`
(F14: only `./b` after the regex). T152's C1 examples (regex, escaped string, multiline comment,
multiline template) report nothing. `import X = require('./x')`, `import typeof T`, `import defer * as
d` and `import type from from` still report their module. `<T,>(x: T) => x` in tsx and JSX text that
looks like an import in javascript also come out right.

### fe-syntax: `crates/fe-syntax/src/imports.rs` (new) and `queries/imports/`

- `queries/imports/javascript.scm`:
  ```scheme
  (import_statement source: (string) @module)
  (export_statement source: (string) @module)
  (call_expression
    function: (identifier) @_require
    arguments: (arguments . (string) @module .)
    (#eq? @_require "require"))
  ```
  `queries/imports/typescript.scm` adds `(import_require_clause source: (string) @module)`.
  javascript uses the first file. typescript and tsx use both, as `outline.rs` composes `parts`.
- `pub fn module_specifiers(syntax: &Syntax, cancelled: &mut dyn FnMut() -> bool) -> Result<Vec<Range<usize>>, ImportsError>`
  returns, in document order, the byte range of each `@module` string *inside its quotes*. Escapes
  stay as written. `pub enum ImportsError { Unsupported, NotParsed, Cancelled }`.
  `pub fn module_specifiers_supported(language_name: &str) -> bool`. Compile each query once
  (`OnceLock`), as `OutlineQuery` does. Re-export them from lib.rs.
- `cancelled` is polled from `QueryCursorOptions::progress_callback`, which stops the cursor, and
  once per match.
- A statement with a syntax error elsewhere in it still reports its source string. tree-sitter's
  recovery decides what counts as a statement, and the hand-written lexer's rules no longer apply.
- It must build with fe-syntax's language features off (the gate's `syntax_features` stage): with no
  grammar, `Unsupported`.

### fe-nav: `crates/fe-nav/src/search.rs`

- For `javascript | typescript | tsx`, `import_lines` takes its module paths from
  `fe_syntax::module_specifiers` over the request text's tree, `syntax`, when `definitions` parsed one.
  The cancel flag goes in as `cancelled`. `Cancelled` maps to `NavError::Cancelled`.
- When the request text has no tree (over its parse limit, or the parse didn't finish by the
  deadline), parse its first `IMPORT_PARSE_BYTES = 256 KiB`, clipped to a char boundary, with
  `parse_until`. Stop at the request's deadline or a cancel, then query that tree. If that parse
  doesn't finish either, the request has no JS/TS imports, so nothing ranks `Imported`. This is a
  decision: missing an `Imported` boost is a ranking loss, while a lexical guess is the source of
  phantom imports. `IMPORT_SCAN_BYTES` stays for the line-based languages.
- Delete the JavaScript lexer and everything only it uses: `js_imports`, `module_of`, `import_clause`,
  `names_or_namespace`, `next_token`, `Token`, `Lex`, `Scope`, `Mode`, `Tokens`, `Lookahead`,
  `line_states`, `string_end`, `inline_trivia_end`, `ts_declaration_follows`, `REGEX_AFTER`, the
  JS-only constants, and the `Work` sites that only the lexer reports. Keep the ranking loop's
  per-definition cancellation polling and the final definitions check from T152 C3/C4. Leave the
  line-based `import_statement` path for Rust, Go, Python, Java, C and the rest unchanged.
- `require('m')` inside a comment no longer counts. T136 counted it "as a plain search for `require(`
  finds it", and T152 kept that. This is a decision: commented-out code imports nothing.

### Tests

- fe-syntax tests go in `crates/fe-syntax/src/imports/tests.rs`, declared from imports.rs by
  `#[cfg(test)] mod tests;`.
- **Regression tests** for F10–F17 go in T152's `crates/fe-nav/src/search_import_tests.rs`, which
  search.rs already declares at base, so Regatta's base overlay compiles them. They may use only
  items that exist at base: `super::tests::imports(language, source)`, which parses the source itself
  once the change lands. The file may lose tests: the lexer-loop cancellation tests go with their
  loops. It may not gain uses of new items.
- Tests that need new items go in a new `crates/fe-nav/src/search_tree_import_tests.rs`, declared from
  search.rs by `#[cfg(test)] #[path = "search_tree_import_tests.rs"] mod tree_import_tests;`.
- `search_followup_tests.rs` (T136's corpus) still builds against base. Edit it only to change the
  expectations allowed below.
- The existing corpus (`tests::javascript_imports_are_their_module_paths`,
  `followup_tests::*`, `import_tests::*`) is the bar. Every case that is valid JavaScript, TypeScript
  or TSX keeps its expectation. Expectations may change only for (a) `require(…)` inside comments, and
  (b) invalid code (malformed named lists such as `import { a ; } from './util'`, and characters that
  aren't identifier chars such as `import { a💥b } from './util'`), where the grammar's recovery now
  reports the module. Keep each changed case, with its new expectation and a short comment, and list
  them in the finish notes.

## Conflicts

- This card depends on T152, which is mergeable. Merge T152 with its C1 narrowed first; the narrowed
  C1 text is with the lead.
- Ready T144 holds `crates/fe-nav/src/search.rs`. Run them one after the other.
- Ready T108 holds fe-nav's Cargo.toml and benches only, so they don't overlap.
- Nothing running touches fe-syntax or fe-nav.

## Implementation notes

Corpus expectations changed (C4), each kept with a comment:
- (a) `require(…)` in comments no longer reported: `statements_start_where_javascript_starts_them`.
- (b) invalid code the grammar recovers: `import { a ; } from`, `a / b`, `a💥b`, bad `\u` escapes (`named_lists_take_whole_identifiers`); `f() import x from './x'` (followup); unterminated comment before `export * from './d'`; JSX type arguments `<Foo<string> …>` in JavaScript (now tsx-only).
- Recovery from the grammar's misreads, all in imports.rs (valid code keeps its expectation): `require(/* c */ 'm')` is found (comments around the one argument are fine, a second argument rejects it; counting stops at two). `export {…}\nfrom './w'` and `export\n{ w } from './w'` are recovered from a top-level string statement after `from`, with no `;` on the export, the `from` or the block, comments allowed. The phantom `util` after `export default class {}` / `function () {}` plus a regex is dropped. `<Foo extends="/" />` in tsx, which tree-sitter-typescript reads as a type parameter followed by a regex that swallows the rest of the file, is recovered by parsing the text again after the regex's `/` (at most 16 times per file).
- Brackets nested past 64 no longer stop the scan, so `imports_before_deep_nesting_are_kept` expects ./a and ./b.
- `fallback_threads_are_bounded` is timing-flaky under load (unrelated; passes alone).
