# tree-sitter-git-config

The git config grammar from
[the-mikedavis/tree-sitter-git-config](https://github.com/the-mikedavis/tree-sitter-git-config)
(MIT, see `LICENSE`), pinned at commit
`3a61756a81a86291a0f48e3eeeaa0692b9981aa9` (2026-07-20). It isn't on crates.io (its
own manifest pins `tree-sitter` ~0.20), so fastedit vendors it as a normal path
dependency.

## What's here

Copied unchanged from upstream: `grammar.js`, `src/parser.c`,
`src/grammar.json`, `src/node-types.json`, `src/tree_sitter/`,
`queries/highlights.scm`, the corpus in `test/corpus/` and `LICENSE`. The grammar
has no external scanner.

Ours: `Cargo.toml` and `bindings/rust/` (a `cc` build script and a `LANGUAGE` /
`HIGHLIGHTS_QUERY` in the shape of the crates.io grammars).

`parser.c` is language ABI 14, which `tree-sitter` 0.27 loads, so it was **not**
regenerated.

## Updating

1. Pick the new upstream commit and copy the files above from it.
2. Update the commit in this file and in `THIRD_PARTY.md`.
3. Keep the empty `[workspace]` table at the end of `Cargo.toml`: without it
   `cargo fmt --all` follows the path dependency here and errors when the repo
   is checked out inside another workspace (the gate's `.verify` checkout).
4. If 0.27 rejects its ABI, or regenerating is needed, run `tree-sitter generate`
   in a checkout and copy `src/` back; say so here.
5. Check `queries/highlights.scm` still names real nodes (a bad name fails the language's query
   compile), then `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` and
   review the diff.
