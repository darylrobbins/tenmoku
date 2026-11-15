---
task_uid: "01m7c01er5qsg6bej2a08763yr"
task: "T634"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features regex,javascript,typescript --test regex_highlights passes: regex_grammar_is_hidden, regex_literal_in_js_is_highlighted_by_the_regex_grammar, regex_literal_in_ts_and_tsx_too"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features regex,javascript injection passes with the expectation at injection/tests.rs ~565 updated to the regex grammar's spans (the string.special ab+c pair is gone)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax highlights_match_golden_files passes with the javascript, typescript and tsx goldens regenerated and containing a regex literal"
    required: true
    manual: false
  - id: "C4"
    text: "cargo deny check licenses passes with tree-sitter-regex in the lock; THIRD_PARTY.md has its row; Cargo.lock differs only by that crate"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-config passes; if a highlight bucket was added, every file in crates/fe-config/themes/ colours it (Ember and Monokai included) and fe-workbench theme_picker_tests pass; otherwise no theme file changed"
    required: true
    manual: false
---

# Built-in regex grammar (tree-sitter-regex, hidden): highlight JS/TS regex literals (Mac T1823)

# Built-in regex grammar: highlight regular expressions in JS/TS regex literals

There is no regex grammar, so JavaScript, TypeScript and TSX's regex-literal injection (from
`tree_sitter_javascript::INJECTIONS_QUERY`) is dead: `/ab+c/g` is flat `string.special` (pinned at
`crates/fe-syntax/src/injection/tests.rs` ~565). Decision 2026-11-13: add `tree-sitter-regex` as a built-in,
injection-only grammar, colour its nodes, and let the existing JS/TS injection light up. Later cards (G1-G4) add
injections per language.

