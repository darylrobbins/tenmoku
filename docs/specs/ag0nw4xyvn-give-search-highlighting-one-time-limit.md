---
task_uid: "01m4ny2zsk5p3vt1ag0nw4xyvn"
task: "T363"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test element::highlight_limit_tests::one_limit_per_frame: with the test limit at 100 ms and vim's highlight a.*z\\|a over 20 displayed rows of 200,000 a (soft wrap off), one draw returns within 1 s and paints at least one match, all on the first displayed row"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test element::highlight_limit_tests::ran_out_latches_until_a_change: after that draw, a second draw with nothing changed searches no row (FOUND is 0) and paints no match; after typing one character the next draw searches again (FOUND \u003e 0); and a new vim search for a on the same buffer paints matches"
    required: true
    manual: false
  - id: "C3"
    text: "Every earlier fe-gpui and fe-vim test passes with its files unchanged (vim::search_tests and view::find_tests included); matches_in keeps its signature and behaviour; view/find.rs's count calls matches_in_until with Deadline::never() (quoted in the decision file); scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Give search highlighting one time limit per painted frame, and stop spending it again until the pattern or the text changes

## Problem

T310 stops `:s` and vim's searches at a wall-clock limit (`fe_vim::search::time_limit()`, 2 s by default).
`matches_in` makes its own deadline on each call and returns the matches it has once the deadline passes.
Search highlighting calls it from `match_rects` (`crates/fe-gpui/src/element.rs`, line 1242) once for each
displayed buffer row. That covers both the find bar's pattern and vim's highlight. So:

- one frame with an adversarial pattern (T310's `a.*z\|a` on long rows of `a`) can spend up to the limit on
  each displayed row: 2 s × 40 rows is 80 s on the UI thread;
- every later frame spends it again, because GPUI repaints on each key, scroll or mouse move. With such a
  pattern on screen, every keystroke takes the limit or more;
- find's off-thread count (`count` in `crates/fe-gpui/src/view/find.rs`) calls `matches_in` per chunk of rows.
  It now stops each chunk at the limit and reports a short count as exact. That count already has its own
  `stop` flag and runs off the UI thread, so it needs no limit.

**Decided** (user, 2026-10-11): search highlighting gets the same time limit as `:s` and searches (T310's
follow-up).

## Change

**fe-vim** (`crates/fe-vim/src/search.rs`):

- Make T310's `Deadline` public: `Deadline::new()` (now plus `time_limit()`), `Deadline::after(Duration)`,
  `Deadline::never()`, and `ran_out(&self) -> bool`.
- Add `pub fn matches_in_until(text, rows, pattern, cap, deadline: &mut Deadline) -> Vec<Range<usize>>`.
  `matches_in` becomes `matches_in_until(.., &mut Deadline::new())`, so its behaviour doesn't change.
- Add `Pattern::regex_str(&self) -> &str`, the translated regex's source, to key the latch below.

**fe-gpui** (`crates/fe-gpui/src/element.rs`):

- `match_rects` makes one `Deadline::after(limit)` per frame, where `limit` is `fe_vim::search::time_limit()`,
  and passes it to `matches_in_until` for every row. Once it runs out, the frame paints the matches found so
  far and searches no more rows.
- **The latch.** `FrameLayout` gains `highlight_ran_out: Option<(String, Version)>`: the pattern's `regex_str`
  and the text's snapshot version for a frame whose highlight ran out. Each frame takes it over from the
  view's last layout (`view.layout`). While the pattern and the version both match, `match_rects` searches
  nothing and paints no matches. Any edit, or a different pattern (a new search, a find query change), clears
  it. Without the latch, each keystroke would spend the limit again.
- For tests only, a `#[cfg(test)]` thread-local in `element.rs` overrides `limit`, so a test doesn't change the
  process-wide limit other fe-gpui tests run under.
- Update `match_rects`' doc comment.

**find's count** (`crates/fe-gpui/src/view/find.rs` `count`): call `matches_in_until(.., &mut
Deadline::never())`. It runs off the UI thread with its own `stop` flag, and T310's limit shouldn't cut it
short.

## Tests (new file)

`crates/fe-gpui/src/element/highlight_limit_tests.rs`, declared from `element.rs` with one
`#[cfg(test)] mod highlight_limit_tests;` line beside `mod tests;`. Set the thread-local limit, not
`set_time_limit`.

## Out of scope

- The `vim.regex_time_limit` setting (its own card); this card reads `time_limit()`, so the setting governs
  it.
- T320's per-window budget for dense rows (it edits the same loop; whichever lands second rebases).
- nvim's syntax-highlighting `redrawtime` behaviour, or showing a message when highlighting stops.
