---
task_uid: "01m4n26pf9fj7701rk4y98a17a"
task: "T347"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T341

Findings of the review of T341 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/search.rs:1797: Quantifying a mixed-ending group reintroduces the alternative-context collapse: repeat ORs the endings and reuses the initial translation for later iterations. Consequently, \%(a\|.\)\{1}. incorrectly matches "x\u{301}", a\%(.\|a\)\{2} incorrectly partitions the marks in "a\u{301}\u{323}", and \%(a\|.\)\{2} misses "a\u{301}". Neovim 0.12.4 confirms the opposite result in each case. Capturing groups have the same defect.
  fix: Preserve each alternative's outgoing context through quantification, including transitions between iterations and into the following atom. Add positive and negative oracle cases for capturing and noncapturing mixed-ending groups with exact, optional, and unbounded repetitions.
  instances: crates/fe-vim/src/search.rs:1797, crates/fe-vim/src/search.rs:1804, crates/fe-vim/src/search.rs:1853

- F2 medium crates/fe-vim/src/search.rs:633: The last capture copy in source order is not necessarily the last copy matched during repetition. For :s/\(a\|.\)\+/[\1]/ on "xa", the dot copy captures "x" and the earlier literal copy captures "a" on the final iteration. Both capture locations remain populated, so reverse find_map selects "x", producing "[x]" instead of Neovim 0.12.4's "[a]". This regresses substitutions even without composing characters.
  fix: Resolve duplicated captures by their latest participation in the matched text rather than their syntactic index, with appropriate handling of empty captures. Add oracle cases that switch between duplicated capture alternatives across repetitions in both directions.
  instances: crates/fe-vim/src/search.rs:633

## Implementation notes

- F1: `repeat` now translates an atom that ends in different ways for both start contexts and unrolls the repeat over the two end states (`repeat_mixed`): exact and bounded counts are written out (up to 8, else `TooLarge`), unbounded ones use a two-state Kleene form. When the last time can end either way, the way that took another time is tried first.
- F2: `RowMatches::group` picks the copy with the latest end (then latest start) instead of the last written.
- Not handled: a trailing empty iteration (`\(a\|b\|\)*` on "ab" gives `\1` = "" in nvim); the regex engine records no empty iteration, so that case isn't in the corpus.
