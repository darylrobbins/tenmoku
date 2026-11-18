---
task_uid: "01m7kp4y6mwbj2yf9jfcj86c50"
task: "T1924"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax's highlight names include markup.strong, markup.italic, markup.strikethrough, markup.quote, markup.list and markup.link.label with aliases for common grammar names; markdown strong/emphasis/strikethrough/quote/list capture them (crates/fe-syntax/tests/markup_captures.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Every theme styles markup.strong bold, markup.italic italic, markup.heading bold and markup.strikethrough struck through, with colours for the new captures; theme token tests pass"
    required: true
    manual: false
  - id: "C3"
    text: "A markdown row draws bold, italic and struck spans as styled runs with unchanged column positions, delimiters in the punctuation colour, and a deprecated semantic token is struck through (crates/fe-gpui/tests/markup_styles.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: the screenshot goldens that show markdown are regenerated and reviewed, and cargo run -p fe-workbench --example screenshots passes"
    required: true
    manual: false
---

# Markdown and markup font styles: **strong** bold, *emphasis* italic, ~~strike~~ struck through, headings bold, quotes and list markers coloured, and LSP-deprecated symbols struck through, through the themes' syntax_style

The user (2026-11-17), looking at PERF.md in the editor: "Shouldn't we actually highlight the ** ** items in bold? Any other formatting we should apply in syntax view?"

Builds on T1600 (style runs: bold and italic spans drawn with the bundled faces) and the theme's existing `[syntax_style]` table (crates/fe-config/src/theme.rs).

## Change
- crates/fe-syntax's highlight NAMES gain `markup.strong`, `markup.italic`, `markup.strikethrough`, `markup.quote`, `markup.list` (the marker) and `markup.link.label`, with ALIASES for the names grammars use (`text.strong`, `text.emphasis`, `text.strike`, `text.title`, `text.literal`, `text.uri`, `text.reference`, `markup.bold`, ...). The built-in markdown/markdown_inline queries (and the injected markdown in doc comments) capture strong_emphasis, emphasis, strikethrough, block quotes and list markers with them.
- Every theme gets colours for the new captures (they may reuse existing tokens) and default font styles: `markup.strong` bold, `markup.italic` italic, `markup.heading` bold, `markup.strikethrough` struck through. Strikethrough is a decoration (GPUI TextRun strikethrough), not a face; add it to SpanStyle if T1600 didn't.
- LSP: a semantic token with the `deprecated` modifier is drawn struck through (the modifier is already advertised in crates/fe-lsp/src/semantic_tokens.rs); diagnostics tagged Deprecated likewise. Unnecessary code stays faded as today.
- The `**`/`*`/`~~` delimiters keep the punctuation colour; only the text between them is styled. Column advance is unchanged (all faces monospaced).

## Tests and goldens
- fe-syntax: capture tests for markdown strong/emphasis/strikethrough/quote/list and the aliases.
- fe-gpui: a drawn-row test that bold, italic and struck spans in a markdown row produce styled runs and unchanged column positions; deprecated semantic token is struck through.
- Theme token tests updated per the theme-token rules (design_role_tokens, Ember and Monokai keys).
- macOS screenshot goldens: a markdown scene showing the styles (new or existing), regenerated on the Mac.

## Implementation notes

