---
task_uid: "01m55v3ac4mh3cv1cmd0yv26zd"
task: "T176"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-markdown-view integration test compact_markdown_lays_out_blocks (tests/compact.rs, top level) passes: blocks within 400 px stacked in order, the fence's keyword in the theme's keyword colour, a link click calling on_link with its destination, and no img for an image; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "MarkdownView's rendering and its goldens are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-markdown-view passes with no existing test edited beyond what the spec names; cargo clippy -p fe-markdown-view --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-markdown-view: lay out a Markdown document statelessly in a narrow column, for hover popovers and completion docs (Mac T955)

Filed on argon from Mac T955 (card ~/coordination/argon-cards/zzr/T955.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty74djakmqz5pza7kc68y. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T847 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T955)".

## Problem

LSP completeness track, item 9 (user-approved 2026-10-17: "render hover and completion docs markdown with
fe-markdown after T846/T847"). Language servers' hover and completion documentation is Markdown: headings,
lists, emphasis, links, inline code and fenced code (rust-analyzer's signatures and doc comments). fastedit
shows it nearly as plain text: fe-gpui's `markdown_blocks` (`crates/fe-gpui/src/hover.rs:692`) only splits out
fenced code, and `render_popover` (`:630`) draws each block as a plain div; the completion menu's docs use the
same blocks (`crates/fe-gpui/src/completion_menu.rs:513-535`). VS Code renders them as Markdown, code
highlighted in the editor's theme.

T847 (fe-markdown-view, ready) renders an fe-markdown document, but as `MarkdownView`: a focusable, scrolling
entity with an 880 px centred column and 26 px padding, made for a preview tab. A hover popover needs the same
blocks laid out statelessly in a narrow box with no padding of its own, and fe-gpui can't use fe-markdown-view at
all: fe-markdown-view depends on fe-gpui (for `EditorTheme`), so fe-gpui depending back would be a cycle.

This card adds the compact, stateless form to fe-markdown-view. The seam in fe-gpui is the next card.

## Change

1. `crates/fe-markdown-view/src/compact.rs` (new), exported from `crates/fe-markdown-view/src/lib.rs`:
   `pub fn markdown_element(document: &fe_markdown::Document, style: &MarkdownStyle, max_width: Pixels,
   on_link: Option<Rc<dyn Fn(&str, &mut Window, &mut App)>>) -> AnyElement`: the document's blocks as
   `MarkdownView` draws them (reusing `blocks.rs`, `inlines.rs` and `table.rs`), in a column at most
   `max_width` wide with no padding, no scroll and no focus; headings at most 1.25 em (hovers shouldn't shout,
   as VS Code's hover sizes them); images never load (shown as their alt text, as blocked ones are); a link
   click calls `on_link` with its raw destination. Each block keeps its `md-block-<ix>` debug selector.
2. `MarkdownStyle` gains `pub fn compact(&self) -> MarkdownStyle`: body text at the editor's font size − 1
   and line height 1.4, as VS Code's hover text, everything else unchanged.
3. Tests: new integration test `crates/fe-markdown-view/tests/compact.rs` (top level, a headless window as
   `tests/render.rs` makes one): `compact_markdown_lays_out_blocks`: a document of a heading, a paragraph with
   `inline code` and a link, a bullet list and a Rust fence, drawn at `max_width` 400 px: each
   `md-block-<ix>` has bounds within 400 px wide and stacked in order; the fence's keyword run has the theme's
   keyword colour; a click on the link calls `on_link` with its destination; an image block draws no `img`.

Linux-OK (headless; no new goldens).
