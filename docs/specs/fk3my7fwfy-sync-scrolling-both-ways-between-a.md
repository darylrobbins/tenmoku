---
task_uid: "01m59h6jxccdeewefk3my7fwfy"
task: "T224"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-markdown-view test source_lines_and_scroll_offsets_map_both_ways (tests/scroll.rs): scroll_to_source_line(n) for a block's first line puts that block at the top and source_line_at_top() then returns n within 0.05; a line inside a list item or table row lands on that item or row; and only a simulated wheel scroll emits MarkdownViewEvent::Scrolled"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workbench_commands_tests::markdown_scroll_sync_tests::editor_scroll_moves_the_preview_to_the_same_source_line: scrolling the source so buffer row 40 is at the top (soft wrap on) puts the preview's source_line_at_top within 1 line of 40, and the editor's top doesn't move again in response"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workbench_commands_tests::markdown_scroll_sync_tests::preview_scroll_moves_the_editor_to_the_same_source_line: a wheel scroll in the preview to the heading on line 80 makes the source's top_buffer_line within 1 line of 80 with no echo back, and a re-render after an edit keeps the preview at the source's line"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui -p fe-markdown-view -p fe-workbench passes with no existing test edited, and cargo clippy on those crates with --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Sync scrolling both ways between a Markdown source and its preview by fractional source line, with no echo (Mac T849)

## Problem

The user, 2026-10-17, as part of the Markdown preview modelled on VS Code's: "scroll sync both ways between the
source and the preview, by source line mapping". VS Code's `markdown.preview.scrollPreviewWithEditor` and
`scrollEditorWithPreview` both default to on. Each block in its preview records the source line it starts on.
Scrolling one side puts the other at the same fractional source line, interpolating between neighbouring
blocks, and a scroll the sync made itself doesn't echo back.

After the previous three cards, `MarkdownPreview` (`crates/fe-workbench/src/markdown_preview.rs`) shows a
`MarkdownView` whose blocks carry `fe_markdown` source lines, and the two sides scroll independently. The
editor already reports user scrolls, with `EditorViewEvent::Scrolled { top }` in screen rows
(`crates/fe-gpui/src/view/host.rs`), and `set_scroll_top` doesn't emit it. But it offers no way to read or set
the top as a fractional buffer row: `viewport_rows` returns a whole row, and `scroll_to_top_row` is
`pub(crate)`.

This is card 4 of 4, the stateful half.

## Change

- `crates/fe-gpui/src/view/host.rs`, next to `scroll_top` and `set_scroll_top`:
  - `pub fn top_buffer_line(&self) -> Option<f32>`: the buffer row at the top of the view, plus the fraction
    of that row scrolled past. Soft-wrapped rows count their screen rows, and folds and blocks are skipped
    through the display and block maps. It is `None` before the first frame.
  - `pub fn scroll_to_buffer_line(&mut self, line: f32, cx)`: the inverse. Like `set_scroll_top`, it emits
    no `Scrolled`.
- `crates/fe-markdown-view/src/view.rs`:
  - Record each block's bounds and source `lines` each frame, nested ones included: list items, table rows
    and the blocks inside quotes. Use `on_children_prepainted` or a bounds-recording wrapper, keeping the
    innermost block that covers each line.
  - `pub fn source_line_at_top(&self) -> Option<f32>`: the block at the column's top, with
    `start + (offset into block / block height) × line count`. In a gap between blocks it interpolates
    between the previous block's end and the next block's start.
  - `pub fn scroll_to_source_line(&mut self, line: f32, cx)`: the inverse. A line past the last block scrolls
    to the end.
  - `MarkdownViewEvent::Scrolled`, emitted only for scrolls the user made (wheel, trackpad), never for
    `scroll_to_source_line` or `scroll_to_heading`.
- `crates/fe-workbench/src/markdown_preview.rs`:
  - On the source's `Scrolled`, call `scroll_to_source_line(top_buffer_line)`. On the preview's `Scrolled`,
    call `scroll_to_buffer_line(source_line_at_top)` on the source.
  - Neither programmatic scroll emits, so there's no echo.
  - After a re-render (`set_document`), the preview re-applies the source's current line once the frame has
    laid out, so typing doesn't make it jump.
  - With the source closed, nothing syncs.

Tests:
- New integration test `crates/fe-markdown-view/tests/scroll.rs`.
- New `crates/fe-workbench/src/markdown_scroll_sync_tests.rs`, declared in the existing
  `crates/fe-workbench/src/workbench_commands_tests.rs` with
  `#[cfg(test)] #[path = "markdown_scroll_sync_tests.rs"] mod markdown_scroll_sync_tests;`.

Out of scope: VS Code's editor-selection marker in the preview, and double-click in the preview to jump to
the source.

## Lent from Mac (y13)

Mac T849; depends on Mac T848 (Markdown preview), landed in mac/main 4a674be0 (Mac T847/T848 are in mac/main, not in your main until you sync it). Linux-OK: headless GPUI tests only, no macOS API. It changes no screenshot pixels by itself; no golden regen expected.
