---
task_uid: "01m78dyvne6dwacw70b3p9ez3t"
task: "T362"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::git_card_markdown (cargo nextest run -p fe-workbench) pass: a_list_and_inline_code_in_the_body_are_drawn_as_blocks, a_fenced_block_is_highlighted_in_the_editor_theme, a_link_opens_through_the_preview_tabs_link_path, a_plain_message_looks_as_before, the_summary_stays_bold_plain_text, a_very_long_body_still_scrolls_with_the_footer_kept, a_trailer_line_is_not_drawn_twice"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_card_tests (contents, trailers_and_refs, crowded_card_keeps_its_footer), git_blame_tests and lsp_hover_ra_html still pass"
    required: true
    manual: false
---

# Git log commit card: render the message body as Markdown, as the LSP hover does (Mac T1746)

# Commit card body as Markdown

User report (2026-11-12): "Support markdown formatting of the hover for a commit message. If we do have some markdown, we should try to format that as much as possible."

Today `CommitCard` (git_log.rs ~:690, section "body" ~:993) draws `body` as one plain text div; the same card is the `git.blame_line` popover. Reuse the hover renderer, `src/lsp_markdown.rs` (fe-markdown `Document`, fe-markdown-view `markdown_element` compact form, code highlighted in the editor theme, links through the preview-tab path), instead of writing a second one. Its `Renderers` are per editor and `pub(super)`: expose a small constructor/function that renders a Markdown string for a given theme and link handler without an editor entity (the log panel has none). Keep the LSP path's behaviour and tests unchanged (tests/lsp_hover_ra_html.rs).

## Do
- Parse `body` (after split_trailers, so trailers stay the trailers section) with fe-markdown; render with the compact style, max width = the card's. Summary stays bold plain text. Commit messages with no Markdown look as today (same lines, same wrapping); hard line breaks in git bodies are kept (a lone newline inside a paragraph is a break, not a space, as git-hosting UIs show).
- Images are never loaded (alt text); HTML shown as the compact form does. Link click goes through the card's existing web-link/opening path; relative links with no root are ignored.
- The card's scroll, size cap, CardProbe (`CardProbe` ~:722 `message` field still joins summary and body with \n) and footer are unchanged.

Platform: Linux-OK.