1. Dependency. `tree-sitter-regex` 0.25.0 (crates.io; MIT; deps `tree-sitter-language ^0.1` only, `tree-sitter` as a
   dev-dep, so it is compatible with the workspace's patched `tree-sitter` 0.27 the way `tree-sitter-printf` is):
   add it to `[workspace.dependencies]` in `Cargo.toml` (pin the exact version the sibling grammars use as style)
   and to `crates/fe-syntax/Cargo.toml` as optional, feature `regex = ["dep:tree-sitter-regex"]`. The `javascript`,
   `typescript` and `python` features (and later users) enable it the way `python` enables `printf`: add
   `"regex"` to those feature lists (and to `go`, `rust`, `java`, `csharp`, `php` once G cards need it; here JS/TS
   only, plus `python`/`go`/`rust` is left to G1 so this card does not touch their golden). MIT is already in
   `deny.toml`'s allow list, so no deny.toml change unless `cargo deny check` says otherwise. Add the crate to
   THIRD_PARTY.md's grammar table (row style: `| regex | tree-sitter-regex | 0.25.0 | <repo URL from the crate's
   Cargo.toml> | MIT | `HIGHLIGHTS_QUERY` |`; read the crate for the real constant name; note it is injection-only)
   and, if the repo's licence-text convention for bundled grammar crates requires a copy, follow what the
   `printf` row does. `cargo update -p tree-sitter-regex` only if the lock needs it: Cargo.lock must change by
   exactly that crate (+ nothing else).
2. Hidden LanguageDef. In `crates/fe-syntax/src/language.rs` add a `Def` named `regex`, `#[cfg(feature = "regex")]`,
   `hidden: true` (see the `printf` defs at ~line 641 and the `hidden` field doc at ~473), `extensions: &[]`,
   `injections: None`, highlights `&[tree_sitter_regex::HIGHLIGHTS_QUERY]` plus a fastedit override
   `queries/regex.scm` only if the crate's query lacks something the buckets below need. It must not appear in the
   language list, picker or `language_for_path`: the existing hidden-language tests
   (`rg 'hidden' crates/fe-syntax/tests crates/fe-syntax/src/tests*` and `language_names`) must still pass; add
   `regex` to whichever fixture list pins the hidden set if one does.
3. Highlight buckets. The vocabulary is `NAMES` in `crates/fe-syntax/src/highlight.rs` (~15-60) plus `ALIASES`.
   Map the regex captures onto existing buckets where an existing one fits (escapes -> `escape`, operators/quantifiers
   -> `operator`, groups -> `punctuation.bracket`, character class contents -> `string.special`,
   anchors/assertions -> `keyword`, named group names -> `label`, backreferences -> `variable.builtin`); add
   at most one new bucket (`string.regexp`), and only if existing ones cannot tell a regex's literal characters from
   its operators; if you add one, put it in `NAMES` in sorted position and give it a colour in EVERY file under
   `crates/fe-config/themes/*.toml` (Ember and Monokai included: a new highlight key needs both, and
   `crates/fe-config/tests/format_specifier_theme_key.rs`/`monokai_theme.rs` style tests pin it), choosing
   colours already used by `string.special`/`escape` in that theme unless the theme has a distinct convention.
   Check `design_role_tokens` (`crates/fe-config/tests/yohen_hakuji_values.rs`) and
   `crates/fe-workbench/src/theme_picker_tests.rs` still pass. Prefer no new bucket: if all captures map to
   existing ones, no theme file changes at all, and say so in the finish notes.
4. The JS/TS injection. Update the expectation at `crates/fe-syntax/src/injection/tests.rs` ~565 (the comment
   "Regex and JSDoc have no grammar here" becomes JSDoc only): `ab+c` in `/ab+c/g` is now highlighted by the regex
   grammar (`b` literal, `+` operator, flags `g` as the grammar/injection says). The `typescript` and `tsx`
   injections are the same string and inherit it.
5. Goldens. Regenerate `tests/golden/javascript.js.golden`, `typescript.ts.golden`, `tsx.tsx.golden` (and markdown
   if a fence has a regex) with `UPDATE_GOLDEN=1`; review that only regex literal spans change. If a sample has no
   regex literal, add one to the three samples (one line each) so the goldens pin it.

**Tests.** New file `crates/fe-syntax/tests/regex_highlights.rs`, `#![cfg(all(feature = "regex", feature =
"javascript"))]` (cargo discovers it): `regex_grammar_is_hidden` (no `language_names()` entry and
`language_by_name("regex")` still resolves for injections, mirroring how the printf test checks it),
`regex_literal_in_js_is_highlighted_by_the_regex_grammar` (`const r = /^(?<y>\d{4})-[a-z]+\b/gi;`: `\d` escape,
`{4}` and `+` operators, `(?<y>` group with the name as `label`, `[a-z]` class, `^`/`\b` keyword-or-operator per your
mapping, each pinned by exact `(text, highlight)` pairs), `regex_literal_in_ts_and_tsx_too`. Also the updated
`injection::tests` case (give its exact id when you finish; it is the one at ~565). Run with
`cargo nextest run -p fe-syntax --features regex,javascript,typescript --test regex_highlights`.

Conflicts: `language.rs` (A, D1 depend-ordered before this), `highlight.rs`, `Cargo.toml`, JS/TS/TSX goldens (E before
this). Themes only if a bucket is added (nothing else open lists them besides T1581, blocked). Linux-OK: theme
files are text and the tests run headless; if a bucket is added, the fe-workbench theme_picker tests need GPUI
and run on the Mac lane only, so in that case relabel at start (`rgt` lead call) and say so; with no new bucket
it stays linux-ok.

## Implementation notes

- No new highlight bucket: every regex capture maps onto an existing one, so no theme file changed. Literal
  characters (`pattern_character`, as class members) stay `string.special`, the host's colour for the literal,
  apart from `operator`. The crate's `@string` for them failed the gate: `spell_ranges_tests::
  what_is_in_a_string_but_no_words_is_excluded` spell-checks `string` spans, and `/regex/` must stay excluded.
- `queries/regex.scm` (ours, added to scope by respec) goes after the crate's `HIGHLIGHTS_QUERY`: assertions
  (`^ $ \b \B`) `keyword`; literal characters and class members `string.special` (range `-` and negating `^` stay the crate's
  `operator`); group names `label`; backreferences (`\1`, `\k<n>`, `(?P=n)`) `variable.builtin`, whole;
  `{n,m}` quantifiers, `.` and lazy `?` `operator`; escapes (`\d`, `\w`, …) stay the crate's `escape`.
- JS/TS regex flags are outside the injected `regex_pattern`, so they stay the host's `string.special`.
- The injection case is `injection::tests::regex_literals_take_the_regex_grammar` (split out of
  `unknown_injected_languages_stay_plain`, which keeps the JSDoc and styled-template checks).
- Goldens: `javascript.js` already had `/Mac|iPhone/` (only its spans changed); `typescript.ts` and `tsx.tsx` gained
  one regex line each at the end. `regex` is enabled by `javascript` and `typescript` only.
