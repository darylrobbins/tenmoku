---
task_uid: "01m4hpx42mhqh09bm46770eb5j"
task: "T309"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T289

Findings of the review of T289 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/search_panel.rs:493: The may_shift filter remains applied after detecting a legacy encoding. A short line whose legacy bytes happen to be valid UTF-8 therefore bypasses conversion. For example, in a detected windows-1252 file, b'\xC3\xA9 needle' previews as 'é needle' with range 3..9, while the editor loads 'Ã© needle' with the word at 5..11. This affects such lines even when another line triggers detection.
  fix: Once detection identifies a legacy encoding, process all matching lines whose decoded text or offsets can differ, rather than retaining the lossy-UTF-8 filter. Add a multiline windows-1252 regression containing a UTF-8-valid byte sequence.
  instances: crates/fe-workbench/src/search_panel.rs:493

- F2 medium crates/fe-workbench/src/search_panel.rs:700: Search previews have lossy-UTF-8 boundaries, which need not be legacy character boundaries. When a preview starts inside a Shift_JIS character, decoded[0] != decoded[1] causes the entire line to remain unconverted. For b'\x82\xA0' repeated 300 times followed by b' needle!!', the preview starts at byte 433 inside a character; the match stays at 601..607 although the editor's word is at 901..907. Similarly, independently decoding a preview ending inside a legacy character introduces a replacement character absent from the editor.
  fix: Derive preview text from the continuous decoding of the line and align or widen both window boundaries to complete legacy characters. Do not abandon range conversion because the original preview boundary splits a character. Add long Shift_JIS fixtures covering both window boundaries.
  instances: crates/fe-workbench/src/search_panel.rs:700, crates/fe-workbench/src/search_panel.rs:704, crates/fe-workbench/src/search_panel.rs:711

- F3 medium crates/fe-workbench/src/search_panel.rs:749: Decoder output growth does not reliably identify source character boundaries during malformed-input recovery, and the decoder is never flushed at the line's end. In Shift_JIS b'\x82 needle', feeding the space emits both the preceding replacement and the space, so a match starting at byte 1 maps to 0 instead of 3 and selects the replacement too. A trailing unmatched lead byte emits its replacement only on finalization, so matches touching it or an end-of-line match receive offsets before the editor's replacement character.
  fix: Track actual source decoding units through malformed-input recovery and finalize pending input at the true line end before resolving offsets. Add tests for a malformed lead followed by ASCII and a trailing incomplete multibyte character, including rounded ends and zero-width end matches.
  instances: crates/fe-workbench/src/search_panel.rs:749, crates/fe-workbench/src/search_panel.rs:754, crates/fe-workbench/src/search_panel.rs:766, crates/fe-workbench/src/search_panel.rs:774
