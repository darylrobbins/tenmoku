---
task_uid: "01m5eznt5pxydg26x2f8yq7g2s"
task: "T199"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests show an interrupted search returns Interrupted rather than a result, window loading stops at the deadline, [\\n-a] on 'x\\n5\\na' gives 2..3 and \\_. followed by U+0301 on 'ab\\ncd' finds no match"
    required: true
    manual: false
---

# fe-vim: search deadlines are checked after every regex call and while loading a window; \n range endpoints and \_. composing marks follow nvim (T161 follow-up)

## Problem

T161 (multi-row patterns, Pattern::find_across) landed with four medium review findings left open (zz49), all in crates/fe-vim/src/search.rs:

- F1 (near 796) `list_row` uses `deadline.check()`, which reads only the recorded expiry flag: sample `deadline.expired()` after every `matches.next()`, including unsuccessful calls, before accepting results.
- F2 (near 688, 784, 1157) window loading never checks the deadline: an unbounded pattern copies the rest of the buffer first. Check before the initial hold and between chunks.
- F3 (near 2368) `\n` as a collection range endpoint (`[\n-a]`) is read as a separate member: on "x\n5\na" it gives 3..4, nvim 2..3. Use nvim's endpoint semantics.
- F4 (near 3161) `\_.` followed by a combining accent (U+0301) wrongly matches a bare line break: on "ab\ncd" it gives 2..3, nvim finds no match. Keep the composing-mark requirement on the newline alternative.

## Change

Fix each, with a test per finding (nvim-probed positions in comments; deadline tests use a per-test `Deadline`).

## Implementation notes

- F1-F4 were already fixed in `Across` and the pattern parser by T162 (e2793cf8), with unit tests in `search.rs`'s `across_tests`. What was left of F1 was in the row-pattern walk: `Pattern::forward` (every `/` from `find_across` for a pattern that stays on a row, and `find`/`find_from`) and `first_in_row` took a regex call's result after `deadline.check()`, which reads only the recorded flag. They now sample the clock after each call (`in_time`), so a call that overran the deadline counts for nothing there too. `substitute.rs` has the same `check()` shape; it is outside this card's scope and left as is.
- `crates/fe-vim/tests/search_deadline_and_ranges.rs` covers C1 through the public API: a regex-call budget that runs out on a search's last call gives `Interrupted` (row and multi-row patterns, both directions); an expired deadline stops before the window loads (no regex call); `[\n-a]` on "x\n5\na" is 2..3; `\_.` + U+0301 on "ab\ncd" finds nothing. The window's chunk count is checked directly by the unit test `a_window_reads_nothing_past_the_deadline`.
- Carried T162 F5: `Across` now keeps the rows it found to have no match as stretches (`none`, a `BTreeMap` of start to end), with the rows the same regex call ruled out after them, and `list` skips them without a regex call. `last_in_rows` first steps back over rows already told (stretches with none, rows with matches listed) before listing a window forward, so each step of a backward count doesn't start over from the window's start. `4000?a\_s*<CR>` over 4,000 a/x row pairs now takes 16,000 regex calls, two a row (it took about eight million and ran out of time); `a_backward_count_over_rows_lists_each_row_once` holds it under six a pair.
