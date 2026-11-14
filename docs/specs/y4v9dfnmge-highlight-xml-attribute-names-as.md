---
task_uid: "01m7c01c1qkgq70jy4v9dfnmge"
task: "T628"
rev: 3
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features xml --test xml_attribute_highlights passes: attribute_names_are_attributes, xml_declaration_pseudo_attributes_are_attributes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features xml highlights_match_golden_files passes with crates/fe-syntax/tests/golden/xml.xml.golden regenerated; only attribute-name spans changed from property to attribute"
    required: true
    manual: false
---

# Highlight XML attribute names as attributes (xml.scm override) (Mac T1816)

# Highlight XML attribute names as attributes

`tree-sitter-xml` 0.7.0's `XML_HIGHLIGHT_QUERY` (language.rs ~line 942) captures attribute names and
`<?xml ... ?>` pseudo-attributes as `@property`. `property` is plain text in the Catppuccin themes, Hakuji,
Yohen and VS Code Monokai, so XML/SVG/plist attributes look unhighlighted, while HTML, JSX, Vue, Svelte, Astro,
Blade, HEEx, Twig, Razor and Jinja already use `@attribute` (coloured everywhere). Make XML match.

New `crates/fe-syntax/queries/xml.scm` (header: "Ours. After the crate's query, so this wins on the same node."):

    (Attribute (Name) @attribute)
    (PseudoAtt (Name) @attribute)

Node names are those of tree-sitter-xml 0.7.0's `LANGUAGE_XML` grammar; check them against its `node-types.json`
(an unknown node fails the whole query) and drop none silently: if one does not exist, say so in the finish notes.
Wire it: the xml `Def` `highlights` becomes `&[tree_sitter_xml::XML_HIGHLIGHT_QUERY, include_str!("../queries/xml.scm")]`.
There is no xml extension, so no `extensions/` change and no `wasm_parity` arm. Add the override to the xml row of
THIRD_PARTY.md ("`XML_HIGHLIGHT_QUERY`, then `queries/xml.scm`", same wording style as the kotlin row).

Regenerate `crates/fe-syntax/tests/golden/xml.xml.golden` with
`UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --features xml golden` and review the diff: only attribute-name
spans (e.g. `version`, `encoding`, `standalone`, `xml-stylesheet` pseudo attributes, `xmlns`, `id`) may move from
`property` to `attribute`; the `<?xml` target stays `keyword`, element names stay `tag`.

**Test.** New file `crates/fe-syntax/tests/xml_attribute_highlights.rs`, `#![cfg(feature = "xml")]`, patterned on
`tests/c_printf_highlights.rs` (`spans(lang, text)` helper with `language_by_name("xml")`, names via
`span.highlight.name()`; cargo discovers it, no registration). `attribute_names_are_attributes`: for
`<a href="x" id='y'>t</a>` the spans include `("href","attribute")` and `("id","attribute")` and the values stay
`string`. `xml_declaration_pseudo_attributes_are_attributes`: `<?xml version="1.0" encoding="UTF-8"?>` gives
`("version","attribute")` and `("encoding","attribute")`. Base fails on the assertion (they are `property`).

Linux-OK (text goldens only).
## Lead decision on the failed gate

`crates/fe-syntax/src/tests.rs` `each_language_highlights_a_snippet` expects `x.svg: "width"` as `property`; with XML attribute names now `attribute` that expectation is the old behaviour. Scope now includes `crates/fe-syntax/src/tests.rs`: change that snippet expectation (and any other xml/svg one) to `attribute`, nothing else. Then rgt finish once.
