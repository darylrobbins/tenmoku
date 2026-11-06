---
task_uid: "01m6qajsy92176yykepf2h6q3t"
task: "T465"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T457

Findings of the review of T457 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106a), for helium batch 105:

- F1 medium crates/fe-vim/src/feature/exchange.rs:242: Requiring the end anchor to remain at a row end drops surviving split-off rows after text is appended. On "ab\nc", starting at (0,0), `cxxli<CR><Esc>AX<Esc>Gcxx` leaves the anchor before X inside bX, so only a is exchanged, producing "c\nbX\na" instead of "c\na\nbX". This regresses the previous behavior.
  fix: Track whether the endpoint still belongs to surviving marked text independently of its current column, and add a regression covering appending text to a split-off row.
  instances: crates/fe-vim/src/feature/exchange.rs:242

- F2 medium crates/fe-vim/src/feature/exchange.rs:245: The frozen lead anchor excludes rows inserted inside the marked region before its last boundary is deleted. On "a\nb\nc\nd", starting at (0,0), `3cxxjoX<Esc>jddGcxx` inserts X between marked b and c, then deletes c. The lead still resolves to b's end, so the exchange produces "d\nX\na\nb" instead of "d\na\nb\nX". Opening X above c with `3cxxjjOX<Esc>jddGcxx` has the same regression.
  fix: Update the surviving endpoint through edits so rows inserted inside the live boundaries remain marked after boundary deletion, while rows opened outside the surviving region stay excluded; add regressions for both opening sequences.
  instances: crates/fe-vim/src/feature/exchange.rs:245
