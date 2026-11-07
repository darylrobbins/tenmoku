---
task_uid: "01m6tjwazkjk5b2gw0m16f9ahb"
task: "T298"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/latex/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh latex passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for latex, with test/sample.tex parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with latex present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.tex.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add LaTeX syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/latex) (Mac T1454)

# LaTeX syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/latex/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: crates.io tree-sitter-latex 0.1.0 (the .crate's sha256 d487325d598890a638805ee048d460ad6e60e5c6acf9ee68f361def902604368), directory `src`. Pin: the crates.io tarball form: latex-lsp/tree-sitter-latex's git tree has no generated parser.c
- Licence: MIT. External scanner: none in 0.1.0. Parser size: 38 MB.
- File extensions: tex, sty, cls, ltx. File names: -. Shebangs: -.
- Comments: line `%`, block `-`.
- Queries: nvim-treesitter queries/latex (Apache-2.0); the crate's own queries if packaged
- Notes: needs the discovery card's crates.io source form; big parser: check wasm size

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_latex"`, `name = "LaTeX syntax"`,
  one `[[contributes.grammars]]` with `language = "latex"`, the extensions,
  file names, `line_comment`/`block_comment` and `shebangs` above (keys from
  the comment-tokens card), and `earlier_patterns_win` matching the query's
  convention (tree-sitter style: true; nvim style: false). No `packs` or
  other new keys: pack membership comes from a later card.
- `highlights.scm`: self-contained (inline anything a `; inherits:` line
  pulls in, adapted to this grammar's node names); captures in fe-syntax's
  vocabulary (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Drop
  patterns using predicates fe-syntax doesn't support rather than leaving
  them inert.
- `injections.scm`: only injections into languages that exist (built-ins, or
  first-party ones listed in `parity_needs`); a comment-only file if none.
  No locals, folds, indents or outline queries (no first-party extension
  ships them and the manifest has no keys for them).
- `test/sample.tex`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.tex.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh latex`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/latex/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- The spec's crate, tree-sitter-latex 0.1.0, can't be used: its `parser.c`
  needs an external scanner, but the crate ships no `scanner.c`, so the module
  fails to load (`invalid import 'tree_sitter_latex_external_scanner_create'`).
  It also has no LICENSE. After the block, the instruction was the git form of
  latex-lsp/tree-sitter-latex, trying 948b89c9, with LICENSE from that repo.
- 948b89c9 and master have no generated `parser.c` in git (only
  `grammar.json` and `scanner.c`), and the build script doesn't run
  `tree-sitter generate`. The pin is `aca6eeba` ("Re-add generated files",
  2024-04-01, package version 0.3.0): the newest commit whose tree has
  `src/parser.c`, before v0.4.0 stopped committing it. It builds a 2.5 MB
  module with its scanner, which loads and passes. `LICENSE` is that commit's.
- The queries are nvim-treesitter's at 2c30e515, written for grammar master.
  The older grammar's node types are a subset of master's, so patterns on the
  newer nodes are dropped and `curly_group_label*` are `curly_group_text*`
  (each file's header lists the changes). Environment names are `@type`
  (nvim's `@label`), and escapes and numbers are added, so the sample has a
  type, an escape and numbers to colour. Injections keep Python (`pycode`,
  `sage*`) and `minted`'s named language.
- THIRD_PARTY.md: only the "Grammars shipped as WebAssembly" row. The
  adapted-code table is for files under `crates/` and `kiln/`
  (`third_party.rs`'s `copied_files_and_rows_agree` refuses a row for any
  other path), so the queries' source and licence go in that row, as Kotlin's do.
