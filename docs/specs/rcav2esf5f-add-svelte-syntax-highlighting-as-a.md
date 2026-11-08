---
task_uid: "01m6tjwyswg7s41xrcav2esf5f"
task: "T503"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/svelte/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh svelte passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for svelte, with test/sample.svelte parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with svelte present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.svelte.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Svelte syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/svelte) (Mac T1447)

# Svelte syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/svelte/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter-grammars/tree-sitter-svelte, directory `src`. Pin: master (HEAD ae5199db)
- Licence: MIT. External scanner: scanner.c. Parser size: 0.2 MB.
- File extensions: svelte. File names: -. Shebangs: -.
- Comments: line `-`, block `<!-- -->`.
- Queries: upstream queries/highlights.scm + injections.scm (inherit html_tags: inline)
- Notes: Injections into built-ins (javascript, typescript, css); upstream also injects scss and pug: keep scss only if the sample avoids it, drop pug. See the vue card's note on built-in injections

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_svelte"`, `name = "Svelte syntax"`,
  one `[[contributes.grammars]]` with `language = "svelte"`, the extensions,
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
- `test/sample.svelte`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.svelte.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh svelte`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/svelte/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned to master's head, `ae5199db` (1.0.2 plus query fixes); it builds to a
  48 KB module and the sample parses cleanly.
- Upstream's queries at that commit predate the grammar's `svelte_raw_text`
  node and its `"else if"` token: they say `raw_text` (a `<script>`/`<style>`
  element's content) and `"else"`. The patterns here use the grammar's names,
  as nvim-treesitter's own svelte queries do. `<script lang="js">` comes from
  nvim-treesitter's svelte injections, since html_tags' plain-script pattern
  skips any `<script lang=...>`.
- The inherited HTML parts are inlined from nvim-treesitter (Apache-2.0,
  `LICENSE-APACHE`), credited in the THIRD_PARTY.md wasm row, as Kotlin's are:
  the "Code copied or adapted" table only admits files under `crates/` and
  `kiln/` (`crates/fastedit/tests/third_party.rs`).
- Dropped injections: `comment`, `regex`, `scss` (also for postcss, less,
  stylus), `pug` (no such language here); `#gsub!`/`#offset!` patterns; and
  `style="..."` into css, which our stylesheet grammar reads as a selector
  (`font-weight` became a tag).
- Known limits, upstream's: code in braces is always injected as JavaScript,
  even with `<script lang="ts">`, so typed `{#snippet row(todo: Todo)}`
  parameters colour as JavaScript; and an `{#each}`'s `as todo, index (todo.id)`
  is one chunk, so the key's `index (…)` reads as a call.
