---
task_uid: "01m4h4bg5zhmvcj06cnjmjfth9"
task: "T301"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T215

Findings of the review of T215 that don't block the merge (below merge.block_severity):

- F8 medium crates/fe-vim/src/search.rs:487: Checking word boundaries after regex matching discards valid quantified matches without trying another endpoint at the same start. For example, \<foo.*\> in "foo bar!" matches "foo bar" in Neovim 0.12.4, but this implementation greedily consumes the exclamation mark, rejects the boundary, and reports no match. Lazy quantifiers can fail similarly. This regresses ordinary searches, their repeats, operator motions, and highlights.
  fix: Make boundary constraints participate in match selection, or restrict this optimization to generated literal word patterns while preserving existing regex behavior for other patterns. Add greedy and lazy quantified-boundary regression tests.
  instances: crates/fe-vim/src/search.rs:487

- F9 medium crates/fe-vim/src/search.rs:528: Removing boundary tokens before translation changes context-sensitive pattern syntax. Removing a leading \< makes a following ^ become a branch-start anchor; removing a trailing \> makes a preceding $ become a branch-end anchor. Consequently, \<^foo\> and \<foo$\> now match "foo", whereas Neovim 0.12.4 and the previous translator find no match. Search operators can therefore modify text for patterns that should fail.
  fix: Preserve the original token context when translating anchors and quantifiers, or limit boundary extraction to generated literal word patterns. Add tests for both leading-boundary/^ and $/trailing-boundary combinations.
  instances: crates/fe-vim/src/search.rs:528, crates/fe-vim/src/search.rs:531
