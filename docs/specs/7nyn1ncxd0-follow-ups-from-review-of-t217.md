---
task_uid: "01m4hkjrygvy30nm7nyn1ncxd0"
task: "T308"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T217

Findings of the review of T217 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-vim/src/ex.rs:319: Filename escaping remains incorrect. The decoder drops a backslash without consuming its escaped character, so four consecutive backslashes become one instead of two, and two backslashes before a space lose the literal backslash entirely. It also preserves backslashes before ordinary characters: the new test expects a\b\c, whereas a Neovim black-box check returns a\bc. These paths cause the host to open the wrong file.
  fix: Decode escape sequences while consuming the escaped character, match Neovim's filename behavior, correct the erroneous test expectation, and cover consecutive backslashes and backslashes adjacent to escaped spaces across edit abbreviations and force forms.
  instances: crates/fe-vim/src/ex.rs:319, crates/fe-vim/src/tests/ex_host.rs:26
