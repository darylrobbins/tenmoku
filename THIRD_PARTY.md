# Third-party code

## Code copied or adapted into this repository

| Our path | Source | Commit | License |
|---|---|---|---|
| `crates/fe-display/src/wrap_map.rs` | [gpui-kit](https://github.com/longbridge/gpui-kit) `crates/base/src/input/editor/display_map/text_wrapper.rs`, `wrap_map.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-display/src/fold_map.rs` | gpui-kit `…/display_map/fold_map.rs`, `folding.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-display/src/display_map.rs` | gpui-kit `…/display_map/display_map.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-syntax/queries/toml.scm` | [nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter) `runtime/queries/toml/highlights.scm` | `e289100ff98969e118c702199d88b764ce9e7fdf` | Apache-2.0 |

gpui-kit is Copyright 2024 - 2026 Longbridge. Its files carry no license
header and the repository has no NOTICE file, so each adapted file starts with
a header naming the source, commit and license and saying what was changed.
Ship the Apache-2.0 license text with them.

## Tree-sitter grammars and highlight queries

`fe-syntax` links these grammars, one Cargo feature each, and uses the
highlight and injections queries each crate ships, unmodified, via the
crate's own constants. Audited 2026-10-02, and the injections queries
2026-10-03: every crate is MIT, and no query names a source other than its
own repository except where noted.

| Feature | Crate | Version | Upstream | License | Query |
|---|---|---|---|---|---|
| (runtime) | tree-sitter | 0.27.0 | https://github.com/tree-sitter/tree-sitter | MIT | |
| rust | tree-sitter-rust | 0.24.2 | https://github.com/tree-sitter/tree-sitter-rust | MIT | `HIGHLIGHTS_QUERY` |
| python | tree-sitter-python | 0.25.0 | https://github.com/tree-sitter/tree-sitter-python | MIT | `HIGHLIGHTS_QUERY` |
| javascript | tree-sitter-javascript | 0.25.0 | https://github.com/tree-sitter/tree-sitter-javascript | MIT | `HIGHLIGHT_QUERY` + `JSX_HIGHLIGHT_QUERY`; `INJECTIONS_QUERY` |
| typescript | tree-sitter-typescript | 0.23.2 | https://github.com/tree-sitter/tree-sitter-typescript | MIT | JavaScript's, then `HIGHLIGHTS_QUERY`; JavaScript's `INJECTIONS_QUERY` |
| json | tree-sitter-json | 0.24.8 | https://github.com/tree-sitter/tree-sitter-json | MIT | `HIGHLIGHTS_QUERY` |
| toml | tree-sitter-toml-ng | 0.7.0 | https://github.com/tree-sitter-grammars/tree-sitter-toml | MIT | `queries/toml.scm`³ |
| go | tree-sitter-go | 0.25.0 | https://github.com/tree-sitter/tree-sitter-go | MIT | `HIGHLIGHTS_QUERY` |
| c | tree-sitter-c | 0.24.2 | https://github.com/tree-sitter/tree-sitter-c | MIT | `HIGHLIGHT_QUERY` |
| cpp | tree-sitter-cpp | 0.23.4 | https://github.com/tree-sitter/tree-sitter-cpp | MIT | C's, then `HIGHLIGHT_QUERY` |
| bash | tree-sitter-bash | 0.25.1 | https://github.com/tree-sitter/tree-sitter-bash | MIT | `HIGHLIGHT_QUERY` |
| html | tree-sitter-html | 0.23.2 | https://github.com/tree-sitter/tree-sitter-html | MIT | `HIGHLIGHTS_QUERY`; `INJECTIONS_QUERY` |
| css | tree-sitter-css | 0.25.0 | https://github.com/tree-sitter/tree-sitter-css | MIT | `HIGHLIGHTS_QUERY` |
| yaml | tree-sitter-yaml | 0.7.2, patched² | https://github.com/tree-sitter-grammars/tree-sitter-yaml | MIT | `HIGHLIGHTS_QUERY` |
| markdown | tree-sitter-md | 0.5.3 | https://github.com/tree-sitter-grammars/tree-sitter-markdown | MIT | `HIGHLIGHT_QUERY_BLOCK`¹; `INJECTION_QUERY_BLOCK`. Inline grammar (`INLINE_LANGUAGE`, our `markdown_inline`): `HIGHLIGHT_QUERY_INLINE`¹; `INJECTION_QUERY_INLINE` |
| java | tree-sitter-java | 0.23.5 | https://github.com/tree-sitter/tree-sitter-java | MIT | `HIGHLIGHTS_QUERY` |

¹ Each of these two files says it comes from nvim-treesitter (Apache-2.0, an
allowed source). Ship the Apache-2.0 license text with them.

² Vendored in `vendor/tree-sitter-yaml/` and used through `[patch.crates-io]`.
It's 0.7.2 with upstream PR #35 (commits `fa76593` and `8025f83`) applied to
`src/scanner.c`, which fixes YAML over 32,768 lines (issue #49). The licence and
queries are unchanged from 0.7.2. The vendored README says when to drop it.

³ Used in place of the crate's `HIGHLIGHTS_QUERY`, whose whole-pair `property`
capture can't be overridden. Adapted from nvim-treesitter (Apache-2.0, an
allowed source); ship the Apache-2.0 license text with it. Keys are `property`
and table headers `type`.

How patterns take precedence follows tree-sitter-highlight 0.25 (MIT), which
was read for the rule, not copied: see `earlier_patterns_win` in
`crates/fe-syntax/src/language.rs`. Injections (`crates/fe-syntax/src/injection.rs`)
implement tree-sitter's documented `@injection.*` captures and `#set!`
properties; no code was copied or adapted for them.

## Shipped MPL-2.0 dependencies (unmodified)

MPL-2.0 requires telling recipients where to get the source of these crates.

| Crate | Version | Source | Pulled in by |
|---|---|---|---|
| dwrote | 0.11.5 | https://crates.io/crates/dwrote | zed-font-kit (Windows only) |
| option-ext | 0.2.0 | https://crates.io/crates/option-ext | dirs-sys via zed-font-kit |

`cbindgen` (MPL-2.0) is a build-time tool only and is not distributed.

## Dependencies

Run `cargo deny list` for every crate and its license. Release builds must ship
the license texts. Generating `THIRD_PARTY_LICENSES` is a release-checklist item.
