---
task_uid: "01m5x3axztrdwxje7ytt78jhf9"
task: "T1198"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T448

Findings of the review of T448 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/prose.rs:376: The new shortcut treats every punctuation-terminated row end like the end of the buffer and ignores count and around. On "Ab.\nCd. Ef.", v$o$isd now deletes only the period, leaving "Ab\nCd. Ef."; Neovim 0.12.4 deletes the first sentence, leaving "\nCd. Ef.". Counted selections also incorrectly stop at that period instead of extending into subsequent sentences.
  fix: Restrict the shortcut to the buffer-end case where it matches Neovim, preserving normal initial-selection behavior elsewhere. Add recorded selections and deletes on non-final rows for is/as and counts greater than one.
  instances: crates/fe-vim/src/prose.rs:376
