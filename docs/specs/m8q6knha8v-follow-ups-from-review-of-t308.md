---
task_uid: "01m4mzvgy14dzpa7m8q6knha8v"
task: "T345"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T308

Findings of the review of T308 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/ex.rs:329: Filename escaping still disagrees with Neovim for trailing whitespace after an even number of backslashes. For example, `e a\\ ` (two backslashes followed by a space) produces a literal backslash but loses the trailing space; local Neovim preserves both. The parity check trims the space before the corrected decoder sees it, so these commands still open the wrong file. The same defect affects trailing tabs and every edit abbreviation and force form.
  fix: Match Neovim's trailing-whitespace handling by preserving a trailing blank immediately preceded by a backslash regardless of backslash parity. Add regression cases for even backslash runs before trailing spaces and tabs, including additional unescaped trailing blanks, across edit abbreviations and force forms.
  instances: crates/fe-vim/src/ex.rs:329
