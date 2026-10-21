---
task_uid: "01m55mhzpq9nvyh63pky2800pp"
task: "T162"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test multiline_search_matches_neovim (top level, in crates/fe-vim/tests/multiline_search_oracle.rs) passes on crates/fe-vim/tests/corpus/multiline_search.toml, at least 14 cases recorded with nvim 0.12.x, among them (asserted by name) slash_across_rows, question_across_rows, n_next_across, N_back_across, count_across, e_offset_across, line_offset_across, d_slash_across, visual_search_across, wrap_across."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::search_tests::multiline_highlight_tests::a_match_across_rows_paints_each_part (new crates/fe-gpui/src/vim/multiline_highlight_tests.rs, declared in crates/fe-gpui/src/vim/search_tests.rs) passes: after /c\\nd in a real view, both rows' parts of each match paint, including a match starting above the first visible row."
    required: true
    manual: false
---

# fe-vim: / ? n and N land on matches that cross rows, with counts and offsets, and the search highlight paints each row's part, as nvim's do (Mac T898)

## Problem

The multi-row pattern card lets a pattern match across rows (`Pattern::find_across`), but `/`, `?`, `n`,
`N` and the search highlight still go row by row, so `/c\nd` can't be used. nvim 0.12.6 lands on (0,2) for
`/c\nd<CR>` on `abc` / `def`. The user approved multi-row patterns in the "Vim parity" track on 2026-10-17.

## Change

- `/`, `?`, `n`, `N` (and an operator over them, `d/c\nd<CR>`) find multi-row patterns through
  `find_across`, with counts, wrapping and offsets (`e`, `s+1`, line offsets) measured from the match's
  start and end as nvim's are (probe each; `crates/fe-vim/src/cmdline/offset.rs`); a visual-mode search
  extends over rows. `crates/fe-vim/src/cmdline.rs` and `crates/fe-vim/src/search.rs`.
- The highlight (`VimState::highlight`, painted by `match_rects` in `crates/fe-gpui/src/element.rs`) paints
  each row's part of a match that crosses rows, including a match starting above the frame's first row (look
  back as far as the deadline allows).
- `*`, `#` and `:g` are unchanged (their patterns don't span rows).

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/multiline_search.toml`, recorded into
`multiline_search.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/multiline_search_oracle.rs` modelled on `tests/line_motions_oracle.rs`; the painting in a
new `crates/fe-gpui/src/vim/multiline_highlight_tests.rs`, declared in the existing
`crates/fe-gpui/src/vim/search_tests.rs` by `#[path = "multiline_highlight_tests.rs"] mod
multiline_highlight_tests;`.

element.rs is also changed by T816 (active), T858 and T198; cmdline.rs by the incsearch card.

## Implementation notes

- Search patterns (`search::parsed`, `check_search`, the incsearch preview) are now parsed with
  `Pattern::parse_across`; a pattern with no row-spanning atom translates exactly as before, so single-row
  searches go through the existing walk unchanged. One that `spans_rows` goes through `offset::across`, built
  on a new `search::Across` walker (the old `find_across` internals, now sharing one `Window` across every
  search of a count). Probed in nvim 0.12.4: the first search of an `e` offset compares matches by their last
  character (forward, any match ending on a later row qualifies, even one starting before the cursor; back,
  those ending at or before it), later count steps compare by start from where the last match landed, as
  nvim's `first_match` does; a match ending in a line break has that line break (the row's end) as its last
  character, so `/c\n/e` lands on `c` and `/c\n/e+1` on the next row's start; going back from a row's start
  begins on the row before.
- The highlight for a pattern that spans rows comes from `search::matches_across_until`: one call per frame
  over the displayed buffer rows, looking back for matches reaching the first row in stretches doubling from
  64 rows, nearest first, as far as the pattern's reach (to the text's start for an unbounded one) or the
  deadline; overlapping matches are dropped so no cell paints twice. Every displayed row but the last is
  listed whole, as a match from it may reach a later displayed row. Rows hidden in a closed fold aren't
  searched, so a match starting inside one doesn't paint on the rows after it (as nvim's highlight starts
  after a closed fold).
- Carried findings fixed: T158 F3 (`VimState::search_preview` takes the `Registers` and matches case as submit
  does; `search_preview_pattern` gives the GPUI host the same pattern to paint, so `fe-gpui/src/vim.rs` and
  `fe-vim/tests/search_preview.rs` changed to pass the registers); T161 F1 (the clock is sampled after every
  regex call while listing a row); T161 F2 (`Window::hold` takes the deadline, checked before reading and
  between chunks); T161 F3 (`\n` as a collection range's end, or followed by `-`, is the character 0x0a, as in
  nvim's NFA engine); T161 F4 (marks after `\_.` are an atom of their own, any character carrying them); T161
  F1 at :1720 (over rows, the next search goes on from the `\zs`/`\ze`-bounded match). Regressions are in
  `search::across_tests`.
- `crates/fe-vim/src/lib.rs`'s search section gained a paragraph saying `/ ? n N` take row-spanning patterns.
- Review round 1 (7rccqh): a backward search that goes round the text's end searches only from the last row
  back to the row it started on, as nvim's second pass does, so a count that can't complete fails (E486)
  rather than coming round to the match it started from (F1; recorded corpus cases for `2?…?e`, `d2?…?e` and a
  later-row `\zs`). A line break a match takes in paints one space wide past its row's end, a blank row's
  being all there is of it, in the highlight and the preview alike (F2). `matches_across_until` lists the
  shown rows first, then looks back, keeping the matches of the regex calls made before the deadline passed
  (F3). `Across` keeps each row's listing (matches, where the next search starts, the last `\zs`/`\ze`
  bound) for all of a count's steps, and `last_char` keeps the row it read, so a count along a dense row
  makes regex calls in proportion to the matches passed (F4).
