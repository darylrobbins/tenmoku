---
task_uid: "01m4saeqpntshb28fndzwxd7sq"
task: "T99"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-text/benches/text.rs has the open, save and lookup groups the spec lists, over a ~100 MB / 1M-line corpus, and 'cargo bench -p fe-text -- --test' runs each bench once and passes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-text passes and cargo clippy -p fe-text --benches -- -D warnings is clean"
    required: true
    manual: false
  - id: "C3"
    text: "One cargo bench -p fe-text run's medians, with the machine and load, are recorded in the lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
  - id: "C4"
    text: "git diff against the base touches only crates/fe-text/Cargo.toml, crates/fe-text/benches/text.rs, Cargo.lock and the lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Add criterion benches for fe-text: open a 100 MB file through the streaming and detecting decoders, save it back encoded, and row and UTF-16 lookups in a 1M-row Text (Mac T581)

## Problem

The plan's Verification section asks for "criterion for rope, buffer, syntax, finder and search", and its large-file
budget is "a 100 MB / 1M-line file is interactive in under 500 ms". fe-buffer's bench (`crates/fe-buffer/benches/buffer.rs`)
times `Buffer::new(&str)` and `Buffer::from_file_contents(&str)` on a 100 MB string, but nothing times the path a
file actually opens through: fe-text's streaming decoders `Text::from_reader` and `Text::from_reader_detecting`
(`crates/fe-text/src/lib.rs` :117, `crates/fe-text/src/encoding.rs` :270), which fe-buffer, fe-editor and fe-nav call.
Saving (`Text::write_to`, lib.rs :337, and `Text::write_encoded`, encoding.rs :315) isn't timed either. fe-text has
no benches at all.

## Change

- `crates/fe-text/Cargo.toml`: `criterion.workspace = true` under `[dev-dependencies]` and a `[[bench]] name = "text"`,
  `harness = false`, as `crates/fe-buffer/Cargo.toml` does.
- New `crates/fe-text/benches/text.rs`, with a module doc giving the budgets and `cargo bench -p fe-text`, in the style
  of fe-buffer's bench. Corpus: the same about 100 MB / 1M-line code-like text with some non-ASCII as fe-buffer's
  `corpus()` (copy the function; benches can't share it). Groups, `sample_size(10)` for the 100 MB ones:
  - `open`: `Text::from_reader` over the UTF-8 bytes (an `&[u8]` reader); `Text::from_reader_detecting` over the UTF-8
    bytes, over the CRLF version, over a UTF-16LE encoding with BOM, and over a windows-1252 encoding of an ASCII plus
    Latin-1 corpus of the same size (the chardetng path).
  - `save`: `write_to` with LF and with CRLF into a reused `Vec<u8>`; `write_encoded` to UTF-16LE and to windows-1252.
  - `lookup` on the 1M-row `Text`: `row_start` and `row_len` at 1,000 random rows, `offset_to_point` and
    `point_to_offset`, `offset_to_point_utf16` and `point_utf16_to_offset` at 1,000 random char boundaries (seeded
    `StdRng`, as fe-buffer does).
- No production code changes. Don't touch fe-buffer's bench.

## Tests

`cargo bench -p fe-text -- --test` runs every bench once as a test. Record one `cargo bench -p fe-text` run's medians
(machine and load noted) in this lane's `docs/specs/<key>-*.md`; no budget is asserted.

## Implementation notes

One `cargo bench -p fe-text` run (release). Machine: Intel Xeon E-2386G @ 3.5 GHz, 12 CPUs, 62 GB RAM, Linux 7.0.
Load: shared box, load average 18-23 during the run, so these are noisy upper-ish figures. Medians:

| bench | median |
|---|---|
| open/from_reader 100MB | 158.44 ms |
| open/detecting utf-8 100MB | 164.97 ms |
| open/detecting utf-8 crlf 100MB | 222.69 ms |
| open/detecting utf-16le bom 100MB | 210.14 ms |
| open/detecting windows-1252 100MB | 229.53 ms |
| save/write_to lf 100MB | 24.301 ms |
| save/write_to crlf 100MB | 50.890 ms |
| save/write_encoded utf-16le 100MB | 413.76 ms |
| save/write_encoded windows-1252 100MB | 423.39 ms |
| lookup/row_start x1000 | 874.46 µs |
| lookup/row_len x1000 | 1.2089 ms |
| lookup/offset_to_point x1000 | 1.0908 ms |
| lookup/point_to_offset x1000 | 2.2228 ms |
| lookup/offset_to_point_utf16 x1000 | 1.4440 ms |
| lookup/point_utf16_to_offset x1000 | 2.9416 ms |

The windows-1252 corpus is ASCII plus Latin-1 (é, ü) rather than the emoji corpus, which the encoding can't hold.

The two offset_to_point medians were re-measured after black-boxing the whole returned point (same machine, load average ~20); the rest are from the first run.
