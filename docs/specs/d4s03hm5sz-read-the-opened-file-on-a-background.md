---
task_uid: "01m417z3q3d0dhjkd4s03hm5sz"
task: "T5"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fastedit test loader::tests::reads_file_into_buffer_off_the_calling_thread passes"
    required: true
    manual: false
  - id: "C2"
    text: "fastedit tests loader::tests::missing_file_is_a_new_buffer and loader::tests::unreadable_file_is_an_error pass"
    required: true
    manual: false
  - id: "C3"
    text: "fastedit --perf on a 100 MB file reports the first frame painted before 'file shown'; both times for 33 MB and 100 MB are recorded in PERF.md (manual)"
    required: true
    manual: false
  - id: "C4"
    text: "Peak resident memory while loading 100 MB is measured and recorded in PERF.md, under 1.5x the file size (manual)"
    required: true
    manual: false
---

# Read the opened file on a background thread so the window paints first

## Problem

`main` loads the whole file before the app starts
(`crates/fastedit/src/main.rs`: `load(path)` runs before
`gpui_platform::application().run`), so first paint waits on the read. The
targets are: a 100 MB file interactive in under 500 ms, and cold start to first
paint under 150 ms. PERF.md (2026-10-02, m2-bench) has 100 MB at 201 ms to the
first frame, of which the load is 43 ms, and warm launches paint at 180–200 ms.
The 500 ms target holds today, but every byte read delays the first frame:
roughly 0.43 ms per MB, so a 1 GB log blocks for about 0.45 s before anything
shows.

## Change

Open the window at once and read the file on a background thread.

- New module `crates/fastedit/src/loader.rs`:
  `fn read(path: PathBuf) -> io::Result<Option<Buffer>>`. It wraps
  `Buffer::from_reader(BufReader::with_capacity(256 * 1024, file))`, the same
  code path as `EditorModel::open`, and returns `Ok(None)` for `NotFound`
  (a new file). `Buffer` is `Send`; `EditorModel` isn't (it holds
  `Box<dyn CharWidth>`), so only the buffer crosses threads. The model is built
  on the main thread with `EditorModel::new(buffer, Some(path))`.
- `Workbench` holds either `Loading { path }` or `Ready { editor }`. While
  loading, the editor area paints the theme background with
  "Loading <file name>…" and the title reads "<name> — loading". `main` spawns
  the read with `cx.background_spawn` inside `application().run`. When it
  finishes, it creates the `EditorView`, focuses it and notifies.
- If the read fails (other than `NotFound`), keep today's behaviour: log the
  error with `log::error!` and quit with exit code 1.
- The whole file is shown when it's ready. Progressive display is not part of
  this card.
- `--perf` (`crates/fastedit/src/perf.rs`) reports two times: "first frame"
  (the loading frame) and "file shown" (the first frame painted with the
  editor). Start the scroll phases only after "file shown".
- Measure peak resident memory while loading a 100 MB file: sample `ps -o rss=`
  as `perf.rs`'s `print_memory` does, at "file shown" and at its peak during the
  read. The streaming reader plus the rope must stay under 1.5× the file size.
  Record both numbers, and the first-frame and file-shown times for 33 MB and
  100 MB files, in PERF.md's "M2 large-file targets" table.

## Notes

- T-29 (startup and fixed overhead) is likely to edit `main.rs` and PERF.md
  too; order the two deliberately. T-13 (untitled buffers) may touch `main.rs`.
- T-10, which the source card was blocked by, is done.
- Encodings (`encoding_rs`) are out of scope; they come later, with M6.

## Implementation notes

- `loader::read` runs on `cx.background_spawn`; the window's first frame is
  the "Loading <file>…" placeholder.
- `open_window` draws a frame but presents it only at the first frame
  request, so `--perf` times the first frame at presentation: its first
  `on_next_frame` callback runs at the start of that request, and if the
  workbench is still loading then, the placeholder is what gets presented.
  If the read won, the editor's first frame is both "first frame" and "file
  shown" (`perf::open_milestones`, unit-tested for both cases).
- `print_memory` now also prints the resident set and its peak (`ru_maxrss`);
  the lifetime peaks at "file shown" cover the background read.
- Measured (PERF.md, "Background loading"): placeholder presented at
  201–258 ms for any size; file shown at 297–339 ms (33 MB) and 271–310 ms
  (100 MB); memory over an empty editor 1.45–1.48×, and the read peaks no
  higher than the finished rope.
- Trade-off: small files now appear 60–90 ms after the placeholder instead
  of on the first frame, because the main thread is busy for 30–80 ms after
  the window opens before it picks up the read's result. A possible
  follow-up: start the read before `application().run` and show the editor
  on the first frame when the read has already finished.
