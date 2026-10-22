---
task_uid: "01m5f6t7s2cmj19ty3fwaes18p"
task: "T250"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-markdown-view test document_element_draws_without_scroll_or_images (tests/element.rs): no scroll container; Images::AltText draws an image's alt text and opens no file; on_link gets a clicked link's destination; code_header is called once per fenced block with its index, info and code; fe-markdown-view's existing tests and goldens pass unchanged and src/view.rs is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-chat-view test replies_render_as_markdown_while_streaming (tests/markdown.rs): deltas that split a fence draw the half-received block as code; the finished reply draws its heading, list and code block; the reply is parsed at most once per frame"
    required: true
    manual: false
  - id: "C3"
    text: "fe-chat-view test code_blocks_get_copy_and_apply (tests/markdown.rs): a block with path=src/lib.rs shows rust and src/lib.rs in its header; Copy puts its code on the clipboard; chat-apply-0-1 emits ApplyBlock with turn 0 and block 1; a relative link with #L12 emits OpenPath at row 12; an https link opens the URL"
    required: true
    manual: false
  - id: "C4"
    text: "On helium the screenshots example builds and exits 0 (printing 'skipped' on Linux) and its macOS scenes (dark and light: a Markdown reply with a code header, a thinking summary, a refused turn, a streaming reply with Stop) type-check with cargo clippy --target aarch64-apple-darwin; the goldens in crates/fe-chat-view/examples/screenshots/golden/ are generated on the Mac with UPDATE_SCREENSHOTS=1 cargo run -p fe-chat-view --example screenshots after the sync"
    required: true
    manual: false
---

# Render chat replies as Markdown while they stream (fe-markdown-view with no scroll or images), with Copy and Apply headers on code blocks and macOS goldens (Mac T970)

# Render chat replies as Markdown while they stream, with code-block headers for Copy and Apply, and macOS goldens

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) renders
replies with fe-markdown (T846) and fe-markdown-view (T847) (record §1, user decision 4).
`fe-chat-view` shows replies as plain text. T847's `MarkdownView` is a whole scrolling tab: it
owns an `overflow_y_scroll` container and a `ScrollHandle`, which can't sit once per reply
inside the chat's own scrolling transcript, and it loads local images, which a model's reply
mustn't make the chat do.

## Change

**In `crates/fe-markdown-view`** (after T847; don't touch `src/view.rs`, which T849 holds):

- `pub fn document_element(document: &Arc<Document>, style: &MarkdownStyle, options:
  ElementOptions) -> AnyElement` in `src/blocks.rs`, exported from `src/lib.rs`: the blocks
  `MarkdownView` draws, with no scroll container and no width cap. `ElementOptions { images:
  Images::{Load { doc_path, root } | AltText}, on_link: Option<Rc<dyn Fn(&str, &mut Window,
  &mut App)>>, code_header: Option<Rc<dyn Fn(CodeHeader<'_>, &mut Window, &mut App) -> AnyElement>> }`,
  where `CodeHeader { index, info, language, code }` is a fenced block's (its index among the
  document's code blocks, and T846's `BlockKind::Code` fields). It shares `blocks.rs`'s drawing
  with `MarkdownView` (one implementation; `MarkdownView`'s own behaviour is `Load`, its link
  event and no header), and `MarkdownView`'s tests and goldens don't change. `src/inlines.rs` takes the image
  policy: `AltText` draws the alt text in muted style and reads nothing.

**In `crates/fe-chat-view`** (add `fe-markdown` and `fe-markdown-view`):

- New `src/reply.rs`: a reply's text blocks joined and parsed with `fe_markdown::parse`, at
  most once per frame while it streams (a frame's deltas coalesce; an unclosed fence parses as
  code to the end, so a half-streamed block draws as code), and once more when it finishes;
  drawn with `document_element`, `Images::AltText`, `MarkdownStyle::new` from the editor and UI
  themes. The thinking summary is drawn the same way, muted.
- Links: `http(s)` opens in the browser (`cx.open_url`); a relative path emits
  `ChatViewEvent::OpenPath { path, row }` (`#L12` read as row 12) for the workbench; anything
  else does nothing.
