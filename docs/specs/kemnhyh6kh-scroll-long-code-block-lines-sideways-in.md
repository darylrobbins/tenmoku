---
task_uid: "01m69pba76nj5ga1kemnhyh6kh"
task: "T270"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A code block whose longest line is wider than the block scrolls sideways in document_element (the chat) and in MarkdownView (the preview). The code text's box (test builds: md-code-\u003cn\u003e-text) is as wide as its longest line, and a sideways wheel moves it. The scroll range ends with the last character plus the 16 px padding at the block's right edge, and blocks whose lines fit don't move. long_code_lines_scroll_sideways, short_code_lines_do_not_scroll and the_preview_scrolls_code_sideways (crates/fe-markdown-view/tests/code_scroll.rs) pass."
    required: true
    manual: false
  - id: "C2"
    text: "In document_element a vertical wheel over a code block still scrolls the host, not the code (restrict_scroll_to_axis kept). Blocks whose lines fit look as before: the fe-markdown-view and fe-chat-view screenshot goldens are unchanged (macOS)."
    required: true
    manual: false
  - id: "C3"
    text: "The hand-in states the cause found (expected: the nowrap text box stretched to the block's width under taffy block layout, so gpui's scroll range was 0). cargo test -p fe-markdown-view -p fe-chat-view and clippy are clean."
    required: true
    manual: false
---

# Scroll long code-block lines sideways in chat replies and the Markdown preview: the nowrap code text is stretched to the block's width, so overflow_x_scroll has no range and lines are cut at the edge (Mac T1260)

## Goal

A long line in a chat reply's code block can be scrolled into view sideways. Today it is cut at
the panel edge with no way to see the rest (the user's deny.toml block: `# Only the platforms we
ship; crates (` ends at the edge). The same code draws the Markdown preview's code blocks, which
are cut the same way, so one fix covers both.

**Decision: horizontal scrolling, not soft wrap.** `blocks.rs` already asks for sideways
scrolling (below), MarkdownView follows VS Code's preview, where code blocks scroll sideways
(`fe-markdown-view/src/lib.rs:7-8`), and a wrapped row would make the diff rows of the
SEARCH/REPLACE card harder to read. Wrap would also need a new per-host option. Scrolling needs
no new option.

**Type: feature, not bugfix.** No base API can see where a code block's text sits or how far it
has scrolled. The text has no debug selector, and the scroll offset is private gpui element
state. So no base-run test can fail on an assertion for the right reason. This card adds that
check (`md-code-<n>-text`).

## Facts found (read from source, not run)

- `crates/fe-markdown-view/src/blocks.rs:321-337`: the code body is
  `div().id(("md-code", id)).w_full().overflow_x_scroll()`, `.restrict_scroll_to_axis()` when
  drawn by `document_element` (comment at :326-327: "a vertical wheel scrolls the view, not the
  code sideways"), `.p(px(16.))`, `.whitespace_nowrap()`, child `StyledText`. T970 added this
  intending sideways scrolling.
- Why it has no range: the div is display Block (gpui-pre 0.3.9 `style.rs:774`, the default).
  taffy 0.13's block layout gives a non-replaced child with auto width the container's width
  (`taffy-0.13.0/src/compute/block.rs:1124-1130`, `width.unwrap_or(stretch_width)`). So the
  `StyledText` box is the block's width, although its unwrapped line is wider (gpui
  `elements/text.rs:657-665`: no wrap width under nowrap). gpui sets a div's scroll range from
  the union of its children's layout bounds plus padding (`elements/div.rs:1990-2012`,
  `:2473-2474`). The maximum x offset is therefore 0, and the overflow is only clipped.
- With a header, the block sits in a `flex_col` wrapper with `overflow_hidden` and rounded
  corners (`blocks.rs:338-349`), outside the scrolling div, which is fine.
- One `render` draws blocks for `MarkdownView`, `markdown_element` and `document_element`
  (`blocks.rs:237`).
- kiln-ui has only a vertical scrollbar (`kiln/crates/kiln-ui/src/scroll_area.rs:117`). No
  horizontal scrollbar is added here.

## Changes

- `blocks.rs`, the `BlockKind::Code` arm: give the code text a box as wide as its longest line,
  for example by making the scrolling div a flex row with the text in a `flex_none` child, or by
  setting a max-content minimum width. Then the x range is that width plus the 16 px padding on
  both sides, minus the block's width. Keep `restrict_scroll_to_axis` for `document_element`.
  Blocks whose lines fit look exactly as they do now.
- In test builds, give that text box the debug selector `md-code-<n>-text`, and list it in
  `lib.rs`'s debug-selector docs (`lib.rs:22-27`).

## Tests

New `crates/fe-markdown-view/tests/code_scroll.rs` (top-level `#[gpui::test]` fns, following
`tests/element.rs`'s host and `tests/scroll.rs`'s `open`):

- `long_code_lines_scroll_sideways`: `document_element` in a 320 px host, with one fenced block
  holding a 200-character line, drawn with a `code_header` and again without one. Assert that
  `md-code-0-text`'s right edge is past `md-code-0`'s. A `ScrollWheelEvent` with x delta -120
  over the block moves the text left by 120 px. Scrolling far right stops with the text's right
  edge plus 16 px at the block's right edge (±1 px). A vertical wheel over the block leaves the
  text's x unchanged.
- `short_code_lines_do_not_scroll`: when every line fits, a sideways wheel leaves the text's x
  unchanged.
- `the_preview_scrolls_code_sideways`: `MarkdownView` with the same long line scrolls it sideways.

## Conflicts

- T1235 (blocked) holds `crates/fe-markdown-view/tests/**` and would merge the crate's tests
  into one `tests/it.rs`. If it lands first, declare the new file through its module list.
- The pinned-header and SEARCH/REPLACE-diff chat cards edit the same `blocks.rs` code arm after
  this one.
- No overlap with T1257, T1258 or T1252.

## Implementation notes

Cause: the nowrap `StyledText` was a block child of the scrolling div, so taffy's block layout stretched its box to the block's width; gpui's scroll range (union of child bounds plus padding) was 0 and the overflow was only clipped. The scrolling div is now a flex row with the text in a `flex_none` child (`md-code-<n>-text`), as wide as its longest line.
