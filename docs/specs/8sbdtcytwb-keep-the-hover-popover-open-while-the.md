---
task_uid: "01m57pcx48npa1q48sbdtcytwb"
task: "T193"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::hover_tests::reach::the_pointer_reaches_the_popover_and_stays_on_it passes: the popover stays while the pointer moves from the word onto it, rests on it 1 s, clicks in it (selections unchanged), or crosses other text to it within 100 ms; it hides 1 s after the pointer leaves both, and at once on escape with the pointer on it"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui unit test view::hover_tests::reach::the_wheel_scrolls_a_tall_popover passes: a 60-line hover is at most 16 line heights tall, and the wheel over it scrolls its content (hover-popover-body moves up inside hover-popover-content) without scrolling the editor or hiding it"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui passes, existing tests edited only to advance the clock past the hide grace before asserting a popover is gone; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Keep the hover popover open while the pointer travels to it and rests on it, and scroll its content with the wheel, as VS Code and Zed do (Mac T979)

Filed on argon from Mac T979 (card ~/coordination/argon-cards/zzw/T979.json rev 1; note zzw; user-reported).

Argon note: declare the new `hover_reach_tests.rs` from the existing test module `view/hover_tests.rs` (as `mod reach`, with `#[path]`), not from production code. rgt's regression check runs the regression tests at the base with test files carried over, so a `mod` line in production code would leave the base with 0 tests. `hover_tests.rs` is therefore also a test-support path.

## Problem

The user, 2026-10-18: "It's also not possible to move the mouse over the hover so you can't scroll or click
links. It goes away when you move towards the hover."

The hover popover (`crates/fe-gpui/src/hover.rs`) is unreachable:

- It is anchored at the pointer's x and the hovered row's bottom (`hover_diagnostics`, `hover.rs:360-367`), and
  drawn by `render_popover` (`:702`) inside `popover_layer`'s canvas (`:819`) as a plain `div` with no id, no
  hitbox and no `occlude()`. The window-wide `MouseMoveEvent` listener in `crates/fe-gpui/src/element.rs`
  (`:1329-1348`) asks `layout.text.is_hovered_at(position)`, which is still true under the popover, then calls
  `hover_annotation` / `hover_diagnostics` with the text position under it. That position is off the word the
  popover shows, so `hover_diagnostics` hides it at once (`:337`, `:359`). The first pixel the pointer moves
  below the hovered row hides the popover. `hover_after_line` (`:393`, `:405`) and `hover_annotation`
  (`:469`, `:476`) hide theirs the same way.
