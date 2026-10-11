---
task_uid: "01m4nt2q7ya4mhd9cfqxmt5y1p"
task: "T37"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T35

Findings of the review of T35 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-text/src/encoding.rs:185: EOF recovery can reuse a finished ISO-2022-JP decoder and panic. For bytes b'\x1B$' with complete=true, EOF reports Malformed(1,1), then the replay call emits '$' and returns InputEmpty with last=true, which finishes the decoder. Because replaying is true, this condition continues the loop; the next buffer-size query panics with 'Must not use a decoder that has finished.' Search conversion can reach this when a match ends at EOF, since its end offset remains pending after the last unit.
  fix: Stop decoding whenever InputEmpty is returned with last=true, including replay calls. Add EOF regressions for truncated ISO-2022-JP escapes such as ESC '$' and ESC '(', checking full traversal and search matches ending at EOF.
  instances: crates/fe-text/src/encoding.rs:185
