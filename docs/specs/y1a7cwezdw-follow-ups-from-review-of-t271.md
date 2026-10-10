---
task_uid: "01m4h8bf1pcv0pq0y1a7cwezdw"
task: "T303"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T271

Findings of the review of T271 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-fileview/src/source.rs:134: Optional context reads still discard readable context when part of their range fails. Let B = CHUNK_BYTES and truncate a completed two-chunk file to B bytes. If the surviving chunk ends with é, char_at(B-2) returns '?' because reading B-1..B+2 fails and discards the readable continuation byte. If it ends with €, is_char_boundary(B-2) incorrectly returns true and clip_offset leaves that interior offset unchanged. A surviving CRLF similarly makes char_at(B-2) return '\r'. The preceding-context read has the same defect when the requested chunk is cached but the previous chunk is unavailable. This violates valid-character and boundary semantics for bytes that remain readable; the added ASCII-only test misses it.
  fix: Collect optional context incrementally from the requested range outward, retaining contiguous readable bytes and stopping at the first unavailable byte or chunk. Preserve error recording. Add truncation tests for surviving multibyte characters and CRLF, covering char_at, both clip biases, is_char_boundary, and partial-break ranges; also cover cached text with unavailable preceding context.
  instances: crates/fe-fileview/src/source.rs:134, crates/fe-fileview/src/source.rs:130
