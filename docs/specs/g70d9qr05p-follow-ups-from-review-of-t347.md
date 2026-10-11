---
task_uid: "01m4n5w4wx2gtfc1g70d9qr05p"
task: "T349"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T347

Findings of the review of T347 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/search.rs:2023: The two-state translation preserves accepted paths but loses Vim's match priority. by_end merges nonadjacent alternatives, and any always tries the opposite ending first, even for lazy repeats. Consequently, substituting \%(a\|.\)* or \%(a\|.\)\{0,2} on "ax" selects "a" instead of Neovim 0.12.4's "ax"; \%(a\|.\)\{-} on "a" selects "a" instead of the empty match. Ordering self-loops before crossing loops also changes captures: \(a\|.\)* on "á" captures the whole character instead of the final composing mark.
  fix: Preserve alternative and repetition priority throughout state composition and elimination, including greedy continuation, lazy stopping, and capture participation. Add oracle regressions for these examples with capturing and noncapturing groups.
  instances: crates/fe-vim/src/search.rs:1793, crates/fe-vim/src/search.rs:1794, crates/fe-vim/src/search.rs:2023, crates/fe-vim/src/search.rs:2048, crates/fe-vim/src/search.rs:2082, crates/fe-vim/src/search.rs:2109, crates/fe-vim/src/search.rs:2114, crates/fe-vim/src/search.rs:2149

- F2 medium crates/fe-vim/src/search.rs:636: Selecting the latest recorded capture does not resolve a final empty iteration that the engine never records. The implementation explicitly leaves this unresolved and excludes it from the corpus, although the task requests appropriate handling of empty captures. For :s/\(a\|b\|\)*/[\1]/ on "ab", Neovim 0.12.4 produces "[]", while the implementation retains a nonempty capture.
  fix: Represent final empty capture participation in the translation or matching layer so capture resolution can select it. Add oracle coverage for trailing empty iterations in bounded and unbounded repetitions.
  instances: crates/fe-vim/src/search.rs:636, docs/specs/rk4y98a17a-follow-ups-from-review-of-t341.md:26

- F3 medium crates/fe-vim/src/search.rs:2130: The new unconditional eight-iteration limit rejects previously supported, small patterns such as \%(a\|.\)\{9} and \%(a\|.\)\{9,}. These now fail with TooLarge even on nine plain ASCII characters, introducing a compatibility regression unrelated to an actual regex size-limit failure.
  fix: Support larger counts with a compact translation or enforce a measured translation resource budget instead of rejecting every count above eight. Add regression tests for exact, bounded, and open-ended counts above eight.
  instances: crates/fe-vim/src/search.rs:2014, crates/fe-vim/src/search.rs:2130

- F4 medium crates/fe-vim/tests/corpus/composing.toml:493: Both new star_then_dot cases write {0,} without the required backslash. In default Vim magic syntax this is literal text, so the unchanged-buffer expectations pass without exercising repetition or its transition into the following dot.
  fix: Use * or an escaped \{0,} quantifier in both fixtures and regenerate their expectations with Neovim. Include inputs where substitution must occur.
  instances: crates/fe-vim/tests/corpus/composing.toml:493, crates/fe-vim/tests/corpus/composing.toml:553, crates/fe-vim/tests/corpus/composing.expected.json:1828, crates/fe-vim/tests/corpus/composing.expected.json:1838, crates/fe-vim/tests/corpus/composing.expected.json:2058, crates/fe-vim/tests/corpus/composing.expected.json:2068


## Known limits (lead decision, 2026-10-11)

Exact Vim match priority for unbounded or large-count repeats of mixed-ending groups, and of
empty-first groups such as `\(\|a\|b\)\+`, past the ordered prefix is out of scope here: exits sit
inside the cycle, so the ordering interleaves without bound and one regex-crate pattern cannot
express it. Past the ordered prefix the compact-form approximation stands, documented as a known
limit in this spec and in search.rs. T351 is the fix (an exact matcher; its approach is the
user's call). F1-F4 are done when every case they name, and every case up to the ordered prefix,
matches nvim; the residual beyond the prefix is T351's, not this task's.

## Implementation notes

- F1: `repeat_mixed` first writes the repeat out in the order nvim tries things in (`ordered`): each way of one time is followed by everything that can come after it before the next way, neighbours that end the same way are merged and nothing else is, a greedy repeat tries another time before stopping and a lazy one stops first. That keeps capture participation too (`\(a\|.\)*` on "á" ends on the mark). It's kept to `MAX_ORDERED_PATHS` ways and `MAX_ORDERED_TEXT` bytes, and an unbounded repeat is written out at most `MAX_OPEN_TIMES` times before the compact form takes the rest. Known limit: past that (about five or more times of a group whose ways end differently and whose continuation depends on how it ended), the order between ways that end on different kinds is the compact form's, which tries the way that ends on the other kind first; it can pick another parse of the same text than nvim. A regex has no way to write that order for an unbounded repeat.
- F2: a group that can match nothing and holds a capture, repeated without an upper bound and greedily, goes through the same path (`empty_last`) even when all its ways end alike, because `regex` records no empty time of a `*`. A time that matches nothing ends the repeat with its captures set, which the written-out times and the compact form's last time (`any`) both take. Known limit: a group whose first way is empty and whose other ways end differently (`\(\|a\|b\)\+`) still takes a time that matches text where nvim stops at the empty one, past the written-out times.
- F3: the compact form is now a matrix of the ways through (`Ways`), squared (`times`, `up_to`), so its text grows with the square of the count and `\{9}`, `\{9,}` and `\{3,12}` work. The only limit left is the text (`MAX_REPEAT_TEXT`) and the compiled size, 64 MiB for a pattern with such a repeat (`MAX_MIXED_COMPILED`) because each copy of a `.` is about 80 KiB compiled.
- F4: the fixtures write `\{0,}`, and the corpus lines that use `*` or `+` must not carry a backslash (the recorder's clipboard check), so the new cases spell them `\{0,}` and `\{1,}`.

### Known limits

- Exact Vim priority past the ordered prefix (unbounded or large-count repeats of mixed-ending groups with a state-dependent continuation, e.g. `:s/\(a\|.\)\{0,}/[\1]/` on "axaxaá", and empty-first groups such as `\(\|a\|b\)\{1,}`) is not kept: the compact form stands. Fixed by T351; commented at `repeat_mixed` in `search.rs`.
- The task has no acceptance criteria, so there is nothing to annotate in the criteria file (`{}`).
