---
task_uid: "01m4n8ydg51zbdd84mx0rf2akm"
task: "T351"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Every T347 review example listed in the spec matches nvim 0.12.4 (oracle cases recorded with nvim)"
    required: true
    manual: false
  - id: "C2"
    text: "All existing composing corpus cases still pass"
    required: true
    manual: false
  - id: "C3"
    text: "The chosen approach is recorded in a decision record"
    required: true
    manual: false
---

# fe-vim search: exact Vim match priority for unbounded repeats of mixed-ending and empty-first groups (T349's known limit)

fe-vim's search translates Vim patterns to one `regex`-crate pattern. For repeats of mixed-ending
groups (`\%(a\|.\)*`, `\%(a\|.\)\{N,}` with large N) and empty-first groups (`\(\|a\|b\)\+`), Vim's
match priority (alternative order, greedy continuation vs lazy stop, last-iteration capture
participation) can only be reproduced up to an ordered prefix: past it, exits sit inside the cycle,
so the ordering interleaves without bound and no single regex-crate pattern (no lookahead, no atomic
groups) can express it. T349 accepted the compact-form approximation past the ordered prefix as a
known limit (see its spec's "Known limits").

Make the match priority exact for these patterns. The T349 worker's options:

- rewrite matching for these patterns with a custom backtracking matcher (bounded, with the
  existing step/size budgets and cancel checks), used only when the translation can't keep Vim's
  priority; or
- a sentinel-transformed haystack that lets one regex-crate pattern encode iteration boundaries.

Decide between them first and record it in a decision record. Keep every case in
crates/fe-vim/tests/corpus/composing.toml passing, and add nvim-recorded oracle cases for the
T347 review examples: `\%(a\|.\)*` and `\%(a\|.\)\{0,2}` on "ax" (match "ax"), `\%(a\|.\)\{-}` on
"a" (empty match), `\(a\|.\)*` on "á" (captures the final composing mark),
`:s/\(a\|b\|\)*/[\1]/` on "ab" ("[]"), and counts above eight (`\%(a\|.\)\{9}`, `\{9,}`).

## Implementation notes

- Approach: a custom backtracking matcher (`crates/fe-vim/src/search/backtrack.rs`), recorded in `docs/decisions/4mx0rf2akm-vim-search-priority.md`. Every repeat of an atom with several ways to end, and every unbounded greedy repeat of a capturing group that can match nothing, is matched by it; the written-out ordered form and the compact form are deleted.
- Checked against nvim 0.12.4 with a throwaway differential run of ~2600 random patterns over ASCII rows (0 differences); not committed.
- New oracle cases at the end of `composing.toml` (`exact_*`, `beyond_*`) cover the T347 examples, counts above eight, and repeats past the old ordered prefix. `*` is spelled `\{0,}` there (the recorder's clipboard check).
- Budgets: `MAX_STEPS`, `MAX_STACK`, `MAX_PROGRAM`, and the operation's `Deadline`.
