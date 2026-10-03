# tree-sitter-yaml 0.7.2, patched

This is tree-sitter-yaml 0.7.2 from crates.io, the release commit
[`7708026`](https://github.com/tree-sitter-grammars/tree-sitter-yaml/commit/7708026449bed86239b1cd5bce6e3c34dbca6415),
with upstream [PR #35](https://github.com/tree-sitter-grammars/tree-sitter-yaml/pull/35)
applied. `Cargo.toml` at the repository root uses it in place of the crates.io
crate through `[patch.crates-io]`. It's MIT; see `LICENSE`.

## Why

The 0.7.2 scanner keeps its row count in an `int16_t`. At line 32,769 the
count wraps negative, the scanner misses a newline, and any YAML longer than
32,768 lines gets error nodes. From about 100k lines the root itself is an
ERROR node. Upstream tracks this in
[issue #49](https://github.com/tree-sitter-grammars/tree-sitter-yaml/issues/49).
`crates/fe-syntax/tests/yaml_long_list.rs` checks it stays fixed.

## What changed

Only `src/scanner.c`: PR #35's two commits, from
glowcloud/tree-sitter-yaml branch `fix/large-definition-error`, head
`8025f83a294d829769d9a99132105bcd68fbaee6`, based on `7708026`:

- `fa76593` fix: fix error when parsing large definitions
- `8025f83` fix: fix overflow for block implicit key row

They widen `row`, `cur_row`, `end_row`, `blk_imp_row` and the local
`bgn_row` to `int32_t`, along with their serialized form. `col` and the
indentation stacks stay 16 bits, so the serialized state grows by 4 bytes.

Besides the patch, this copy differs from the crates.io package in a few ways.
The packaged files are kept except `README.md`, which is this file, and the
cargo metadata files. `Cargo.toml` is the package's original manifest without
its `tree-sitter` dev-dependency, which only the crate's own unit test used.
The parser, the queries and the bindings are unchanged.

## When to drop it

Drop this directory and the `[patch.crates-io]` entry once a tree-sitter-yaml
release includes the fix for issue #49, whether PR #35 or another. Bump the
workspace dependency to that release and audit it as for any grammar bump
(`THIRD_PARTY.md`). Then check that `yaml_long_list` and the YAML golden still pass.
