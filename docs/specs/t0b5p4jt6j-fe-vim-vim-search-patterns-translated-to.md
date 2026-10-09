---
task_uid: "01m4ewtgjccemnayt0b5p4jt6j"
task: "T213"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::search_pattern::translates_or_fails: Pattern::parse accepts and matches as nvim does foo\\|bar, \\\u003cis\\\u003e, a\\{2,3}, a\\{-1,}, \\(ab\\)\\+, \\%(ab\\)c, [^a-c]x, \\d\\+, \\v(a|b)+, \\Vf.o (a literal dot), \\cFOO, x\\=y and a~b (literal), except the known limits src/lib.rs lists (\\\u003c and \\\u003e word boundaries beyond ASCII, nvim's alternation priority, composing characters under literals, ., character classes and collections); \\c folds as nvim does, including the Kelvin sign, long s and ß/ẞ; a zero-width match advances by a whole character, composing characters included, and a match ending on the last character is found; it rejects \\zs, \\_s, \\n, \\%V and \\1 with an error naming the atom; a pattern over regex's size limit is an error, not a panic"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::search_pattern::find_is_row_bounded: Pattern::find never returns a match spanning a line break; it wraps once past either end; a match at the start offset is skipped going forward; an empty-match pattern (x*) moves to the next position; on a 200,000-row buffer with no match one call visits each row at most once (a cfg(test) row counter shows at most 200,001)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::search_pattern::matches_in_is_capped: matches_in(text, rows, pattern, cap) returns every match on the given rows in order, at most cap on a row holding 100,000 matches, zero-width matches as empty ranges"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus and unit test passes with its files unchanged; regex is the only new dependency: the root Cargo.toml and Cargo.lock change only by added lines and Cargo.lock gains no [[package]]; the gate's deny stage passes with deny.toml unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no unsafe in crates/fe-vim; src/lib.rs's crate docs list the supported pattern atoms, the row-by-row limit and the known limits C1 names"
    required: true
    manual: false
---

# fe-vim: Vim search patterns translated to regex (magic, \v, \V, \c), and a bounded row-by-row matcher

# fe-vim: Vim search patterns translated to regex, and a bounded row-by-row matcher

Milestone M7 "Vim", second batch (resized), card 04a: the first third of the old card 4 (search).
This card adds only the pattern language and the matcher, as a library inside fe-vim; 04b adds the
`/` `?` command line that uses it and 04c `n N * #` and the highlight. Design:
`docs/decisions/vim-engine.md` §4 (Search: patterns translated from Vim's magic syntax to `regex`).

**Clean room** as every M7 card. Behaviour from `nvim --clean` (0.12.4) and `:help pattern`,
`:help /magic`, `:help /\v`; never from source, hjkl (including its `substitute.rs` translation),
Zed or Helix.

## What exists

Nothing in the repo searches a buffer. `regex` 1.x is already in `Cargo.lock` (through
`grep-regex`), so adding it to fe-vim adds no package.

## Change (new `src/search.rs`, `pub mod search;` in `src/lib.rs`)

- `Pattern::parse(text, …) -> Result<Pattern, PatternError>` translates Vim's default **magic**
  syntax to a `regex::Regex`: literal text; `.` `*` `[…]` (with `^` negation and ranges) `^` `$`
  (at the ends only, as Vim reads them); `\+` `\=` `\?` `\{n,m}` (and `\{n}`, `\{-}`, `\{-n,}`
  non-greedy); `\(…\)` `\%(…\)` `\|`; `\<` `\>`; the classes `\s \S \d \D \w \W \a \A \l \L \u \U
  \x \X`; `\c` and `\C`; `~` as a literal (`:help /~` needs the last substitute string, which
  doesn't exist yet). `\v` and `\V` switch to very magic and very nomagic.
- Anything else (`\n`, `\_x`, `\zs`, `\ze`, `\%V`, `\%#`, back-references, `\@`) is
  **unsupported**: an error naming the atom (e.g. "pattern not supported: \zs"), never a silently
  different match.
- A pattern that fails to compile or exceeds `regex`'s size limit is an error, not a panic.
- Matching is per row: a match never spans a line break. `Pattern::find(text, from, forward) ->
  Option<Range<usize>>` walks rows from `from`, wrapping once past either end ('wrapscan'), and
  visits each row at most once per call. A match at `from` itself is skipped going forward, as
  `/` skips the cursor's match; an empty match moves on one position, as nvim does. Case is
  matched exactly unless `\c` (nvim's default 'ignorecase' is off).
- `fe_vim::search::matches_in(text, rows, pattern, cap) -> Vec<Range<usize>>`: every match on
  `rows`, at most `cap` per call, for a host to paint. Zero-width matches are empty ranges.
- `src/lib.rs`'s crate docs list the supported atoms and the row-by-row limit.

No command, key or `VimState` change here.

## Tests (new files)

- `src/tests/search_pattern.rs`, declared in `src/tests/mod.rs`. Expected matches come from
  hand-run `nvim --clean` searches, cited in the test.

## Dependencies

`regex = "1"` in the root `[workspace.dependencies]` and `regex.workspace = true` in
`crates/fe-vim/Cargo.toml`. Root `Cargo.toml` and `Cargo.lock` are append-only shared files
(`regatta.toml` `[scopes]`): change them only by added lines; `Cargo.lock` gains no package.

## Size

About 400 non-test lines (translation ~300, row walker and `matches_in` ~100).

## Out of scope

- The command line, `/` `?` `n` `N` `*` `#`, registers, highlight state (04b, 04c).
- Search offsets, multi-line patterns, 'ignorecase'/'smartcase', `:s` (05c).
- Any change outside `crates/fe-vim` and the two `regex` dependency lines.

## Decided (lead defaults; the user may revisit)

- **A subset of Vim's regex dialect**, translated to the `regex` crate, row by row. Unsupported
  atoms are an error.
- **The matcher lives in fe-vim.** A future non-vim find bar could move the row walker to
  `fe-editor`; not now.
