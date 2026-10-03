# Third-party code

## Code copied or adapted into this repository

| Our path | Source | Commit | License |
|---|---|---|---|
| _none yet_ | | | |

## Tree-sitter grammars and highlight queries

`fe-syntax` links these grammars, one Cargo feature each, and uses the
highlight query each crate ships, unmodified, via the crate's own constant.
Audited 2026-10-02: every crate is MIT, and no query names a source other
than its own repository except where noted.

| Feature | Crate | Version | Upstream | License | Query |
|---|---|---|---|---|---|
| (runtime) | tree-sitter | 0.27.0 | https://github.com/tree-sitter/tree-sitter | MIT | |
| rust | tree-sitter-rust | 0.24.2 | https://github.com/tree-sitter/tree-sitter-rust | MIT | `HIGHLIGHTS_QUERY` |
| python | tree-sitter-python | 0.25.0 | https://github.com/tree-sitter/tree-sitter-python | MIT | `HIGHLIGHTS_QUERY` |
| javascript | tree-sitter-javascript | 0.25.0 | https://github.com/tree-sitter/tree-sitter-javascript | MIT | `HIGHLIGHT_QUERY` + `JSX_HIGHLIGHT_QUERY` |
| typescript | tree-sitter-typescript | 0.23.2 | https://github.com/tree-sitter/tree-sitter-typescript | MIT | JavaScript's, then `HIGHLIGHTS_QUERY` |
| json | tree-sitter-json | 0.24.8 | https://github.com/tree-sitter/tree-sitter-json | MIT | `HIGHLIGHTS_QUERY` |
| toml | tree-sitter-toml-ng | 0.7.0 | https://github.com/tree-sitter-grammars/tree-sitter-toml | MIT | `HIGHLIGHTS_QUERY` |
| go | tree-sitter-go | 0.25.0 | https://github.com/tree-sitter/tree-sitter-go | MIT | `HIGHLIGHTS_QUERY` |
| c | tree-sitter-c | 0.24.2 | https://github.com/tree-sitter/tree-sitter-c | MIT | `HIGHLIGHT_QUERY` |
| cpp | tree-sitter-cpp | 0.23.4 | https://github.com/tree-sitter/tree-sitter-cpp | MIT | C's, then `HIGHLIGHT_QUERY` |
| bash | tree-sitter-bash | 0.25.1 | https://github.com/tree-sitter/tree-sitter-bash | MIT | `HIGHLIGHT_QUERY` |
| html | tree-sitter-html | 0.23.2 | https://github.com/tree-sitter/tree-sitter-html | MIT | `HIGHLIGHTS_QUERY` |
| css | tree-sitter-css | 0.25.0 | https://github.com/tree-sitter/tree-sitter-css | MIT | `HIGHLIGHTS_QUERY` |
| yaml | tree-sitter-yaml | 0.7.2 | https://github.com/tree-sitter-grammars/tree-sitter-yaml | MIT | `HIGHLIGHTS_QUERY` |
| markdown | tree-sitter-md | 0.5.3 | https://github.com/tree-sitter-grammars/tree-sitter-markdown | MIT | `HIGHLIGHT_QUERY_BLOCK`¹ |
| java | tree-sitter-java | 0.23.5 | https://github.com/tree-sitter/tree-sitter-java | MIT | `HIGHLIGHTS_QUERY` |

¹ The file says it comes from nvim-treesitter (Apache-2.0, an allowed
source). Ship the Apache-2.0 license text with it.

How patterns take precedence follows tree-sitter-highlight 0.25 (MIT), which
was read for the rule, not copied: see `earlier_patterns_win` in
`crates/fe-syntax/src/language.rs`.

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
