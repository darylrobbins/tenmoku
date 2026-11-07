---
task_uid: "01m6sp8a9rw4f4vfqtzrjd35zv"
task: "T301"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/haskell/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh haskell passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for haskell, with test/sample.hs parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with haskell present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.hs.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Haskell syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/haskell) (Mac T1437)

# Haskell syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/haskell/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter/tree-sitter-haskell, directory `src`. Pin: master (HEAD 97288e58)
- Licence: MIT. External scanner: scanner.c. Parser size: 19 MB.
- File extensions: hs. File names: -. Shebangs: runhaskell, runghc.
- Comments: line `--`, block `{- -}`.
- Queries: upstream queries/highlights.scm
- Notes: big parser: check the wasm size and load time; .lhs (literate) is out of scope

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_haskell"`, `name = "Haskell syntax"`,
  one `[[contributes.grammars]]` with `language = "haskell"`, the extensions,
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
- `test/sample.hs`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.hs.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh haskell`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/haskell/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned master's head, `97288e585b0bd44199720280d067fa35bacd81ff` (0.24.1).
  The 19 MB `parser.c` compiles to a 3.7 MB `tree-sitter-haskell.wasm`, well
  under `MAX_GRAMMAR_BYTES`, and loads and highlights the sample within the
  parity test's deadlines.
- The queries are the grammar's own (MIT, so no extra licence file).
  `highlights.scm` is Neovim-style (later patterns win, so
  `earlier_patterns_win = false`) and needed two fixes, which its header
  lists: upstream's bare `(variable) @type` near the end repainted every
  variable and function as a type, so it's narrowed to `(type/variable)`;
  and a pattern captured a whole type signature as `@function`, colouring
  the spaces in it, so it captures the name.
- `injections.scm` keeps the quasi-quoters for css, html, javascript,
  typescript, json and sql; `[sql| ... |]` is in the sample, so
  `parity_needs = ["sql"]`. Dropped: the "comment" and
  "haskell_persistent" injections, and the one using any quoter's name as
  a language.
- Left as upstream has it: a destructured parameter
  (`describe Item { itemName = name }`) is coloured `variable.parameter`
  as a whole, so the spaces between its tokens carry that
  colour in the golden file; `error`, `undefined`, `bracket_` and friends
  are keywords.
