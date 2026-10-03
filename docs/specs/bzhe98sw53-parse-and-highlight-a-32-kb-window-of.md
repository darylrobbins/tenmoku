---
task_uid: "01m417z4b0m92mgwbzhe98sw53"
task: "T22"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax: in a buffer over MAX_PARSE_BYTES, highlights in a window starting at an item boundary equal a full parse of the window's text, shifted to buffer offsets (test syntax::window_tests::a_window_highlights_like_a_parse_of_its_text)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: a window job covers at most WINDOW_BYTES, starts at a row start and ends on a char boundary, including on a 9 MB single-line file; under the limit window_job returns None (test syntax::window_tests::window_parses_are_bounded)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax: an edit keeps the window's highlights outside the edited text until a newer window lands, and an older window result never replaces a newer one (test syntax::window_tests::the_window_follows_edits_until_replaced)"
    required: true
    manual: false
---

# Parse and highlight a 32 KB window of files over the parse limit

Source: Paddock T-16 (`c9403034-4af6-4293-9707-993a834ed9c9`), P2, draft (needs the user's approval).
Split from T-16 along the risk line: this is the GPUI-free window parse and highlight in `Syntax`;
scheduling it off the UI thread while scrolling is T-16b.

## Problem

Files over `MAX_PARSE_BYTES` (8 MB since `bec5dd8`) show as plain text: `Syntax::edit` drops the tree
and `needs_parse` is false. The plan says files above the limit should highlight the viewport only.
A full tree can't be the answer: PERF.md measures trees at 29–96× the source (Rust 40×, JSON 30×,
Markdown 71–85×), against a plan target of ≤ 1.5× file size.

## Change

In `crates/fe-syntax/src/syntax.rs`:

- `const WINDOW_BYTES: usize = 32 * 1024`. At the worst measured ratio (96×) a window's tree stays
  near 3 MB, under 0.5× of the smallest file it applies to (8 MB).
- `Syntax::window_job(&self, visible: Range<usize>) -> Option<ParseJob>`, `None` unless the snapshot
  is over `MAX_PARSE_BYTES` and the buffer hasn't failed. The window is `WINDOW_BYTES` centred on
  `visible`, clipped to the text, its start moved forward to a row start and its end back to a char
  boundary. The job parses with `Parser::set_included_ranges(&[window])` over the whole rope, so node
  offsets are absolute and `Syntax::query`'s predicates and `flat_paths` work unchanged. A window
  job never has an old tree.
- `Parsed` remembers its window. `Syntax::finish` installs a window result as
  `window: Option<(Range<usize>, Tree)>`, separate from `tree`, under the same staleness rules as
  today (same buffer, not older than the current window's parse). An older window result never
  replaces a newer one.
- `Syntax::edit` edits the window tree forward with `input_edit` like the main tree and shifts its
  range (`Tree::edit` also shifts included ranges; check this in a test). The window stays until a
  newer one replaces it, so spans don't flicker while the next parse runs.
- `Syntax::highlights(range)` falls back to the window tree when there's no full tree, querying only
  `range ∩ window` and caching as today. `Syntax::window(&self) -> Option<Range<usize>>` exposes
  the current window so a scheduler can tell whether it still covers the viewport.
- A panic in a window parse or query fails the buffer to plain text, as for full trees.

Results are approximate on purpose: a string or comment opened above the window highlights wrongly.
That's acceptable for logs, data dumps and generated code.

Tests go in a new module declared from `syntax.rs` as `#[cfg(test)] mod window_tests;`
(`crates/fe-syntax/src/syntax/window_tests.rs`). Build over-limit buffers by repeating a Rust item
past `MAX_PARSE_BYTES`; building the rope takes milliseconds and only the 32 KB window is parsed.

## Not here

Scheduling, debouncing and the GPUI side (T-16b). Never spawn threads or read clocks in this card.

## Implementation notes

- The window's range is read back from the tree's included ranges after each `Tree::edit`, so it
  can't drift from what tree-sitter shifted. `the_window_follows_edits_until_replaced` checks both
  agree with the expected shift.
- Staleness: a window result is used if it's for the same buffer, no newer than the snapshot, the
  buffer is still over the limit, and its version is ≥ the current window's. Equal versions replace
  (the view scrolled without editing); an older version never does. Two window jobs at the same
  version finishing out of order can't be told apart, so T-16b should keep one in flight at a time.
- A full tree landing drops the window. Going back under the limit keeps the window until then, so
  spans don't flicker.
- When moving the start forward to a row start would pass the window's end (the middle of a line
  longer than the window, such as minified JSON), `window_job` returns `None`: only the start of a
  very long line highlights. Starting mid-line instead would be a one-line change if wanted.
- `WINDOW_BYTES` is private to `syntax.rs`; `lib.rs` was out of scope. Export it in T-16b if the
  scheduler needs it.
- Injected layers are painted from the window tree as from a full tree.