- Each fenced code block gets a header: its language, its `path=` if the info string has one
  (record §8), a Copy button (the code to the clipboard) and an Apply button that emits
  `ChatViewEvent::ApplyBlock { turn, block }` (`block` the fence's index in the reply). The
  workbench acts on it in the next card. Debug selectors `chat-code-<turn>-<block>` and
  `chat-apply-<turn>-<block>`.

**Goldens**: `crates/fe-chat-view/examples/screenshots.rs` on the kiln-platform golden harness,
as fe-markdown-view's does, with goldens in `crates/fe-chat-view/examples/screenshots/golden/`:
a chat whose reply has a heading, a list, inline code and a Rust block with `path=src/lib.rs`
(header shown), a thinking summary, a refused turn's banner, and a reply mid-stream with the
Stop button; dark and light. macOS only, as every golden.

Tests in `crates/fe-chat-view/tests/markdown.rs` and `crates/fe-markdown-view/tests/element.rs`
(top-level fns).
## On helium: goldens come from the Mac (review round 1, F2)

GPUI's `VisualTestAppContext` needs Metal, so the chat goldens (`golden/chat_dark.png`, `chat_light.png`) cannot be rendered on Linux; the example prints 'skipped' and exits 0, and its macOS scenes type-check with `cargo clippy --target aarch64-apple-darwin`. The scenes are written here; the goldens are generated on the Mac with `UPDATE_SCREENSHOTS=1 cargo run -p fe-chat-view --example screenshots` after the batch lands (a documented limit of this card).

## Implementation notes

- **How `document_element` reaches the shared drawing without `src/view.rs`.** `RenderCx`
  is declared in `view.rs`, which this card mustn't touch, so it can't carry the image
  policy or the code header. `blocks.rs` sets them for the duration of the element's own
  render in a scoped thread-local (`with_element`, restored on drop, even on panic), and the
  `Code` arm in `blocks.rs` and `Piece::render`/`max_width` in `inlines.rs` read it. A
  `MarkdownView` or `markdown_element` draw sees `None` and draws exactly as before. The
  only change on their path is a test-only `md-code-<n>` debug selector on code blocks.
- In `document_element`, code blocks use `restrict_scroll_to_axis`, so a vertical wheel
  over a wide block scrolls the chat and doesn't push the code sideways. `code_header` is
  called for every code block, indented ones too (fe-markdown can't tell an indented block
  from a fence with no info string). `index` follows the same document order as
  `ChatView::code_block`.
- Selectors: `md-image-alt-<n>` for `Images::AltText`, `md-code-<n>` for code blocks;
  in the chat, `chat-code-<turn>-<block>` (the header), `chat-copy-…` and `chat-apply-…`.
- **Chat.** `reply.rs` keeps each turn's last parsed text and document, and the view parses
  only in `render` when the text has changed, so a frame's deltas are parsed once (the test
  platform draws on every update, so the test checks parses ≤ frames). Apply is disabled
  while the turn streams. Code blocks are drawn on the editor's background, since the
  chat's own background is the panel colour the preview uses for code. Links: `http(s)` opens with
  `cx.open_url`; a relative path whose components are all normal (no `..`, no root,
  no `file:`) emits `OpenPath { path, row }`, with `row` 1-based as fe-chat's rows are
  (`#L12` → `Some(12)`); `mailto:`, anchors and anything else do nothing.
  `ChatView::code_block(turn, block)` gives the workbench a block's language, `path=` and
  code for `ApplyBlock`. `ChatView::set_thinking_open` lets the screenshot scene open the
  thinking.
- **Scope additions (respec):** `tests/view.rs`, whose tail-follow fixture streamed
  `line N\n` lines, which Markdown joins into one wrapped paragraph that no longer
  overflows (each line is now its own paragraph); and `fe-workbench/src/chat_panel.rs`,
  whose exhaustive match on `ChatViewEvent` gets a no-op arm for the two new events until
  `chat_apply.rs`.
- **C4 on helium:** the goldens are Metal-only. On Linux the example prints "skipped" and
  exits 0, and `cargo clippy -p fe-chat-view --all-targets --target aarch64-apple-darwin`
  type-checks the scenes. The PNGs aren't committed: the Mac makes them with
  `UPDATE_SCREENSHOTS=1 cargo run -p fe-chat-view --example screenshots` after the sync
  (`chat_dark.png`, `chat_light.png`).
- **Review f7qek3 (round 1).**
  - F1: each reply (`("chat-reply", turn)`) and open thinking summary
    (`("chat-thinking-text", turn)`) now has an element ID of its own. Each document numbers its
    texts and code blocks from 0, so without the IDs, replies shared click and scroll state.
    `each_reply_keeps_its_own_link_state` presses one document's link and releases on another's;
    without the scopes it fires the second link. GPUI keeps a text's press until the mouse comes up
    over that same text, inside one document too, so the test never releases over a text pressed
    earlier.
  - F3: any scheme but `http`/`https` now does nothing, including a relative `file:src/lib.rs`.
  - F4: the parse cache also records whether the turn had finished, so a finish arriving in a
    frame after the last delta parses the reply once more. The streaming test now sends the
    finish in a later frame.
  - F2 / C4: the goldens can only be rendered with Metal; see the C4 note above.
