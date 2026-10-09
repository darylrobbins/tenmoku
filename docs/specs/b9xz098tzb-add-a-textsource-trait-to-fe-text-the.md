---
task_uid: "01m4fksrj37xs4rhb9xz098tzb"
task: "T270"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-text test source::text_through_the_trait_matches_text: for proptest texts with multi-byte chars and empty rows, every TextSource method called through \u0026dyn TextSource equals Text's inherent method at every row, at every offset 0..=len with both biases, and on 200 random ranges; slice_to_string and row_to_string equal the concatenated chunks_in_range"
    required: true
    manual: false
  - id: "C2"
    text: "fe-text test source::shared_handle_is_send_sync: an Arc\u003cdyn TextSource\u003e from Text::share() moves to another thread and answers len() and row_count() there"
    required: true
    manual: false
  - id: "C3"
    text: "Outside the new files, crates/fe-text/src/lib.rs changes only by a mod line and a pub use line"
    required: true
    manual: false
---

# Add a TextSource trait to fe-text: the read subset of Text, object-safe and Send + Sync, implemented by Text

## Problem

`fe-display` (`display_map.rs`, `wrap_map.rs`, `long_row.rs`, `layout.rs`) reads text only as `fe_text::Text`, the
rope. T241's `fe-fileview` has no rope, so it can't be displayed. `docs/decisions/mmap-read-only.md` (T6),
"Surface a mapped view must provide", says: "A read-only source needs a trait that is the read subset of `Text`:
len, max_point/line count, line_len(row); point ↔ offset conversion; chunks_in_range(range) yielding &str; cheap
clone/snapshot." T241's spec puts that trait in fe-text ("the read-only text-source trait in fe-text, with
fe-display and fe-editor taking it").

fe-display calls these `Text` methods today: `len`, `row_count`, `row_start`, `row_len`, `clip_offset`,
`is_char_boundary`, `char_at`, `chunks_in_range`, `slice_to_string`, plus `clone` (`long_row.rs` `RowIndex` keeps
one).

## Change

Add `crates/fe-text/src/source.rs` with `pub trait TextSource: Send + Sync`. It must be object-safe, because
callers hold `&dyn TextSource` and `Arc<dyn TextSource>`. Export it from `crates/fe-text/src/lib.rs` (a `mod` line
and a `pub use` line only). Implement it for `Text` by forwarding to the inherent methods, which stay as they are.

Methods, all on `&self`, with `Text`'s meaning and units (byte offsets, `u32` rows, `Point`):

- `len() -> usize`, `row_count() -> u32`, `row_start(row) -> usize`, `row_len(row) -> u32` (excluding the line
  break);
- `offset_to_point(offset) -> Point`, `point_to_offset(point) -> usize`, and `max_point() -> Point`, provided from
  the other methods;
- `clip_offset(offset, Bias) -> usize`, `is_char_boundary(offset) -> bool`, `char_at(offset) -> Option<char>`;
- `chunks_in_range(range) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_>`, so the rope yields borrowed chunks and a
  file-backed source yields owned ones;
- `slice_to_string(range) -> String` and `row_to_string(row) -> String`, provided from `chunks_in_range`;
- `share() -> Arc<dyn TextSource>`, a cheap owned handle (for `Text`, a rope clone in an `Arc`). It replaces
  `RowIndex`'s `text.clone()`.

The trait's docs state the contract a second implementation must keep:

- A line break is one byte (`\n`) or two (`\r\n`). `Text` always uses one. So callers must use `row_start`/`row_len`
  and never assume `row_start(r + 1) == row_start(r) + row_len(r) + 1`.
- Offsets inside a two-byte break aren't char boundaries.
- Within one row, `slice_to_string(range).len() == range.len()`.
- Methods are infallible. A source that can fail, such as a file, answers from what it could read and reports the
  failure through its own API.

Nothing else changes in this card: no caller moves to the trait yet.

## Tests

In the new file `crates/fe-text/tests/source.rs`.
