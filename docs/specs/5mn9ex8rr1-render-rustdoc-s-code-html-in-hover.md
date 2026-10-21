---
task_uid: "01m5f7cbh4ap0hss5mn9ex8rr1"
task: "T256"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "RA's transpose hover parses with no Html block or Inline::Html left"
    required: true
    manual: false
  - id: "C2"
    text: "Links inside \u003ccode\u003e stay links with Inline::Code labels; escapes resolve (no backslash in (_))"
    required: true
    manual: false
  - id: "C3"
    text: "Unknown tags, tags with attributes and unbalanced tags stay Html as today"
    required: true
    manual: false
  - id: "C4"
    text: "End-to-end hover of the fixture lays out no '\u003ccode\u003e' or '](https://' in the live popover, and both rust fences are present with keyword-coloured glyphs in the same document rendered with the popover's theme (painted-glyph colours are a documented limit, see Known limits)"
    required: true
    manual: false
---

# Render rustdoc's <code> HTML in hover Markdown as code spans with working links, as rust-analyzer sends it (Option::transpose) (Mac T1024)

## Problem (user bug, screenshot of hover over `Option::transpose`)

rust-analyzer (1.98.1) hover documentation contains rustdoc's `<code>` HTML around
intra-doc links. RA rewrites the links and emits `<code>` followed by a newline:

```
<code>
[Some](https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.Some)([Ok](https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Ok)(\_))</code> is mapped to <code>
[Ok](...)([Some](...)(\_))</code>,
...
```

The full payload, captured from rust-analyzer 1.98.1 over LSP, is at
`/private/tmp/claude-501/-Users-daryl-code-fastedit/4bb8932f-6c33-441a-8ba6-653953dc1606/scratchpad/cards/ra_hover_transpose.md`;
copy it verbatim to `crates/fe-markdown/tests/fixtures/ra_hover_transpose.md`.

Because `<code>` is alone on its line after a blank line, CommonMark makes the whole paragraph an
HTML block (start condition 7). fe-markdown (`parse.rs`, `Tag::HtmlBlock` arm) keeps it as
`BlockKind::Html`, and fe-markdown-view `blocks.rs` draws it as written. The hover then shows
literal `<code>`, `</code>`, `[Some](https://...)` and `\_`. Inline `<code>`/`</code>` inside an
ordinary paragraph come through as `Inline::Html` and are drawn as text too.

