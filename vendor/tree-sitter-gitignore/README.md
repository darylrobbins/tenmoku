# tree-sitter-gitignore

The gitignore grammar from
[shunsambongi/tree-sitter-gitignore](https://github.com/shunsambongi/tree-sitter-gitignore)
(MIT, see `LICENSE`), pinned at commit
`f4685bf11ac466dd278449bcfe5fd014e94aa504` (2022-05-04, the head of `main`).
crates.io has no upstream crate for it (only a third-party republish, which we
don't take), so fastedit vendors it. It's a normal path dependency, not a
`[patch.crates-io]` entry.

## What's here

Copied unchanged from upstream: `grammar.js`, `src/parser.c`,
`src/grammar.json`, `src/node-types.json`, `src/tree_sitter/`, the corpus in
`test/corpus/` and `LICENSE`. The grammar has no external scanner.

Ours: `Cargo.toml`, `bindings/rust/` (a `cc` build script and a `LANGUAGE` /
`HIGHLIGHTS_QUERY` in the shape of the crates.io grammars; upstream's binding
targets an old `tree-sitter` API) and `queries/highlights.scm`, since upstream
has no highlight query.

`parser.c` is language ABI 13, which `tree-sitter` 0.27 still loads
(its minimum is 13), so it was **not** regenerated.

## Updating

1. Pick the new upstream commit and copy the files above from it.
2. Update the commit in this file and in `THIRD_PARTY.md`.
3. Keep the empty `[workspace]` table at the end of `Cargo.toml`: without it
   `cargo fmt --all` follows the path dependency here and errors when the repo
   is checked out inside another workspace (the gate's `.verify` checkout).
4. If it's newer than ABI 13 and 0.27 rejects it, or regenerating is needed,
   run `tree-sitter generate` in a checkout and copy `src/` back; say so here.
5. Check `queries/highlights.scm` still names real nodes (a bad name fails the
   language's query compile), then `UPDATE_GOLDEN=1 cargo nextest run -p
   fe-syntax golden` and review the diff.
