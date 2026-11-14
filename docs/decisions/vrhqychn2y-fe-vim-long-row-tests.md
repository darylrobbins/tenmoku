# Sizing down fe-vim's long-row tests (T380)

The Linux gate's test stage stopped finishing in 30 minutes: about ten fe-vim tests take minutes under parallel load. HeadlessVim's per-key cost grows with the row (a plain key on an 80 KB row is a few ms; a visual selection of a thousand columns about 0.8 s), and these tests run hundreds of keys over rows past the 64 KiB (`4 * CARET_WINDOW`) at which rows are read in windows.

## Changes

- `caret_window.rs`: `LONG_UNITS` replaces the `REACH`-unit tail and head (`a b ` x 16384 = 64 KiB) with `3 * REACH / 4 + 64` units, which with the pad of about `REACH` bytes is just past 64 KiB (each test still asserts `long.len() > 4 * REACH`). The long indent in `first_non_blanks_past_a_long_indent...` goes from 400,000 blanks to `8 * REACH + 1000`, past `BLANK_REACH` (128 KiB). The sentence test's tail goes from `REACH` units of `Ab cd. ` to `3 * REACH / 7 + 64`.
- `argument_object.rs`: the long suffix of `argument_visual_objects_with_a_long_suffix_select_the_list` goes from 300,000 to 32,768 bytes (twice the window, eight times the work chunk `WORK`), a row long enough that a whole-row copy would show.
- Every assertion is unchanged.
- `.config/nextest.toml`: the tests that stay over 10 s go in the slow set (post-merge only).

## Timings

Host `argon` (12 cores, shared with other Regatta lanes, one-minute load average 11 to 25 during the runs, so each time varies with load by up to about 2x). Each test alone, debug: `cargo nextest run -p fe-vim -E 'test(~<name>)'`, wall time of the command with the test binary already built. Before = at base c6d24d9 (the replay and regional tests are untouched, so their before is the same binary).

| Test | Before | After | Where |
|---|---|---|---|
| `graphemes_joining_across_the_windows_edge_stay_whole` | 214.9 s | 78.4 s | slow set |
| `graphemes_joining_across_the_windows_left_edge_stay_whole` | 92.2 s | 27.1 s | slow set |
| `a_visual_counted_l_goes_over_the_windows_edge` | 39.7 s | 19.2 s | slow set |
| `columns_far_along_a_huge_row_are_the_rows_own` | 38.9 s | 14.5 s | slow set |
| `first_non_blanks_past_a_long_indent_land_as_past_a_short_one` | 51.1 s | 6.8 s | gate |
| `counted_replay_of_an_oversized_register_is_refused` | 44.9 s | 44.9 s | slow set |
| `dot_replay_of_an_oversized_register_is_refused` | 47.2 s | 47.2 s | slow set |
| `counted_replace_of_regional_indicators_caps_at_max_put` | 103.2 s | 103.2 s (49.8 s on a quieter run) | slow set |
| `sentence_objects_and_motions_go_over_the_windows_edge` | 14.7 s (tail 114,688 B) | 18.2 s (tail 49,216 B; host busier) | slow set |
| `argument_visual_objects_with_a_long_suffix_select_the_list` | 41.3 s (300,000 B) | 4.7 s (32,768 B) | gate |
| `keys_from_the_middle_of_a_100_mb_row_land_as_on_a_short_row` | 36.9 s | unchanged | slow set |

Other fe-vim tests that took over 10 s only in the parallel full run were under 10 s alone after this change (`a_word_takes_a_long_run_of_blanks_before_it` 9.5 s, `word_objects_take_a_run_joined_by_prepend_chars` 8.0 s, `counted_word_objects_go_over_the_windows_edge` 8.7 s before).

## Why each slow-set test stays

- The two grapheme tests: the claim is that a window's edge falls at every byte of each sequence, so 4 and 3 sequences x about 20 to 30 edges x 7 to 9 keys, each key counted over 16 KiB, each against a short and a long row; fewer edges or keys drop cases.
- `a_visual_counted_l...` and `columns_far_along...`: a visual selection (or a column) must reach 16 KiB and 40,000 bytes into a row past the window; the selection's cost is the claim.
- `sentence_objects...`: 5 edges x 9 keys, each a sentence scan across the 16 KiB window edge on two rows; about 0.2 s a key; the tail is already the least that triggers windowing.
- `keys_from_the_middle_of_a_100_mb_row...`: the 100 MB row is the claim.
- `counted_replace_of_regional_indicators_caps_at_max_put` and the two replay tests: the claim is the clamp to `MAX_PUT` (64 MiB), so each writes or replays 64 MiB; a smaller count or register would not reach the cap.
