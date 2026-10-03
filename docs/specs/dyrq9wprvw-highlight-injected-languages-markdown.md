---
task_uid: "01m417z43qs1wb23dyrq9wprvw"
task: "T16"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test injection::tests::markdown_inline_and_fenced_code_are_highlighted passes: code spans and links get inline-grammar highlights and a rust fence gets Rust keyword highlights"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test injection::tests::html_script_and_style_are_highlighted passes"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test injection::tests::only_injections_overlapping_the_range_are_parsed passes"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax test injection::tests::unknown_injected_languages_stay_plain passes: an unknown fence language or regex/jsdoc injection keeps host highlights without panicking"
    required: true
    manual: false
  - id: "C5"
    text: "fe-syntax test tests::golden::highlights_match_golden_files passes with regenerated markdown, html, javascript, typescript and tsx goldens"
    required: true
    manual: false
---

# Highlight injected languages: Markdown inline and fenced code, HTML script/style, JS template strings

## Problem

fe-syntax has no language injections (`crates/fe-syntax/src/lib.rs` says so, and `language.rs:254` notes that Markdown is "block structure only"). Markdown only highlights its blocks: inline emphasis, code spans and links need tree-sitter-md's inline grammar. Code in fenced blocks, HTML `<script>`/`<style>`, and JS/TS tagged template strings stay plain.

This card covers injected layers parsed on demand for the range being highlighted. Keeping layer trees across edits and reparsing them incrementally is the follow-up card (T-12 part 2), which depends on this one.

## Change

- **Queries.** Each `Def` in `crates/fe-syntax/src/language.rs` gets an optional injections query, compiled with its highlights: tree-sitter-md `INJECTION_QUERY_BLOCK`, html `INJECTIONS_QUERY`, javascript `INJECTIONS_QUERY` (also used by typescript and tsx, which ship none). Add an internal `markdown_inline` language (`INLINE_LANGUAGE`, `HIGHLIGHT_QUERY_INLINE`, `INJECTION_QUERY_INLINE`) that `language_by_name` resolves but `language_names()` and path lookup don't list. Use only the queries the grammar crates ship. They are MIT; the md inline highlight query says it comes from nvim-treesitter (Apache-2.0, allowed). Record each in `THIRD_PARTY.md`.
- **Clean room.** Never open Zed's `language`/`languages` crates or Zed's injection or highlight queries (`CLEAN_ROOM.md`). gpui-kit's highlighter (Apache-2.0) is an allowed reference; any code adapted from it carries the header and a `THIRD_PARTY.md` row.
- **Resolution.** Support `@injection.content`, `@injection.language` (the captured text) and `#set! injection.language`, `injection.combined` and `injection.include-children`. Map common aliases to our names (`js`, `ts`, `sh`/`shell`, `py`, `yml`, `c++`), case-insensitively. A language that's unknown (`regex`, `jsdoc`, `latex`) or left out of this build by Cargo features stays plain. It never fails.
- **Layers.** In a new `crates/fe-syntax/src/injection.rs`: for a host tree and a byte range, find the injections whose content overlaps the range, and parse each with its own `Parser` using `set_included_ranges`. Injections nest one level deeper as needed: markdown block, then inline, then HTML. `Syntax::highlights(range)` uses it, so only layers overlapping the requested (visible) range are ever parsed. Layers are cached with the highlight cache and thrown away when the tree changes. Layer parsing shares the per-call highlight budget, so a layer that doesn't fit stays plain for that call. Huge fenced blocks get incremental reparsing in part 2.
- **Merging.** Inside a layer's ranges its spans win over the host's. Bytes the layer leaves uncaptured keep the host highlight (fenced code keeps `markup.raw` underneath). Merge in `crates/fe-syntax/src/highlight.rs` next to `flatten`.
- Capture names our vocabulary lacks (`text.emphasis`, `text.strong`) stay unmapped. Italic and bold are T-14's open font-style question.
- Extend the golden samples `markdown.md` (inline markup, a fenced Rust and a fenced unknown-language block, front matter) and `html.html` (script and style), and regenerate the `.golden` files for markdown, html, javascript, typescript and tsx.

Tests go in `crates/fe-syntax/src/injection/tests.rs`.

## Implementation notes

- **Children.** Without `injection.include-children`, only a content node's *named* children are cut out, such as the `block_continuation` that carries a block quote's `> `. tree-sitter-md exposes the content's punctuation as anonymous children of `inline`, `code_fence_content` and `html_block`, and cutting those out broke every layer.
- **Combined.** `injection.combined` joins a content node with its siblings of the same kind (the pieces of one template string around its `${…}`s), not with every match in the file. That way a layer's ranges, and its cache key, don't depend on which range is being highlighted.
- **Compiling injected languages.** `language_by_name` blocks for up to 20 ms, so `highlights` never compiles a query. An injected language that isn't compiled yet stays plain and is listed by `Syntax::missing_languages()`. After each frame's layout, `EditorView::load_injected_languages` compiles any missing ones on GPUI's background executor and then calls `notify`, the same way the view already loads the host language. Nested injections, such as HTML inside Markdown inline, load in a later round the same way. fe-syntax's unit and golden tests use the test-only `Syntax::highlights_compiling`, which compiles missing languages on the spot. Because of this, the change reaches outside the card's scope: `crates/fe-gpui/src/{view,element,tests}.rs` and one line of `crates/fe-syntax/src/tests/golden.rs`.
- **Caching.** A result isn't cached if an injection was left plain because its language is missing, or because time ran out while finding the injections, preparing their ranges or parsing them. The next call tries again and keeps the layer trees already parsed, which stay cached until the tree changes. A host query cut short by the budget is still cached as before. A layer that never fits in the 2 ms budget is retried on every call; part 2's incremental reparsing fixes that.
- **Deadline checks.** Besides the query and parse progress callbacks, the deadline is checked for each match, each sibling of a combined injection, each named child left out of the content, and each range converted to a point.
- **Cost.** Release build, 60 rows of the Markdown bench corpus (28 layers: inline paragraphs, list items and a Rust fence): the host query takes about 50 µs, and parsing and querying every layer cold about 0.4 ms.
- **What the shipped queries leave plain.** Each inline `html_tag` is its own layer, so `</kbd>` alone parses as an erroneous end tag and its name gets no `tag` highlight. Pipe-table cells aren't injected with inline markup, because the block query only injects `inline` nodes.
