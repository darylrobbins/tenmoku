---
task_uid: "01m6tjwy0h59v09eamwpb0n7xt"
task: "T502"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/vue/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh vue passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for vue, with test/sample.vue parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with vue present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.vue.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Vue syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/vue) (Mac T1446)

# Vue syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/vue/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter-grammars/tree-sitter-vue, directory `src`. Pin: main (HEAD ce8011a4)
- Licence: MIT. External scanner: scanner.c (C). Parser size: 0.1 MB.
- File extensions: vue. File names: -. Shebangs: -.
- Comments: line `-`, block `<!-- -->`.
- Queries: upstream queries are `; inherits html` only: adapt nvim-treesitter vue + html_tags (Apache-2.0)
- Notes: Injections into built-ins: <script> -> javascript, <script lang="ts"> -> typescript, <style> -> css, {{ }} and directive values -> javascript. php's wasm parity already shows a wasm grammar injecting built-ins (html), so this works as is; the sample must use only built-in injections (no lang="scss": scss is wasm-only). Avoid ikatyang/tree-sitter-vue: its scanner is C++

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_vue"`, `name = "Vue syntax"`,
  one `[[contributes.grammars]]` with `language = "vue"`, the extensions,
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
- `test/sample.vue`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.vue.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh vue`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/vue/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned `ce8011a414fdf8091f4e4071752efc376f4afb08`, main's head (ABI 15);
  it builds to a 30 KB module that loads and parses the sample cleanly.
- The grammar repository at that commit does carry full `queries/vue` and
  `queries/html_tags` (not only `; inherits`): they're nvim-treesitter's,
  which differ only by one `#set!` url property. As the card says, the
  queries are adapted from nvim-treesitter (`915f2bd3`), Apache-2.0, with
  nvim-treesitter's own `LICENSE` copied verbatim as `LICENSE-APACHE`.
- `@none` fails `every_extension_is_well_formed`, so the `{{ }}` and a
  directive value's quotes are captured as `punctuation.special` instead of
  the whole node with `@none` on its inside.
- Not injected: css into a `style` attribute (the built-in CSS parses
  `color: red` as a selector), and the languages fe-syntax lacks (scss,
  pug, regex, comment).
- String escapes in the sample (`\"`, `\n`, `\t`, `✓`, CSS `\2714`)
  sit inside strings coloured `string`: the built-in JavaScript, TypeScript
  and CSS queries have no escape capture, so they aren't coloured apart.
- THIRD_PARTY.md gets one row, in "Grammars shipped as WebAssembly", which
  names the nvim-treesitter commit too: `crates/fastedit/tests/third_party.rs`
  checks that each copied-code row names a file under `crates/` or `kiln/`,
  so an `extensions/` row there fails (Kotlin's extension has none either).
