---
task_uid: "01m6wfez5xq4q9gtf4jtqt37pt"
task: "T327"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/fortran/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh fortran passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for fortran, with test/sample.f90 parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with fortran present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.f90.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Fortran syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/fortran) (Mac T1475)

# Fortran syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/fortran/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/stadelmanma/tree-sitter-fortran, directory `src`. Pin: master (HEAD 72f8759d)
- Licence: MIT. External scanner: scanner.c. Parser size: 35 MB.
- File extensions: f90, f95, f03, f08. File names: -. Shebangs: -.
- Comments: line `!`, block `-`.
- Queries: upstream queries/highlights.scm
- Notes: free form only: don't claim fixed-form `.f`/`.for`; big parser: check wasm size

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_fortran"`, `name = "Fortran syntax"`,
  one `[[contributes.grammars]]` with `language = "fortran"`, the extensions,
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
- `test/sample.f90`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.f90.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh fortran`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/fortran/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned upstream HEAD `72f8759dea28ff6671e0868a9ed5ad4a18413917` (version
  0.5.1). The module is 6.8 MB, under `MAX_GRAMMAR_BYTES`.
- Load deadline, measured with a scratch test (not committed) that calls
  `register_language` once in a fresh process under
  `GrammarDeadlines::DEFAULT` (10 s load), then parses `test/sample.f90`. On
  this Linux host, at load average about 30, with `--release` (the profile
  the app ships in): 1.97 s cold, then 0.79 s and 0.84 s; the parse took
  12-28 ms with no ERROR or MISSING node. In the unoptimized test profile,
  the load took 13.5 s and 26.1 s and failed with
  `Deadline { ..., deadline: 10s }`; C#'s 5.3 MB module took 5.4 s there.
  So a release build loads it well within the deadline, and a debug build
  (`cargo run` without `--release`) refuses it. Making debug builds faster
  (the dev profile's opt-level for wasmtime and cranelift) is outside this
  card's scope. `every_extension_highlights_its_sample` raises the load
  deadline to 120 s, so its pass doesn't show this.
- No `block_comment` (Fortran has none) and no `file_names` or `shebangs`;
  only the free-form extensions `f90`, `f95`, `f03`, `f08`.
- `highlights.scm` is upstream's query, adapted as its header lists. Upstream
  ships no injections, so `injections.scm` is a comment.
- Function references in expressions (`size(pts)`, `area(c)`) stay
  `variable`, as upstream leaves them: the grammar can't tell them from array
  element references. Definitions, `call`s, interface names and procedure
  bindings are `function`.
- A doubled quote (`'Bob''s'`), Fortran's only string escape, stays
  `string`: the scanner's node for it covers only the first quote of the
  pair, so an `@escape` capture would colour half of it.
- The query's provenance is in the "Grammars shipped as WebAssembly" row; the
  "Code copied or adapted" table covers only `crates/` and `kiln/` files.
