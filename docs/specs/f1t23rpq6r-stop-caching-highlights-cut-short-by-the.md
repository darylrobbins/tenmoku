---
task_uid: "01m4kcax8fc2hvvnf1t23rpq6r"
task: "T3"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::query_budget_cache::a_query_cut_short_is_not_cached: with DEADLINE_AFTER tripping mid-query on a ~2,000-row TOML buffer, Syntax::highlights(range) returns fewer spans than Syntax::query(tree, range, split, Duration::MAX), and the next highlights(range) equals that full result"
    required: true
    manual: false
  - id: "C2"
    text: "tests::query_budget_cache::a_cut_short_query_asks_for_a_redraw: after a cut-short highlights call, take_missing_languages() contains the host language (\"toml\"); after a complete call it is empty"
    required: true
    manual: false
  - id: "C3"
    text: "tests::query_budget_cache::a_range_cut_short_at_the_cap_is_cached: when every attempt on one range runs out, the budget doubles 2/4/8/16 ms; after the 16 ms attempt the partial result is cached, so the next call makes no deadline checks and take_missing_languages() is empty"
    required: true
    manual: false
  - id: "C4"
    text: "Under cfg(test) QUERY_BUDGET is a far deadline that DEADLINE_AFTER can trip, while the no-query-budget feature still has no deadline; scripts/check-syntax-features.sh and cargo nextest -p fe-syntax pass"
    required: true
    manual: false
---

# Stop caching highlights cut short by the query budget, so rows below the cut get their colours on the next frame

## Problem

The user, 2026-10-10: "Sometimes when scrolling a file, it loses it's highlighting temporarily. Also, when
searching, the formatting sometimes breaks like in this screenshot." The screenshot shows regatta.toml,
with cmd-F open on "fastedit" ("3 of 4") and the caret on line 81. Lines 54–56 at the top of the viewport
are coloured. Every row from 57/58 to 84 is in the default foreground.

The cause is in `Syntax::highlights` (crates/fe-syntax/src/syntax.rs:353–418):

- Each call queries under `QUERY_BUDGET`, which is 2 ms outside tests (syntax.rs:41–45). When the budget
  runs out, `highlight_query` stops where it is and returns the captures found so far
  (syntax.rs:510–535). Captures come in start order, so you get colour on the top rows and plain text
  below them, as in the screenshot. On a debug build or a loaded machine (regatta lanes keep this Mac at
  load 40–90), 30 rows of TOML can go past 2 ms.
- The cut-short result is still cached as complete. `complete` comes only from `injection::paint`
  (injection.rs:286–312), which looks at injections and not at whether the host query finished, so
  syntax.rs:409–415 pushes the truncated spans into `cache.ranges`. The doc comment at syntax.rs:345–347
  says this is deliberate ("they're cached like any others").
- Every later frame that shows the same rows gets the cached truncated spans (syntax.rs:373–377). Only an
  edit, a reparse, or a new visible range clears them. A find jump followed by no scrolling keeps the
  broken colours. While scrolling, each new range is queried fresh, so the loss is "temporary".
- Nothing asks for another frame after a cut-short query anyway. fe-gpui redraws for highlights only when
  `Syntax::take_missing_languages` returns something (crates/fe-gpui/src/view.rs:806–833, called from
  element.rs prepaint).

## Change

All in fe-syntax. No fe-gpui change is needed:

1. `highlight_query` (syntax.rs) also returns whether it ran to the end. `Syntax::highlights` caches a
   result only when the host query and `injection::paint` both completed.
2. When a range runs out of time, `Cache` records it with an attempt count. The next query of the same
   range doubles the budget: 2, 4, 8, then 16 ms (`QUERY_BUDGET_MAX`). A range that still runs out at
   16 ms is pathological. Cache its partial result like a complete one, so it can't redraw forever. Any
   tree change resets the record, as it resets `ranges`.
3. A cut-short call that isn't cached adds the host language's name to `layers.missing`.
   `take_missing_languages` then returns it, and EditorView's existing `load_injected_languages` path
   (view.rs:806–833) calls `language_by_name` (a cache hit) and draws again with no input from the user.
   Update the doc comments on `highlights` and `take_missing_languages` to say this.
4. Under `cfg(test)`, `QUERY_BUDGET` becomes a far deadline (1 h) instead of `Duration::MAX`. That way
   `DEADLINE_AFTER` can trip it, as the injection tests already do with `far_deadline()`.
   `no-query-budget` stays unlimited (`None`) for the fuzz target and the integration tests.

Tests go in a new file, `crates/fe-syntax/src/tests/query_budget_cache.rs`, declared with
`mod query_budget_cache;` in `crates/fe-syntax/src/tests.rs`. They use only base API
(`Syntax::highlights`, `Syntax::query`, `take_missing_languages`, `syntax::{DEADLINE_AFTER,
DEADLINE_CHECKS}`), with a local `with_deadline_after` helper. Use a TOML buffer of about 2,000 rows
(`[[pipeline.stages]]` / `name = "test"` blocks). Trip the deadline at half the checks that a full call
makes, so the cut lands mid-range.

At base, the regression test fails at its first assertion (the call is not cut short), because the test
build at base has no deadline. Record that in the decision file. Also show the real bug: at base, apply
only change 4 and run the test. It must fail at the second assertion, because the truncated result was
cached.


_Moved from the Mac board (T327) to run on neon; it lands back on main through the lead._
