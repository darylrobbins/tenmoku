---
task_uid: "01m55b7990zdfsxgcwagzxkjqq"
task: "T818"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo bench -p fe-gpui --bench scroll runs smooth/1m-plain, jump/1m-plain, jump/7mb-rust and jump/1m-plain-wrapped, 600 frames each, headless with no screen, and prints p50, p99 and max of frame (dispatch to presented), prepaint and paint, and the frames over 8.33 ms"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-gpui/benches/keystroke/harness.rs is unchanged: scroll.rs includes it by #[path] and builds its cases by struct update from harness::cases()"
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md has a 'Scrolling, headless' subsection under M2 large-file targets from one Mac run (machine, commit, date, load) and says per case whether frame p99 \u003c= 8.33 ms holds; a miss is attributed to prepaint, paint or present with a follow-up named"
    required: true
    manual: false
  - id: "C4"
    text: "Cargo.lock gains no [[package]]; crates/fe-gpui/Cargo.toml only gains the [[bench]] entry; cargo bench -p fe-gpui --no-run builds"
    required: true
    manual: false
---

# Measure scrolling frame time headless on Metal: a fe-gpui scroll bench on 1M lines and 7 MiB of highlighted Rust against 120 fps

## Problem

M2's done-when is "1M-line file scrolls at 120 fps". The only measurement is `fastedit --perf`
(`crates/fastedit/src/perf.rs`: 600 smooth frames of 40 px, then 600 jump frames of ~1000 rows), which
needs a real, unlocked screen and a quiet machine, so it runs by hand (T408 is waiting for a person) and
can't run in a lane or in the bench workflow T405 adds. Its last numbers are from 2026-10-02, before soft
wrap, folds, the block layer, diagnostics, find highlights, the change bar and vim landed (PERF.md
"Scrolling, per frame"): jump-scroll prepaint was 2.4 / 3.8 ms (p50 / p99) on 1M plain lines and
6.5 / 11.6 ms on 16 MB of highlighted Rust, the latter already past a 120 Hz frame (8.33 ms).

The keystroke bench shows the way: `crates/fe-gpui/benches/keystroke/harness.rs` drives a focused
`EditorView` in a headless GPUI window with the real macOS text system and the headless Metal renderer,
and presents each frame with `Window::render_to_image`, with no screen.

## Change

New bench `crates/fe-gpui/benches/scroll.rs` (`[[bench]] name = "scroll"`, `harness = false`, in
`crates/fe-gpui/Cargo.toml`), a plain `main` like the keystroke bench's percentile printing.

- Reuse the keystroke harness without changing it: `#[path = "keystroke/harness.rs"] mod harness;`, build
  each case with struct update from an existing case (`Case { name, corpus, language, ..cases()[0] }`, so
  fields the harness gains later don't break it), open it with `Harness::new` and the headless renderer.
- Per frame: dispatch a `PlatformInput::ScrollWheel` (pixel delta, over the editor, as `perf.rs` does),
  let the window draw, `render_to_image`, and time dispatch -> presented; read prepaint and paint from
  `EditorView::frame_timings()`.
- Cases, 600 frames each: `smooth/1m-plain` (40 px a frame, 1M lines, 33 MB), `jump/1m-plain` (~1000 rows a
  frame), `jump/7mb-rust` (7 MiB of generated Rust, highlighted, just under Rust's 7.5 MiB parse limit, after
  its background parse has landed), and `jump/1m-plain-wrapped` (soft wrap on). A jump case wraps to the top
  when it reaches the end.
- Print per case: p50, p99 and max of the frame (dispatch to presented), prepaint and paint, and frames
  over 8.33 ms.

Budget: frame p99 at most 8.33 ms (120 fps) in every case. Run it on the Mac (release, private
`CARGO_TARGET_DIR`; no screen needed) and add a "Scrolling, headless (`cargo bench -p fe-gpui --bench
scroll`)" subsection under PERF.md's "M2 large-file targets", with machine, commit, date and load. For a
case over budget, attribute it to prepaint, paint or present and name a follow-up card; don't fix it here.
The numbers leave out vsync, the compositor and the GPU's own time, as the keystroke bench's do.
