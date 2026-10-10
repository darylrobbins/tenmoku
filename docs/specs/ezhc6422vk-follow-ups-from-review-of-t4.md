---
task_uid: "01m4kt1kevzfqzchezhc6422vk"
task: "T11"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T4

Findings of the review of T4 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-nav/src/references.rs:772: Decoded lengths are not additive when a hit boundary splits a multibyte legacy character. For example, in a detected Shift_JIS file, a line containing bytes `\xC2\x81target target` produces two UTF-8 byte-search hits: the first follows the non-word control U+0081. Decoding the prefix independently flushes the pending Shift_JIS lead byte as a replacement, so the cursor accumulates one extra byte. Although the first hit fails validation, the corrupted cursor persists and the later real `target` also fails location validation, leaving an incorrect raw-byte navigation range. The previous full-prefix mapping located the later hit correctly.
  fix: Preserve decoder state while processing sorted boundaries, or build a reusable mapping that handles boundaries inside encoded characters without accumulating replacement-length errors. Add a multibyte-encoding regression with a rejected hit followed by a valid hit on the same line.
  instances: crates/fe-nav/src/references.rs:772
