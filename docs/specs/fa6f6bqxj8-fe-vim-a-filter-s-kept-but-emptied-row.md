---
task_uid: "01m5h7ratzfstd8pfa6f6bqxj8"
task: "T220"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Regression tests in crates/fe-vim/tests/filter.rs, one each for F2 (a kept row the filter empties keeps its marks), F3 (a filter's . prepares its motion as the ! did) and F4 (. after !gc finds the comment run again), each failing on trunk and passing with the fix"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim passes and cargo clippy -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-vim: a filter's kept-but-emptied row keeps its marks, its . prepares its motion as the ! did, and . after !gc finds the comment run (T166 F2/F3/F4, Mac T908 follow-up)

Review follow-up of argon T166 (abandoned as a duplicate of trunk's Mac T908 = T255, 0f44fea5): round-3 findings F2, F3, F4 against trunk's filter. Lead decision z39.

## Problem
1. F2: a kept row that the filter empties keeps its marks (the rows after are taken out or put in; marks must follow the row tracking).
2. F3: a filter's `.` must make its motion ready as the `!` did: display rows, fold candidates, a search checked and made the last.
3. F4: `.` after `!gc` finds the comment run around the caret again.

## Change
Port from branch `keep/t166`: 15d32156 (F2), 2ab4a1ba (F3), fc0acabb (F4), onto trunk's `ex/filter.rs`, `feature/filter.rs`, `marks.rs`, `operator.rs`. Depends on the provenance card because both touch `ex/filter.rs`.

## Tests
One regression test per finding in `crates/fe-vim/tests/filter.rs`, each failing on trunk and passing with the fix.

## Implementation notes

- Ported by hand: trunk's filter (Mac T908) is shaped differently from `keep/t166`, so the three commits
  don't cherry-pick.
- **F2:** `apply_filter`'s put step (rows put in after the range's last) first calls
  `Marks::refilled` (new in `marks.rs`) on the kept rows the output rewrote, setting their marks anew as
  `Place::on` sets them, so an emptied row's marks stay where they are when rows go in below it. The
  drop step already ran first on trunk, so only the put needed it. `apply`'s steps now take the state
  and the snapshot.
- **F3:** `vim.filter` (and so `.`) runs in `line_motions::with_view` and `folds::with_candidates`, with
  `insert::began_line`, as `vim.operate` does. The search check was already on trunk; the `zj` case is
  the one that failed there.
- **F4:** on trunk `!gc` failed outright (a stated known limit). `operator.rs` now gives `!` over the
  `gc` object to `feature::filter::operate_object`, which runs `comment::operate_object` (its visual
  selection opens `'<,'>!`) and then sets the opened line's target to the object. `.` finds the run
  again through the same call, with `Filters::finding` set so that `operate` notes the selection's rows
  and leaves the visual mode rather than opening a line. This is state on `Filters`, not the original's
  thread-local. The motion-and-visual spec's known limit is updated.
- Tests: `an_emptied_kept_row_keeps_its_marks`, `repeating_a_filter_prepares_its_motion` and
  `repeating_a_gc_filter_finds_the_comment_run` in `tests/filter.rs`. Each fails on trunk.
- **Carried findings in `marks.rs`:**
  - Fixed T159 F2: deleting `'^` records `Newer { exit }`. Test:
    `tests::marks_special::deleting_the_insert_mark_while_a_clone_closes_keeps_it_deleted`, covering
    both `:delmarks ^` and `:delmarks!`. It lives in `src/tests/marks_special.rs`, next to the closing
    clone pattern it reuses.
  - Fixed T160 F5: `Motion::Position` past the last row is E19 and the caret stays. Test:
    `ex_jumps::picking_an_entry_past_the_last_row_is_e19`, in `tests/ex_jumps.rs`.
  - Left T42 F22 and T42 F25. Each needs new batch metadata: the corrected `.` carried through
    `insert.rs`'s `Left` and adoption, and the put kind and extent carried from `operator.rs`'s put.
    Each also needs new oracle cases. That is a card's worth of work each, mostly outside this scope
    (`insert.rs`), so they are proposed as follow-ups.
