# T565: delimiters_regressions pattern-name tests run under unhurried()

Cause confirmed as test-side: the searches use the 2 s DEFAULT_TIME_LIMIT; nothing in
`crates/fe-vim/src/` changed.

## Guarded (C1, C2)

`let _limit = unhurried();` is now the first line of
`a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered` and of the other
unguarded pattern-name tests:
`distinct_joined_chars_in_a_pattern_name_keep_their_own_requirements`,
`a_magic_dot_before_a_zwj_pairs_with_nothing`, `a_star_before_a_zwj_is_a_multi`,
`a_private_use_char_in_the_text_is_not_a_stand_in`,
`a_bare_lt_before_an_empty_closing_tag_keeps_the_boundary`.

## Left alone on purpose

`pattern_pairing_past_many_other_tags_finishes_in_time`: it is about the default limit
(it sets DEFAULT_TIME_LIMIT itself). The tests that already had `unhurried()` are unchanged.

## Load proof (C3)

Host has 24 cores, so the target is a 1-minute load average of at least 48. An
earlier attempt with 48 loops peaked at 43 and was rejected. This one: `proof2.sh`
registers `trap ... EXIT INT TERM` (kill the loops, wait) before starting 96
`while :; do :; done` loops, waits until the 1-minute load average is at least 50, then
runs 20 times in a row:

`cargo nextest run -p fe-vim --test delimiters_regressions --retries 0`

Load average (1 min) before -> after each run, all PASS:
1: 50.06 -> 53.98; 2: 53.98 -> 58.15; 3: 58.15 -> 58.15; 4: 58.15 -> 61.58;
5: 61.58 -> 61.58; 6: 61.58 -> 64.50; 7: 64.50 -> 67.10; 8: 67.10 -> 67.10;
9: 67.10 -> 70.05; 10: 70.05 -> 70.05; 11: 70.05 -> 72.21; 12: 72.21 -> 74.28;
13: 74.28 -> 74.28; 14: 74.28 -> 76.74; 15: 76.74 -> 76.74; 16: 76.74 -> 78.76;
17: 78.76 -> 78.76; 18: 78.76 -> 80.46; 19: 80.46 -> 80.46; 20: 80.46 -> 81.79.

Result: 20/20 passed, load at least 50 (2x the cores) for every run. The trap ran at
exit; no busy-loop process remained.
