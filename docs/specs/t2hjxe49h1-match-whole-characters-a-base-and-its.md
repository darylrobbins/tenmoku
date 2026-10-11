---
task_uid: "01m4k6yhgdbcsag7t2hjxe49h1"
task: "T324"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test composing_substitute_takes_whole_characters (tests/composing_oracle.rs): through HeadlessVim, on \"a\\u{301}b\": :s/./X/g gives \"XX\"; :s/\\(.\\)/[\\1]/g gives \"[a\\u{301}][b]\"; :s/./\u003c\u0026\u003e/g gives \"\u003ca\\u{301}\u003e\u003cb\u003e\"; :s/a/X/ leaves the buffer unchanged and reports E486. On \"a\\u{301}b a\" with the cursor at (0,0), /a\u003cCR\u003e lands on byte 5."
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test composing_matches_never_split_a_character: for each of . \\a \\w \\S [a-z] [^ ] x* .* a\\|a. .\\{2} \\(.\\)\\(.\\) on \"e\\u{301}\\u{323}a\\u{301} \\u{301}b\\nx\\u{301}\": every range from fe_vim::search::matches_in, and every :s group, starts and ends on a boundary between a non-mark and the marks after it. A FindQuery regex \".\" still matches U+0301 alone (find bar unchanged)."
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test composing_matches_neovim (tests/composing_oracle.rs): tests/corpus/composing.toml has at least 16 cases on decomposed text, recorded from nvim 0.12.4 --clean by record_corpus_with_neovim, all matching. They include cases named dot_global, literal_base_not_found (E486, last key), capture_whole_character, whole_match_ampersand, class_alpha, collection_range, collection_of_marks, stacked_marks_dot, literal_with_mark, leading_mark, search_slash_skips_composed, search_question_backward, search_n_whole_characters, dot_star, empty_match_global and ignore_case_composed."
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus and unit test passes with its files unchanged, including src/tests/search_pattern.rs, src/tests/find_query.rs and tests/corpus/substitute.toml. Cargo.toml and Cargo.lock are unchanged. scripts/check-no-gpui.sh prints ok: fe-vim. There is no unsafe in crates/fe-vim. src/lib.rs no longer lists composing characters under literals, ., classes and collections as a difference from Vim, and it names what stays out (emoji ZWJ sequences, flags, Hangul jamo)."
    required: true
    manual: false
---

# Match whole characters (a base and its composing characters) in fe-vim's / ? search and :s, as nvim does

# fe-vim: / ? and :s match whole characters (a base and its composing characters), as nvim does

Follow-up from T218's review dywjpd (codex, F3 medium, `crates/fe-vim/src/search.rs:610`, `:614`).
It fixes a known limit that T213 (card 04a) recorded in `src/lib.rs`. Read 04a (T213) and 05c (T218)
first.

**Clean room**, as on every M7 card. Behaviour comes from `nvim --clean` (0.12.4) and `:help
patterns-composing`, `:help /.`, `:help /[]` and `:help /\a`. Never read source, hjkl, Zed or Helix.

## Problem

In fe-vim, a Vim pattern matches Unicode scalars, while nvim matches characters: a base character
plus the composing characters after it. The review gives the evidence:

> Both substitution matcher paths expose Unicode scalar ranges without Neovim's composing-character
> semantics. On "áb" (a plus U+0301 plus b), :s/./X/g produces "XXX" rather than "XX"; whole-match and
> captured replacements split the accented character into separate matches. A literal :s/a/X/ also
> edits its base character, whereas Neovim leaves the buffer unchanged with E486.

`src/lib.rs` ("Where the translation differs from Vim") lists this as a known limit. There, `e`
matches in e+U+0301, `.` and `\s` take the base alone, `[̀-ͯ]` matches the mark, and the
next search resumes inside the character. `/`, `?`, `n`, `N`, `*`, `#`, the search highlight and
`:s` all share `Pattern` and `RowMatches`, so every one of them is affected.

## Change (`crates/fe-vim/src/search.rs`; the known-limits doc in `crates/fe-vim/src/lib.rs`)

Here a composing character is a Unicode mark (`\p{M}`) that follows a non-mark. A character is a
non-mark and the marks after it. This change applies to Vim patterns only (`Pattern::parse`,
`Pattern::parse_with`). A find bar's `Pattern::from_query` keeps the `regex` crate's scalar semantics
unchanged.

- **Atoms take whole characters.** The translator (`Translator::branch` and the token translations
  it calls) makes `.`, the classes (`\s \S \d \w \a \l \u \x` and their negations), collections
  (`[…]`, `[^…]`, `[:name:]`) and case-folded literals each match one non-mark and then all the marks
  after it. Where a multi applies (`.*`, `\{n,m}`), it counts whole characters.
- **A literal matches only the character it spells.** A literal base written without composing
  characters doesn't match that base when marks follow it: `a` finds nothing in "a\u{301}b", so
  `:s/a/X/` fails with E486. A base followed in the pattern by composing characters matches that
  base with those marks. The corpus records whether nvim also accepts extra marks, and fe-vim does
  what it records.
- **A pattern that starts with a composing character** matches any whole character that carries
  that composing character (`:help patterns-composing`).
- **No match starts or ends inside a character.** The `regex` crate has no lookaround. Enforce the
  end boundary inside the regex, not by filtering matches afterwards. Wrap the translated pattern
  as group 1 and follow it with "a non-mark or the end of the row" (`(P)(?:\P{M}|$)`). The match is
  group 1, and `RowMatches::group(n)` reads group `n + 1`. Leftmost-first priority then picks the
  first alternative that ends on a boundary, where a filter would drop it. Every search starts on a
  character boundary, and the translated atoms start on a non-mark, so no match can start inside a
  character.
- `:s`'s whole match (`&`, `\0`) and its groups (`\1`–`\9`) therefore cover whole characters.
  `RowMatches`' resume points (after a match, after an empty one, after a word-boundary reject) stay
  on character boundaries, and each still resumes forward, never from the row's start.
- Remove the composing-characters bullet from `src/lib.rs`'s "Where the translation differs from
  Vim". Add one line there for what stays out: emoji ZWJ sequences, regional-indicator flags and
  Hangul jamo are still matched by scalar, apart from the empty-match step that `unit` already takes.

## Tests (one new file)

`crates/fe-vim/tests/composing_oracle.rs`, an integration test that declares
`#[allow(dead_code)] mod support;`. It uses only the public API that exists at the task base
(`fe_vim::search::{Pattern, matches_in}`, `fe_vim::HeadlessVim`), so the regression tests compile there:

- `composing_substitute_takes_whole_characters` (regression)
- `composing_matches_never_split_a_character` (regression)
- `composing_matches_neovim`, the oracle over the new `tests/corpus/composing.toml` and its
  `composing.expected.json`, written by the ignored recorder `record_corpus_with_neovim` in
  `tests/oracle.rs`

Corpus conventions are T218's. A failing command (E486) must be the case's last key, because nvim
flushes typeahead after an error. The last substitute leaks between cases, so a case that uses `~`
or `&` makes its own substitute first. A composing character in a pattern is typed as the raw
character in `keys`, or as `́` inside a collection.

## Size

About 120 non-test lines: atom suffixes and the literal rule are about 70, and the boundary group
with its renumbering about 50.

## Out of scope

- `\Z`, `\%C` and `\%[…]`, which remain unsupported errors.
- Emoji ZWJ sequences, flags and Hangul jamo under atoms.
- The cost of adversarial patterns (T310).
- The find bar's `FindQuery` semantics.
- Any change outside `crates/fe-vim`.
