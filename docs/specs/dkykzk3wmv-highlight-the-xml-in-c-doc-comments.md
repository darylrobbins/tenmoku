---
task_uid: "01m7cw16gjsnerhjdkykzk3wmv"
task: "T649"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features csharp,xml --test csharp_xml_doc_highlights passes: xml_doc_tags_and_attributes_are_highlighted, multi_line_summary_tags_are_highlighted, doc_comment_marker_stays_comment, plain_comments_are_not_injected, self_closing_see_cref"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features csharp injection::doc_prefix_tests passes (the engine directive or property, documented in injection.rs's module doc)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --features csharp --test wasm_parity csharp_extension_is_as_built_in passes and compares injections: extensions/grammars/csharp/injections.scm equals crates/fe-syntax/queries/csharp-injections.scm; cargo nextest run -p fe-syntax --features regex,csharp,java --test regex_injections_csharp_java still passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax --features csharp highlights_match_golden_files passes with csharp.cs.golden regenerated; only doc-comment spans changed"
    required: true
    manual: false
---

# Highlight the XML in C# /// doc comments without a language server: inject xml (the /// kept comment colour), tags and attribute names coloured, built-in and wasm csharp injections identical (Mac T1835)

# Highlight the XML in C# `///` doc comments without a language server: inject xml, tag and attribute names coloured

User-approved 2026-11-13. Today a C# `/// <summary>…</summary>` line is one `comment` span: tags,
attribute names and values are all comment colour unless a server's semantic tokens say otherwise
(see the separate semantic-tokens card).

## Facts (checked at the base)

- XML is a **built-in** language (`xml` is a default feature of fe-syntax, `tree-sitter-xml` 0.7.0,
  `LANGUAGE_XML`), so no grammar ships with this card. There is no `extensions/grammars/xml`, and
  none is needed: injections name it as `xml`.
- C# ships as the **WebAssembly extension** `extensions/grammars/csharp` (what users run); the
  built-in `csharp` `Def` (feature `csharp`, `crates/fe-syntax/src/language.rs`) exists for the tests
  and `wasm_parity`. T1825 adds the built-in's `crates/fe-syntax/queries/csharp-injections.scm`
  (regex) and leaves the extension's `injections.scm` empty "until someone decides".
- tree-sitter-c-sharp 0.23.5's `comment` is one token per line **including** the `///`
  (`token(seq('//', /[^\n\r]*/))`), so the content node starts with `///` and a `<summary>` block is
  three sibling nodes.
- The injection engine (`crates/fe-syntax/src/injection.rs`) has `injection.combined` (combines a
  node with **every** same-kind sibling under its parent, so plain `//` comments and other members'
  doc blocks would join), `include-children` and our `include-gaps`. It has no `#offset!`. T1826
  (Lent to helium) may add an offset or delimiter-trim directive for PHP `preg_*`: read what it
  added first and reuse it if it fits.
- Precedent: `crates/fe-syntax/queries/rust-injections.scm` injects `rustdoc_inline` one layer per
  `///` line, a stated limit.
- tree-sitter-xml's `document` wants one root element; a lone `<summary>` or `</summary>` line, or
  several elements in a row, parse with error recovery.

## What to build

1. Inject `xml` into `///` comments (not `//` or `/* */`), with the `///` left out of the injected
   ranges and kept comment colour. Choose the smallest engine change that makes the criteria pass:
   a per-node start offset (nvim's `#offset!` semantics, or T1826's directive), and if per-line
   layers can't colour a lone `</summary>`, a way to combine only a **consecutive run** of `///`
   siblings into one layer. Any engine addition is a property or directive of our own, documented in
   `injection.rs`'s module doc, with its own test. Bytes the XML layer leaves uncaptured (text)
   keep the comment colour, as the engine already does.
2. The query goes in **both** `crates/fe-syntax/queries/csharp-injections.scm` (after T1825's regex
   patterns) and `extensions/grammars/csharp/injections.scm`, which become identical. That also ships
   T1825's regex injections to the WebAssembly C# (the built-in regex grammar is hidden but present),
   answering T1825's open point; say so in both headers. Update `wasm_parity.rs`'s
   `csharp_extension_is_as_built_in` to compare the injections (read `csharp-injections.scm` at run
   time, as it reads `csharp.scm`). The wasm is unchanged, so no `pack.toml` or registry edit.
3. Header comments state the limits (e.g. `/** */` doc comments not injected; whatever the chosen
   layer shape can't do).

## Colours

With T1816 (xml.scm, attribute names as `attribute`) landed first: element names `tag`, attribute
names `attribute`, attribute values `string`, text the comment colour.

## Tests and golden

- New `crates/fe-syntax/tests/csharp_xml_doc_highlights.rs`,
  `#![cfg(all(feature = "csharp", feature = "xml"))]`, patterned on `tests/c_printf_highlights.rs`
  (`spans(lang, text)` with `language_by_name("csharp")`), as T1816 and T1825 do. Cases:
  - `xml_doc_tags_and_attributes_are_highlighted`: `/// <param name="count">How many.</param>`
    gives `("param","tag")` twice, `("name","attribute")`, `("\"count\"","string")` (or the value's
    span as the xml query gives it), and `How many.` stays `comment`;
  - `multi_line_summary_tags_are_highlighted`: a three-line `/// <summary>` / `/// Text` /
    `/// </summary>` block gives both `summary` names `tag`;
  - `doc_comment_marker_stays_comment`: every `///` span is `comment`;
  - `plain_comments_are_not_injected`: `// <b>x</b>` and `/* <b>x</b> */` stay one `comment` span;
  - `self_closing_see_cref`: `/// <see cref="Foo"/>` gives `("see","tag")`, `("cref","attribute")`.
- Engine directive or property tests in a new file declared from `injection.rs`
  (`#[cfg(test)] #[path = "injection/doc_prefix_tests.rs"] mod doc_prefix_tests;`), not
  `injection/tests.rs` (held by T1823 and T1826).
- Golden: add a multi-line `<summary>`, a `<param name="…">` and a `<see cref="…"/>` to
  `crates/fe-syntax/tests/golden/csharp.cs` (line 9 already has a one-line summary), regenerate
  `csharp.cs.golden` with `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --features csharp golden`,
  and review the diff: only doc-comment spans may change.

## Notes

- Record design calls (the layer shape, the directive) in the card's decision file,
  `docs/decisions/<lane>-*.md`.
- Linux-OK: text goldens only, no screenshot golden moves.

## Implementation notes

- Engine: `injection.doc-prefix "///"` (ours, `injection.rs` module doc and
  `DocRun`): cuts the prefix, skips `////`, combines consecutive-row runs of
  `///` siblings into one layer. Tests in `injection/doc_prefix_tests.rs`.
- Design record: `docs/decisions/dkykzk3wmv-csharp-xml-doc-injection.md`.
- XML value spans: tree-sitter-xml paints an attribute value's quotes as
  `punctuation.delimiter`, so the `string` span is the text inside them
  (`count`, `Foo`).
