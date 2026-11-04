---
task_uid: "01m6j3ze8a1gbkyrrj49m90rs5"
task: "T434"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T239

Findings of the review of T239 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/ex/lines.rs:658: :sort pattern matching incorrectly applies smartcase. With ic scs, sorting ["b Xa", "a xB", "c XC"] using :sort /X/ produces ["a xB", "b Xa", "c XC"] here; Neovim 0.12.4 produces ["a xB", "c XC", "b Xa"]. The recorded smartcase case passes because its input yields the same order under either matching rule.
  fix: Use ignorecase alone for sort pattern matching, preserving inline case overrides. Add a recorded smartcase case whose order distinguishes sensitive and insensitive matching.
  instances: crates/fe-vim/src/ex/lines.rs:658

- F2 medium crates/fe-vim/src/substitute.rs:430: Passing false unconditionally loses the smartcase exemption of a reused * or # search. With ic scs on "Foo foo FOO", *:s//x/g replaces all three words in Neovim 0.12.4, but this implementation replaces only Foo. The same problem affects :g// and empty search addresses: on ["Foo", "foo", "FOO"], *gg://d deletes foo in Neovim but selects Foo here. The highlight compilation also loses this case policy.
  fix: Preserve the reused pattern's smartcase exemption when resolving empty patterns and search repeats, including subsequent substitute repeats and highlights; explicit new patterns should still apply smartcase. Add recorded cases following uppercase * and # searches for substitution, global commands, addresses, and repeats.
  instances: crates/fe-vim/src/substitute.rs:430, crates/fe-vim/src/substitute.rs:466, crates/fe-vim/src/ex/global.rs:313, crates/fe-vim/src/ex.rs:1560
