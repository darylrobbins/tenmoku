---
task_uid: "01m4ny3q6c78131ws8wppmp378"
task: "T43"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test substitute_stops_at_the_limit (tests/regex_budget.rs): with set_time_limit(100 ms), :s/a.*z\\|a/X/g on a row of 200,000 a returns within 5 s, leaves the text unchanged with no undo step, and the message is Interrupted"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test searches_stop_at_the_limit (tests/regex_budget.rs): with set_time_limit(100 ms) on the same row, ?a.*z\\|a from the row end, /a.*z\\|a from column 150,000 and N after it each return within 5 s with the cursor unchanged and the message Interrupted"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test linear_work_is_not_interrupted (tests/regex_budget.rs): DEFAULT_TIME_LIMIT is 2 s; with it set, :s/a/X/g on a row of 20,000 a makes 20,000 substitutions, and /a, ?a and 3n move as before"
    required: true
    manual: false
  - id: "C4"
    text: "The limit is wall-clock time measured from the start of the operation and checked before every regex call, with no call exempt (review F1). Every earlier fe-vim test, including the oracle corpora, passes with its files unchanged, except that src/tests/substitute.rs flags_specials_and_bounds may set a higher time limit for its 200,000-row :%s (that one change only; its assertions stay the same). scripts/check-no-gpui.sh prints ok: fe-vim; cargo check --workspace passes with fe-gpui unchanged (Pattern::find, find_from, nth and matches_in keep their signatures)"
    required: true
    manual: false
  - id: "C5"
    text: "Of the worker's own changes, only crates/fe-vim/src/search.rs, substitute.rs, cmdline.rs, tests/regex_budget.rs, src/tests/substitute.rs (as C4 allows) and its decision record change; Regatta's spec file in docs/specs/ is not the worker's change. Cargo.toml and Cargo.lock are unchanged"
    required: true
    manual: false
---

# Stop :s and searches that pass a time limit on adversarial regex patterns, reporting Interrupted (Mac T310)

# fe-vim: stop `:s` and searches that pass a time limit on adversarial patterns

## Problem

Follow-up from T218's review, F1 (disputed there). fe-vim matches each row through
`RowMatches::next` (`crates/fe-vim/src/search.rs`). Each call to `captures_read_at` resumes where the
last match ended. On an adversarial pattern every call scans to the row's end, which is the regex
crate's documented leftmost-first worst case. So listing every match on a row costs O(n²). For
example, `:s/a.*z\|a/X/g` on a row of 40k `a`s. nvim 0.12.4 is quadratic too (40k `a`s took 194 s),
but there ctrl-c interrupts it. fe-vim's own work is linear: each row is read once, with one edit
per row.

The paths that list every match on a row, and so hang the editor's thread:

- `:s` with `g` (`substitute.rs` `find`)
- `?` and `N` backward (`Pattern::find`: `matches(hay).last()` on every row)
- `/` forward on the cursor's row, up to the cursor's column
- counts (`Pattern::nth` lists each row's matches)

`:g` and ex `/pat/` addresses take only the first match per row, so they stay linear.

**Decided** (user, 2026-10-11):

- a 2 s wall-clock limit, rather than waiting for ctrl-c;
- an interrupted `:s` changes nothing in the buffer;
- the limit becomes a user setting;
- search highlighting gets the same limit.

This card does the first two. The setting and the highlighting are two follow-up cards that depend on it,
filed alongside this revision: a `vim.regex_time_limit` setting applied through `set_time_limit`, and one
time limit for each painted frame of search highlighting.

## Change: a wall-clock limit per command, checked between regex calls

All in `crates/fe-vim/src/search.rs`, unless named otherwise.

- **The limit.** Add `pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(2)` (nvim's
  `'redrawtime'` default), and `pub fn set_time_limit(Duration)` / `pub fn time_limit() -> Duration`,
  stored process-wide in an atomic. Hosts and tests may change it; nothing else does. The setting card will
  call `set_time_limit` at startup and on every settings reload, between commands, so each operation reads
  `time_limit()` when it makes its deadline, and a change applies from the next command.
- **The check.** A `Deadline` (start + `time_limit()`, plus whether it ran out) is made once per
  operation and threaded into `RowMatches`. Before each `captures_read_at` call, `RowMatches::next`
  checks it, and once it's past, the iteration ends and the deadline records that. One call scans at
  most one row, so the overrun is bounded by one row's scan. `Rows` (for `nth`) and every row loop
  use the same deadline.
- **Public API.** The existing signatures don't change, so fe-gpui builds untouched:
  - `Pattern::find`, `find_from`, `nth` and `matches_in` each make their own deadline. Once it runs
    out, they return what they have: `None`, or the matches so far.
  - Add `pub fn try_find(&self, text, from, forward) -> Result<Option<Range<usize>>, Interrupted>`,
    where `Interrupted` is a new public unit error in `search.rs`. `find` is
    `try_find(..).ok().flatten()`.
- **`/`, `?`, `n`, `N`, `*`, `#`** (`crates/fe-vim/src/cmdline.rs` `check_search`) use `try_find`.
  On `Interrupted`:
  - the message is `Interrupted` (nvim's word for an interrupted command), not E486;
  - the search still becomes the last search, as now;
  - the cursor doesn't move.

  `motion.rs` is unchanged: if its `nth` runs out, the caret stays.
- **`:s`** (`crates/fe-vim/src/substitute.rs`): `find` takes one deadline for the whole command and
  returns `Result<Found, Interrupted>`. On `Interrupted`, `run` changes nothing in the buffer: no
  transaction, no jump mark, no caret move. It returns `ExError::Pattern("Interrupted".into())`.
  `ex.rs`, which T221 holds, doesn't change. The last search, the last substitute and the highlight
  are set as they are now.
- **Docs.** Document the limit and `Interrupted` on `set_time_limit`, `try_find` and
  `substitute::run`. Record the choice in this task's decision record:
  - why a limit rather than ctrl-c (ctrl-c can't reach a command running on the UI thread);
  - why there's no partial `:s` (nvim keeps the rows already done);
  - the unbounded cost measured on this box for the test rows.

## Tests (new file)

`crates/fe-vim/tests/regex_budget.rs`, an integration test through `fe_vim::HeadlessVim`, so no
`src/tests/mod.rs` change. Each test sets its own limit with `set_time_limit` first: the setting is
process-wide.

## Out of scope

- A real ctrl-c interrupt, which would mean running `:s` off the UI thread (fe-gpui).
- The `vim.regex_time_limit` setting (fe-config, fe-gpui and the host): its own follow-up card.
- A different regex engine, or any Cargo change.
- Search highlighting's limit (its own follow-up card). Until that card lands, each `matches_in` call
  from fe-gpui stops at the limit. `match_rects` (`crates/fe-gpui/src/element.rs`) calls it once for each
  displayed buffer row, so one frame can spend up to the limit on each of those rows. find's off-thread
  count (`crates/fe-gpui/src/view/find.rs` `count`) can also stop early on such a pattern. The follow-up
  card fixes both.
- T351's matcher.
