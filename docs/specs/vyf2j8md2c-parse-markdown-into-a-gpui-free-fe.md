---
task_uid: "01m55c6eg96mssyavyf2j8md2c"
task: "T161"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-markdown test blocks_carry_their_source_lines (tests/blocks.rs): for a fixture of a heading, a two-line paragraph, a nested list, a fenced block, a table and a quote, each Block's lines, each ListItem's and each table Row's equal the 0-based source rows they span (end exclusive), with CRLF and LF fixtures alike"
    required: true
    manual: false
  - id: "C2"
    text: "fe-markdown test gfm_constructs_parse (tests/gfm.rs): table alignments and cells, [x]/[ ] task items, ~~strikethrough~~, \u003chttps://..\u003e autolinks, and bare https://, www. and e-mail literals become Link inlines with GFM's trailing-punctuation and unmatched-paren trimming, but not inside code spans or existing links"
    required: true
    manual: false
  - id: "C3"
    text: "fe-markdown tests headings_get_github_slugs (tests/blocks.rs: Hello, World! -\u003e hello-world, a repeat -\u003e hello-world-1, C++ \u0026 Rust -\u003e c--rust) and fenced_code_is_highlighted_by_its_fence_language (tests/code_blocks.rs: rust, py3 and js:app.js fences get fe-syntax keyword spans; unknown, absent and indented code get none) pass"
    required: true
    manual: false
  - id: "C4"
    text: "fe-markdown tests link_targets_resolve_as_vs_code_opens_them and only_local_images_inside_the_root_or_document_folder_load (tests/targets.rs) pass: #frag -\u003e Anchor, http(s)/mailto -\u003e Web, relative and file: -\u003e File with #L12 -\u003e line 11 and /x against the root, javascript:/data: -\u003e Unsupported; images: inside root or doc folder -\u003e Local, ../ outside -\u003e Blocked(Outside), http(s)/data: -\u003e Blocked(Remote)"
    required: true
    manual: false
  - id: "C5"
    text: "pulldown-cmark is a workspace dependency with default-features = false; cargo deny check passes; scripts/check-no-gpui.sh prints ok: fe-markdown; cargo clippy -p fe-markdown -p fe-syntax --all-targets -- -D warnings is clean and no existing fe-syntax test is edited"
    required: true
    manual: false
---

# Parse Markdown into a GPUI-free fe-markdown document with source lines, GFM tables, task lists, strikethrough and autolinks, highlighted fences and resolved link and image targets (Mac T846)

## Problem

The user, 2026-10-17: "We also need a markdown preview view", modelled on VS Code's built-in Markdown preview:
two commands, a read-only preview tab that re-renders as the source changes, scroll sync by source line,
CommonMark plus GFM, highlighted fences, working links, local images with remote ones blocked, the theme's
colours, a "Preview README.md" title.

fastedit has no Markdown model at Mac main 92c57c57. Nothing in the lock parses Markdown (`Cargo.lock` has no
`pulldown-cmark` or `markdown`). gpui-pre 0.3.7 has no Markdown renderer. gpui-kit's rich-text view
(`crates/base/src/text/`, gpui-kit@2154ba84) parses with the `markdown` crate (mdast) and is tied to its own
theme, selection and GlobalState, so its parser isn't worth copying (the renderer card copies some rendering
pieces).

This is the first of four cards (model, renderer, workbench item and commands, scroll sync). This card builds
the GPUI-free model that the other three use. It can be tested headless, and CI's no-GPUI check
(`scripts/check-no-gpui.sh`) keeps it GPUI-free.

## Change

**Parser: pulldown-cmark 0.13 (MIT).** `deny.toml`'s allow list has MIT. Its only new transitive crate is
`unicase` (MIT OR Apache-2.0); `bitflags` 2 and `memchr` are already in the lock. Use it with
`default-features = false`, which drops `getopts` and the `html` feature. It is the fastest of the candidates,
and its `into_offset_iter` gives each event its byte range, which this card turns into source lines. The
`markdown` crate (MIT) has GFM autolink literals built in but is several times slower on every live re-parse,
so it isn't used.

Workspace `Cargo.toml`:
- `pulldown-cmark = { version = "0.13", default-features = false }`, commented as above.
- `percent-encoding = "2"`, already in the lock, for link destinations.
- `fe-markdown = { path = "crates/fe-markdown" }`.

