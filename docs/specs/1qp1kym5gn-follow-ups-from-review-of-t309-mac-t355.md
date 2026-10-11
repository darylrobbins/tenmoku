---
task_uid: "01m4nqf221800dvw1qp1kym5gn"
task: "T35"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T309 (Mac T355)

Findings of the review of T309 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-text/src/encoding.rs:177: Malformed recovery incorrectly assigns consumed `extra` bytes to the replacement, although the decoder can reprocess those bytes. For GBK/GB18030 bytes b'\x81\x30 needle', the editor decodes '�0 needle', but a match for '0' at source range 1..2 maps to 0..3 instead of 3..4. The subsequent output also groups the recovered digit and space into one unit. More complex recovery can produce overlapping or reversed source ranges. The Shift_JIS tests never exercise nonzero `extra`.
  fix: Track reprocessed bytes separately from malformed bytes and split recovered output into its actual source units. Add GBK/GB18030 and ISO-2022-JP regressions covering nonzero `extra`, checking ordered source ranges, preview windows, and individual recovered-character matches.
  instances: crates/fe-text/src/encoding.rs:163, crates/fe-text/src/encoding.rs:177, crates/fe-text/src/encoding.rs:180

- F2 medium crates/fe-workbench/src/search_panel.rs:535: A newline-terminated line is finalized as though it ends the decoder stream, but the editor decodes continuously through the newline. These produce different malformed recovery for GBK/GB18030: b'needle\x81\x30\n' becomes 'needle�0\n' in the editor, while this conversion flushes b'needle\x81\x30' at EOF and previews 'needle�'. Matches touching the digit or line end consequently receive incorrect offsets. The added trailing-lead test covers actual file EOF only.
  fix: Distinguish newline termination from actual file EOF and decode enough of the real terminator to settle pending source units with the editor's recovery behavior, excluding the terminator from the preview. Add newline-terminated malformed GBK fixtures with digit and end-of-line matches.
  instances: crates/fe-workbench/src/search_panel.rs:535, crates/fe-text/src/encoding.rs:157