- **Vocabulary.** fe-syntax gains `markup.strong`, `markup.italic`, `markup.strikethrough`, `markup.quote`, `markup.list` and `markup.link.label`, with aliases `text.strong`, `text.emphasis`, `text.strike`, `text.quote`, `markup.bold`, `markup.emphasis`, `markup.strike`, `emphasis`, `emphasis.strong`, `title`, `link_text`, `link_uri` and `punctuation.list_marker` (nvim-treesitter's old names, Helix's and Zed's). tree-sitter-md's own query already says `text.strong`/`text.emphasis`, so those now resolve; `queries/markdown.scm` and `queries/markdown-inline.scm` (after the crate's, later patterns win) add block quotes, list and task-list markers, strikethrough and link labels.
- **Doc comments.** `rust_doc_comment_colours` pins that emphasis in a Rust doc comment keeps the comment colour, as VS Code does. So `rustdoc_inline` captures it as new names `comment.documentation.{strong,italic,strikethrough}`: coloured by prefix as the comment, styled by markup's face added to the doc comment's own style (bold text in Yohen's italic comments is bold italic). The delimiters are `comment.documentation`.
- **Styles every theme has.** `[syntax_style]` was a theme's own, so Kiln's themes and user themes had none. Markup's meaning shouldn't depend on the theme, so `fe_config::theme::MARKUP_STYLES` (headings and strong bold, emphasis italic, strikethrough struck, and the doc-comment three) is every palette's base, and a theme's table sets any of them otherwise by exact name (`"markup.strong" = "normal"`). `FontStyle` gains `strikethrough` (the word `strikethrough` in a style string).
- **Colours.** Each of the 13 bundled themes colours the six new captures itself, so none takes Yohen's or Hakuji's colour for its appearance. The VS Code and Catppuccin themes take upstream's markdown scopes (dark_vs/2026/monokai `markup.bold`, `markup.italic`, `markup.quote`, list bullets; catppuccin-vsc's `tokens/markdown.ts`): strike is upstream's subtext0 for Catppuccin and the text colour where upstream only strikes through. Yohen and Hakuji draw strong and emphasis in ink and let the face carry them, struck text in the comment's colour, quotes in the string's, markers as special punctuation (as before), labels as links. Tenmoku Dark/Light/Ember follow One Dark (strong the constant's, emphasis the keyword's).
- **Drawing.** `SpanStyle` gains `strikethrough`; the decoration runs carry a 1 px GPUI strikethrough in the run's colour. It's not a face, so shaping is unchanged.
- **Nested markup** (review F1). Highlights flatten to one winner a byte, so `Highlights::markup` also lists every markup capture in the range (`Highlight::is_markup`: `markup.*` and `comment.documentation.*`), of every layer, nested ones too. fe-gpui's `nested_faces` gives a markup span the union of the faces of the markup around it, colour from the winner: `***x***` and a heading's emphasis are bold italic, `~~**x**~~` bold and struck. Delimiters (`punctuation.*`, a doc comment's `comment.documentation`) and code aren't markup, so take no face.
- **Doc-comment blocks** (review F3, rounds 1 and 2). A run of `///` lines, or of `//!` lines, is one layer of block Markdown, `rustdoc`, its prefixes left out by the `injection.doc-prefix` mechanism C# doc comments use; Rust's `line_comment` takes its line break, so a run's next line is found from the row its text ends on (`last_row`). A `/** */` or `/*! */` comment is one too, less the ` * ` starting its lines: a new `injection.line-prefix "*"` property leaves out the blanks and the prefix starting each line, only when every line after the first has it followed by a blank (review F7: an undecorated comment's `**bold**`, `*slanted*` and `* item` lines keep their stars); it reads a chunk at a time, checking the deadline at each (F9). A `//!!` line is an inner doc line and joins its run: only a prefix ending in a repeated character (`///`) rejects one more of it (F10). So lists nest however deep, quotes span lines, and a fenced example is a code block, not prose. The quote is `comment.documentation.quote`, markers `comment.documentation.list` (comment-coloured by prefix), with `rustdoc_inline` injected into its paragraphs. Injections now nest four deep, so a Markdown fence's Rust keeps its doc comments' inline markup. The Rust highlight golden didn't move: its two `///` lines are plain prose either way.
- **Markup faces under decorations** (review F4). A `Foreground` decoration (a doc-comment semantic token, a TODO) recolours markup and keeps its face: `nested_faces` marks text in markup, and `with_faces` adds markup's face back after the decorations are laid over.
- **One strike for deprecated code** (review F5). Deprecated diagnostics join the `Strikethrough` decorations' struck ranges (`deprecated_ranges`), in the text area and sticky-scroll rows, so a symbol deprecated by both is struck once, as GPUI draws the run; the diagnostic's own strike quad is gone. Squiggles and Unnecessary fading are unchanged.
- **Escapes** (review F6). An escaped character (`escape`) takes the markup's faces around it in its own colour, as text in it; delimiters still take none.
- **Reference links** (review F2). A collapsed reference link's `[text][]` text stays `markup.link`, so spell check reads it; only a full reference's `[label]` is `markup.link.label`.
- **Deprecated.** fe-editor gains `DecorationKind::Strikethrough`; the workbench adds one for a semantic token with the `deprecated` modifier (`modifier_marks`, beside `mutable`'s underline). fe-gpui strikes the spans under it, keeping colour and face, in the text area and sticky-scroll rows. Deprecated diagnostics were already struck.
- **Goldens.** A new screenshots scene, `markdown_scene`, renders a README in Yohen and Hakuji (`markdown_dark.png`, `markdown_light.png`); no existing golden shows a Markdown file.
