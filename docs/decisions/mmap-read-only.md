# Memory-mapped read-only mode for very large files

Status: **build**, threshold 512 MiB, `pread`-backed, read-only with
"convert to editable".
Decided 2026-10-03. Source card T-18.

## Context

Every file becomes a rope: `Buffer::from_reader` → `fe_text::Text::from_reader`.
That is fine at 100 MB (PERF.md: 43 ms, 1.46× the file) but scales linearly,
so a multi-GB log costs seconds and several GB of RAM before it can be
browsed. For this product browsing matters more than editing at that size.
`memmap2` 0.9 (MIT/Apache-2.0) is already in `Cargo.lock` through GPUI.

## Measurements

M3 Max, 36 GB RAM, release builds, warm page cache, one run per cell (the
numbers are for ordering and scale, not precision). Files are the
`load_memory` example's line (`    let value = compute_something(…)?; // é 😀`)
repeated to size; about 87 bytes/line, ASCII plus a few multibyte chars.
The throwaway prototype (not committed) lived outside the repo and used
`fe_buffer::Buffer::from_reader` for the rope and `memmap2::Mmap::map` for the
mapping. RSS is `ps -o rss` growth over process start.

| File | Rope load | Rope RSS | mmap map + first screen | mmap RSS | mmap + full line index | Index RSS |
|---|---|---|---|---|---|---|
| 100 MB | 75 ms | 151 MB (1.4×) | 0.3 ms | 0.2 MB | 36 ms | 105 MB |
| 1 GB | 989 ms | 1.54 GB (1.4×) | 0.4 ms | 0.1 MB | 370 ms | 1.08 GB |
| 4 GB | 6.5 s | 5.84 GB (1.4×) | 0.05 ms | 0.1 MB | 7.8 s | 4.30 GB |

- The "line index" column is a naive single-threaded scan that records every
  64th line start. It touches every page, so mapped pages become resident
  (macOS counts them in RSS until reclaimed; they are clean and reclaimable,
  unlike rope memory). The point it makes is that **an eager full scan is no
  better than the rope**. A mapped view only wins if the index is built lazily
  or in the background (the first screen needs no index) and the view is
  allowed to drop pages.
- 4 GB: 12 GB free disk was the limit on this machine; the rope held 5.8 GB.
- Random access after mapping: 1000 random 4 KiB page touches took 10–15 µs
  warm (100 MB, 1 GB) and 0.8 ms at 4 GB, which on a warm cache is
  unmeasurably cheap. Cold-cache reads are bounded by disk, not measured.
- 512 MiB was not measured; it sits between the 100 MB and 1 GB rows
  (~0.5 s load, ~750 MB by linear interpolation).

## Surface a mapped view must provide

`fe-display` (`display_map`, `wrap_map`), `fe-syntax` (`syntax`, `driver`) and
`fe-editor` take `fe_text::Text` or `BufferSnapshot`, which are rope types.
A read-only source needs a trait that is the read subset of `Text`:

- `len`, `max_point`/line count, `line_len(row)`;
- point ↔ offset conversion (needs the line index);
- `chunks_in_range(range)` yielding `&str` (see below);
- cheap clone/snapshot (an `Arc<Mmap>`).

Complications:

- **UTF-8 validity.** The rope decodes lossily on load; a mapping holds raw
  bytes. The view must validate or lossily decode per chunk, and split on
  char boundaries. Files that are not valid UTF-8 are the common case for
  logs; they should open as lossy per chunk rather than be refused.
- **Syntax.** `MAX_PARSE_BYTES` is 8 MiB, so no mapped file is ever parsed;
  `fe-syntax` only has to accept "no highlighting". No work needed there
  beyond not requiring a `Text`.
- **Wrap and folds.** `wrap_map` over 50 M lines must be lazy (viewport
  only) or disabled in this mode. Soft wrap off by default is acceptable.
- **Line endings.** Detect from the first chunk; CRLF handled at line-split.
- **Editor.** Selection, search and copy work against the trait; every
  mutating command is disabled and shows a "read-only, convert to edit"
  affordance.

This is the bulk of the cost: it touches the rope-typed signatures in three
crates, which is why this is a card of its own rather than a patch.

## Safety: another process changes the file