rustdoc renders this as "Some(Ok(_)) is mapped to Ok(Some(_)), ..." in code style, with `Some`,
`Ok`, `Err` and `None` as links. VS Code hides the tags (RA's client enables HTML in hovers).

## Fix (in fe-markdown only, so the view and T970's markdown-view work are untouched)

- An HTML block whose tags are all inline phrasing tags from a small allow-list (`code`, `kbd`,
  `b`, `strong`, `i`, `em`, `s`, `del`, `sub`, `sup`, `br`) and that has no other HTML becomes a
  paragraph. Its text is parsed as Markdown inlines, so links, escapes and emphasis resolve.
- Inside a paragraph, a `<code>` … `</code>` pair becomes code. Every text run between the tags
  becomes `Inline::Code`, including the label of a link between them, so a link stays a link with
  a code-styled label. The tags themselves are dropped. `<b>/<strong>` map to `Strong`,
  `<i>/<em>` to `Emphasis`, `<s>/<del>` to `Strikethrough`, `<br>` to `HardBreak`.
- Any other HTML (unknown tags, attributes on the tags, unbalanced tags, block tags) stays as it
  is today.
- This also applies to the Markdown preview, which uses the same parser. That is closer to VS
  Code's preview, which renders HTML.

Put the HTML-to-inline mapping in a new module, `crates/fe-markdown/src/html_inline.rs`.

## Tests

- New `crates/fe-markdown/tests/html_code_spans.rs`:
  - The fixture parses with no `BlockKind::Html` and no `Inline::Html` left.
  - The paragraph after "Transposes …" holds `Link`s to `…enum.Option.html#variant.Some` and
    `…enum.Result.html#variant.Ok` whose content is `Inline::Code("Some")` / `Inline::Code("Ok")`,
    and the code text reads `(_)`, with no backslash.
  - Unit cases: inline `<code>x</code>` mid-paragraph, `<b>`, `<br>`, an unknown tag, an
    attribute (`<code class="x">`) and an unbalanced `<code>` stay as `Html`.
- New `crates/fe-workbench/src/tests/lsp_hover_ra_html.rs`, registered next to
  `hover_markdown` in `crates/fe-workbench/src/tests/lsp_aliases.rs`. A fake server replies
  with the fixture. Check that:
  - the popover's painted text contains no `<code>` and no `](https://`;
  - both ```rust fences (blocks 0 and 1) are drawn, with keyword-coloured glyphs for `fn`/`impl`,
    as in `lsp_hover_markdown.rs`.

  In the user's screenshot the popover is at its 16-row cap and scrolled past the fences, so this
  test also guards the "code blocks not formatted" report.

## Fixture

The fixture `crates/fe-markdown/tests/fixtures/ra_hover_transpose.md` (real rust-analyzer 1.98.1 hover text) is saved at /home/daryl/coordination/ra_hover_transpose.md; copy it to that path unchanged.
## Known limits (review round 2, y16 process)

- GPUI's `TestAppContext` records layout, not painted glyph colours, so the end-to-end hover test (C4) asserts the live popover's laid-out text (no `<code>` or `](https://`, both fences present, scrolled and unscrolled) and takes the keyword colour of each fence's glyphs from the same document rendered separately with the same theme. Asserting the colours of painted glyphs in the popover itself needs a headless renderer with a recording scene; that is a follow-up card (Mac T1024 F3).

## Implementation notes

- `html_inline::tags` runs on every paragraph's tokens after autolinks. Every opening tag, of
  any name and with attributes or not (but not void or `<x/>` ones), goes on one stack, with the
  Markdown tag it's in; a Markdown tag that closes leaves them open. A closing tag pops the
  innermost only when its name matches, and the pair maps only when the opener is a mapped tag
  with no attributes, in the same Markdown tag. So `<code><code class="x">a</code>`,
  `a <code>*<code class="x">b*</code>`, and crossed or unclosed tags stay `Inline::Html`.
  Whitespace may end a tag (`<code >`). `<code>`/`<kbd>` open `Open::Code`, whose content
  `inline::build` splices into its parent with each text run (and line ending, as a space) made
  `Inline::Code`. `<sub>`/`<sup>` have no inline, so their content shows without them.
- A line ending just inside a pair's tags, as RA's `<code>\n[Some](…)` has, moves outside them,
  or goes where the text outside already has a space there, as HTML collapses it.
- An HTML block whose first line is a mapped tag is read again after `"a\n"`, which makes its
  first line continue a paragraph. When a later row starts a block (`# x`, `- x`, `1. x`, `> x`,
  a fence, a rule, a setext underline or a table's delimiter row), a backslash escape goes
  before that row's marker (its first punctuation mark, or the `.`/`)` after a list number) and
  it's read again, until it's one paragraph. An escape decodes away in text, titles and
  destinations, and a code span holding one is read again from its source without it, so
  nothing is added to what shows. Rows that start nothing are read as written. It becomes that
  paragraph, without the lead, only if no raw HTML is left; a row that starts an HTML block, or
  that no escape stops, keeps it `BlockKind::Html`. The document's link reference definitions
  resolve in it through a broken-link callback.
- pulldown-cmark hands over an HTML block's lines without their `\r`, so `\r\n` in a code span
  there reads as CommonMark's one space, not the two a paragraph's gives.
- The workbench test builds its `TestAppContext` by hand with `build_with_text_system`, so the
  live popover's lines are recorded as the window lays them out, and scrolls the capped popover
  past its fences. gpui's `TestAppContext` can't take a headless renderer (its
  `build_with_platform` is private), so the test window paints no glyph: the fences' colours
  are read by painting them through the editor's own renderer in a `HeadlessAppContext`, as
  `lsp_hover_markdown.rs` does. The workbench re-sets each editor's renderer every frame, so a
  wrapper can't capture the Markdown it's handed; the test checks the popover holds the
  server's text instead.
- Scope also took `inline.rs` (`Open::Code`), `tests/gfm.rs` (its raw-HTML case used `<b>`, now
  `<span>`) and `lsp_hover_markdown.rs` (its `painted` helper is shared, and records the text
  it lays out).