New crate `crates/fe-markdown` ("Markdown documents parsed for the preview, with source lines. Must not depend
on GPUI."). Its dependencies are pulldown-cmark, percent-encoding, fe-syntax and fe-buffer. Files:

- `src/lib.rs`: the public types and `pub fn parse(text: &str) -> Document`. It is pure and `Send`, so the
  workbench can run it on a background thread.
  - `Document { blocks: Vec<Block> }`, where `Block { kind: BlockKind, lines: Range<u32> }` and `lines` are the
    0-based source rows the block spans, end exclusive.
  - `BlockKind`: `Paragraph(Vec<Inline>)`; `Heading { level, slug, content }`; `Quote(Vec<Block>)`;
    `List { start: Option<u64>, tight, items: Vec<ListItem> }`, where
    `ListItem { task: Option<bool>, blocks, lines }`; `Code { info, language: Option<&'static str>, code,
    highlights: Vec<fe_syntax::HighlightSpan> }`; `Table { align: Vec<Align>, head: Vec<Cell>,
    rows: Vec<Row> }`, where `Row { cells, lines }`; `Rule`; `Html(String)`.
  - `Inline`: `Text`, `Code`, `Emphasis`, `Strong`, `Strikethrough`, `Link { dest, title, content }`,
    `Image { dest, title, alt }`, `SoftBreak`, `HardBreak`, `Html`.
  - Raw HTML is kept as its source text. The preview shows it escaped and doesn't render it.
- `src/parse.rs`: walks `Parser::new_ext(text, ENABLE_TABLES | ENABLE_TASKLISTS | ENABLE_STRIKETHROUGH)` with
  `into_offset_iter`, through `TextMergeWithOffset`. It maps byte offsets to rows with a line-start index built
  once per parse. Footnotes, math, smart punctuation and heading attributes stay off, as in VS Code's preview.
- `src/autolink.rs`: GFM's autolink literals, which pulldown-cmark doesn't do. In text outside code spans and
  links, bare `http://` and `https://` URLs, `www.` hosts (dest `http://www.…`) and e-mail addresses (dest
  `mailto:…`) become `Inline::Link`. GFM's rules apply: trailing `?!.,:*_~` is trimmed, and so is a `)` that
  has no match inside the URL. `<https://…>` autolinks come from pulldown-cmark as links already.
- `src/slug.rs`: heading slugs, as VS Code's `githubSlugifier` makes them. Take the heading's text content,
  trim it and lowercase it. Remove ASCII punctuation other than `-` and `_`, turn each whitespace char into
  `-`, then number repeats in document order (`-1`, `-2`). For example, `Hello, World!` gives `hello-world`
  and `C++ & Rust` gives `c--rust`.
- `src/code.rs`: fenced code highlighting. `language` is `fe_syntax::fence_language(info)`, and highlights
  come from an `fe_syntax::Syntax` over `fe_buffer::Buffer::new(code).snapshot()`, parsed whole with
  `parse_within` and then `highlights(0..len)`. An unknown or absent language gets no highlights; an indented
  code block gets none either.
- `src/target.rs`: where a link or image points, resolved against the document's path and the workspace root
  (`None` with no folder open, in which case the document's folder stands in for it):
  - `pub fn link_target(dest, doc: &Path, root: Option<&Path>) -> LinkTarget`.
    - `#frag` gives `Anchor(slug)`: percent-decoded, matched against heading slugs as written, then
      lowercased.
    - `http:`, `https:` and `mailto:` give `Web(url)`.
    - `file:` URLs and relative paths give `File { path, line }`. The path is percent-decoded, `?query` is
      dropped, a leading `/` resolves against the root and anything else against the document's folder.
      A `#L12` or `#L12C3` fragment gives `line: Some(11)`; other fragments give `None`.
    - Any other scheme (`javascript:`, `data:`, `vscode:` and the rest) gives `Unsupported`.
  - `pub fn image_source(dest, doc, root) -> ImageSource`. It returns `Local(path)` only for a relative path or
    `file:` URL whose canonical path (lexically normalised if the file doesn't exist) lies inside the root or
    the document's folder, as VS Code's preview limits local resources to those folders. `http(s):`, `data:`
    and every other scheme give `Blocked(BlockReason::Remote)`, and local paths outside both folders give
    `Blocked(BlockReason::Outside)`. Remote images are always blocked; there is no setting yet.

`crates/fe-syntax/src/fence.rs` (new), with `mod fence; pub use fence::fence_language;` in
`crates/fe-syntax/src/lib.rs`:
`pub fn fence_language(info: &str) -> Option<&'static Language>`. It is the `injected_language` lookup
(`language.rs`, which is `pub(crate)` and unchanged) that Markdown fences already use: name, extension or
`ALIASES`, including T772's VS Code names (landed as c79049ce), and cut at `,` or `:`. A `Missing` language is
compiled with `language_by_name`. `language.rs` isn't touched (T790 holds it).

Tests are integration tests under `crates/fe-markdown/tests/`: `blocks.rs`, `gfm.rs`, `code_blocks.rs` and
`targets.rs`.


## On helium (Mac lead, note zzs)

Helium's trunk does not carry Mac T401 (the gpui-pre bump, which owns Cargo.toml and Cargo.lock on the Mac). Add the pulldown-cmark workspace dependency and the fe-markdown member here against helium's current Cargo.toml and Cargo.lock, touching nothing else in either file; the Mac's stack resolves Cargo.lock against T401 when it picks this card.

## Implementation notes

- `fence_language` takes the info string's first word before the `injected_language` lookup, so
  ```` ```rust ignore ```` is Rust, as the Markdown grammar's `language` node is.
- `LinkTarget::Anchor` holds the decoded fragment; `Document::heading(slug)` does the matching (as written,
  then lowercased) and `Document::headings()` lists every heading's slug, including those in quotes and lists.
- Repeated slugs are numbered as github-slugger does, skipping a number another heading already took
  (`a`, `a-1`, `a` gives `a`, `a-1`, `a-2`).
- URL and `www.` literals are found in the paragraph's source, as cmark-gfm finds them while it reads it, not
  in pulldown-cmark's decoded text (`src/inline.rs`): `https://a.com/foo*bar*baz` is one link,
  `www.a.com/b&amp;` ends before the entity, and a delimiter whose pair a link took shows as written
  (`https://a.com/*b c*` is a link and ` c*`), as does the rest of a tag a link ends in (`](z "title")`). So
  text events aren't merged through `TextMergeWithOffset`: each entity or escape stays an event of its own,
  whose source differs from its text, and adjacent text is joined when the tree is built. A link's text shows
  entities and escapes decoded. Links start at a line's or cell's first character too (`>www.a.com`, after a
  break, `|www.a.com|`). `http(s)://` may follow anything but a letter, as cmark-gfm reads the scheme back over
  letters; `www.` needs a line start, whitespace or `*_~(`.
- E-mail addresses are found afterwards in each run of decoded text, as cmark-gfm's postprocessing finds them:
  `foo\+bar@example.com` and `foo&#64;example.com` are addresses.
- Trimming follows cmark-gfm's `autolink_delim`: besides `?!.,:*_~` and unmatched `)`, it drops a trailing `'`,
  `"`, `;` and an entity reference such as `&amp;`, and ends a link at `<`. A domain needs segments with an
  alphanumeric each (two for `www.`) and no `_` in its last two; a final `.` or `_` counts only if the link goes
  on past it (`www.a.com_.` links `www.a.com`, `www.a.com_/x` isn't a link). A domain longer than DNS's 253
  bytes isn't a link, which keeps rejected candidates such as `_www._www._www.…` linear.
- Quotes, list items and inline markup nest at most 64 deep together (`MAX_NESTING`); deeper ones are left out
  and their content kept, so a document of thousands of `>`s can't run the parsing thread out of stack.
  pulldown-cmark itself handles such nesting without recursing.
- A block's lines leave out trailing blank lines, except those that are a code or HTML block's content (an
  unclosed fence or `<pre>` runs to the end); a quote or list item ends no earlier than its last block.
- `image_source` makes paths absolute (a bare file name's folder is the current directory) and resolves
  symlinks as far as the path exists, so a missing file under a linked folder that points outside is
  `Outside` too. A folder that can't be resolved allows nothing.
- Fenced code is stored with `\n` line endings so highlights line up in CRLF documents.
- The dev-dependency on fe-syntax turns on `no-query-budget`, as fe-syntax's own tests do, so highlight
  assertions can't be cut short under load.
