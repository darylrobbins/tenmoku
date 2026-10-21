---
task_uid: "01m55mhzf8vygr7phg3wdkxa8x"
task: "T161"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test multiline_patterns_find_across_rows (top level, in crates/fe-vim/tests/multiline_pattern.rs) passes: Pattern::find_across gives the nvim-probed ranges (probes in the test's comments) for c\\nd, \\_s\\+, a\\_.\\{-}z over three rows, \\_[ab], \\n\\n over an empty row and x\\_$, forward and backward, wrapping and with no match."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test multiline_pattern_is_interrupted (top level, same file) passes: a slow multi-row pattern returns Interrupted within its own Deadline (not search::set_time_limit), and every existing search oracle corpus passes unchanged."
    required: true
    manual: false
---

# fe-vim: patterns with \n, \_x classes, \_. and \_[...] match across rows (Pattern::find_across), as nvim's do, instead of being refused (Mac T897)

## Problem

At Mac main 6f032b12 fe-vim's patterns match within one row (`crates/fe-vim/src/search.rs`: "translated to
a `regex::Regex` and matched one row at a time"), and the parser refuses every multi-row atom: `\n`, `\_x`
classes (`\_s`, `\_.`, `\_a`, …) and `\_[...]` (`'_' | 'z' => ... unsupported`, around search.rs:1496), so
`/c\nd` fails with E486-style "unsupported" where nvim 0.12.6 lands (`abc` / `def` from (0,0): `/c\nd<CR>`
lands on (0,2)). The user approved multi-row patterns in the "Vim parity" track on 2026-10-17. This is the
pattern half; search motions and highlighting over such matches are the next card, `:s` the one after.

## Change

In `crates/fe-vim/src/search.rs` and `crates/fe-vim/src/search/backtrack.rs`:

- Parse `\n` (a line break), `\_x` for each class `x` fe-vim supports and `\_.` (the class or a line
  break), `\_[...]` and a collection holding `\n`, and `\_^`/`\_$` (start/end of row anywhere in the pattern),
  as `:help /\_x` says.
- A pattern holding any of them is multi-row: `Pattern::spans_rows()`. For it, a new
  `Pattern::find_across(&self, text: &Text, from: usize, forward: bool, deadline: &Deadline) ->
  Result<Option<Range<usize>>, Interrupted>` matches over the text's rows as one haystack, a window of rows
  at a time (a match may run past the window: grow it, bounded by the deadline), so a match can start on one
  row and end on a later one. Single-row patterns keep today's per-row path unchanged.
- Matching is still bounded by the command deadline (`search::Deadline`), never quadratic in the rows
  scanned for a pattern that can't cross a row (the cost codex has chased on this module before: don't
  promise "no superlinear work" for regex paths, and test the deadline with a per-test `Deadline`, not
  `set_time_limit`).

Tests in a new integration file `crates/fe-vim/tests/multiline_pattern.rs`, comparing `find_across` with
nvim-probed positions (write the probes in the test's comments): `c\nd`, `\_s\+`, `a\_.\{-}z` across three
rows, `\_[ab]`, `\n\n` (an empty row), `x\_$`, backwards, wrapping, no match, an interrupted slow pattern.

search.rs and backtrack.rs are changed by the lent T799 and T807, and search.rs by the search-case card:
this card depends on that card (which depends on T807).

## Implementation notes

- The atoms are read by a new constructor, `Pattern::parse_across(pattern, ignore_case)`; `Pattern::parse`
  and `parse_with` still refuse them with the same "pattern not supported" error as before, so `/`, `?`,
  `:s`, `:g` and highlighting, which match row by row and are wired by the next cards, keep their behavior
  (and every corpus passes unchanged) instead of silently finding nothing for `/c\nd`. The next card
  switches its callers to `parse_across` and `find_across` when `spans_rows()`.
- Read as nvim 0.12.4 does: `\n`; `\_x` for every class fe-vim has and `\_.`; `\_[...]` and `\n` in a
  collection (ignored after `^`: `[^\n]` is `.`); `\_^` and `\_$`; a magic `^` after `\n` and `$` before it.
  A collection without `\n`, a negated class (`\S`) and `.` never match a line break, even where their
  ranges cover it (`[\x01-\x7f]`, `[[:space:]]`). `\_[` with no `]` is invalid (nvim's E769).
- nvim matches a line break after the last row too, as if the text ended in one, so a match that takes
  it (`c\n` on `abc`) ends one past the text's end; a match `\zs` starts after it starts there too, where
  nvim's cursor goes to the text's last character (the motion card clamps).
- Windows: each pattern's reach (the most line breaks a match can take: `c\nd` 1, `x\_$` 0, `\_s\+`
  unbounded) decides how many rows past a match's first row a window holds; a window holds 256 rows more
  so a search for the next row with a match scans many rows per regex call and skips the rows it rules
  out. Unbounded patterns hold the rest of the text. Backward searches list windows forward from the last.
- Checked beyond the probed cases against nvim 0.12.4 with a throwaway fuzz harness (not committed):
  3,000 random short buffers and 400 buffers of 250–800 rows over 25 patterns each, forward and back,
  comparing match start and end: no differences.
- Carried T158 F1: `has_uppercase` now finds the pattern's magic as nvim's `skip_regexp_ex()` does,
  stepping over collections, with nvim-probed unit and end-to-end cases both ways.
