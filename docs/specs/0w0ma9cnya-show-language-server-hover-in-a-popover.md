---
task_uid: "01m49phg0gv178yd0w0ma9cnya"
task: "T146"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::hover::hover_replies_convert: markdown MarkupContent, plaintext MarkupContent, a MarkedString and an array of MarkedStrings each become Hover text; a reply range becomes bytes in the request's version, with an emoji earlier on the line, under utf-8 and utf-16 sessions; a null reply is None; a reply later than the 3 s timeout on the injected clock is None and sends $/cancelRequest; initialize advertises hover.contentFormat [markdown, plaintext]"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::hover_tests::popover_after_the_delay: with a test HoverProvider, resting the pointer on a word for the hover delay calls the provider once and shows a popover with its text, fenced code in the editor font and the rest as plain text; moving off the word, typing, scrolling and escape each dismiss it; a reply that arrives after the caret's buffer version changed is not shown"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::hover_tests::show_hover_command: cmd-k cmd-i shows hover for the word at the caret; with a diagnostic under the caret the popover shows the diagnostic message and the hover text together"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::lsp_hover::hover_from_the_server: with a FakeLauncher whose fake answers textDocument/hover with markdown, cmd-k cmd-i in an open .rs file shows that text; a fake that never answers shows nothing and leaves the editor usable"
    required: true
    manual: false
  - id: "C5"
    text: "editor.show_hover is registered through the command registry and bound to cmd-k cmd-i in the editor context in default-macos.toml; no KeyBinding::new or actions! is added; the binding counts in fe-gpui command_tests.rs and keymap_tests.rs are updated and pass; fe-gpui has no fe-lsp dependency; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Show language server hover in a popover, after a hover delay or on cmd-k cmd-i

# Show language server hover in a popover, after a hover delay or on cmd-k cmd-i

Card 8 of `docs/decisions/lsp-client.md` §8 (GPUI). The design is §7's Hover row, with §3.5's hover timeout
(3 s, silently nothing). It builds on card 5's workbench wiring, card 6's request admission, and the hover
popover surface card 7 adds in `crates/fe-gpui/src/hover.rs`.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"): none beyond those cards'
(Q1/Q3 for the client).

## Change

- **`crates/fe-lsp/src/hover.rs` (new).**
  - `LspManager::hover(buffer, version, offset) -> impl Future<Output = Result<Option<Hover>,
    LspError>>`, where `Hover { text: String, range: Option<Range<usize>> }` has its range in bytes of
    the request's version.
  - Markdown `MarkupContent` is kept as a string. `plaintext`, `MarkedString` and arrays of them are
    joined.
  - Advertise `hover.contentFormat = [markdown, plaintext]`.
  - A timeout or error gives `None` to the UI, not an error.
- **fe-gpui.**
  - **A provider seam.** The view gets a `HoverProvider` seam (a trait object the host sets on the
    view, returning a future), so fe-gpui stays LSP-free.
  - **Delay.** Resting the pointer over a word for the hover delay requests hover once.
  - **The popover.** It shows in card 7's popover, below any diagnostic message there: markdown as
    text, with fenced code blocks in the monospace editor font. There is no markdown renderer in M4.
  - **Dismissal.** Moving off the range, typing, scrolling or escape dismisses it. A reply for an
    older position or buffer version is dropped.
- **`editor.show_hover`** (cmd-k cmd-i, editor context; VS Code's chord) shows hover at the caret.
  - It is registered through fe-gpui's editor command registry and bound in
    `crates/fe-keymap/keymaps/default-macos.toml`.
  - Update the binding counts pinned in `crates/fe-gpui/src/command_tests.rs` and
    `crates/fe-gpui/src/keymap_tests.rs`.
- **`crates/fe-workbench/src/lsp.rs`** sets each editor's `HoverProvider` to one backed by its
  buffer's server.

## Out of scope

- Signature help.
- Rich markdown rendering.
- Links in hover text.
- Hover for search-only files.
