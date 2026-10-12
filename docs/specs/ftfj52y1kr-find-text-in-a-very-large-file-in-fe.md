---
task_uid: "01m4s3rjkr3rd30mftfj52y1kr"
task: "T93"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-fileview test find::forward_and_backward: in a file over 2 FIND_BLOCKs, forward from an offset finds the next match and backward the previous one, a match straddling a block edge is found exactly once each way, and matches outside within are ignored"
    required: true
    manual: false
  - id: "C2"
    text: "fe-fileview test find::wrap_case_and_empty: past the last match forward (or the first backward) the search wraps and says wrapped; case-insensitive folds ASCII only; an empty needle or one holding a line break is NotFound; a needle longer than within is NotFound"
    required: true
    manual: false
  - id: "C3"
    text: "fe-fileview test find::stops_and_leaves_the_cache: with the stop flag set the search returns Stopped; a whole-file search on a fresh FileView leaves cached_bytes() at 0; a file truncated before the search is Err(ReadError::Changed)"
    required: true
    manual: false
---

# Find text in a very large file in fe-fileview: fe_fileview::find steps forward or back from an offset, wrapping, by raw bytes past the chunk cache, and stops when asked (Mac T568)

## Problem

Files of 512 MiB and more are browsed through fe-fileview's `FileView` (`docs/decisions/mmap-read-only.md`), and
the plan's first priority is the read path, large files included (`~/.claude/plans/glittery-foraging-graham.md`
"Large files"). A large log is mostly searched, but nothing can search a `FileView`: the editor's find
(`crates/fe-gpui/src/view/find.rs`) runs fe-vim's `Pattern` over a rope `Text`, which a viewer doesn't have, and
reading the file through `FileView::read` would push the whole file through the 16 MiB chunk cache and evict what
the screen is showing. `FileView::read_uncached` (`crates/fe-fileview/src/lib.rs` :248) already reads past the
cache for the line index's scan.

## Change

New module `crates/fe-fileview/src/find.rs`, GPUI-free like the rest of the crate, with no new dependencies:

- `pub struct FindQuery { pub needle: String, pub case_sensitive: bool }`. Case-insensitive matching folds ASCII
  letters only; other bytes match exactly. A needle that is empty or holds `\n` or `\r` never matches (a find bar
  is one line, and the viewer shows `\r\n` as a row break).
- `pub enum Direction { Forward, Backward }`.
- `pub enum FindOutcome { Found { range: Range<u64>, wrapped: bool }, NotFound, Stopped }`.
- `pub const FIND_BLOCK: u64 = 1 << 20;`
- `pub fn find(view: &FileView, query: &FindQuery, within: Range<u64>, from: u64, direction: Direction, stop:
  &AtomicBool) -> Result<FindOutcome, ReadError>`. Offsets are file byte offsets, as everywhere in the crate.
  `within` is clamped to `view.len()`, and only matches lying wholly inside it count (a host passes its snapshot's
  length while the index is building). Forward finds the first match starting at or after `from`, else wraps to the
  first one from `within.start` (`wrapped: true`). Backward finds the last match ending at or before `from`, else
  wraps to the last one before `within.end`. A match found without wrapping wins over a wrapped one, and the one
  match covering `from` is found on the wrap if nothing else is.
- It matches raw bytes, reading `FIND_BLOCK` bytes at a time with `read_uncached`, each block overlapping the last
  by `needle.len() - 1` bytes so a match across a block edge is found once. It never touches the chunk cache. It
  checks `stop` before each block and returns `Stopped` once it is set. A short read is `ReadError::Changed`, as for
  every other read.
- `crates/fe-fileview/src/lib.rs`: the `mod find;` line and `pub use find::{Direction, FIND_BLOCK, FindOutcome,
  FindQuery, find};`.

No UI here: a FileViewer find over this is the next card.

## Tests

New file `crates/fe-fileview/tests/find.rs`, temp files written by the test (a few MiB, not the 512 MiB threshold).
