---
task_uid: "01m6snv1jvc5qj0t0mjb4sxrfz"
task: "T1463"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/astro/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh astro passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for astro, with test/sample.astro parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with astro present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.astro.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Astro syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/astro)

# Astro syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/astro/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/virchau13/tree-sitter-astro, directory `src`. Pin: master (HEAD 213f6e69, last push 2025-04)
- Licence: MIT. External scanner: scanner.c. Parser size: 0.1 MB.
- File extensions: astro. File names: -. Shebangs: -.
- Comments: line `-`, block `<!-- -->`.
- Queries: upstream queries (inherit html: inline)
- Notes: Injections into built-ins: frontmatter and {} expressions -> typescript, <script> -> javascript/typescript, <style> -> css; drop upstream's scss injection or keep it out of the sample (wasm-only). See the vue card's note

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_astro"`, `name = "Astro syntax"`,
  one `[[contributes.grammars]]` with `language = "astro"`, the extensions,
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
- `test/sample.astro`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.astro.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh astro`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/astro/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned master's head, 213f6e6973d9b456c6e50e86f19f66877e7ef0ee (ABI 14, 30 KB wasm); it builds and loads first time.
- `earlier_patterns_win = true`: the query is tree-sitter-html's style, and no two patterns overlap.
- Injections: frontmatter, `{}` expressions and attribute expressions → typescript; `<script>` → typescript
  (Astro compiles scripts as TypeScript); every `<style>` → css (upstream's scss injection dropped).
- The highlight query adds `"/>"` from tree-sitter-html 0.23.2 (MIT), the frontmatter fences, the
  interpolation braces and the backtick/quoted attribute values.
- The queries' attribution is in astro's "Grammars shipped as WebAssembly" row, as kotlin's and cpp's are, not in
  "Code copied or adapted into this repository": that table covers files under `crates/` and `kiln/` only
  (`crates/fastedit/tests/third_party.rs`, rule rows-have-files fails on any other path), and a source there must
  also be in NOTICE (`crates/fastedit/tests/notice.rs`), which is outside this card's scope.
