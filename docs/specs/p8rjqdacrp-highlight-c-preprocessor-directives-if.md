---
task_uid: "01m79cbcxv9hdvv3p8rjqdacrp"
task: "T339"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features csharp --test csharp_preproc_highlights passes: preprocessor_directives_are_keywords, region_and_pragma_are_keywords"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features csharp --test wasm_parity csharp_extension_is_as_built_in passes"
    required: true
    manual: false
---

# Highlight C# preprocessor directives (#if, #region, #pragma, ...) as keywords (Mac T1765)

# Highlight C# preprocessor directives (#if, #region, #pragma, ...) as keywords

`tree-sitter-c-sharp` 0.23.5's `HIGHLIGHTS_QUERY` has no preprocessor captures, so `#if DEBUG`, `#region`,
`#nullable enable` etc. render as plain text. Add a fastedit-owned extra query, the way kotlin does it
(`crates/fe-syntax/queries/kotlin.scm`, appended to the crate's query in `language.rs` and mirrored in the
extension's `highlights.scm`; `wasm_parity.rs` `<lang>_extension_is_as_built_in` requires the two to match
byte for byte).

New `crates/fe-syntax/queries/csharp.scm`:

    ["#if" "#elif" "#else" "#endif" "#region" "#endregion" "#define" "#undef" "#pragma"
     "#nullable" "#line" "#error" "#warning"] @keyword
    (preproc_directive) @keyword

All 13 tokens and the named `preproc_directive` pattern token were checked against the 0.23.5
`node-types.json` / `grammar.json` (an unknown anonymous token would fail the whole query). If one turns out
not to compile in the real grammar, drop that token, do not drop the file. Skip `preproc_arg` captures: it is
free text (region names, pragma args) and plain is right.

Wire it: `language.rs` csharp `Def` highlights becomes
`&[tree_sitter_c_sharp::HIGHLIGHTS_QUERY, include_str!("../queries/csharp.scm")]`;
`extensions/grammars/csharp/highlights.scm` becomes the crate query followed by `csharp.scm` (same layout as
`extensions/grammars/kotlin/highlights.scm`); the csharp arm of `csharp_extension_is_as_built_in` in
`crates/fe-syntax/tests/wasm_parity.rs` lists both strings.

Shipping: the recorded grammar hashes in `extensions/grammars/pack.toml` are of the wasm files, and the wasm
is unchanged, so no `pack.toml` edit and no entry in `NOT_RELEASED`. Whether a grammar-pack release must
repack the extension directory to deliver `highlights.scm` to installed copies is a lead call (T1486/T1487
own the pack machinery); the built-in (native) csharp feature gets it with the next app build either way.

**Test.** New file `crates/fe-syntax/tests/csharp_preproc_highlights.rs`, `#![cfg(feature = "csharp")]`,
patterned on `tests/c_printf_highlights.rs` (`spans(lang, text)` helper with `language_by_name("csharp")`).
`preprocessor_directives_are_keywords`: for text `#if DEBUG\nclass A {}\n#endif\n` the spans include
`("#if", "keyword")` and `("#endif", "keyword")` (names via `span.highlight.name()`); start with `assert_eq!` on the
filtered span list. `region_and_pragma_are_keywords` for `#region Foo`, `#pragma warning disable`,
`#nullable enable`. A directive inside a string stays a string. Base fails on the assertion (no span).

Linux-OK.

## Implementation notes

`(preproc_directive)` is a hidden grammar token in tree-sitter-c-sharp 0.23.5 and is not a valid query node ("Invalid node type"), so it was dropped; the 13 anonymous tokens compile and are captured.
