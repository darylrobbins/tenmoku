# fe-grammar-csv

CSV and TSV grammars for tree-sitter, written for Tenmoku under the workspace's
licence. A column's index is its position among its row's children, which is what
`crates/fe-syntax/queries/csv.scm` counts to colour columns. The grammar on
crates.io, `tree-sitter-csv` 1.2.0, can't do that: it drops empty fields, rejects
one-character fields and fields with spaces, and needs `tree-sitter` ~0.20.

## What's here

- `grammar.js`: the factory, taking a dialect's name and separator.
- `csv/grammar.js`, `tsv/grammar.js`: the two calls to it.
- `tree-sitter.json`: the metadata the CLI needs to generate ABI 15.
- `csv/src/`, `tsv/src/`: generated, committed. Don't edit them.
- `src/lib.rs`, `build.rs`: `LANGUAGE_CSV` and `LANGUAGE_TSV`, and the `cc` build.
- `tests/shape.rs`: the tree shape, and that no text is a parse error.

## Regenerating

After changing a `grammar.js`, with Node on the path:

```sh
cd crates/fe-grammar-csv
for dialect in csv tsv; do
  (cd "$dialect" && npx tree-sitter-cli@0.25 generate)
done
```

Keep the CLI at 0.25, which writes ABI 15 (`LANGUAGE_VERSION` in `parser.c`) when
`tree-sitter.json` is there; without that file it falls back to ABI 14. The goldens
in `crates/fe-syntax/tests/golden/` (`csv.csv`, `tsv.tsv`) then show what changed.
