---
task_uid: "01m6v5n30dqpber70x52z9c6r4"
task: "T513"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "the six named fe-vim tests (delimiters_regressions other_character_class_atoms_in_a_pattern_name_pair and eleven_distinct_joined_chars_in_a_pattern_name_pair, matchit_tags_are_bounded, seek_counts_and_bounds, substitute_preview_matches_the_real_substitute, substitute_preview_is_capped_and_timed) still prove what they proved, including that bounded operations stay bounded, without a 2 s wall-clock race"
    required: true
    manual: false
  - id: "C2"
    text: "each of the four test binaries passes 20 runs in a row under host load 30+, with the slowest time and the load recorded in the Implementation notes"
    required: true
    manual: false
  - id: "C3"
    text: "only the four scoped test files change, and cargo clippy -p fe-vim --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Give fe-vim's search-bounded tests headroom under load: delimiters_regressions, matchit_tags_are_bounded, seek_counts_and_bounds, substitute_preview (flake)

Load flake seen in T505's second finish gate (run 01m6v374d6dwmw0sv6wx1ypjrc, 2026-11-07, load ~45), and in T501's `rgt ci run --fast` (matchit_tags_are_bounded). Six fe-vim tests failed; all six pass when rerun alone in the same worktree. T505 touched only extensions/grammars/graphql, so none of this is its change.

# Give fe-vim's search-bounded tests headroom under load

## Problem
fe-vim stops a search after `search::time_limit()`, process-wide, `DEFAULT_TIME_LIMIT` = 2 s (crates/fe-vim/src/search.rs:32). On a timeout the command or text object fails quietly: the register stays empty, the cursor doesn't move, or the substitute preview comes back `None`. At host load 40+ these tests' searches cross 2 s of wall time:

- `delimiters_regressions` `other_character_class_atoms_in_a_pattern_name_pair` (crates/fe-vim/tests/delimiters_regressions.rs:92) and `eleven_distinct_joined_chars_in_a_pattern_name_pair` (:106): `dit` returned an empty register at 2.1-2.2 s. The file's one warm-up run (around :62) isn't enough.
- `matchit_tags` `matchit_tags_are_bounded` (crates/fe-vim/tests/matchit_tags.rs:410): cursor stayed at (0,1) instead of (19999,1); the test ran 15.6 s.
- `seek_objects` `seek_counts_and_bounds` (crates/fe-vim/tests/seek_objects.rs:181): `din(` took 2.38 s against `took < DEFAULT_TIME_LIMIT`.
- `substitute_preview` `substitute_preview_matches_the_real_substitute` (crates/fe-vim/tests/substitute_preview.rs:53, "no preview of %s/one/1/gi") and `substitute_preview_is_capped_and_timed` (:259, unwrap on a None preview). The file's `unhurried()` hook (:16) raises only `set_search_preview_limit`, but crates/fe-vim/src/substitute.rs:739 caps the preview at `search_preview_limit().min(search::time_limit())`, so it is still 2 s.

## Change
Test only, in the four test files above. For each failing test, keep what it proves and remove the wall-clock race:
- Where the test only needs the search to finish (the delimiters, matchit and substitute-preview cases), raise the limit for that test with `search::set_time_limit` (as find_replace.rs, search_preview.rs and argument_object.rs already do), restoring it afterwards if other tests in the same binary depend on the default. For substitute_preview, `unhurried()` must raise `search::set_time_limit` as well.
- Where the assertion is the bound itself (seek_counts_and_bounds' `took < DEFAULT_TIME_LIMIT`, and any "is_bounded"/"capped_and_timed" check), keep a check that the work is bounded, but measure it in a way load can't break: a work budget or step count if fe-vim exposes one (see regex_budget.rs and search_offset_budget.rs), else a looser wall bound well clear of the default with a comment saying why.
- Remember the limit is process-wide: tests in one binary run in parallel threads, so a raised limit in one test must not weaken another test's bound check in that binary. Serialize or scope as needed.

No production code changes.

## Tests
This can't fail at base on demand, so prove it with repeated runs under load: run each of the four test binaries 20 times in a row (`cargo test -p fe-vim --test <name>`) while host load is 30+, and record the slowest time and the load in the Implementation notes.

## Not in scope
fe-vim's search or substitute code, DEFAULT_TIME_LIMIT itself, and other fe-vim timing tests that haven't failed.

## Implementation notes

- delimiters_regressions, matchit_tags (`matchit_tags_are_bounded`), seek_objects (`seek_counts_and_bounds`): raise `set_time_limit` to 60 s in the test, never restored (nothing else in those binaries relies on the default). Matchit's bounds stay the work counts; seek's `took < DEFAULT_TIME_LIMIT` became a 30 s runaway bound with a comment.
- substitute_preview: `unhurried()` raises the search limit as well as the preview's, and returns a mutex guard held for the test so the one test that lowers the limit to 2 s for its slow-pattern check can't weaken or be weakened by the others.
- Two sets of 20 consecutive runs per binary at host load 36-45 (loadavg), all passed. Slowest run (test time) across both: delimiters_regressions 1.36 s, matchit_tags 4.07 s, seek_objects 2.26 s, substitute_preview 7.59 s.
