---
task_uid: "01m7j35nb5gvh1gqkqh3k82vka"
task: "T1900"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/prisma/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form to https://github.com/victorhqc/tree-sitter-prisma at a full 40-hex commit (3556b2c1f20ec9ac91e92d32c43d9d2a0ca3cc49 or newer), the MIT LICENSE verbatim, and extension.toml with packs [\"web\", \"data\"]; nothing else changes except one appended THIRD_PARTY.md WebAssembly row and a prisma NOT_RELEASED row in grammar_pack_recorded.rs"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh prisma passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for prisma, with test/sample.prisma (datasource, generator, models with relations and attributes, enum, view, composite type, // and /// comments) parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with prisma present, its extension.toml giving extension prisma, line_comment //, no shebangs and no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.prisma.golden colours keywords (including type and view), model, enum and type names, field names, field types, attributes, functions, strings, numbers, booleans and comments"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes, including grammar_pack_recorded"
    required: true
    manual: false
---

# Add Prisma schema syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/prisma)

# Prisma syntax as a first-party WebAssembly grammar

The user asked (2026-11-16): "Procfiles don't seem to be syntax highlighted.
Are there any other file formats like this that we're missing that are common
within certain types of projects?" The lead proposed Prisma schemas (every
Prisma project's `schema.prisma`, Node/TypeScript backends) among others and
the user said "Go for it".

One new directory, `extensions/grammars/prisma/`, following
`extensions/grammars/README.md` and the Nim card T1629 (landed 96836cb2b:
`git show 96836cb2b` is the model, file for file).

## This language

- Grammar: https://github.com/victorhqc/tree-sitter-prisma (the grammar
  nvim-treesitter, Helix and Zed use), directory `src`. Pin: the head of
  `main`, 3556b2c1f20ec9ac91e92d32c43d9d2a0ca3cc49 (v1.6.0, 2025-10-02), or
  a newer commit that builds and passes. Generated `parser.c` (230 KB,
  LANGUAGE_VERSION 15) is committed; export `tree_sitter_prisma`, so
  `grammar = "prisma"`, `wasm = "tree-sitter-prisma.wasm"`.
- Licence: MIT (`LICENSE`). No external scanner. No fork expected: if ordinary
  Prisma (below) gives an ERROR node, block the card with the snippet and its
  parse tree; a patched fork is a user decision, as Nim's was.
- File extensions: `prisma`. File names: none. Shebangs: none.
- Comments: line `//` (`///` is a documentation comment). No block comment.
- Queries: upstream `queries/highlights.scm` (MIT, 39 lines, nvim style, so
  `earlier_patterns_win = false`): adapt it. It misses the `type` keyword
  (composite types) and gives `///` the same capture as `//`: add the keyword
  and colour `///` `comment.documentation` if the grammar tells them apart
  (`comment` vs `developer_comment`). No injections: `injections.scm` is
  comment-only.
- Version: `1.6.0` (upstream `tree-sitter.json`).
- Packs: `packs = ["web", "data"]` (main pack Web: Prisma is the ORM schema of
  Node/TypeScript web backends; also Data).

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = full 40-hex commit,
  `src`, `grammar`), with Nim's comment lines.
- `LICENSE` copied verbatim from the grammar at that commit.
- `extension.toml`: `id = "tenmoku.grammar_prisma"`, `name = "Prisma syntax"`,
  `version`, `license = "MIT"`, `[engine] tenmoku = "1.0"`, one
  `[[contributes.grammars]]` with `language = "prisma"`, the wasm, the
  extensions, `highlights`, `injections`, `packs`, `earlier_patterns_win`,
  `license = "MIT"`, `line_comment = "//"`. No other keys
  (`grammar_install_tests::every_first_party_manifest_parses`).
- `highlights.scm`: self-contained; a header comment says where it came from,
  at which commit, under what licence, and what was changed. Captures in
  fe-syntax's vocabulary (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES).
  Drop patterns using predicates fe-syntax doesn't support.
- `test/sample.prisma`: 40-120 lines of an idiomatic schema: `datasource`
  and `generator` blocks with `env("DATABASE_URL")`, several `model`s with
  scalar, optional (`?`) and list (`[]`) fields, relations
  (`@relation(fields: [authorId], references: [id], onDelete: Cascade)`),
  `@id`, `@default(autoincrement())`, `@default(now())`, `@unique`,
  `@updatedAt`, `@db.VarChar(255)`, block attributes `@@index`, `@@unique`,
  `@@map`, an `enum`, a `view`, a composite `type`, and `//` and `///`
  comments. It must parse with no ERROR or MISSING node. Generate
  `test/sample.prisma.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh prisma`, then read it:
  keywords, model/enum/type names, field names, types, attributes, functions,
  strings, numbers, booleans and comments are coloured and nothing is
  obviously wrong.
- `crates/fastedit/tests/grammar_pack_recorded.rs`: add a `NOT_RELEASED` row
  for `prisma` in alphabetical order (after `nim`), "added after grammars-2;
  recorded by the Mac's next grammar release" (`pack.toml` has no entry until
  a grammar release records it; don't edit `pack.toml`).
- Attribution: append one `prisma` row to `THIRD_PARTY.md`'s "Grammars
  shipped as WebAssembly" table, after the last row, in the `nim`/`verilog`
  rows' format: repository and full commit, and a licence cell naming the
  grammar's licence and the source of `highlights.scm` (upstream's
  `queries/highlights.scm` at that commit, MIT, adapted as its header says).
  That row plus the query's header is the whole attribution (the T1463
  convention): do NOT add a row to "Code copied or adapted into this
  repository" (it covers `crates/` and `kiln/` only; an `extensions/` row fails
  `third_party::copied_files_and_rows_agree`) and do NOT touch `NOTICE`
  (`notice::sources_are_in_notice`). Reviewers: a missing copied-code row or
  NOTICE entry is this convention, not an omission. Append only; don't reorder
  or edit other rows (`THIRD_PARTY.md` is in `regatta.toml`'s
  `shared_append_only`).
- Change nothing else. If the module won't load (ABI, the table validator, a
  libc import the wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the
  load deadline), or a built-in already claims `.prisma`, block the card with
  the exact error: fixing fe-syntax, the vendored tree-sitter or the build
  script is out of scope.
