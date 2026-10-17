---
task_uid: "01m557xc7xn2brwt9w5pny1ce7"
task: "T141"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-project::finder_quoted_query quoted_words_match_contiguously_as_vs_code_does (tests/finder_quoted_query.rs, top level) passes: over src/main.rs, src/m_a_i_n.rs, docs/Domain.md, main/m_a_i_n.txt and src/lib.rs, \"main\" and \"MAIN\" in quotes find docs/Domain.md, main/m_a_i_n.txt and src/main.rs (not src/m_a_i_n.rs), \"src/main\" finds src/main.rs, \"main\" \"rs\" finds src/main.rs, a quote that wraps no whole word (\"main, \"main rs\") is dropped, \"\" and a lone \" list paths as an empty query does, and the highlights of \"main\" are the unbroken run ([7,8,9,10] in docs/Domain.md, [0,1,2,3] in main/m_a_i_n.txt, [4,5,6,7] in src/main.rs); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unquoted words match and rank as before: fe-project::finder_query_words space_separated_words_each_match_the_path, fe-project::finder_query_vscode_chars backslashes_wildcards_and_ellipses_read_as_vs_code_reads_them and the --lib finder unit tests pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-project passes with no existing test edited; cargo clippy -p fe-project --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Quick open matches a query word in double quotes only where its text appears unbroken, as VS Code's quick open does, instead of finding nothing (Mac T789)

## Problem

Quick open (`FinderIndex::query`, `crates/fe-project/src/finder.rs`) matches every word fuzzily, and a
`"` in a word is matched as a character, which no path holds. VS Code's quick open reads a query piece
that starts and ends with `"` as an exact match (`queryExpectsExactMatch` and
`expectContiguousMatch` in src/vs/base/common/fuzzyScorer.ts; shipped in VS Code's workbench bundle as
`s.startsWith('"')&&s.endsWith('"')`): the piece, with its quotes dropped by `normalizeQuery`
(`replace(/[\*…\s"]/g, '')`), must appear unbroken (`allowNonContiguousMatches` false), ignoring
case, in the file name or else the path. A `"` that doesn't wrap a whole piece is just dropped, so
`"main rs"` reads as `main rs`.

Reproduced at Mac main 3e3eaa03 plus neon T138 (Mac T780, `normalize_word`): over `src/main.rs`,
`src/m_a_i_n.rs`, `docs/Domain.md`, `main/m_a_i_n.txt` and `src/lib.rs`, `query("\"main\"", 10)` returns
nothing; VS Code finds `src/main.rs`, `docs/Domain.md` and `main/m_a_i_n.txt` (its path holds `main`)
but not `src/m_a_i_n.rs`. `"MAIN"`, `"src/main"` and `"main" "rs"` find nothing either.

## Change

Build on T780's `normalize_word`: it also drops `"`. `query` notes, per whitespace-split word, whether
the raw word was at least two chars and started and ended with `"` (exact), before normalizing. An exact
word keeps the fuzzy candidates of `word_candidates` only where its text appears unbroken, char by char
with `case_matches`, in the haystack of the candidate's tier: the file name for the name tier, the path
for the path tier. Filter the name tier first and build the `named` set from what survives, so an item
whose file name only fuzzily matches but whose path holds the word unbroken (`main/m_a_i_n.txt`) still
matches in the path tier. For an exact word, `indices` returns the first unbroken run in that haystack
(as char indices into the path) instead of frizbee's alignment. Ranking, the joint-word scoring and
non-exact words are unchanged; `""` and a lone `"` are no word, as an empty query. Test in a new
`crates/fe-project/tests/finder_quoted_query.rs`. Prototyped on 3e3eaa03 plus T138's diff: the new test
and all of fe-project's finder tests passed.
