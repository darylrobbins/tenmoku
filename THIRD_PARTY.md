# Third-party code

## Code copied or adapted into this repository

| Our path | Source | Commit | License |
|---|---|---|---|
| `crates/fe-display/src/wrap_map.rs` | [gpui-kit](https://github.com/longbridge/gpui-kit) `crates/base/src/input/editor/display_map/text_wrapper.rs`, `wrap_map.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-display/src/fold_map.rs` | gpui-kit `…/display_map/fold_map.rs`, `folding.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-display/src/display_map.rs` | gpui-kit `…/display_map/display_map.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-editor/src/diagnostics.rs` | gpui-kit `crates/base/src/input/editor/diagnostics.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-syntax/queries/toml.scm` | [nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter) `runtime/queries/toml/highlights.scm` | `e289100ff98969e118c702199d88b764ce9e7fdf` | Apache-2.0 |
| `crates/fe-syntax/queries/kotlin.scm` | nvim-treesitter `runtime/queries/kotlin/highlights.scm` | `3e1f54c1ce0a86959313f1cb7f60262ea57aaa23` | Apache-2.0 |
| `crates/fe-syntax/queries/outline/ruby.scm` | tree-sitter-ruby crate `queries/tags.scm` | version 0.23.1 | MIT |
| `crates/fe-syntax/queries/outline/php.scm` | tree-sitter-php crate `queries/tags.scm` | version 0.24.2 | MIT |
| `crates/fe-syntax/queries/definitions/php.scm` | tree-sitter-php crate `queries/tags.scm` | version 0.24.2 | MIT |
| `crates/fe-syntax/queries/outline/kotlin.scm` | tree-sitter-kotlin-sg crate `queries/tags.scm` | version 0.4.1 | MIT |
| `crates/fe-syntax/queries/definitions/kotlin.scm` | tree-sitter-kotlin-sg crate `queries/tags.scm` | version 0.4.1 | MIT |
| `crates/fe-syntax/queries/outline/scala.scm` | tree-sitter-scala crate `queries/tags.scm` | version 0.26.2 | MIT |
| `crates/fe-syntax/queries/definitions/scala.scm` | tree-sitter-scala crate `queries/tags.scm` | version 0.26.2 | MIT |
| `crates/fe-syntax/queries/outline/lua.scm` | tree-sitter-lua crate `queries/tags.scm` | version 0.5.0 | MIT |
| `kiln/crates/kiln-ui/src/text_field.rs` | [gpui-pre](https://crates.io/crates/gpui-pre) 0.3.7 `examples/input.rs` (Zed's Apache-2.0 GPUI) | zed `1a28cff` | Apache-2.0 |

gpui-kit is Copyright 2024 - 2026 Longbridge. Its files carry no license
header and the repository has no NOTICE file, so each adapted file starts with
a header naming the source, commit and license and saying what was changed.
Ship the Apache-2.0 license text with them.

kiln's `TextField` adapts the text-input example that ships with GPUI
(Copyright Zed Industries, Apache-2.0): its line shaping, selection and caret
quads, IME marked-range underline and input-handler methods. The file's
header names the source and licence and its module docs say what was changed.

The outline and definitions queries adapted from a grammar crate's
`queries/tags.scm` come from the crate version `fe-syntax` already links
(MIT, listed below), so its licence covers them. Each starts with a header
naming the source file, version and licence and saying what was changed.
None adapts nvim-treesitter. The other outline and definitions queries for
C++, C#, Ruby, Swift and Lua are ours, written from the grammar's
`node-types.json`, and say so in their headers.

## Tree-sitter grammars and highlight queries

`fe-syntax` links these grammars, one Cargo feature each, and uses the
highlight and injections queries each crate ships, unmodified, via the
crate's own constants. Audited 2026-10-02, the injections queries
2026-10-03, and Ruby to XML and Swift to Make 2026-10-03: every crate is MIT,
and no query names a source other than its own repository except where noted.
The Ruby, Lua, XML, SQL and Make crates ship no licence file, only
`license = "MIT"` in their manifests; their repositories hold the MIT text.

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
| markdown | tree-sitter-md | 0.5.3 | https://github.com/tree-sitter-grammars/tree-sitter-markdown | MIT | `HIGHLIGHT_QUERY_BLOCK`¹, then one pattern of ours⁹; `INJECTION_QUERY_BLOCK`. Inline grammar (`INLINE_LANGUAGE`, our `markdown_inline`): `HIGHLIGHT_QUERY_INLINE`¹; `INJECTION_QUERY_INLINE` |
| java | tree-sitter-java | 0.23.5 | https://github.com/tree-sitter/tree-sitter-java | MIT | `HIGHLIGHTS_QUERY` |
| ruby | tree-sitter-ruby | 0.23.1 | https://github.com/tree-sitter/tree-sitter-ruby | MIT | `HIGHLIGHTS_QUERY`, then `queries/ruby.scm`⁴ |
| lua | tree-sitter-lua | 0.5.0 | https://github.com/tree-sitter-grammars/tree-sitter-lua | MIT | `HIGHLIGHTS_QUERY`; `INJECTIONS_QUERY` |
| php | tree-sitter-php | 0.24.2 | https://github.com/tree-sitter/tree-sitter-php | MIT | `HIGHLIGHTS_QUERY`; `queries/php-injections.scm`⁸ (grammar `LANGUAGE_PHP`) |
| csharp | tree-sitter-c-sharp | 0.23.5 | https://github.com/tree-sitter/tree-sitter-c-sharp | MIT | `HIGHLIGHTS_QUERY` |
| scala | tree-sitter-scala | 0.26.2 | https://github.com/tree-sitter/tree-sitter-scala | MIT | `HIGHLIGHTS_QUERY` |
| xml | tree-sitter-xml | 0.7.0 | https://github.com/tree-sitter-grammars/tree-sitter-xml | MIT | `XML_HIGHLIGHT_QUERY` (grammar `LANGUAGE_XML`) |
| swift | tree-sitter-swift | 0.7.3 | https://github.com/alex-pinkus/tree-sitter-swift | MIT | `HIGHLIGHTS_QUERY` |
| kotlin | tree-sitter-kotlin-sg | 0.4.1 | https://github.com/ast-grep/tree-sitter-kotlin, a publication of https://github.com/fwcd/tree-sitter-kotlin⁵ | MIT | `HIGHLIGHTS_QUERY`¹, then `queries/kotlin.scm`⁶ |
| sql | tree-sitter-sequel | 0.3.11 | https://github.com/derekstride/tree-sitter-sql | MIT | `HIGHLIGHTS_QUERY`, then `queries/sql.scm`⁷ |
| dockerfile | tree-sitter-containerfile | 0.9.2 | https://github.com/wharflab/tree-sitter-containerfile⁵ | MIT | `HIGHLIGHTS_QUERY`; `INJECTIONS_QUERY` |
| make | tree-sitter-make | 1.1.1 | https://github.com/tree-sitter-grammars/tree-sitter-make | MIT | `HIGHLIGHTS_QUERY` |

¹ Each of these files says it comes from nvim-treesitter (Apache-2.0, an
allowed source). Ship the Apache-2.0 license text with them.

² Vendored in `vendor/tree-sitter-yaml/` and used through `[patch.crates-io]`.
It's 0.7.2 with upstream PR #35 (commits `fa76593` and `8025f83`) applied to
`src/scanner.c`, which fixes YAML over 32,768 lines (issue #49). The licence and
queries are unchanged from 0.7.2. The vendored README says when to drop it.

³ Used in place of the crate's `HIGHLIGHTS_QUERY`, whose whole-pair `property`
capture can't be overridden. Adapted from nvim-treesitter (Apache-2.0, an
allowed source); ship the Apache-2.0 license text with it. Keys are `property`
and table headers `type`.

⁴ Goes after the crate's `HIGHLIGHTS_QUERY`. It restates that query's own
patterns for identifiers (MIT, same crate), so local variables aren't
highlighted as methods: the crate's method pattern depends on `#is-not? local`,
which we don't evaluate.

⁵ The canonical crates don't build against `tree-sitter` 0.27:
`tree-sitter-kotlin` 0.3.8 (fwcd) needs `tree-sitter` < 0.23 and
`tree-sitter-dockerfile` 0.2.0 (camdencheek) needs 0.20. `tree-sitter-kotlin-sg`
is fwcd's repository with only the crate name and metadata changed; its
`LICENSE` is fwcd's. `tree-sitter-containerfile` continues camdencheek's
grammar and ships both its own and camdencheek's MIT licence.

⁶ Goes after the crate's `HIGHLIGHTS_QUERY`, which is an older copy of the same
nvim-treesitter file. Adapted from nvim-treesitter (Apache-2.0, an allowed
source); ship the Apache-2.0 license text with it. It names functions declared
with modifiers or annotations, the `package` keyword and path, `companion`,
and SCREAMING_CASE constants.

⁷ Goes after the crate's `HIGHLIGHTS_QUERY`. It restates that query's two
number patterns (MIT, same crate) as regexes: the crate writes them with the
Lua class `%d`, so they never matched and numbers were strings.

⁸ Used in place of the crate's `INJECTIONS_QUERY`. Adapted from that query
(MIT, same crate): heredocs and nowdocs capture their text pieces, combined,
instead of the whole body, whose text sits in named children that injections
leave out.

⁹ `(code_fence_content) @text.literal`, written for fastedit, in
`crates/fe-syntax/src/language.rs`. The crate's query clears fenced code with
`@none`; this keeps it `markup.raw`.

How patterns take precedence follows tree-sitter-highlight 0.25 (MIT), which
was read for the rule, not copied: see `earlier_patterns_win` in
`crates/fe-syntax/src/language.rs`. Injections (`crates/fe-syntax/src/injection.rs`)
implement tree-sitter's documented `@injection.*` captures and `#set!`
properties; no code was copied or adapted for them.

## Icons

| Our path | Source | Tag / commit | License | Files |
|---|---|---|---|---|
| `kiln/crates/kiln-ui/assets/icons/` | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) `icons/` | 1.52.0 @ `500620a2e8123f8d1db191538886dc0c223f69a9` | ISC (Feather-derived icons: MIT) | `arrow-left.svg`, `arrow-right.svg`, `check.svg`, `chevron-right.svg`, `columns-2.svg`, `container.svg`, `copy.svg`, `database.svg`, `ellipsis.svg`, `file.svg`, `file-archive.svg`, `file-braces.svg`, `file-code.svg`, `file-cog.svg`, `file-diff.svg`, `file-image.svg`, `file-key.svg`, `file-lock.svg`, `file-music.svg`, `file-spreadsheet.svg`, `file-symlink.svg`, `file-terminal.svg`, `file-text.svg`, `file-video-camera.svg`, `files.svg`, `folder.svg`, `folder-open.svg`, `git-branch.svg`, `list-tree.svg`, `minus.svg`, `package.svg`, `rows-2.svg`, `scale.svg`, `search.svg`, `square.svg`, `x.svg` |

The 36 SVGs are copied unmodified, compiled into `kiln-ui` with `include_bytes!`.
Lucide is ISC, "Copyright (c) 2026 Lucide Icons and Contributors". Icons derived
from Feather are MIT, "Copyright (c) 2013-present Cole Bemis". Of ours, these are
Feather-derived:

- named in Lucide's `LICENSE` Feather list: `arrow-left`, `arrow-right`,
  `check`, `chevron-right`, `database`, `minus`, `search`, `square`, `x`, `columns-2`
  (listed under its old name `columns`) and `ellipsis` (listed as
  `more-horizontal`);
- not in that list, but crediting Feather's author `colebemis` as a contributor
  in `icons/<name>.json` at the commit, so treated as MIT too: `copy`, `file`,
  `file-text`, `folder`, `git-branch` and `package`.

The rest are ISC only. Lucide's `LICENSE`, which carries both notices, is
copied beside them as `kiln/crates/kiln-ui/assets/icons/LICENSE` and must ship with
the binary's notices.

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
