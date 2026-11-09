---
task_uid: "01m6z36hm9x9b56dpn7crdtx7q"
task: "T334"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/d/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of https://github.com/gdamore/tree-sitter-d) and the grammar's licence files verbatim; nothing else changes except one appended THIRD_PARTY.md row"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh d passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for d, with test/sample.d parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with d present, its extension.toml giving the spec's file extensions, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.d.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add D syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/d) (Mac T1630)

# D syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/d/`, following
`extensions/grammars/README.md` and the existing directories
(`extensions/grammars/hlsl/` and `objc/` are the latest; `ruby/` is the
plainest).

## This language

- Grammar: https://github.com/gdamore/tree-sitter-d, directory `src`. Pin: main (HEAD 64f27931b4e6fdd75af1102c79bacbca68a8dacc = tag v0.9.1, the commit nvim-treesitter pins)
- Licence: MIT (upstream file is `LICENSE.txt`). External scanner: scanner.c (uses iswalnum/iswspace/iswdigit, all in the wasm stdlib). Parser size: 23 MB.
- File extensions: d, di. File names: -. Shebangs: rdmd.
- Comments: line `//`, block `/* */`. D's nesting `/+ +/` is a second block form the manifest can't hold (one pair only); the sample should still use it and it must colour as a comment.
- Queries: upstream `queries/highlights.scm` (MIT, Garrett D'Amore) at that commit; nvim-treesitter's `runtime/queries/d/highlights.scm` at `19bdfc04f40ed2ccb3340591d125f759b0406875` (Apache-2.0) may be merged in. Upstream's and nvim's injections target only `comment`/`printf`, which fastedit lacks.
- Version: `0.9.1` (the tag; upstream's `VERSION` file still says 0.8.2).
- Notes: big parser: check wasm size (landed Perl's 29 MB parser.c gave a
  4.7 MB module, so expect roughly 4 MB). `.d` is also make's
  dependency-file and DTrace's extension; no built-in claims it.

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`). Pin the newest commit that builds and passes; take
  `grammar` from the `tree_sitter_<x>` export in `parser.c`, which also fixes
  `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` is upstream's `LICENSE.txt` copied verbatim at that commit. If
  nvim-treesitter's queries are used, add `LICENSE-APACHE` and say so as
  `kotlin/` and `hlsl/` do (`license = "MIT AND Apache-2.0"` then, grammar
  `license = "MIT"`). Don't copy Helix or Zed queries (MPL-2.0 / mixed); the
  repository's `helix-*.scm` files are upstream's own MIT files and may be
  read like its `highlights.scm`.
- `extension.toml`: `id = "tenmoku.grammar_d"`, `name = "D syntax"`, one
  `[[contributes.grammars]]` with `language = "d"`, the extensions,
  `line_comment = "//"`, `block_comment = ["/*", "*/"]`,
  `shebangs = ["rdmd"]`, and `earlier_patterns_win` matching the query's
  convention (tree-sitter style: true; nvim style: false). No `packs` or
  other new keys: pack membership comes from a later card (T1487).
- `highlights.scm`: self-contained; a header comment says where each part
  came from, at which commit, under what licence. Captures in fe-syntax's
  vocabulary (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Drop
  patterns using predicates fe-syntax doesn't support rather than leaving
  them inert.
- `injections.scm`: only injections into languages that exist (built-ins, or
  first-party ones listed in `parity_needs`); a comment-only file if none.
  No locals, folds, indents or outline queries (no first-party extension
  ships them and the manifest has no keys for them).
- `test/sample.d`: 40-120 lines of idiomatic code touching keywords,
  strings with escapes (and a wysiwyg `r"..."` or backquoted string), numbers,
  comments (all three forms, including a `///` or `/** */` doc comment),
  functions, templates, structs/classes, attributes (`@safe`, `pure`,
  `nothrow`) and `import`; it must parse with no ERROR or MISSING node.
  Generate `test/sample.d.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh d`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one `d` row to `THIRD_PARTY.md`'s "Grammars shipped as
  WebAssembly" table, after the last row, in the `objc`/`hlsl` rows' format:
  the repository and full commit, and a licence cell naming the grammar's
  licence and every source `highlights.scm` was adapted from, with its commit
  and licence (as the `hlsl` row names nvim-treesitter and `LICENSE-APACHE`).
  That row plus the query's header comment is the whole attribution, the
  convention settled on T1463 (Astro): do NOT add rows to "Code copied or
  adapted into this repository" (that table covers `crates/` and `kiln/`
  only; an `extensions/` row fails `third_party::copied_files_and_rows_agree`)
  and do NOT touch `NOTICE` (`notice::sources_are_in_notice`). Reviewers: a
  missing copied-code row or NOTICE entry is this convention, not an omission.
  Append only; don't reorder or edit other rows. `THIRD_PARTY.md` is in
  `regatta.toml`'s `shared_append_only`, which is why it isn't in the scope.
- Change nothing outside `extensions/grammars/d/` and that appended row. If
  the module won't load (ABI, the table validator, a libc import the wasm
  stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline), or a
  built-in already claims an extension, block the card with the exact error
  (for size, the module's byte count): fixing fe-syntax, the vendored
  tree-sitter or the build script (optimisation flags included) is out of
  scope.
