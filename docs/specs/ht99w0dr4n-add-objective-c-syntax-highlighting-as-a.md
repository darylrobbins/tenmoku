---
task_uid: "01m6sp8dbtrfffw4ht99w0dr4n"
task: "T305"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/objc/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows (adapted queries are credited in the WebAssembly row, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh objc passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for objc, with test/sample.m parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with objc present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.m.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Objective-C syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/objc) (Mac T1441)

# Objective-C syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/objc/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter-grammars/tree-sitter-objc, directory `src`. Pin: master (HEAD 181a81b8)
- Licence: MIT. External scanner: none. Parser size: 27 MB.
- File extensions: m. File names: -. Shebangs: -.
- Comments: line `//`, block `/* */`.
- Queries: upstream queries/highlights.scm inherits c: inline C's patterns
- Notes: `.h` stays C (built-in); `.mm` is Objective-C++, not this grammar; big parser: check wasm size

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_objc"`, `name = "Objective-C syntax"`,
  one `[[contributes.grammars]]` with `language = "objc"`, the extensions,
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
- `test/sample.m`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.m.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh objc`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/objc/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned `181a81b8f23a2d593e7ab4259981f50122909fda` (master HEAD, 3.0.2). The
  module is 5.4 MB, well under `MAX_GRAMMAR_BYTES`.
- Upstream's query is `; inherits: c`. The inlined C part is tree-sitter-c
  0.24.2's own query (the one fe-syntax's built-in C uses, MIT), not
  nvim-treesitter's, so no Apache text is needed; its copyright holder differs
  from the grammar's, so its licence is copied as `LICENSE-TREE-SITTER-C`.
- The grammar doesn't parse `NS_ENUM(...)` typedefs, so the sample uses a
  plain `typedef enum`.
- `injections.scm` carries the built-in C language's printf-c injection.
- `THIRD_PARTY.md` gets one row, in "Grammars shipped as WebAssembly", which
  names both of `highlights.scm`'s sources (tree-sitter-c 0.24.2 and upstream
  at the pinned commit), as `cpp/`'s row names its inlined tree-sitter-c query.
  The "Code copied or adapted" table can't hold an `extensions/` file: rows
  added there in review round 1 failed the gate, because
  `crates/fastedit/tests/third_party.rs` (rows-have-files) reads only
  `crates/` and `kiln/`, and `crates/fastedit/tests/notice.rs` requires every
  source there to be in `NOTICE`. Both are outside this card's scope, so those
  rows were reverted.
