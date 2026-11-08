---
task_uid: "01m6wfez1r508qbac6r8cv9tq5"
task: "T326"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/hlsl/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows (adapted queries are credited in the WebAssembly row, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh hlsl passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for hlsl, with test/sample.hlsl parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with hlsl present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.hlsl.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add HLSL syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/hlsl) (Mac T1474)

# HLSL syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/hlsl/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter-grammars/tree-sitter-hlsl, directory `src`. Pin: master (HEAD bab91119)
- Licence: MIT. External scanner: scanner.c. Parser size: 20 MB.
- File extensions: hlsl, hlsli, fx, fxh. File names: -. Shebangs: -.
- Comments: line `//`, block `/* */`.
- Queries: none upstream: nvim-treesitter queries/hlsl inherits cpp, which inherits c: inline both (Apache-2.0)
- Notes: big parser: check wasm size

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_hlsl"`, `name = "HLSL syntax"`,
  one `[[contributes.grammars]]` with `language = "hlsl"`, the extensions,
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
- `test/sample.hlsl`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.hlsl.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh hlsl`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/hlsl/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned at bab91119 (0.2.0, master's head). The module is 4.2 MB, well under
  `MAX_GRAMMAR_BYTES`, and loads.
- The grammar doesn't parse `cbuffer Name : register(b0) { ... }` (it wants a
  `;` after the register), and parses `cbuffer Name { ... }` as a function
  definition whose return type is `cbuffer`. The sample uses the second form,
  and `highlights.scm` adds a pattern that colours it as a keyword and a type.
- `highlights.scm` inlines nvim-treesitter's C, C++ and HLSL queries (in that
  order, last pattern wins), with C++'s capitalised-call `@constructor`
  patterns dropped: HLSL functions are capitalised, and HLSL has no
  constructors. Added: HLSL scalar/vector/matrix types (`float3`, `uint`,
  `float4x4`) as `@type.builtin`, also where they convert (`float4(v, 1)`),
  and attribute names in place of the whole-node `(hlsl_attribute)` capture.
  Also added (review round 1): scalar conversions (`uint(x)`) as
  `@type.builtin`, numeric macro values (`#define N 4`) as `@number`, and
  `precise` as `@keyword.modifier`.
- THIRD_PARTY.md: the query provenance (nvim-treesitter's three query paths,
  commit 19bdfc04 and Apache-2.0) is in the hlsl row of "Grammars shipped as
  WebAssembly", as cpp's row does for tree-sitter-c. It isn't in the "Code
  copied or adapted" table: that table's checks
  (`crates/fastedit/tests/third_party.rs` rows-have-files, which reads only
  `crates/` and `kiln/`, and `tests/notice.rs`, which wants each source in
  NOTICE) fail on an `extensions/` path, and NOTICE is outside this card's
  scope.
- `injections.scm` is comments only: upstream's targets are languages
  fastedit lacks (comment, doxygen, re2c), `injection.self`, or C's printf
  family, of which HLSL has only a debugging-only `printf`, left plain.
