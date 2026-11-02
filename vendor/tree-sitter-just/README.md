# tree-sitter-just

The justfile grammar from
[casey/tree-sitter-just](https://github.com/casey/tree-sitter-just) (Apache-2.0, see
`LICENSE`; upstream's `Cargo.toml` says MIT, but `LICENSE` is the licence text it
ships, so that is what applies here), pinned at commit
`5685543a6e64f66335e25518c9ae8ffa1dae3d01` (2026-03-25). The crates.io release,
0.2.0, depends on `tree-sitter` ~0.25.5, which can't link beside our 0.27, so
fastedit vendors it as a normal path dependency.

## What's here

Copied unchanged from upstream: `grammar.js`, `src/parser.c`, `src/scanner.c`,
`src/grammar.json`, `src/node-types.json`, `src/tree_sitter/`,
`queries-src/highlights.scm`, `queries-src/injections.scm`, the corpus in
`test/corpus/` and `LICENSE`.

Ours: `Cargo.toml` and `bindings/rust/` (a `cc` build script and a `LANGUAGE` /
`HIGHLIGHTS_QUERY` / `INJECTIONS_QUERY` in the shape of the crates.io grammars).

`parser.c` is language ABI 15, which `tree-sitter` 0.27 loads, so it was **not**
regenerated.

## Updating

1. Pick the new upstream commit and copy the files above from it.
2. Update the commit in this file and in `THIRD_PARTY.md`, and check `LICENSE`
   is still Apache-2.0.
3. Keep the empty `[workspace]` table at the end of `Cargo.toml`: without it
   `cargo fmt --all` follows the path dependency here and errors when the repo
   is checked out inside another workspace (the gate's `.verify` checkout).
4. If 0.27 rejects its ABI, or regenerating is needed, run `tree-sitter generate`
   in a checkout and copy `src/` back; say so here.
5. Check both queries still name real nodes (a bad name fails the language's
   query compile), then `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden`
   and review the diff.
