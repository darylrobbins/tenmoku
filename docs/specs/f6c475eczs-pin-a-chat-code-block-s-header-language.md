---
task_uid: "01m69pbaczdv0mhjf6c475eczs"
task: "T271"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "While a reply's code block has its top above the transcript viewport's top and at least a header's height of it still visible, a copy of its header (chat-code-pinned) is drawn at the viewport's top (±1 px) and ChatView::pinned_code_block() names its (turn, block). Otherwise nothing is pinned: at the top, between blocks, and past the block. a_code_header_is_pinned_while_its_block_is_scrolled_under (crates/fe-chat-view/tests/pinned_code_header.rs) passes."
    required: true
    manual: false
  - id: "C2"
    text: "The pinned copy's Apply (chat-apply-pinned) emits ApplyBlock for the pinned block, and its Copy (chat-copy-pinned) copies that block's code. Its Apply is disabled while the turn streams. A wheel over it still scrolls the transcript, and clicks don't reach the text under it. the_pinned_header_of_a_streaming_reply_cannot_apply passes."
    required: true
    manual: false
  - id: "C3"
    text: "fe-markdown-view's ElementOptions gains on_code_bounds (each code block's index and bounds at prepaint) and Default, documented in lib.rs. The existing fe-markdown-view and fe-chat-view tests and screenshot goldens pass unchanged. cargo test -p fe-markdown-view -p fe-chat-view and clippy are clean."
    required: true
    manual: false
---

# Pin a chat code block's header (language, path, Copy, Apply) to the top of the transcript while its block is scrolled under the top edge (Mac T1261)

## Goal

A chat code block's header (language, `path=`, Copy, Apply) stays pinned to the top of the
chat's transcript viewport while its block has scrolled under the top edge, so mid-block the
user still sees which file the block is for and can press Apply. Today the header scrolls away
with a long block.

**Decision: pinned (sticky) header, not a repeated Apply at the block's end.** gpui has no CSS
`position: sticky`, but the chat can draw it: the transcript is a `relative` kiln `ScrollArea`
whose offset `ChatView` already holds in a `ScrollHandle`, and gpui re-renders the view that
built a scroll container on every offset change (gpui-pre 0.3.9 `elements/div.rs:3381-3383`,
`cx.notify(current_view)`). The view therefore knows the offset each frame. What it lacks is
where each code block sits. fe-markdown-view can report that the way it already reports block
bounds for the preview's line map. A footer at the block's end would still leave no file name
mid-block.

## Facts found

- The header is the host's: `ElementOptions::code_header`
  (`crates/fe-markdown-view/src/blocks.rs:45-55`), called per code block with
  `CodeHeader { index, info, language, code }` (`:305-317`) and stacked over the body in a
  `flex_col` wrapper (`:338-349`).
- `crates/fe-chat-view/src/reply.rs:195-215` `reply_element` passes the header closure, and
  `:235-284` `code_header` builds it with debug selectors `chat-code-<turn>-<block>`,
  `chat-copy-…` and `chat-apply-…`. Apply is disabled while the turn streams (`:279`), and Copy
  copies `CodeBlock::new(info, code).code` (`:243-250`).
- `crates/fe-chat-view/src/view.rs:1237-1242`: the transcript is
  `ScrollArea::new("chat-transcript").track_scroll(&self.scroll)`, with content
  `v_flex().p_3().gap_4()`. `self.scroll` is set at `view.rs:198, 368`. `follow_tail`
  (`:873-893`) reads `offset()` and `max_offset()`. Each reply is its own id scope,
  `("chat-reply", index)` (`:991-1000`).
- `kiln/crates/kiln-ui/src/scroll_area.rs:97-106`: the `ScrollArea` is `relative()` and
  `overflow_hidden()`, holding an `overflow_y_scroll` content div plus an 8 px scrollbar over the
  right edge (`SCROLLBAR_WIDTH`, `:18`). Its children all go into the scrolling div, so a pinned
  layer must be a sibling over it.
- Precedent for reporting bounds: `RenderCx::track` uses `on_children_prepainted`
  (`crates/fe-markdown-view/src/view.rs:405-410`).
- `ElementOptions` is built as a struct literal in `reply.rs:209, 223`,
  `crates/fe-markdown-view/tests/element.rs:55` and `tests/painted_runs.rs:22`.

## Changes

**fe-markdown-view** (`src/blocks.rs`, `src/lib.rs`):
- `ElementOptions` gains `on_code_bounds: Option<CodeBoundsFn>`, where
  `CodeBoundsFn = Rc<dyn Fn(usize, Bounds<Pixels>, &mut Window, &mut App)>`. It is called at
  prepaint with each code block's index and window bounds, header included, for blocks drawn
  with or without a header. Export it from `lib.rs` and document it there.
