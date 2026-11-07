---
task_uid: "01m6tjw0gwat0khf20dyks5dd3"
task: "T315"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/ocaml/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh ocaml passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for ocaml, with test/sample.ml parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with ocaml present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.ml.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add OCaml syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/ocaml) (Mac T1456)

# OCaml syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/ocaml/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter/tree-sitter-ocaml, directory `grammars/ocaml/src`. Pin: master (HEAD 3b2e14e0)
- Licence: MIT. External scanner: scanner.c, includes ../../../common/scanner.h (whole checkout needed). Parser size: 23 MB.
- File extensions: ml. File names: -. Shebangs: ocaml.
- Comments: line `-`, block `(* *)`.
- Queries: upstream queries/highlights.scm
- Notes: `.mli` needs the repo's interface grammar: out of scope (a follow-up card)

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_ocaml"`, `name = "OCaml syntax"`,
  one `[[contributes.grammars]]` with `language = "ocaml"`, the extensions,
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
- `test/sample.ml`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.ml.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh ocaml`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/ocaml/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned `a1c562e73ab787dcc69fdc2e5510c53bbce5364f`, master's head (one
  commit after the spec's `3b2e14e0`): it changes only
  `queries/highlights.scm` (unboxed-type highlights), and the grammar builds
  and passes there. The module is 4.8 MB, under `MAX_GRAMMAR_BYTES`.
- `highlights.scm` is upstream's at that commit, with a header added and
  nothing else changed: every capture is already in fe-syntax's vocabulary,
  and its only predicates are `#eq?` and `#match?`. Upstream's
  `test/highlight/*.ml` expect later patterns to win (`let f x` is
  `function`, though `(value_name) @variable` comes first), so
  `earlier_patterns_win = false`.
- Upstream has no injections query, so `injections.scm` is a comment.
- The spec gives no line comment (`-`), so the manifest has only
  `block_comment = ["(*", "*)"]`. No built-in claims `ml`.
- The queries are the grammar repository's own, under its MIT licence, so
  there's no adapted-queries row: the "Grammars shipped as WebAssembly" row
  says where `highlights.scm` came from.
