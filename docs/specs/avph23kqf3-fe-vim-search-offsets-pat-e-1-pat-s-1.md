---
task_uid: "01m4p2z764088wv0avph23kqf3"
task: "T421"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests in src/cmdline/offset_tests.rs: e, e+N, e-N, s, s+N, b-N, +N, -N and a bare + or - parse to SearchOffset; chained (;) and malformed offsets fail with the current not-supported message; a Search with no offset serializes without an offset field"
    required: true
    manual: false
  - id: "C2"
    text: "A search with an offset moves as nvim does: e, s and b offsets by chars across rows, line offsets N rows to column 0; d with an e offset is inclusive and with a line offset linewise; n and N reuse the last offset; an empty pattern with an offset uses the last pattern Out of scope, deferred to follow-up T565 (review F1, which the user overrode): a row where a later match exhausts the backtracking budget, which can discard an earlier match in that row."
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test search_offset_matches_neovim (tests/search_offset_oracle.rs): tests/corpus/search_offset.toml has at least 35 cases (each offset kind, ? with e, a row end crossed, n and N after each, //e, d/pat/e, d/pat/+1, c/pat/s, v, dot after d/pat/e, * after an offset search); each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; src/tests/search.rs changes only its Search literals"
    required: true
    manual: false
---

# fe-vim: search offsets (/pat/e+1, /pat/s-1, /pat/+2, ?pat?b), kept by n and N, checked against nvim

# fe-vim: search offsets (/pat/e+1, /pat/s-1, /pat/+2, ?pat?b), kept by n and N

## Problem

"Search offsets (`/foo/e`) … aren't supported: a line with an offset fails, changing nothing"
(`src/lib.rs` Known limits ~561). `cmdline.rs` `search()` already splits the offset off
(`search::split_search`) and refuses it with "search offsets not supported". Gap analysis
2026-10-11.

## Change

After the seam card (which this depends on) no other breadth card changes `args.rs` or `motion.rs`,
so this card owns them:

- **`args.rs`**: `Search` gains `offset: SearchOffset` (`#[serde(default, skip_serializing_if =
  "SearchOffset::is_none")]`, so recorded args without one don't change), a `Copy` enum: `None`,
  `Lines(i32)` (`+N`/`-N`, bare `+`/`-` being 1), `End(i32)` (`e`, `e±N`), `Start(i32)` (`s`/`b`,
  `s±N`/`b±N`). `Motion::class()` for a search with `End` is inclusive and with `Lines` linewise
  (`:help search-offset`).
- **`src/cmdline/offset.rs`** (new, `mod offset;` in `cmdline.rs`): parsing the offset text; an
  offset that isn't one of those (`;` chaining, junk) still fails with the current message.
- **`cmdline.rs`**: `search()` builds the `Search` with its offset; an empty pattern with an offset
  (`//e`) uses the last pattern.
- **`motion.rs`**: the `Search` arm applies the offset after finding the match: `e±N`/`s±N` move by
  chars, crossing rows as nvim does; `±N` goes N rows down/up to column 0 (`:help search-offset`).
- **`register.rs`**: the last search keeps its offset (`Registers::last_search_offset()`), and
  `n`/`N` (`cmdline::resolve`) reuse it; `*` and `#` keep or drop it as nvim does (the corpus records it).
- `cmdline.rs`'s module docs describe offsets.

**Clean room**: `nvim --clean` (0.12.4) and `:help search-offset`, `:help //;` (for what's left
out); never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/search_offset.toml`, at least 35 cases: `/pat/e`, `/pat/e+1`, `/pat/e-1`,
`/pat/s+2`, `/pat/b-1`, `/pat/+1`, `/pat/-`, `?pat?e`, offsets crossing a row end, `n` and `N`
after each, `//e`, `d/pat/e` (inclusive), `d/pat/+1` (linewise), `c/pat/s`, in `v`, `.` after
`d/pat/e`, `/pat/e` then `*` then `n`. A failing search is a case's last key.

## Tests (new files)

- `src/cmdline/offset_tests.rs` (declared from `offset.rs`): parsing.
- `tests/search_offset_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `search_offset_matches_neovim`.
- `src/tests/search.rs` changes only where a `Search { pattern, forward }` literal needs the new
  field.

## Size

About 250 non-test lines.

## Out of scope

`;` chained searches (`/foo/;/bar`), 'incsearch', search history (the next card), offsets in ex
line searches (`:/pat/+1` is an ex address already), the GPUI check.

## Conflicts

`src/cmdline.rs` is also in T310's scope (inbox); `src/search.rs` (T351, T310) isn't touched.

## Implementation notes

Rules fitted by probing `nvim --clean` 0.12.4 (no source read), all in `src/cmdline/offset.rs`:

- **Parsing**: `e`, `s` and `b` take an optional sign and digits (`e2` is `e+2`, a bare sign is 1);
  anything else is a line offset (`+N`, `-N`, bare digits, `+0`). nvim ignores junk after an offset
  (`/foo/x` searches with none); fe-vim refuses it with the not-supported message, as C1 asks.
- **Where the search starts**: a character offset is first taken back from the caret, N characters
  the other way across rows; stepping off either end searches from before the start or after the
  end (taking a match at either end). A line offset searches from the caret.
- **`e` compares by the last character**: the first match whose last character is past that place
  (or the last before it). An empty match compares as the character before it on its row (itself
  at column 0), but the offset counts from the match itself (`/\>/e` lands on the match).
- **Counts**: each further search goes on from the last match. Forward, that is the next match a
  row's search finds, so with `e` an empty match right after a non-empty one counts (`2/a*/e` on
  ` abbaa` lands on column 2), though a search from that place passes it (`/a*/e` then `n` lands on
  column 3). Back, it goes from where the last match landed, so with `e` an empty match finds
  itself again (`2?\>?e` = `?\>?e`). Without `e`, match by match as before.
- **Stepping**: an empty row is one character; from a match at a row's end (`$`, `\>`) the first
  step is the next row's first character; steps stop at the text's ends. In a visual mode a step
  past the text's last character (and a match at a row's end) leaves the cursor past the row's
  end, as nvim's does; this also fixes plain `v/$`, which landed one column short.
- **`n` and `N` don't stay put**: when they would leave the cursor where it was without wrapping
  round the text (an offset cut short at the text's end does this), they go one match further
  (count + 1); not with a line offset (`/foo/-1` then `n` stays), and not `/<CR>`, which does
  stay. `Search` carries this as `again: bool` (serde-skipped, set only by `cmdline::resolve`),
  beyond the spec's `offset` field.
- **What keeps the offset**: `/<CR>` and `?<CR>` keep it, `//` drops it, `//e` replaces it, `*`
  and `#` drop it, `:s` and `:g` keep it. An ex address's own pattern (`:/foo/`, `://`, `:??`)
  drops it; a bare `:/` or `:?` ending the line, `:\/` and `:\&` keep it.
- **Work**: counted `e` searches and the `n`/`N` wrap check go through `offset::Walk`, which lists
  each row's matches once and finds the next by binary search; a match coming back ends the walk
  (the rest of the count goes round), so the work is bounded by the text, not the count. One
  deadline covers the whole walk: rows are listed under it (`search::all_in_row`, never a row cut
  short), and the clock is read for every row and every counted step
  (`tests/search_offset_budget.rs`).

Scope beyond the card's file list (each a line or two):

- `src/ex.rs`: an ex address's own pattern clears the offset (`Registers::set_search_offset`), and
  `LineSearch` gains `Repeat { forward }` for a bare `/` or `?` ending the line (which keeps it);
  a field on `LineSearch::Pattern` would have broken earlier tests' literals. `src/ex/global.rs`
  gets the matching arm (`:g` never makes one).
- `src/operator.rs` (`motion_span`): a yank over a search with a line offset leaves the caret at
  the range's start (`y?pat?-` on the caret's row goes to column 0), as nvim's does.
- `src/visual.rs` (`move_cursor`) and `src/motion.rs` (`Cursor::visual`): the visual cursor flag.
- `src/lib.rs`: the crate docs on searches and the Known limits line on offsets.
- `src/search.rs`: `all_in_row`, beside `first_in_row` (additive, 15 lines): a row's matches under a
  caller's deadline, which `matches_in` (its own deadline, quietly partial) can't give.

A scratch differential fuzz (about 9,000 random cases: offsets of every kind with `n`, `N`, counts,
`d y c > gU` and `v`, one nvim per case) ended with mismatches only in two pre-existing classes
that reproduce without any offset: a yank's caret after `:help exclusive` turns a range linewise
(`y?\>` ending at a later row's column 0), and linewise deletes on a buffer a delete emptied
(`dddd` on one row).