- Add `impl Default for ElementOptions` (`Images::AltText`, no callbacks) and update the four
  literal sites to `..ElementOptions::default()`, so later fields don't touch them again.

**fe-chat-view** (`src/view.rs`, `src/reply.rs`):
- `reply_element` passes an `on_code_bounds` that records each block's top and bottom per
  `(turn, block)` in transcript-content coordinates: window y minus the viewport's top minus the
  scroll offset at that prepaint. These positions don't change when the transcript scrolls, so
  the pinned header doesn't lag a frame while scrolling. It lags only when content reflows
  (streaming, resize).
- `ChatView::render` wraps the `ScrollArea` in a `relative`, `flex_1`, `min_h_0` container. If a
  reply's code block has its top above the viewport's top and at least one header height of
  itself still below it, the container draws a copy of that block's header absolutely at the
  viewport's top. The copy uses the transcript's horizontal padding, so the scrollbar stays
  visible, and the same `code_header` content (Apply still disabled while streaming).
- The copy's debug selectors are `chat-code-pinned`, `chat-copy-pinned` and
  `chat-apply-pinned`, with no duplicate ids against the real header. Its hitbox blocks mouse
  except scroll (`block_mouse_except_scroll`), so clicks don't reach the text under it and a
  wheel over it still scrolls the transcript.
- `pub fn pinned_code_block(&self) -> Option<(usize, usize)>` returns the `(turn, block)` whose
  header is pinned, for tests and hosts. Thinking summaries have no headers and are not pinned.

## Tests

New `crates/fe-chat-view/tests/pinned_code_header.rs` (top-level fns; reuse the
`markdown.rs`-style harness: a `ScriptedProvider` reply, `simulate_resize` to about 400×300,
and `ScrollWheelEvent`s over `chat-transcript` as `tests/view.rs:737` does).

The reply is a paragraph, a ```` ```toml path=deny.toml ```` block of 80 lines, a paragraph, a
```` ```rust path=src/lib.rs ```` block of 80 lines, and a paragraph:
- `a_code_header_is_pinned_while_its_block_is_scrolled_under`:
  - At the top, nothing is pinned (`chat-code-pinned` is not drawn, and `pinned_code_block()` is
    `None`).
  - Scrolled so that `md-code-0` is mid-viewport and its header is above the top:
    `pinned_code_block() == Some((0, 0))`, and `chat-code-pinned`'s top equals
    `chat-transcript`'s top (±1 px).
  - Clicking `chat-apply-pinned` emits `ApplyBlock { turn: 0, block: 0 }`, and clicking
    `chat-copy-pinned` puts block 0's code on the clipboard.
  - Scrolled to the paragraph between the blocks: nothing is pinned.
  - Scrolled into the second block: `Some((0, 1))`.
- `the_pinned_header_of_a_streaming_reply_cannot_apply`: while the turn streams, the pinned
  Apply emits nothing.

The existing fe-chat-view and fe-markdown-view tests and screenshot goldens pass unchanged.

## Conflicts

- Shares `blocks.rs` with the sideways-scroll card (it depends on that card) and `reply.rs` and
  `view.rs` with the SEARCH/REPLACE-diff and block-outcome cards, which come after it.
- T1235 (blocked) holds `crates/fe-chat-view/tests/**` and `crates/fe-markdown-view/tests/**`.
- No overlap with T1257 or T1258 (`chat_panel.rs` isn't touched).

## Implementation notes

- `crates/fe-markdown-view/tests/code_scroll.rs` (the sideways-scroll card's, landed after this
  spec was written) builds `ElementOptions` as a fifth literal; it now ends in
  `..ElementOptions::default()` too, outside the listed scope, since the new field forces it.
- The bounds come from `on_children_prepainted`: on the header-and-code column when there is a
  header, else on a `w_full` wrapper drawn only when `on_code_bounds` is set, so the Markdown
  preview's tree is unchanged.
- `reply::CodeSpans` holds the spans. Each frame's prepaint records them, and an empty canvas
  after the transcript commits them for the next render. If, committed and as scrolled then
  (the tail followed, or the reply reflowed), a different block should be pinned than the one
  drawn, it asks for another frame, as `rewrap_check` does for the composer.
- Each header's height is measured from the real header, per block (each sits in a one-child
  column that reports it), since a long path wraps; a block pins only with at least its own
  header's height of it inside the transcript, so a transcript shorter than the header pins
  nothing, and the layer over the transcript clips the copy to it. The pinned copy is as wide and as far left
  as its block, so a block in a list or quote is matched, and it takes the document's text size
  and line height, which the real header inherits.
