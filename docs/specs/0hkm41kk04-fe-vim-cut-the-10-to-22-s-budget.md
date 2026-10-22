---
task_uid: "01m5fzze2rktm0pq0hkm41kk04"
task: "T215"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "insert::keys::tests::replace_copies_count_the_indent_their_keys_write, insert::keys::tests::opened_rows_copying_a_growing_indent_stop_at_the_budget and insert::keys::tests::rows_opened_above_stop_at_the_budget each take under 5 s in a debug build on an idle machine where the worker measured a drop (cargo nextest run -p fe-vim keys::tests), still asserting the text stays within MAX_PUT and that rows or copies number between 1000 and the count; the keys_tests ones that cannot be cut without a bigger change (they need 64 MiB MAX_PUT of work) stay as they are, a documented limit"
    required: true
    manual: false
  - id: "C2"
    text: "tests::increment::a_block_over_a_long_row_is_quick, tests::substitute::flags_specials_and_bounds and tests::delimiters::brackets_and_angles_select_as_parens_do each take under 4 s (cargo nextest run -p fe-vim \u003cname\u003e); tests::macros::replays_end_cleanly, tests::macros::record_and_play and tests::replace::session_overwrite_and_backspace stay as they are, a documented limit (they need more than 500k keys through HeadlessVim::feed_key at about 10 us each; a fast path in headless.rs is out of scope), to move to the slow set"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# fe-vim: cut the 10 to 22 s budget, increment, macro, replace, substitute and delimiter unit tests (Mac T1069)

Test-only speed-up, user-approved 2026-10-21. Keep every assertion's meaning.

- keys_tests: the three tests run counts of 10000o, 10000O and 100000R against the 64 MiB MAX_PUT. T1010 removed their wall-clock asserts; what remains is the cost of writing up to 64 MiB. They are cheaper if the input hits the budget sooner: use a count just past where the budget bites (the budget stops after ~sqrt(2 x MAX_PUT / 4) = ~5,800 copies for the growing indent, so 10000 is already close; try 7000 / 7000 / 8000) AND/OR start the text with a long "foo" line of indent so each copy writes more. If MAX_PUT can only be reached by 64 MiB of work, take these three off the list: report that in the hand-in and the lead moves them to the slow group instead.
- a_block_over_a_long_row_is_quick: 6 rounds x (10,000 + 100,000)-char rows with the 10x ratio. Use 3 rounds and lengths 4,000 / 40,000 (the same 10x; ratio 50 bound unchanged), keeping the CPU-time clock and the alternation.
- replays_end_cleanly records a 600,000-space `:` line and replays it twice (600,002 keys each, mapped through a Ctrl-key switch). The test proves the replay budget; use 60,000 spaces if the budget constant it checks is smaller than that (check the REPLAY budget in fe-vim/src), else leave the size and look for other costs.
- record_and_play, session_overwrite_and_backspace, flags_specials_and_bounds, brackets_and_angles_select_as_parens_do: none has a big input in the first screens of the source, so 10 to 13 s is almost certainly shared setup (commit(), registry(), parse_keys) or a column x row sweep (delimiters: every column of 7 texts x two oracles). Profile with cargo nextest run -p fe-vim <name> --no-capture, hoist an expensive registry()/keymap build into a std::sync::OnceLock, and thin sweeps (every 2nd column) where the property is per-column symmetric. The fe-vim oracle tests share one nvim and fuzz one case per process; do not add any new nvim process here.
Pure fe-vim, runs on Linux.

## Implementation notes

Done (solo debug timings): a_block_over_a_long_row_is_quick 1.0 s (3 rounds, 4,000/40,000), flags_specials_and_bounds 3.3 s (50,000 rows / 25,000 matches, still linear checks), brackets_and_angles 2.7 s (every 2nd column).

Not reachable in the test files alone:
- keys_tests (3): the budget stops at ~5,800 copies, so 7,000 vs 10,000 changed nothing (12/15/20 s). Reaching MAX_PUT is 64 MiB of work.
- replays_end_cleanly (14 s), record_and_play (7.3 s): the cap is REPLAY_CAP = 1,000,000, so two replays must exceed it (>500,000 spaces); each fed key costs ~10 us in HeadlessVim::feed_key (sync_visual + follow_cursor per key, headless.rs, out of scope). 60,000 would stop proving the cap.
- session_overwrite_and_backspace (7.6 s): `u32::MAX R` asserts text.len() == MAX_PUT, again 64 MiB.
