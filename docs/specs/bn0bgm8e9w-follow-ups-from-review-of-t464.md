---
task_uid: "01m6qk53rzjsvqc3bn0bgm8e9w"
task: "T468"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T464

Findings of the review of T464 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106c), for helium batch 106:

- F1 medium crates/fe-vim/src/insert/keys.rs:638: Both new overwrite paths bypass prepared(), which enforces MAX_PUT across carets. Record a multiline plain register insertion, replace that register with more than 64 MiB of single-line text, then press dot: replay now overwrites without checking the limit and creates a restoration entry per character per caret, potentially exhausting memory. Counted replay also converts unchecked register contents directly into text pieces.
  fix: Validate the normalized register text against MAX_PUT across participating carets before either overwrite path runs, preserving the existing refusal behavior. Add regression coverage for oversized registers during dot and counted replay.
  instances: crates/fe-vim/src/insert/keys.rs:638, crates/fe-vim/src/insert/keys.rs:1528
