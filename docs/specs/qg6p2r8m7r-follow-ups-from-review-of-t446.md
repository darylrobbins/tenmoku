---
task_uid: "01m6pw5155da7avmqg6p2r8m7r"
task: "T457"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T446

Findings of the review of T446 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F1 medium crates/fe-vim/src/feature/exchange.rs:184: The end anchor can include unmarked rows after further edits. On "ab\nc\nd", `cxxli<CR><Esc>jddGcxx` deletes the split-off row, leaving the anchor at c's start; `last.max(row(end))` then includes c, producing "d\na\nc" instead of "d\nc\na". The deleted-boundary fallback also loses the o distinction: on "a\nb\nc\nd", `3cxxjjddkoX<Esc>Gcxx` includes the newly opened X row, producing "d\na\nb\nX" instead of "d\nX\na\nb".
  fix: Track the surviving last row through subsequent deletions and row openings, preserving whether the endpoint belongs to surviving marked text or the following row. Add regressions for both sequences.
  instances: crates/fe-vim/src/feature/exchange.rs:184, crates/fe-vim/src/feature/exchange.rs:185

- F2 medium crates/fe-vim/src/feature/exchange.rs:179: Equal anchor offsets do not establish that every marked row was deleted: an empty interior row surviving at EOF also has zero extent. On "z\na\n\nc", starting at (1,0), `3cxxddjddggcxx` deletes both boundaries but leaves the marked empty row. This condition clears that mark, leaving "z\n" instead of swapping it with z to produce "\nz".
  fix: Track surviving empty rows independently of character extent so collapsed anchors only clear a genuinely deleted region. Add a regression for deleting both boundaries around an empty interior row at EOF.
  instances: crates/fe-vim/src/feature/exchange.rs:179