A mapped file that is truncated by another process raises **SIGBUS** when a
page past the new end is touched. Confirmed on this machine: mapping a
100 MB file, truncating it to 4 KiB, then reading the last byte kills the
process (exit 138 = 128 + SIGBUS). In-place rewrites do not fault, but `memmap2`'s safety contract makes
modifying a mapped file undefined behaviour, so the view could show torn or
inconsistent text and Rust's guarantees about the `&[u8]` no longer hold.
Log rotation (`truncate`, `copytruncate`) and appenders are the likely
triggers in this product, and we cannot stop other processes writing.

Options considered:

1. **Copy-on-open under a threshold.** Already the rope behaviour; it is why
   the file-backed path only exists above the threshold.
2. **Advisory lock** (`flock`). Does not bind writers that do not take it.
   Rejected.
3. **Metadata check** (size, mtime, inode) plus a file watcher. Detects that
   a file went stale and drives a "file changed, reload" banner. It does
   **not** establish memory safety: the file can change between the check
   and the read, so it is a UX signal only.
4. **Signal handler** that recovers from SIGBUS. Unsound across Rust frames.
   Rejected.
5. **`pread` into owned buffers** (a bounded chunk cache). Reads return
   copies, so there is no shared mutable memory, no SIGBUS and no UB; a
   truncated file just returns a short read or EOF, which the view reports
   as "file changed". Loses only zero-copy chunks.

**Decision:** externally mutable files must use **`pread` with owned
buffers**. `memmap2` is acceptable only behind an enforceable immutable
backing (none exists for ordinary files on macOS), so it is not shipped; it
served as the measurement baseline above because mapping and a `pread` cache
have the same cost profile for the first screen and browsing. Metadata checks
and the watcher are still added, as staleness detection only. The follow-up
must confirm the `pread` cache matches the first-screen and RSS numbers in the
table.

## First edit: view → rope

Converting runs `Buffer::from_reader` over the source. The table is from the
mapping prototype; a `pread` source adds read-syscall overhead to confirm:

| File | Convert | RSS after |
|---|---|---|
| 100 MB | 55 ms | 257 MB (mapped pages + rope) |
| 1 GB | 544 ms | 2.6 GB |
| 4 GB | 8.4 s | 8.0 GB |

The first edit itself on the rope is ~15–25 µs. The cost is the conversion:
the same as opening as a rope plus the resident mapped pages, which are
released when the map is dropped. Seconds-long on multi-GB files, so the
conversion must be explicit ("Edit this file", with a size warning above
~1 GB), run off the UI thread, and refuse if the file changed on disk. It reads through the same `pread`
path (not the mapping), so it has the same safety as browsing.

## Decision

**Build.** Files at or above **512 MiB** open as a read-only `pread`-backed view; smaller files keep the rope path unchanged. Editing
larger files is out of scope: read-only plus explicit convert-to-editable is
enough (answering the open question), consistent with browsing mattering
more than editing here.

Why 512 MiB: below it the rope loads in about half a second and ≈1.4× the
file, inside the M2 targets; at 1 GB the rope costs ~1 s and 1.5 GB, at 4 GB
6.5 s and 5.8 GB, while the view's first screen costs well under a
millisecond and almost no resident memory.

What would change the decision:

- Moving to **defer** if the trait work in `fe-display`/`fe-editor` turns out
  to cost more than a rope fast path (e.g. a faster loader that brings 4 GB
  under ~2 s).
- Moving the threshold if the real workload is mostly 200–500 MB files.
- If the `pread` cache cannot match the mapping's first-screen and memory
  numbers, revisit; `memmap2` stays off the table for mutable files.
- **Reject** if users turn out to need editing at that size; then the effort
  goes to a piece-table/rope on disk instead.

## Follow-up card (draft)

> **Read-only view for files ≥ 512 MiB**
>
> Introduce a read-only text source trait (len, line count, point↔offset,
> `chunks_in_range`) implemented by the rope and by a file-backed view
> (`pread` buffer cache; no `memmap2`).
> Build the line index lazily on a background thread; show the first screen
> before it exists. Lossy UTF-8 per chunk. Check size/mtime/inode and a watcher
> event to flag staleness (not for safety); show a "file changed, reload" banner.
> Disable mutating commands and highlighting. "Edit this file" converts to a
> rope off-thread with a size warning. Acceptance: 4 GB file shows its first
> screen in < 100 ms with < 100 MB resident before scrolling; truncating or rewriting the
> file underneath never crashes the editor or yields UB (short reads become
> a reload banner) in a 1000-iteration stress test.
