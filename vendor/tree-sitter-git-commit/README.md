# tree-sitter-git-commit

The git commit message grammar from
[the-mikedavis/tree-sitter-git-commit](https://github.com/the-mikedavis/tree-sitter-git-commit)
(MIT, see `LICENSE`), pinned at commit
`5a50da19b3841ac51e9d483cd9c856a85232233d` (2025-06-27). It isn't on crates.io, so
fastedit vendors it as a normal path dependency.

Upstream is archived and its README defers to
[gbprod/tree-sitter-gitcommit](https://github.com/gbprod/tree-sitter-gitcommit). We
take it anyway: gbprod's `parser.c` is 3.2 MB (about 1.6 MB of object code, mostly
localised status headers), and its crates.io 0.5.0 is WTFPL.

## What's here

Copied unchanged from upstream: `grammar.js`, `src/parser.c`,
`src/grammar.json`, `src/node-types.json`, `src/tree_sitter/`,
`queries/highlights.scm`, the corpus in `test/corpus/` and `LICENSE`. The grammar
has no external scanner.

Ours: `Cargo.toml` and `bindings/rust/` (a `cc` build script and a `LANGUAGE` /
`HIGHLIGHTS_QUERY` in the shape of the crates.io grammars).

Upstream's query only has placeholder captures (`@subject`, `@plus`, …), so
`fe-syntax` doesn't use `HIGHLIGHTS_QUERY`: `crates/fe-syntax/queries/gitcommit.scm`
stands in for it.

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
5. Check `crates/fe-syntax/queries/gitcommit.scm` still names real nodes (a bad name fails the language's query
   compile), then `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` and
   review the diff.
