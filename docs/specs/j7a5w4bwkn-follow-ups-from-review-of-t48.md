---
task_uid: "01m5x64mgw0raa7wj7a5w4bwkn"
task: "T367"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T48

Findings of the review of T48 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-gpui/src/view/ime.rs:296: Explicit-range replacement remains incomplete. The new literal fallback preserves multiline contents but bypasses Replace's restoration ledger: replacing 0..1 of "abcd" with "e\u{301}" after R cannot restore "a" with Backspace, and dot repetition loses the replacement. The remaining keystroke path can also delete outside the supplied range: replacing UTF-16 0..1 of "a\u{301}b" with "x" consumes the combining mark outside that range.
  fix: Handle explicit ranges independently of Replace keystroke semantics while updating replacement recording and restoration tracking. Add regressions for partial-grapheme ranges, Backspace restoration and dot/count repetition after multiline and Unicode replacements.
  instances: crates/fe-gpui/src/view/ime.rs:296, crates/fe-gpui/src/view/ime.rs:300, crates/fe-gpui/src/view/ime.rs:336

## Implementation notes

- T351 F2: `go_to_bracket` unfolds every destination row before setting the carets; regression `a_partner_inside_a_fold_is_unfolded`.
- T48 F3: a replace-mode explicit range that starts or ends inside a grapheme is now replaced literally, so nothing outside the range is consumed (regressions in `explicit_range_commits_keep_their_range`).
- Known limit: the replace session's restoration/dot ledger lives in fe-vim (`replace.rs`), outside this scope, and has no entry point for an explicit range. Literal replacements (line breaks, tabs, joining marks, partial graphemes) therefore stay out of the ledger: `<BS>` doesn't restore them. Needs an fe-vim API (e.g. `VimState::replace_range`) as a follow-up card.
