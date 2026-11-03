---
task_uid: "01m6ezvfn5zh33vbswke4m33v2"
task: "T421"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test spell_ranges_tests::rust_comments_and_strings_only (cargo nextest run -p fe-workbench) passes: for a rust buffer with a line comment, a doc comment, a string literal, an identifier and a keyword, ranges cover the comment and string text (not the // or quote delimiters) and nothing else, with scopes comments and strings individually switchable. Known limits (follow-up T430): delimiter handling for shell heredoc markers and injected named raw-string delimiters; literal structure told from node-name substrings (e.g. Python string forms); language-dependent escape widths in injected strings"
    required: true
    manual: false
  - id: "C2"
    text: "Test spell_ranges_tests::string_escapes_and_format_holes_are_excluded (cargo nextest run -p fe-workbench) passes: in \"hello\\nworld {name}\" the \\n escape and {name} are excluded so 'nworld' is never checked. Known limit: escape widths in injected-language strings are language-independent (e.g. JavaScript hex escapes inside an injection); follow-up T430"
    required: true
    manual: false
  - id: "C3"
    text: "Test spell_ranges_tests::markdown_prose_only (cargo nextest run -p fe-workbench) passes: headings, paragraphs, list items, emphasis and block quotes are spellable; fenced and indented code, inline code spans, link destinations and autolinks, HTML tags, reference definitions and front matter are not"
    required: true
    manual: false
  - id: "C4"
    text: "Test spell_ranges_tests::plain_text_is_all_spellable (cargo nextest run -p fe-workbench) passes: a plain text buffer with no grammar is one range; a language with a grammar but no comment or string captures yields none"
    required: true
    manual: false
  - id: "C5"
    text: "Test spell_ranges_tests::injected_languages_follow_their_layer (cargo nextest run -p fe-workbench) passes: a rust fence in Markdown is excluded as code, but a comment inside that rust fence is not spellable (v1: fenced code is skipped entirely)"
    required: true
    manual: false
  - id: "C6"
    text: "Test spell_ranges_tests::viewport_window_and_large_files (cargo nextest run -p fe-workbench) passes: ranges_in(row_range) returns only ranges intersecting the rows, expanded to whole tokens of the host language; a buffer over 2 MB or in large-file mode returns none; the function never reparses the buffer (it reads highlight spans from the already-parsed syntax). Known limit: tokens in injected languages are not expanded whole at a viewport edge (the injected layer's token boundaries are not exposed); a follow-up card holds the injected-tree API"
    required: true
    manual: false
  - id: "C7"
    text: "Test (cargo nextest run -p fe-syntax) passes: highlights reports whether it completed, and an injection that was skipped is reported by a separate 'skipped' flag in crates/fe-syntax/src/injection.rs (not complete=false); a finished parse reports complete, an interrupted or unparsed-edit one does not, and spellable_ranges treats an incomplete result as no ranges; cargo nextest run -p fe-syntax and -p fe-workbench pass"
    required: true
    manual: false
---

# Find the spellable ranges of a buffer: comment and string tokens by tree-sitter highlight, and prose of plain text and Markdown without code, links or front matter (Mac T1333)

## Problem
f9a checks text it is given; we must decide which text. Code identifiers must never be checked.

## Change
New crates/fe-workbench/src/spell_ranges.rs: `spellable_ranges(syntax: Option<&Syntax>, text, scopes, rows) -> Vec<Range<usize>>`. For languages with a grammar use the existing highlight spans (fe-syntax HighlightSpan with Highlight names `comment*` and `string*`; see crates/fe-syntax/src/highlight.rs) and trim delimiters; for Markdown use the existing markdown parsing in crates/fe-markdown (reuse, add no parser); for plain text return the whole range. Mac call (option A, after review round 1): Markdown goes through Syntax's tree and highlights like every other language (no separate fe-markdown path needed), and fe-syntax is in scope for a small completeness API: `highlights` reports whether it completed (a partial or timed-out result is distinguishable from a finished one), and `email_autolink` may be captured in fe-syntax too. Do not edit fe-markdown; read its public API only.

## Implementation notes

- Markdown goes through `Syntax`'s tree and highlights, per the Mac call in the Change section: the block tree's `inline` nodes, less what the inline layer paints.
- The completeness API is a new `Syntax::checked_highlights`, returning `Highlights { spans, complete }`, beside `highlights`, whose signature stays since fe-gpui and fe-nav call it and aren't in scope. `complete` is false for a query or injection that ran out of time, a missing injected language, a panic, a window, a stale tree or none; a result cached at the time cap keeps its false. An injection left out for good, by `MAX_INJECTIONS`, `MAX_LAYER_RANGES`, `MAX_DEPTH` or a grammar that can't be set up, makes it false too through a separate `skipped` flag, which leaves such a result cached, as requerying wouldn't find it. `spellable_ranges` gives no ranges for any of those (F2, F3, C7).
- `email_autolink` is captured in fe-syntax as `markup.link.url` (F6), so email autolinks now colour like URL ones in the editor.
- `ranges_in(row_range)` in C6 is the `rows` argument of `spellable_ranges`.
- "`string*`" is narrowed to `string`, and "`comment*`" leaves out `comment.documentation.code`. `string.placeholder`, `string.special` (regular expressions, symbols) and `string.special.key` (object keys) name things rather than say words.
- A token that crosses `rows` comes back in as many ranges as it has lines, so a range of it can lie outside `rows`; a token that doesn't touch them never does. Tokens are the tree's nodes with `string` or `comment` in their kind, so escapes and holes don't split one. Runs of `///` lines count as one token, as their highlights merge.
- A string's delimiters, escapes and interpolations come from the host tree: whatever children of the literal aren't named as its content (F5, F8). Text trimming remains only for a string the tree has as one token and for strings in an injected language. Raw HTML in Markdown is cut by scanning the inline source (F9), which also loses a `<b and c>` that merely looks like a tag. An attribute quote that doesn't close on its line is one bad tag, per the Mac call on F9: the tag ends at the next `>` on that line, else before the next `<`, else at the line's end, and later tags are read for quotes as usual. A quoted value that really does span lines is read the same way.
- Each request walks the tree once for its literals and each span once, and a line's braces are read once (F12); the scaling tests call the step after highlighting directly, since highlighting's time budget would otherwise just report incomplete. A string nested in an interpolation is code, with no text (F10). Only tokens in an enabled scope expand across `rows` (F13).
- Known limits: tokens in injected languages are not expanded whole (F4, follow-up T430 holds the injected-tree API). Also for T430: shell heredoc markers and injected named raw-string delimiters (F5), escape widths in injected strings (F8), and literal structure told from node-kind substrings (F11). Markdown table cells are left out, since the grammar gives them no inline layer to tell code from words. `%s`-style holes aren't cut, as the tokenizer skips short words. Backticks in comments are only code where the grammar says so (Rust doc comments). A buffer edited since its last parse gets no ranges (review F3).
