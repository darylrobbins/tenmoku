---
task_uid: "01m54670jz8tgewd7mf89z6gq1"
task: "T136"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-text test utf16_without_bom::utf16_without_a_bom_is_read_by_its_zero_bytes passes: \"hello\\r\\nworld\\r\\n\" encoded as UTF-16LE and as UTF-16BE with no BOM reads (Text::from_reader_detecting) as that encoding with bom false, text \"hello\\nworld\\n\", CrLf and nothing replaced; FileEncoding::detect agrees; write_encoded writes the same bytes back with no BOM added; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-text test utf16_without_bom::zero_bytes_out_of_the_utf16_pattern_stay_utf8 passes: \"a\\0\\0b\" (zeros at both an even and an odd index) stays UTF-8 with its text unchanged, and plain ASCII stays FileEncoding::UTF8"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-text -p fe-buffer passes with no existing test edited; cargo clippy -p fe-text --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Read a UTF-16 file without a byte order mark as UTF-16LE or UTF-16BE from its zero bytes in the first 512 bytes, as VS Code does, instead of as UTF-8 with NULs (Mac T742)

## Problem

A UTF-16 file with no byte order mark opens as UTF-8, with a NUL between every char.
`fe_text::encoding::detect_head` (crates/fe-text/src/encoding.rs:233) checks the BOM, then whether
the first 64 KiB are valid UTF-8 (`starts_with_utf8`). NUL bytes are valid UTF-8, so mostly-ASCII
UTF-16 always passes and `chardetng` never runs. `Buffer::from_reader_detecting` and
`FileEncoding::detect` both use it.

Reproduced at Mac main 9328d2ca. crates/fe-text is byte-identical at mac/main de87d2b5. The bytes are
`"hello\r\nworld\r\n"` encoded as UTF-16LE with no BOM (`68 00 65 00 …`):
- expected: `FileEncoding { UTF-16LE, bom: false }`, text `"hello\nworld\n"`, CrLf, and a save
  writes the same bytes back;
- actual: `FileEncoding { UTF-8, bom: false }`, text `"h\0e\0l\0l\0o\0\0\n\0w\0…"`.
UTF-16BE fails the same way.

VS Code's `detectEncodingFromBuffer` (workbench textfile `encoding.ts`) always looks at zero bytes in
the first 512 bytes after the BOM check. If the window holds a zero byte, every odd byte is zero and
no even one is, the file is UTF-16LE. The reverse pattern is UTF-16BE. Otherwise the file is left to
the other checks. Windows tools (PowerShell 5 `Out-File`, `reg export`, some MSBuild/VS logs) write
such files.

## Change

In crates/fe-text/src/encoding.rs, add `fn utf16_by_zero_bytes(head: &[u8]) -> Option<&'static Encoding>`
and a `ZERO_BYTE_SNIFF_LEN: usize = 512` window. If the window holds a zero byte and each byte's
zero-ness matches the LE pattern (zero exactly at odd indices), return `UTF_16LE`. For the BE pattern
(zero exactly at even indices), return `UTF_16BE`. Otherwise return `None`. In `detect_head`, call it
after `Encoding::for_bom` and before `starts_with_utf8`, giving `FileEncoding { encoding, bom: false }`
with BOM length 0. The existing non-UTF-8 decode path and `write_encoded`'s `Sink::Utf16` already read
and write it. Bytes with zeros outside both patterns keep today's behaviour (UTF-8 when valid).
Update the module doc, `FileEncoding::detect`'s doc and `Text::from_reader_detecting`'s numbered list
to name the new step. No other crate changes. fe-nav's extract and the project search still skip
files with a NUL as binary, which is ripgrep's behaviour too.
