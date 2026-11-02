# tree-sitter-json5

The JSON5 grammar from
[Joakker/tree-sitter-json5](https://github.com/Joakker/tree-sitter-json5) (MIT, see
`LICENSE`), pinned at commit `248b8564567087d7866be76569b182f6dd7e14e9` (2026-05-04,
the head of `main`). The crates.io release, 0.1.0, needs `tree-sitter` ~0.20, which
can't link beside our 0.27, so fastedit vendors it as a normal path dependency, not a
`[patch.crates-io]` entry.

## What's here

Copied unchanged from upstream: `grammar.js`, `utils.js` (which `grammar.js`
requires), `src/parser.c`, `src/grammar.json`, `src/node-types.json`,
`src/tree_sitter/`, `queries/highlights.scm`, the corpus in `test/corpus/` and
`LICENSE`. The grammar has no external scanner. Upstream writes the grammar in
`grammar.ts` and `utils.ts` and compiles them to the `.js` files; the TypeScript isn't
copied.

Ours: `Cargo.toml`, `bindings/rust/` (a `cc` build script and a `LANGUAGE` /
`HIGHLIGHTS_QUERY` in the shape of the crates.io grammars; upstream's binding depends
on `tree-sitter` itself) and this file, since upstream has no README.

`parser.c` is language ABI 15, which `tree-sitter` 0.27 loads, so it was **not**
regenerated.

## Updating

1. Pick the new upstream commit and copy the files above from it.
2. Update the commit in this file and in `THIRD_PARTY.md`.
3. Keep the empty `[workspace]` table at the end of `Cargo.toml`: without it
   `cargo fmt --all` follows the path dependency here and errors when the repo is
   checked out inside another workspace (the gate's `.verify` checkout).
4. Check `crates/fe-syntax/queries/json5.scm` still names real nodes (a bad name fails
   the language's query compile), then `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax
   golden` and review the diff.