- It never scrolls: the popover has `max_w(560px)` but no maximum height and no scroll container, so a long
  hover (rust-analyzer's docs for a std item) runs off the window. The wheel over it reaches the editor's
  `on_scroll` (`crates/fe-gpui/src/view.rs:1207`), which scrolls the editor and hides the popover (`:1214`).

VS Code and Zed keep a hover open while the pointer travels to it (VS Code's `editor.hover.hidingDelay`,
300 ms) and while it is over it, scroll its content with the wheel, and hide it when the pointer has left both
the hovered text and the popover. Clickable links in the hover come with the Markdown chain (T955-T957); this
card makes the popover reachable so those links can be clicked once T957 lands.

## Change

All in `crates/fe-gpui/src/hover.rs` and `crates/fe-gpui/src/element.rs`.

1. The popover takes the pointer. `render_popover` gives the popover box `.id("hover-popover")`, `.occlude()`
   and `.debug_selector(|| "hover-popover".into())`, so the editor's text, its root `div`'s mouse listeners
   (`view.rs:1412-1423`) and the scroll wheel don't see a pointer over it. The popover's painted bounds are
   recorded each frame in the popover state (`self.popover`), and cleared when it isn't drawn.
2. Over the popover, it stays. The window-wide move listener in `element.rs` (`:1329`) first checks the
   pointer against the popover's recorded bounds (or hitbox, honouring occlusion as `is_hovered_at` does); over
   it, it calls neither `hover_annotation` nor `hover_diagnostics`, clears the editor's hover range
   (`hover_at(None, …)`), and cancels any pending hide (below). A mouse down, up or click inside the popover
   leaves the editor's selections and the popover as they are.
3. A grace period before hiding. New `pub(crate) const HOVER_HIDE_DELAY: Duration = 300 ms` (VS Code's
   `hidingDelay`). Where a *shown* popover the pointer found (diagnostic/provider, after-line, gutter
   annotation; not one shown at the caret, whose rules don't change) would be hidden because the pointer left
   what it shows, it instead starts a hide timer (a `Task` in the popover state, replaced by each new one):
   - the pointer reaching the popover, or coming back onto what it shows, before the timer fires cancels it;
   - when it fires, the popover hides and the pointer's last position is handled as a fresh move, so a word
     under a still pointer starts its own wait as today;
   - leaving the popover for anywhere but what it shows starts the same timer.

   Leaving the window, a scroll of the editor, an edit, the caret moving (for a caret popover), `escape`, and a
   popover going stale (`popover_stands`) still hide it at once, with the pointer anywhere, on the popover too.
   A popover that is only waiting (not yet shown) is dropped at once as today.
4. Scrolling. The popover's content sits in a vertical scroll container
   (`.id("hover-popover-content")`, `.overflow_y_scroll()`, `.debug_selector(|| "hover-popover-content".into())`)
   around the column of blocks (`.debug_selector(|| "hover-popover-body".into())`), with a `ScrollHandle` in
   the popover state reset to the top for each new popover. It is at most `MAX_POPOVER_ROWS = 16` editor line heights tall and never taller than the window less 2 × 8 px. The wheel
   over the popover scrolls that content, not the editor, and doesn't hide the popover. A popover shorter than
   the maximum is drawn exactly as today. The `cfg(test)` `painted_popover` / `painted_popover_blocks`
   still hold the whole text.
5. The module doc (`hover.rs:1-29`, the **Hiding** bullet) says the above.
6. Tests: new `crates/fe-gpui/src/view/hover_reach_tests.rs`, declared at the end of
   `crates/fe-gpui/src/view/hover_tests.rs` as `#[path = "hover_reach_tests.rs"] mod reach;` and using that
   file's helpers (`editor`, `at`, `rest`, `move_to`, `calls`) through `super::`. It must compile against the
   base API (Regatta overlays it at base): no new item by name, durations as literals (`Duration::from_millis`),
   the popover found with `cx.debug_bounds("hover-popover")`, `("hover-popover-content")` and
   `("hover-popover-body")`, which are `None` at base.
   - `the_pointer_reaches_the_popover_and_stays_on_it`: rest on `alpha` until `popover_text()` is the docs.
     Move straight down to 4 px below the hovered row's bottom, then to the popover's centre; advance 1 s: the
     docs still show, the provider asked once. Click at the popover's centre: the selections are unchanged and
     the docs still show. Move from `alpha` onto `total` (left of the popover, other text) and then into the
     popover 100 ms later: still shown, no request for `total`. Move to an empty point below the text and off
     the popover and advance 1 s: hidden. Rest on `alpha` again, move onto the popover and press `escape`:
     hidden.
   - `the_wheel_scrolls_a_tall_popover`: the provider answers 60 lines of text. Rested on `alpha`, the
     popover's height (`debug_bounds("hover-popover")`) is at most 16 line heights plus its padding and
     border. A wheel of 5 lines over it leaves `view.scroll` unchanged and the popover shown, and the next frame's
     `hover-popover-body` top is more than one line height above `hover-popover-content`'s top. A wheel over
     the text outside it scrolls the editor and hides it, as today.

   Existing tests that move the pointer off a shown popover and expect it gone at once (in `hover.rs`'s
   `host_popover_tests`, `view/hover_tests.rs`, and any other fe-gpui test `cargo nextest run -p fe-gpui`
   turns up) advance the clock past the grace (a literal 1 s) before asserting, and nothing else.

Order: `hover.rs` is also in the scope of T916, T935 and T956 (all inbox, behind unfinished dependencies), and
`hover_tests.rs` in T956's. This bugfix goes first; T956's `render_popover` change draws its renderer's element
inside the scroll container this card adds. Linux-OK (painted bounds and test-clock timing, no goldens).

## Implementation notes

- The popover's painted bounds come from `on_children_prepainted` on a plain wrapper around the
  `hover-popover` box, stored in a shared `Cell` in `HoverPopover`, cleared at the start of each
  `render_popover`. The move listener's body is now `element::pointer_moved`; the hide timer reruns it
  for the still pointer (`element::pointer_still`) once it has hidden the popover.
- While the grace runs, the pointer finding something else (another word, an annotation entry, an
  after-line label) doesn't start its wait: that comes as the fresh move once the timer fires. The
  timer starts at the first move off and isn't restarted by later ones.
- Existing tests edited only to advance the clock 1 s: `view/hover_tests.rs` (three "gone" checks and
  one move off before a fresh rest), `view/diagnostics_tests.rs` (one "gone" check, and its `rest`
  helper, which now waits past the grace too).
- Carried T45 F2: `FrameLayout::columns_past_row_end` takes the text and, off the frame's rows or below
  the last, uses the row `offset_at` clamps to (`frame.row_containing(offset)`), so a box drag keeps
  its column below EOF. Regression test `view::multi_cursor_tests::a_box_keeps_its_column_below_the_last_row`.
