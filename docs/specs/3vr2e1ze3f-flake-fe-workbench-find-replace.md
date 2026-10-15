---
task_uid: "01m4ynpxtxr6r3tv3vr2e1ze3f"
task: "T721"
rev: 4
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim search.rs gains one small test seam, a process-wide count of the regex calls made under a Deadline, as a #[doc(hidden)] pub fn (no cargo feature: the gate builds no features); and a Deadline whose limit overflows Instant (e.g. Duration::MAX, and Deadline::never) never expires, instead of expiring at once (end: None, no clock read needed). No other search result, limit or timing changes; the existing fe-vim search tests pass unchanged, plus one fe-vim unit test that a Duration::MAX limit never runs out"
    required: true
    manual: false
  - id: "C2"
    text: "find_replace_interrupted::replace_on_an_expensive_row_steps_only_as_far_as_it_needs asserts through that seam that replace_next does far less work than walking the whole expensive row, still fails if it walks the whole row (temporary local mutation in the evidence), and asserts no wall-clock bound"
    required: true
    manual: false
  - id: "C3"
    text: "find_replace_interrupted::the_wanted_match_ends_the_search asserts through that seam that the search stops at the wanted match, still fails if it runs past it (temporary local mutation in the evidence), and both counted operations run with a never-expiring limit (Duration::MAX), so neither outcome depends on any wall-clock limit or measured call"
    required: true
    manual: false
  - id: "C4"
    text: "Both tests pass 20 times in a row under load (cargo nextest run -p fe-workbench --test find_replace_interrupted with a parallel workspace build), with the commands and results in the evidence; the other find_replace_interrupted tests pass unchanged; cargo clippy -p fe-vim -p fe-workbench --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Flake: fe-workbench find_replace_interrupted replace_on_an_expensive_row_steps_only_as_far_as_it_needs (SLOW_LIMIT/2 bound) and the_wanted_match_ends_the_search (call*2 deadline) fail under load

## Problem

`fe-workbench::find_replace_interrupted replace_on_an_expensive_row_steps_only_as_far_as_it_needs`
(crates/fe-workbench/tests/find_replace_interrupted.rs:225-242, added by Mac T505 = neon T112, landed in host stack 9,
e4d2b586) failed post_merge run 01m4ymw974fnqv50q7qrtk1t4j on 611c6f67 with `381.198291ms` at line 241:
`assert!(took < SLOW_LIMIT / 2)`, where SLOW_LIMIT is 400 ms. The machine was heavily loaded (other tests ran past
60-150 s in the same run), and the test passed in the stack 9 gate. A wall-clock bound of 200 ms on a single dispatch
is a load flake.

## Change

Test-only. Prove "steps only as far as it needs" without a wall-clock bound: assert on the work done (for example the
number of rows or matches the replace visited, through whatever count the find code already exposes to tests, or a
step counter in the test's own fake), or compare against the full-row cost measured in the same test with a wide
ratio. Don't change production code; if no existing seam can count the work, say so in the spec and block.

## Also: the_wanted_match_ends_the_search

The same file's `the_wanted_match_ends_the_search` (line 349) failed post_merge run 01m4ynmkacdrfm4tejc1xwtbt8 on
611c6f67 with "one call took 162.085209ms" and label "Search took too long": the test gives the search a deadline of
`call * 2`, where `call` is the max of 3 earlier measured calls (162 ms), and one load stall during the real call breaks
that 2x margin. replace_on_an_expensive_row also failed T719's fast run 01m4ym2tvnanf52ch4m7y4z8h3 on 9da30aa0 (468 ms).
Both failures stalled the Mac main post-check twice and triggered an automatic revert of the innocent T675 (kiln only).
Fix both tests the same way: test-only, no wall-clock margin a load stall can break (assert on calls made or rows
visited, or a margin derived so a stalled call can't cross it).
