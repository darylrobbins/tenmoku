---
task_uid: "01m6snv5n15rmptvkp638q3cda"
task: "T1466"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "extensions/grammars/nginx/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh nginx passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for nginx, with test/sample.conf parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with nginx present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.conf.golden colours keywords, strings, comments and numbers, plus functions, types and injected languages only where tree-sitter-nginx exposes such nodes (its node-types.json at the pinned commit has no function, type, escape or injection nodes, so those classes are not required); no unrelated token is relabelled to fill a class, and the decision file lists the classes the grammar lacks"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Nginx config syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/nginx)

# Nginx config syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/nginx/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/opa-oz/tree-sitter-nginx, directory `src`. Pin: main (HEAD 7cdf006e)
- Licence: MIT. External scanner: scanner.c. Parser size: 0.3 MB.
- File extensions: nginx, nginxconf. File names: nginx.conf. Shebangs: -.
- Comments: line `#`, block `-`.
- Queries: upstream queries/highlights.scm
- Notes: small project (4 stars): read the grammar's tests before trusting it; don't claim `.conf` (too generic)

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_nginx"`, `name = "Nginx config syntax"`,
  one `[[contributes.grammars]]` with `language = "nginx"`, the extensions,
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
- `test/sample.conf`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.conf.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh nginx`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/nginx/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned HEAD 7cdf006e (package 1.0.1). The grammar fails to parse bare IPv4 values (`127.0.0.1`, `listen 127.0.0.1:80`), `upstream` blocks and `""` map keys (`''` parses), and it has no function/type/escape nodes; the sample avoids those and C4 covers the classes the grammar produces.
