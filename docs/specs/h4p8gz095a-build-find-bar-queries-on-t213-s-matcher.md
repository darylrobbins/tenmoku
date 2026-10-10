---
task_uid: "01m4fhv7y2e80sedh4p8gz095a"
task: "T263"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::find_query::literal_regex_and_case: Pattern::from_query matches a.b literally (not axb) with regex off and as a regex with it on, ignores case unless match_case, gives PatternError::Invalid(\"empty pattern\") for empty text and PatternError::Regex for ( with regex on, and a\\nb never matches across rows"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::find_query::whole_word_like_project_search: with whole_word, foo matches in foo.bar, (foo) and the third foo of 'foofoo foo', and not in foobar, _foo or éfoo"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::find_query::find_from_includes_from: Pattern::find_from returns a match starting at from, else the next one, wrapping once, and searches each row at most once (ROWS_SEARCHED)"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier fe-vim corpus and unit test passes with its files unchanged, tests::search_pattern included; Pattern::parse, Pattern::find and matches_in keep their signatures; Cargo.lock gains no package; nothing outside crates/fe-vim, Cargo.lock and the lane's own spec file docs/specs/h4p8gz095a-build-find-bar-queries-on-t213-s-matcher.md changes"
    required: true
    manual: false
---

# Build find-bar queries on T213's matcher: literal or regex, match case, whole word, and a find that includes its start

## Problem

The user asked (2026-10-09, recorded by the lead): "In-file find (cmd-F) reusing T213's row matcher: card after
T213 lands." T213 (commit 3b03f81) added `fe_vim::search::Pattern` with a bounded row-by-row walker
(`Pattern::find`, `matches_in`), but a `Pattern` can only be built from Vim's pattern syntax (`Pattern::parse`).
A find bar needs what the project search panel already offers (`fe_project::search::SearchQuery`, T65/T77): a
literal or a `regex`-crate pattern, Match Case and Match Whole Word. Its type-ahead also needs "the first match
at or after here", while `Pattern::find` going forward always skips a match at `from` (Vim's `/`).

This is card 1 of 4 (query, editor state, find bar, keys) plus a human check.

## Change (`crates/fe-vim/src/search.rs` only, plus its test file)

The matcher stays in fe-vim (T213's "Decided" note): moving the walker to fe-editor would rewrite the
`search.rs` that T214, T215, T216 and T218 build on and would need fe-editor's `lib.rs` and `tests/mod.rs`, held
by T145 and T147. This card is additive: `Pattern::parse`, `Pattern::find` and `matches_in` keep their
signatures and behaviour.

- `pub struct FindQuery { pub text: String, pub regex: bool, pub match_case: bool, pub whole_word: bool }`
  (`Clone, Debug, Default, PartialEq, Eq`), the same four fields and meanings as
  `fe_project::search::SearchQuery`.
- `Pattern::from_query(&FindQuery) -> Result<Pattern, PatternError>`:
  - empty text is `PatternError::Invalid("empty pattern")`, as `parse`;
  - `regex: false` matches the text literally (`regex::escape`); `regex: true` takes the `regex` crate's syntax
    as written (the project search panel's syntax), never Vim's;
  - case-insensitive unless `match_case`;
  - a regex that doesn't compile is a new `PatternError::Regex(String)` ("invalid regex: …", the first line of
    `regex`'s message); past the size limit is `TooLarge`, as `parse`.
  - Matching stays row-bounded: `\n` in a regex never matches (documented on `from_query`).
- Whole word: a `Pattern` built with `whole_word` keeps a match only when the character before its start and
  the character after its end are not word characters (Unicode `\w`, `regex_syntax::is_word_character`, the
  same test as `fe_project::search::is_word_char`). When a candidate is refused, the row's search resumes one
  character after the candidate's start, so `foo` finds the third `foo` in `foofoo foo`. Patterns from
  `parse` never set it. Add `regex-syntax = "0.8"` to `crates/fe-vim/Cargo.toml` as fe-project has it
  (Cargo.lock gains no package; it is append-only shared).
- `Pattern::find_from(&self, text, from) -> Option<Range<usize>>`: going forward, the first match starting at
  or after `from`, then row by row, wrapping once, each row searched once: `find`'s forward walk with the match
  at `from` included. Share the walker with `find`; don't copy it.

## Tests (new file)

`crates/fe-vim/src/tests/find_query.rs`, declared in `crates/fe-vim/src/tests/mod.rs`.

## Size

About 100 non-test lines.

## Out of scope

Any UI, `EditorView` or keymap change (the next cards); multi-line matches; Vim syntax in the find bar.
