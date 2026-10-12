---
task_uid: "01m4rkhr6gqx427v99zd9v0pse"
task: "T565"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "From (0,0) in 'x\\nb ' followed by 40 'a' characters, '/b\\|\\%(a\\|.\\)*c/e\u003cCR\u003e' moves to (1,0) as Neovim 0.12.4 does, within the search time limit; and '2/b\\|\\%(a\\|.\\)*c/s\u003cCR\u003e' on 'x\\nbb ' + 40 'a' reaches the second b. Both recorded as oracle cases (T421 review F1)."
    required: true
    manual: false
  - id: "C2"
    text: "Offset traversal and the counted-search cache enumerate a row's matches incrementally and stop once the requested qualifying match is known; one deadline covers the search; an interrupted enumeration is never treated as a completed row (an unfinished cache is distinguishable from a finished one). Unit tests in the new crates/fe-vim/src/tests/search_incremental.rs cover both paths."
    required: true
    manual: false
  - id: "C3"
    text: "Existing search, offset and counted-search tests and oracle corpora still pass unchanged."
    required: true
    manual: false
  - id: "C4"
    text: "Nothing outside the scoped files changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# Follow-up from review of T421: enumerate row matches incrementally in search offsets and counted search, so a budget-exhausting later match never discards an already-found one (F1)

## Implementation notes

- `search::RowList` is one row's matches listed lazily: `list_until(pattern, deadline, enough)`
  lists on from where it stopped (`Pattern::matches_from` at the last resume point) until `enough`
  holds of the matches listed or the row has none left. It reads the clock first, and again after a
  search that finds nothing more, so a row whose last regex call overran the deadline stays
  unfinished (`done()` is false) — the carried T421 F4. An interrupted listing keeps its matches
  but never marks the row done. It replaces `search::all_in_row`.
- The row is read once (`row_kept`: borrowed when it lies in one rope chunk, else gathered) and its
  text kept until every match is listed, so stepping match by match through a long row doesn't
  re-read it (`counted_end_searches_read_each_row_once` still holds). The cost: a long row a
  search stopped partway through stays copied until the search ends.
- Both caches use it: the offset walk (`cmdline::offset::Walk`, each step asks for the first match
  past its origin, by start or by end with an `e` offset) and the counted-search cache
  (`search::Rows`, behind `Pattern::nth`), whose `step` now returns `Err(Interrupted)` rather
  than the match it started from.
- `offset::land` makes one `Deadline` and passes it through `place`, `by_start` (`nth_by`,
  `forward`, `try_find_by`, now `pub(crate)`) and `nth_match`, so the whole search, `n`'s second
  try included, runs under one deadline.
- The unit tests in `src/tests/search_incremental.rs` are declared from `search.rs`
  (`search::incremental_tests`), keeping `src/tests/mod.rs` out of scope. Two of `search.rs`'s own
  unit tests changed only for `Rows::step`'s new `Result` and for building listed rows through
  `RowList`.
- Review round 1: `list_until` reads the clock after every regex call, so one that overran the
  deadline counts for nothing, whether it found a match or nothing more (F1); `nth_by` and
  `nth_match` also check after each step. `enough` is also given where the next unlisted match
  could start at the earliest, so searches going back stop once none left can come before their
  origin (F2): by start, unlisted starts at or past it; by end (`e`), unlisted starts past it,
  since an empty match at the origin still qualifies. `Pattern::try_find`'s backward search lists
  the caret's row the same way (the rest only if it wraps back to it), so `?pat?s` doesn't wait on
  a slow search past the caret. Going back with `e`, a row whose next search starts exactly at the
  origin still needs that search (an empty match there would qualify).
