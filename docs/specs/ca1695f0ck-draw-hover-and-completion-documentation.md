---
task_uid: "01m5aqxgpdhdm8kkca1695f0ck"
task: "T231"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::hover_tests::markdown_popover::a_set_renderer_draws_the_hover_and_the_docs passes: with a renderer set, a hover's markdown and a completion item's docs are drawn by it; without one painted_popover_blocks is as at the base; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui passes with no existing test edited beyond what the spec names; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Draw hover and completion documentation through a host-set Markdown renderer in fe-gpui, falling back to today's text (Mac T956)

Filed on argon from Mac T956 (card ~/coordination/argon-cards/zzr/T956.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty8j8a5j3fnne0rvvbh5d. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T816, Mac T884, Mac T916 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T956)".

## Problem

LSP completeness track, item 9 (user-approved 2026-10-17). Hover popovers and the completion menu's
documentation pane draw Markdown as near-plain text: `HoverContent { text, range }` (`crates/fe-gpui/src/hover.rs:48`)
is split by `markdown_blocks` (`:692`) into fenced code and the rest, and `render_popover` (`:630`) draws each
block as a plain div; the completion menu does the same with an item's `documentation`
(`crates/fe-gpui/src/completion_menu.rs:513-535`). Once the card this depends on lands, fe-markdown-view can lay
out a document compactly (`markdown_element`), but fe-gpui can't call it: fe-markdown-view depends on fe-gpui.

This card adds the seam in fe-gpui: a host-set renderer used for hover and completion documentation, with
today's text rendering as the fallback.

## Change

1. `crates/fe-gpui/src/hover.rs`:
   - New `pub trait MarkdownRenderer { fn render(&self, markdown: &str, theme: &EditorTheme, max_width: Pixels,
     window: &mut Window, cx: &mut App) -> AnyElement; }`.
   - `render_popover`: with a renderer set on the view, the hover's markdown (and a diagnostic hover's
     message, which stays plain text: only `HoverContent` text is Markdown) is drawn by it at the popover's
     maximum width; without one, exactly as today. The popover's size, placement, delay and dismissal are
     unchanged; `popover_text()` still returns the source text.
2. `crates/fe-gpui/src/completion_menu.rs`: the documentation pane draws the selected item's `documentation`
   through the renderer when one is set, as above.
3. `crates/fe-gpui/src/view.rs`: `EditorView::set_markdown_renderer(Option<Rc<dyn MarkdownRenderer>>)` and
   `markdown_renderer()`.
4. `crates/fe-gpui/src/lib.rs`: export `MarkdownRenderer`.
5. Tests: new `crates/fe-gpui/src/view/markdown_popover_tests.rs`, declared at the end of
   `crates/fe-gpui/src/view/hover_tests.rs` as `#[path = "markdown_popover_tests.rs"] mod markdown_popover;`,
   with a test renderer that records its calls and draws a div with a debug selector:
   - `a_set_renderer_draws_the_hover_and_the_docs`: with the renderer set, a hover of `# T\n\`x\`` calls it
     once with that markdown and the popover holds its element; a completion item's documentation calls it
     for the docs pane; with no renderer the painted popover blocks are as at the base
     (`painted_popover_blocks`).

Order: `hover.rs` is held by T816 (mergeable) and T916; `completion_menu.rs` by T884 (ready); `view.rs` by
T816, T858 and others. Linux-OK (painted state, no goldens).

## Implementation notes

- `render_popover` and `render_completion` now take the `Window` mutably (and the app), as a renderer needs them; the popover's diagnostic message stays a plain text block above the renderer's element.
- The renderer is asked each frame the popover or docs pane is drawn, at the pane's maximum width less its padding.
- Carried findings T197 F2 and T223 F2/F4/F5/F6 are not fixed here: `view.rs` and `completion_menu.rs` are touched only to add a field, a parameter and the renderer branch, and each finding is a separate behaviour change (mouse-press ordering, layout freshness, keyword-menu placement) of its own size. They want follow-up cards.
