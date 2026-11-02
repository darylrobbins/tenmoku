# tree-sitter-git-rebase

The interactive rebase todo grammar from
[the-mikedavis/tree-sitter-git-rebase](https://github.com/the-mikedavis/tree-sitter-git-rebase)
(MIT, see `LICENSE`), pinned at commit
`32686d6b72980b36f876ae2d07719c9c3ed154e2` (2026-05-25). It isn't on crates.io, so
fastedit vendors it as a normal path dependency.

## What's here

Copied unchanged from upstream: `grammar.js`, `src/parser.c`,
`src/grammar.json`, `src/node-types.json`, `src/tree_sitter/`,
`queries/highlights.scm`, the corpus in `test/corpus/` and `LICENSE`. The grammar
has no external scanner.

Ours: `Cargo.toml` and `bindings/rust/` (a `cc` build script and a `LANGUAGE` /
`HIGHLIGHTS_QUERY` in the shape of the crates.io grammars).

`crates/fe-syntax/queries/gitrebase.scm` goes after `HIGHLIGHTS_QUERY`, for the
forms upstream's query leaves out or misreads.

`parser.c` is language ABI 15, which `tree-sitter` 0.27 loads, so it was **not**
regenerated.

## Updating

1. Pick the new upstream commit and copy the files above from it.
2. Update the commit in this file and in `THIRD_PARTY.md`.
3. Keep the empty `[workspace]` table at the end of `Cargo.toml`: without it
   `cargo fmt --all` follows the path dependency here and errors when the repo
   is checked out inside another workspace (the gate's `.verify` checkout).
4. If 0.27 rejects its ABI, or regenerating is needed, run `tree-sitter generate`
   in a checkout and copy `src/` back; say so here.
5. Check `queries/highlights.scm` and `crates/fe-syntax/queries/gitrebase.scm` still
   name real nodes (a bad name fails the language's query compile), then `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` and
   review the diff.
