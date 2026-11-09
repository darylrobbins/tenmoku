# Third-party code

kiln/, the app framework, keeps its own copy of its rows (the `kiln/…` rows
below: `TextField`'s GPUI adaptation, the Lucide icons, the Tabler icons and
the Material Icon Theme icons) in `kiln/THIRD_PARTY.md`, with `kiln/NOTICE`, for when it leaves
this repository. Change both files together.

## Code copied or adapted into this repository

| Our path | Source | Commit | License |
|---|---|---|---|
| `crates/fe-display/src/wrap_map.rs` | [gpui-kit](https://github.com/longbridge/gpui-kit) `crates/base/src/input/editor/display_map/text_wrapper.rs`, `wrap_map.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-display/src/fold_map.rs` | gpui-kit `…/display_map/fold_map.rs`, `folding.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-display/src/display_map.rs` | gpui-kit `…/display_map/display_map.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-editor/src/diagnostics.rs` | gpui-kit `crates/base/src/input/editor/diagnostics.rs` | `2c5162f8c5b0c7fcec066ed53125d304c632bfe2` | Apache-2.0 |
| `crates/fe-markdown-view/src/blocks.rs` | gpui-kit `crates/base/src/text/utils.rs` (`list_item_prefix`, `ordered_list_ordinal`), `node.rs` (`CHECK_SVG_LIGHT`, `CHECK_SVG_DARK`) | `2154ba84afd2e771f117fd17e55c060dd5e03fd4` | Apache-2.0 |
| `crates/fe-markdown-view/src/table.rs` | gpui-kit `crates/base/src/text/node.rs` (`render_table`, `render_table_rows`) | `2154ba84afd2e771f117fd17e55c060dd5e03fd4` | Apache-2.0 |
| `crates/fe-config/themes/vscode-dark-modern.toml` | [VS Code](https://github.com/microsoft/vscode) `extensions/theme-defaults/themes/dark_modern.json`, `dark_plus.json`, `dark_vs.json` | `ab8f31f0c0b1a4dd86d8dbee1248542f5806574d` | MIT |
| `crates/fe-config/themes/vscode-2026-dark.toml` | [VS Code](https://github.com/microsoft/vscode) `extensions/theme-defaults/themes/2026-dark.json`, `dark_modern.json`, `dark_plus.json`, `dark_vs.json` | `d956e8a6ac5d904486557eea07e1d6bd62a4d36f` | MIT |
| `crates/fe-config/themes/vscode-2026-light.toml` | [VS Code](https://github.com/microsoft/vscode) `extensions/theme-defaults/themes/2026-light.json`, `light_modern.json`, `light_plus.json`, `light_vs.json` | `d956e8a6ac5d904486557eea07e1d6bd62a4d36f` | MIT |
| `crates/fe-config/themes/vscode-monokai.toml` | [VS Code](https://github.com/microsoft/vscode) `extensions/theme-monokai/themes/monokai-color-theme.json` (colours from [Colorsublime-Themes](https://github.com/Colorsublime/Colorsublime-Themes) per its `cgmanifest.json`) | `f93fdeb454a0f0fafe5f310408cec48dd3b6f58a` | MIT |
| `crates/fe-config/themes/catppuccin-latte.toml` | [Catppuccin](https://github.com/catppuccin/vscode) `packages/catppuccin-vsc/themes/latte.json` (built; as published in Catppuccin.catppuccin-vsc 3.19.0) | `befc9e6fc41980f4241408f7049755d47c06ff45` | MIT |
| `crates/fe-config/themes/catppuccin-frappe.toml` | [Catppuccin](https://github.com/catppuccin/vscode) `packages/catppuccin-vsc/themes/frappe.json` (built; as published in Catppuccin.catppuccin-vsc 3.19.0) | `befc9e6fc41980f4241408f7049755d47c06ff45` | MIT |
| `crates/fe-config/themes/catppuccin-macchiato.toml` | [Catppuccin](https://github.com/catppuccin/vscode) `packages/catppuccin-vsc/themes/macchiato.json` (built; as published in Catppuccin.catppuccin-vsc 3.19.0) | `befc9e6fc41980f4241408f7049755d47c06ff45` | MIT |
| `crates/fe-config/themes/catppuccin-mocha.toml` | [Catppuccin](https://github.com/catppuccin/vscode) `packages/catppuccin-vsc/themes/mocha.json` (built; as published in Catppuccin.catppuccin-vsc 3.19.0) | `befc9e6fc41980f4241408f7049755d47c06ff45` | MIT |
| `crates/fe-syntax/queries/toml.scm` | [nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter) `runtime/queries/toml/highlights.scm` | `e289100ff98969e118c702199d88b764ce9e7fdf` | Apache-2.0 |
| `crates/fe-syntax/queries/kotlin.scm` | nvim-treesitter `runtime/queries/kotlin/highlights.scm` | `3e1f54c1ce0a86959313f1cb7f60262ea57aaa23` | Apache-2.0 |
| `crates/fe-syntax/src/indent.rs` | nvim-treesitter `lua/nvim-treesitter/indent.lua` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/c.scm` | nvim-treesitter `runtime/queries/c/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/cpp.scm` | nvim-treesitter `runtime/queries/cpp/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/css.scm` | nvim-treesitter `runtime/queries/css/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/go.scm` | nvim-treesitter `runtime/queries/go/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/java.scm` | nvim-treesitter `runtime/queries/java/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/javascript.scm` | nvim-treesitter `runtime/queries/ecma/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/json.scm` | nvim-treesitter `runtime/queries/json/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/jsx.scm` | nvim-treesitter `runtime/queries/jsx/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/lua.scm` | nvim-treesitter `runtime/queries/lua/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/python.scm` | nvim-treesitter `runtime/queries/python/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/rust.scm` | nvim-treesitter `runtime/queries/rust/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/tsx.scm` | nvim-treesitter `runtime/queries/tsx/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
| `crates/fe-syntax/queries/indents/typescript.scm` | nvim-treesitter `runtime/queries/typescript/indents.scm` | `1a4f47b03dfe92d28de7d5654c67327ef008c9e6` | Apache-2.0 |
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

`crates/fe-syntax/src/indent.rs` reimplements in Rust the walk of
nvim-treesitter's `indent.lua` that works a row's indentation out from those
captures; its module docs say what was changed.

The indent queries in `crates/fe-syntax/queries/indents/` adapt
nvim-treesitter's `indents.scm` for twelve languages (JavaScript's from its
shared `ecma` file). Each starts with a header naming the source file,
commit and licence and saying what was changed: mostly captures on brackets
dropped, as `indent.rs` handles brackets itself, `@indent.align`, which we
don't support, replaced, and patterns of ours added at the end.

The outline and definitions queries adapted from a grammar crate's
`queries/tags.scm` come from the crate version `fe-syntax` already links
(MIT, listed below), so its licence covers them. Each starts with a header
naming the source file, version and licence and saying what was changed.
None adapts nvim-treesitter. The other outline and definitions queries for
C++, C#, Ruby, Swift and Lua are ours, written from the grammar's
`node-types.json`, and say so in their headers.

## Bundled assets audit (2026-10-31)

One line per class of shipped non-code asset. Result: one gap, fixed in this card (see icons).

- Fonts: none bundled at the audit; text rendered in system fonts. No gap. Since T1511, Instrument Sans, Spline Sans Mono and Instrument Serif (OFL-1.1) ship in `crates/fe-fonts/fonts/`, each with its `OFL.txt`, in the Fonts table below and NOTICE.
- App icon: `crates/fastedit/assets/icon/fastedit.svg` and `fastedit-1024.png` are the author's own work, Apache-2.0 like Tenmoku, with a NOTICE entry. No gap.
- Themes: `tenmoku-*` are our own; `vscode-*` and `catppuccin-*` each have a row above and a NOTICE entry (MIT). No gap.
- Icons: Lucide (ISC, Feather-derived MIT) and Material Icon Theme (MIT), each with its LICENSE file under `kiln/crates/kiln-ui/assets/`. Gap found and fixed: eight Lucide icons (`chevron-down`, `cloud`, `fold-vertical`, `list-x`, `refresh-cw`, `rotate-ccw`, `tag`, `unfold-vertical`) shipped with no row; they are now in the icons table, the count is 68 and the Feather classification is updated.
- Grammars and queries: every grammar crate is in the table below with its licence; adapted queries have rows above, the rest are ours. No gap.
- Vendored crates: each of the nine under `vendor/` has a LICENSE file and a footnote below. No gap.

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
| (runtime) | tree-sitter | 0.27.0, patched¹⁰ | https://github.com/tree-sitter/tree-sitter | MIT | |
| rust | tree-sitter-rust | 0.24.2 | https://github.com/tree-sitter/tree-sitter-rust | MIT | `HIGHLIGHTS_QUERY`; `queries/rust-injections.scm`¹¹ |
| python | tree-sitter-python | 0.25.0 | https://github.com/tree-sitter/tree-sitter-python | MIT | `HIGHLIGHTS_QUERY` |
| javascript | tree-sitter-javascript | 0.25.0 | https://github.com/tree-sitter/tree-sitter-javascript | MIT | `HIGHLIGHT_QUERY` + `JSX_HIGHLIGHT_QUERY`; `INJECTIONS_QUERY` |
| typescript | tree-sitter-typescript | 0.23.2 | https://github.com/tree-sitter/tree-sitter-typescript | MIT | JavaScript's, then `HIGHLIGHTS_QUERY`; JavaScript's `INJECTIONS_QUERY` |
| json | tree-sitter-json | 0.24.8 | https://github.com/tree-sitter/tree-sitter-json | MIT | `HIGHLIGHTS_QUERY` |
| toml | tree-sitter-toml-ng | 0.7.0 | https://github.com/tree-sitter-grammars/tree-sitter-toml | MIT | `queries/toml.scm`³ |
| go | tree-sitter-go | 0.25.0 | https://github.com/tree-sitter/tree-sitter-go | MIT | `HIGHLIGHTS_QUERY` |
| printf | tree-sitter-printf | 0.5.1 | https://github.com/tree-sitter-grammars/tree-sitter-printf | ISC | parse only; verbs found by `src/verbs.rs` (ours); `queries/go-injections.scm` (idea from nvim-treesitter `queries/go/injections.scm`, Apache-2.0; patterns ours) |
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
| gitignore | tree-sitter-gitignore, vendored¹² | commit `f4685bf` | https://github.com/shunsambongi/tree-sitter-gitignore | MIT | `queries/highlights.scm` (ours; upstream has none) |
| gitcommit | tree-sitter-git-commit, vendored¹³ | commit `5a50da1` | https://github.com/the-mikedavis/tree-sitter-git-commit | MIT | `queries/gitcommit.scm` (ours, in place of upstream's placeholder captures) |
| gitrebase | tree-sitter-git-rebase, vendored¹³ | commit `32686d6` | https://github.com/the-mikedavis/tree-sitter-git-rebase | MIT | `HIGHLIGHTS_QUERY`, then `queries/gitrebase.scm` (ours) |
| gitconfig | tree-sitter-git-config, vendored¹³ | commit `3a61756` | https://github.com/the-mikedavis/tree-sitter-git-config | MIT | `HIGHLIGHTS_QUERY` |
| gitattributes | tree-sitter-gitattributes, vendored¹³ | commit `1b7af09` | https://github.com/tree-sitter-grammars/tree-sitter-gitattributes | MIT | `HIGHLIGHTS_QUERY` |
| ini | tree-sitter-ini | 1.4.0 | https://github.com/justinmk/tree-sitter-ini | Apache-2.0 | `HIGHLIGHTS_QUERY`, then `queries/ini.scm` (ours) |
| dotenv | tree-sitter-bash, as bash above | 0.25.1 | https://github.com/tree-sitter/tree-sitter-bash | MIT | bash's `HIGHLIGHT_QUERY`, then `queries/dotenv.scm` (ours) |
| just | tree-sitter-just, vendored¹⁴ | commit `5685543` | https://github.com/casey/tree-sitter-just | Apache-2.0¹⁴ | `HIGHLIGHTS_QUERY`; `INJECTIONS_QUERY` |
| diff | tree-sitter-diff | 0.1.0 | https://github.com/the-mikedavis/tree-sitter-diff | MIT | `HIGHLIGHTS_QUERY`, then `queries/diff.scm` (ours) |
| json5 | tree-sitter-json5, vendored¹⁵ | commit `248b856` | https://github.com/Joakker/tree-sitter-json5 | MIT | `HIGHLIGHTS_QUERY`, then `queries/json5.scm` (ours) |
| csv, tsv | fe-grammar-csv, ours¹⁶ | in this repository | `crates/fe-grammar-csv` | ours | `queries/csv.scm` (ours) |

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

¹⁰ Vendored in `vendor/tree-sitter/` and used through `[patch.crates-io]`.
It's 0.27.0 (upstream commit `6070dbf`) with a fastedit patch to
`src/wasm_store.c`, `src/parser.c` and `binding_rust/wasm_language.rs`, for
extensions' WebAssembly grammars. Each Wasm store gets an epoch deadline and
a callback that stops code the thread's check says to stop. The assertions
after calls into a module become errors. The lexer and scanner functions a
module supplies are type-checked, and so are the tokens they return and the
host callbacks they make. The licence is unchanged. The
vendored README lists the changes and says when to drop it.

¹¹ Used in place of the crate's `INJECTIONS_QUERY`. Adapted from that query
(MIT, same crate): it keeps only a pattern injecting doc comments as inline
Markdown, combined, and drops the macro token-tree patterns.

How patterns take precedence follows tree-sitter-highlight 0.25 (MIT), which
was read for the rule, not copied: see `earlier_patterns_win` in
`crates/fe-syntax/src/language.rs`. Injections (`crates/fe-syntax/src/injection.rs`)
implement tree-sitter's documented `@injection.*` captures and `#set!`
properties; no code was copied or adapted for them.

### Grammars shipped as WebAssembly

`.github/workflows/grammars.yml` compiles these from the pinned crate's source
and attaches the modules to a release. `extensions/grammars/<language>/` holds
each one's manifest, queries (the same as the built-in language's, above) and
licence. No module is committed.

| Language | Release asset | Pin (`Cargo.lock`) | Licence |
|---|---|---|---|
| kotlin | `tree-sitter-kotlin.wasm` | tree-sitter-kotlin-sg 0.4.1 | MIT; `highlights.scm` also has nvim-treesitter's Apache-2.0 patterns⁶, with the text in `LICENSE-APACHE` |
| csharp | `tree-sitter-c-sharp.wasm` | tree-sitter-c-sharp 0.23.5 | MIT |
| scala | `tree-sitter-scala.wasm` | tree-sitter-scala 0.26.2 | MIT |
| swift | `tree-sitter-swift.wasm` | tree-sitter-swift 0.7.3 | MIT |
| cpp | `tree-sitter-cpp.wasm` | tree-sitter-cpp 0.23.4 | MIT; `highlights.scm` starts with tree-sitter-c 0.24.2's query, whose licence is the same text and holder |
| sql | `tree-sitter-sql.wasm` | tree-sitter-sequel 0.3.11 | MIT |
| starlark | `tree-sitter-starlark.wasm` | tree-sitter-starlark 1.3.0, commit `a453dbf3ba433db0e5ec621a38a7e59d72e4dc69` of github.com/tree-sitter-grammars/tree-sitter-starlark | MIT; `highlights.scm` is upstream's query, adapted, which comes from tree-sitter-python (MIT) |
| php | `tree-sitter-php.wasm` | tree-sitter-php 0.24.2 (grammar `php`, not `php_only`) | MIT |
| ruby | `tree-sitter-ruby.wasm` | tree-sitter-ruby 0.23.1 | MIT |
| julia | `tree-sitter-julia.wasm` | https://github.com/tree-sitter/tree-sitter-julia at `e0f9dcd180fdcfcfa8d79a3531e11d99e79321d3` (0.25.0), `src` | MIT |
| vue | `tree-sitter-vue.wasm` | https://github.com/tree-sitter-grammars/tree-sitter-vue `ce8011a414fdf8091f4e4071752efc376f4afb08` | MIT; `highlights.scm` and `injections.scm` are nvim-treesitter's Apache-2.0 queries (`runtime/queries/html_tags/` and `vue/` at `915f2bd3d4e2a33704bfc476b5137e9befa82f3d`), adapted, with the text in `LICENSE-APACHE` |
| proto | `tree-sitter-proto.wasm` | https://github.com/coder3101/tree-sitter-proto `18366491f37d43d6d5d02cbb1e79b650b5728145` | MIT; `highlights.scm` is the grammar's own query, adapted |
| cmake | `tree-sitter-cmake.wasm` | https://github.com/uyha/tree-sitter-cmake at `a3ec2b2144daf7132e7ee824d8cab9610523d10f` (`src`), v0.7.6; `highlights.scm` is upstream's `queries/highlights.scm`, adapted to drop `#lua-match?` and unused captures, plus a number pattern | MIT |
| nix | `tree-sitter-nix.wasm` | [nix-community/tree-sitter-nix](https://github.com/nix-community/tree-sitter-nix) @ `e9f150648a7e0015b77f4c9b315648b389d6364e` | MIT; `highlights.scm` and `injections.scm` are upstream's (same licence), less the `#is-not? local` predicates |
| latex | `tree-sitter-latex.wasm` | https://github.com/latex-lsp/tree-sitter-latex at `aca6eeba1b9556685bf0aa1cdc4a902cba155823` (`src`), package version 0.3.0, the newest commit with a generated `parser.c`; `highlights.scm` and `injections.scm` are nvim-treesitter's `runtime/queries/latex/` at `2c30e515ebe79037ab8d15f7e59c0e2690f50626`, adapted to this grammar | MIT; the queries are nvim-treesitter's Apache-2.0, with the text in `LICENSE-APACHE` |
| dart | `tree-sitter-dart.wasm` | https://github.com/UserNobody14/tree-sitter-dart at `0038280b774b65c1b89ba7fec50624d8baed9fe1` | MIT; `highlights.scm` is upstream's `queries/highlights.scm`, with captures renamed |
| haskell | `tree-sitter-haskell.wasm` | https://github.com/tree-sitter/tree-sitter-haskell at `97288e585b0bd44199720280d067fa35bacd81ff` | MIT; `highlights.scm` and `injections.scm` are upstream's `queries/`, adapted as their headers say |
| zig | `tree-sitter-zig.wasm` | [tree-sitter-grammars/tree-sitter-zig](https://github.com/tree-sitter-grammars/tree-sitter-zig) at `6479aa13f32f701c383083d8b28360ebd682fb7d` | MIT; `highlights.scm` is adapted from the repository's own `queries/highlights.scm` at that commit (same licence) |
| elixir | `tree-sitter-elixir.wasm` | https://github.com/elixir-lang/tree-sitter-elixir at `cb32ccbe17c67037b09e31bf9beb22cdae6ed8e9` | Apache-2.0 (with upstream's `NOTICE`); `highlights.scm` and `injections.scm` are upstream's queries, adapted |
| elm | `tree-sitter-elm.wasm` | https://github.com/elm-tooling/tree-sitter-elm `e1e8fea161a1e66f3997855d316be2a43e4e956f` | MIT; `highlights.scm` is the grammar's own query, adapted: captures renamed to fe-syntax's vocabulary, GLSL injection dropped |
| gleam | `tree-sitter-gleam.wasm` | https://github.com/gleam-lang/tree-sitter-gleam @ `fa6d0d94804f1b342e175865a3dbeb316ee5115e` | Apache-2.0; `highlights.scm` and `injections.scm` are adapted from its `queries/` |
| solidity | `tree-sitter-solidity.wasm` | [JoranHonig/tree-sitter-solidity](https://github.com/JoranHonig/tree-sitter-solidity) @ `048fe686cb1fde267243739b8bdbec8fc3a55272` | MIT; `highlights.scm` is upstream's `queries/highlights-nvim.scm` (same licence and holder), with a few patterns dropped or adapted for our predicates and captures, and the member-call pattern reordered |
| astro | `tree-sitter-astro.wasm` | https://github.com/virchau13/tree-sitter-astro @ `213f6e6973d9b456c6e50e86f19f66877e7ef0ee` | MIT; `highlights.scm` and `injections.scm` are adapted from the grammar's own `queries/` at that commit, and add tree-sitter-html 0.23.2's `"/>"` pattern and `<style>` → css injection, whose licence is MIT too; their headers say what changed |
| twig | `tree-sitter-twig.wasm` | https://github.com/gbprod/tree-sitter-twig `2208d2a3c3ee7ef378e97df2e51c18feb7ee9dfc`; `highlights.scm` and `injections.scm` are its `queries/` (`@spell` dropped) | MIT |
| properties | `tree-sitter-properties.wasm` | [tree-sitter-grammars/tree-sitter-properties](https://github.com/tree-sitter-grammars/tree-sitter-properties) @ `6310671b24d4e04b803577b1c675d765cbd5773b` (`src`), package version 0.3.0 | MIT; `highlights.scm` is upstream's (same licence), with the secret provider key as `@string.special` rather than `@function` |
| kdl | `tree-sitter-kdl.wasm` | [tree-sitter-grammars/tree-sitter-kdl](https://github.com/tree-sitter-grammars/tree-sitter-kdl) @ `e8ff98a113e175b84b92ec2096820517c1e10a66` (`src`) | MIT; `highlights.scm` is upstream's (same licence), less the `#any-of?` pattern and `@spell`; `injections.scm` is comment-only |
| terraform | `tree-sitter-terraform.wasm` | https://github.com/tree-sitter-grammars/tree-sitter-hcl at `64ad62785d442eb4d45df3a1764962dafd5bc98b` (`dialects/terraform/src`) | Apache-2.0; `highlights.scm` is nvim-treesitter's Apache-2.0 `runtime/queries/hcl/highlights.scm` and `runtime/queries/terraform/highlights.scm` at `915f2bd3d4e2a33704bfc476b5137e9befa82f3d`, adapted, with the text in `LICENSE-APACHE` |
| groovy | `tree-sitter-groovy.wasm` | https://github.com/murtaza64/tree-sitter-groovy `2a6ddd558b6aa39c5b77d8db9fe9baf817486b2c` (`src`) | MIT; `highlights.scm` is that repository's own `queries/highlights.scm` (MIT) |
| hcl | `tree-sitter-hcl.wasm` | https://github.com/tree-sitter-grammars/tree-sitter-hcl @ `64ad62785d442eb4d45df3a1764962dafd5bc98b` | Apache-2.0; `highlights.scm` is adapted from nvim-treesitter's, also Apache-2.0, with the text in `LICENSE-APACHE` |
| svelte | `tree-sitter-svelte.wasm` | [tree-sitter-grammars/tree-sitter-svelte](https://github.com/tree-sitter-grammars/tree-sitter-svelte) `ae5199db47757f785e43a14b332118a5474de1a2` | MIT; `highlights.scm` and `injections.scm` start with nvim-treesitter's Apache-2.0 HTML patterns (commit `915f2bd3d4e2a33704bfc476b5137e9befa82f3d`), with the text in `LICENSE-APACHE` |
| erlang | `tree-sitter-erlang.wasm` | https://github.com/WhatsApp/tree-sitter-erlang at `62c82e67b73e13f3ef06e4c8c07ff035a8235d30` | Apache-2.0; `highlights.scm` is upstream's `queries/highlights.scm`, with captures renamed and type names as `@type` |
| r | `tree-sitter-r.wasm` | https://github.com/r-lib/tree-sitter-r at `58a22794466c0fc15b0d3b40531db751593721e8` | MIT; `highlights.scm` is upstream's `queries/highlights.scm`, with the `(ERROR)` capture dropped |
| ocaml | `tree-sitter-ocaml.wasm` | https://github.com/tree-sitter/tree-sitter-ocaml at `a1c562e73ab787dcc69fdc2e5510c53bbce5364f` (grammar `ocaml`, not `ocaml_interface` or `ocaml_type`) | MIT; `highlights.scm` is upstream's `queries/highlights.scm` at that commit, unchanged but for its header |
| perl | `tree-sitter-perl.wasm` | https://github.com/tree-sitter-perl/tree-sitter-perl at `4e56e286bbe60b1da410cf5cb3ebef75e134f30e` | MIT; `highlights.scm` and `injections.scm` are upstream's `queries/`, adapted as their headers say |
| fsharp | `tree-sitter-fsharp.wasm` | https://github.com/ionide/tree-sitter-fsharp at `aefd0c8741bdf3aeb827a228aa4a996a6536697e` (grammar `fsharp`, not `fsharp_signature`) | MIT; `highlights.scm` is upstream's `queries/highlights.scm` at that commit, with captures renamed as its header says |
| racket | `tree-sitter-racket.wasm` | https://github.com/6cdh/tree-sitter-racket @ `b3cbbde75ce2de3079e33c7fe90bd41b1d45be48` | MIT; `highlights.scm` is the grammar's own `queries/highlights.scm`, with a struct-name rule added |
| vim | `tree-sitter-vim.wasm` | [tree-sitter-grammars/tree-sitter-vim](https://github.com/tree-sitter-grammars/tree-sitter-vim) @ `039c8d0aa1deae00ddeb0374dd70bcc0ec56938d` (`grammar-source.toml`); `highlights.scm` and `injections.scm` are upstream's, trimmed | MIT |
| nginx | `tree-sitter-nginx.wasm` | https://github.com/opa-oz/tree-sitter-nginx at `7cdf006ed1addef698fc680ae31721950f85e5e9` (`src`), package version 1.0.1; `highlights.scm` is upstream's `queries/highlights.scm` less the `@spell` capture; `injections.scm` is comment-only (upstream's injects a `comment` language we lack) | MIT |
| scss | `tree-sitter-scss.wasm` | https://github.com/tree-sitter-grammars/tree-sitter-scss @ `5da5ba71a558b007b352c505bfcd9095ae337022` | MIT; `highlights.scm` is that grammar's query with tree-sitter-css 0.25.0's (MIT, another holder, text in `LICENSE-CSS`) inlined |
| graphql | `tree-sitter-graphql.wasm` | https://github.com/bkegley/tree-sitter-graphql `5e66e961eee421786bdda8495ed1db045e06b5fe` | MIT; `highlights.scm` is the grammar's own `queries/graphql/highlights.scm`, adapted |
| awk | `tree-sitter-awk.wasm` | [Beaglefoot/tree-sitter-awk](https://github.com/Beaglefoot/tree-sitter-awk) @ `34bbdc7cce8e803096f47b625979e34c1be38127` (`grammar-source.toml`); `highlights.scm` is upstream's `queries/highlights.scm`, adapted to fe-syntax's capture names | MIT |
| fish | `tree-sitter-fish.wasm` | [ram02z/tree-sitter-fish](https://github.com/ram02z/tree-sitter-fish) @ `b7f1d682941e0c62dfcd6bf9ef481638351bab16` (`src`), master HEAD; `highlights.scm` is upstream's `queries/highlights.scm`, unchanged; `injections.scm` has no patterns | Unlicense (public-domain dedication) |
| ron | `tree-sitter-ron.wasm` | [tree-sitter-grammars/tree-sitter-ron](https://github.com/tree-sitter-grammars/tree-sitter-ron) @ `78938553b93075e638035f624973083451b29055` (`grammar-source.toml`); `highlights.scm` and `injections.scm` are upstream's, trimmed | MIT OR Apache-2.0 |
| powershell | `tree-sitter-powershell.wasm` | [airbus-cert/tree-sitter-powershell](https://github.com/airbus-cert/tree-sitter-powershell) at `65ea6919e816a7ac142acec7ee72757f89db8439` | MIT; `highlights.scm` is adapted from the repository's own `queries/highlights.scm` at that commit (same licence) |
| clojure | `tree-sitter-clojure.wasm` | https://github.com/sogaiu/tree-sitter-clojure at `e43eff80d17cf34852dcd92ca5e6986d23a7040f` | CC0-1.0 (public-domain dedication); `highlights.scm` extends upstream's `queries/highlights.scm` (same licence) |
| fortran | `tree-sitter-fortran.wasm` | https://github.com/stadelmanma/tree-sitter-fortran at `72f8759dea28ff6671e0868a9ed5ad4a18413917` | MIT; `highlights.scm` is adapted from upstream's `queries/highlights.scm` at that commit, as its header says |
| objc | `tree-sitter-objc.wasm` | https://github.com/tree-sitter-grammars/tree-sitter-objc at `181a81b8f23a2d593e7ab4259981f50122909fda` | MIT; `highlights.scm` is tree-sitter-c 0.24.2's query (its licence text in `LICENSE-TREE-SITTER-C`) followed by upstream's `queries/highlights.scm`, adapted as its header says |
| hlsl | `tree-sitter-hlsl.wasm` | [tree-sitter-grammars/tree-sitter-hlsl](https://github.com/tree-sitter-grammars/tree-sitter-hlsl) at `bab9111922d53d43668fabb61869bec51bbcb915` | MIT; `highlights.scm` inlines nvim-treesitter's `runtime/queries/c/highlights.scm`, `runtime/queries/cpp/highlights.scm` and `runtime/queries/hlsl/highlights.scm` at `19bdfc04f40ed2ccb3340591d125f759b0406875` (Apache-2.0, text in `LICENSE-APACHE`), adapted as its header says |
| d | `tree-sitter-d.wasm` | [gdamore/tree-sitter-d](https://github.com/gdamore/tree-sitter-d) at `64f27931b4e6fdd75af1102c79bacbca68a8dacc` (v0.9.1) | MIT; `highlights.scm` inlines upstream's own `queries/highlights.scm` (MIT) at that commit and nvim-treesitter's `runtime/queries/d/highlights.scm` at `19bdfc04f40ed2ccb3340591d125f759b0406875` (Apache-2.0, text in `LICENSE-APACHE`), adapted as its header says |
| erb | `tree-sitter-embedded-template.wasm` | https://github.com/tree-sitter/tree-sitter-embedded-template @ `3499d85f0a0d937c507a4a65368f2f63772786e1` (`src`) | MIT; `highlights.scm` is the grammar's own `queries/highlights.scm` and `injections.scm` its `queries/injections-erb.scm` (HTML and Ruby), both at that commit and MIT, unchanged |
| ejs | `tree-sitter-embedded-template.wasm` | https://github.com/tree-sitter/tree-sitter-embedded-template @ `3499d85f0a0d937c507a4a65368f2f63772786e1` (`src`) | MIT; `highlights.scm` is the grammar's own `queries/highlights.scm` and `injections.scm` its `queries/injections-ejs.scm` (HTML and JavaScript), both at that commit and MIT, unchanged |
| php_only | `tree-sitter-php-only.wasm` | tree-sitter-php 0.24.2 from crates.io (sha256 `0d8c17c3ab69052c5eeaa7ff5cd972dd1bc25d1b97ee779fec391ad3b5df5592`, `php_only/src`; grammar `php_only`, not `php`) | MIT; `highlights.scm` and `injections.scm` are php/'s: the crate's `queries/highlights.scm` unchanged, and its `queries/injections.scm` adapted (heredoc pieces combined, `phpdoc` dropped), as their headers say |
| blade | `tree-sitter-blade.wasm` | https://github.com/EmranMR/tree-sitter-blade @ `b5291d1ba207a8ebb8383b2ecb8a8a6535210a50` (`src`) | MIT; `highlights.scm` and `injections.scm` are adapted from the grammar's own `queries/` at that commit (the Envoy bash injection dropped, and the Alpine `:attr` javascript injection kept off `x-*` components, whose `:attr` is php_only, and the tag-scoped attribute patterns also matching attributes in `@if … @endif` groups), and inline tree-sitter-html 0.23.2's `queries/highlights.scm` and `queries/injections.scm` in place of their `; inherits: html`, whose licence is MIT too; their headers say what changed |
| gomod | `tree-sitter-gomod-orchard.wasm` | https://github.com/grammar-orchard/tree-sitter-gomod-orchard @ `97801af57aac43d45fb68dab0ac47f0d0ab18bd1` (`src`), 0.5.8, the maintained fork of camdencheek/tree-sitter-go-mod | MIT; `highlights.scm` is adapted from the grammar's own `queries/highlights.scm` at that commit (same licence), its header saying what changed |
| gowork | `tree-sitter-gowork.wasm` | https://github.com/omertuc/tree-sitter-go-work @ `949a8a470559543857a62102c84700d291fc984c` (`src`), 0.1.0 | MIT (the `LICENSE` file; `package.json` says ISC); `highlights.scm` is adapted from the grammar's own `queries/highlights.scm` at that commit (same licence), its header saying what changed |
| gosum | `tree-sitter-gosum.wasm` | https://github.com/tree-sitter-grammars/tree-sitter-go-sum @ `27816eb6b7315746ae9fcf711e4e1396dc1cf237` (`src`), 1.0.0 | MIT; `highlights.scm` is adapted from the grammar's own `queries/highlights.scm` at that commit (same licence), its header saying what changed |
| heex | `tree-sitter-heex.wasm` | https://github.com/phoenixframework/tree-sitter-heex at `5842537f734d7c12685bf27d6005313e3e5a47a0` (`src`) | MIT; `highlights.scm` and `injections.scm` are the grammar's own `queries/` at that commit (same licence), adapted as their headers say: no `(ERROR)` capture, and `<style>`/`<script>` inject their `(text)` children rather than using `#offset!` |
| razor | `tree-sitter-razor.wasm` | https://github.com/tris203/tree-sitter-razor at `d4664e409caaea12f73c9525484e3cf88b1cf718` (`src`), package version 0.1.1 | MIT; `highlights.scm` inlines tree-sitter-c-sharp 0.23.5's `queries/highlights.scm` (commit `cac6d5fb595f5811a076336682d5d595ac1c9e85` of https://github.com/tree-sitter/tree-sitter-c-sharp, MIT, another holder, text in `LICENSE-TREE-SITTER-CSHARP`), which upstream's query inherits, then upstream's own `queries/highlights.scm` at that commit (same licence); `injections.scm` is upstream's, less the `comment` injections; their headers say what changed |
| liquid | `tree-sitter-liquid.wasm` | https://github.com/hankthetank27/tree-sitter-liquid `e45dbac8c5fa95b1f0e00e7e0c04bc8855823391`; `highlights.scm` and `injections.scm` are its `queries/` (`#set! priority` and `@none` dropped) | MIT |
| gotmpl | `tree-sitter-gotmpl.wasm` | [ngalaiko/tree-sitter-go-template](https://github.com/ngalaiko/tree-sitter-go-template) at `06b2fd2ac32a7be71cdcf925eeeb868a68e2acde` | MIT; `highlights.scm` is adapted from the grammar's own `queries/highlights.scm` at that commit (MIT too), as its header says; `injections.scm` is comment-only |

A language added after these needn't have a `Cargo.lock` pin. Its directory's
`grammar-source.toml` names its source in one of two more forms: a GitHub
repository at a full 40-hex commit (`git`, `rev`), fetched whole, or a crates.io
archive checked against its SHA-256 (`registry = "crates.io"`, `crate`,
`version`, `sha256`). Its row's pin is that repository and commit, or that crate
and version; its queries say in their headers where they came from
(`extensions/grammars/README.md`).

## Icons

| Our path | Source | Tag / commit | License | Files |
|---|---|---|---|---|
| `kiln/crates/kiln-ui/assets/icons/` | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) `icons/` | 1.52.0 @ `500620a2e8123f8d1db191538886dc0c223f69a9` | ISC (Feather-derived icons: MIT) | `app-window.svg`, `arrow-left.svg`, `arrow-right.svg`, `asterisk.svg`, `bell.svg`, `bot.svg`, `box.svg`, `braces.svg`, `bug.svg`, `case-sensitive.svg`, `check.svg`, `chevron-down.svg`, `chevron-right.svg`, `chevron-up.svg`, `circle-check.svg`, `circle-dot.svg`, `circle-x.svg`, `cloud.svg`, `code-xml.svg`, `code.svg`, `coffee.svg`, `columns-2.svg`, `component.svg`, `container.svg`, `copy.svg`, `croissant.svg`, `database.svg`, `diamond.svg`, `droplet.svg`, `ellipsis.svg`, `eye.svg`, `file-archive.svg`, `file-box.svg`, `file-braces.svg`, `file-clock.svg`, `file-code.svg`, `file-cog.svg`, `file-diff.svg`, `file-image.svg`, `file-key.svg`, `file-lock.svg`, `file-music.svg`, `file-sliders.svg`, `file-spreadsheet.svg`, `file-symlink.svg`, `file-terminal.svg`, `file-text.svg`, `file-type.svg`, `file-video-camera.svg`, `file.svg`, `files.svg`, `flask-conical.svg`, `fold-vertical.svg`, `folder-archive.svg`, `folder-bookmark.svg`, `folder-check.svg`, `folder-clock.svg`, `folder-code.svg`, `folder-cog.svg`, `folder-git-2.svg`, `folder-git.svg`, `folder-input.svg`, `folder-kanban.svg`, `folder-open.svg`, `folder-output.svg`, `folder-pen.svg`, `folder.svg`, `gem.svg`, `git-branch.svg`, `git-commit-vertical.svg`, `git-graph.svg`, `hammer.svg`, `hash.svg`, `heading.svg`, `lambda.svg`, `layers.svg`, `link.svg`, `list-checks.svg`, `list-tree.svg`, `list-video.svg`, `list-x.svg`, `list.svg`, `loader-circle.svg`, `lock.svg`, `log-out.svg`, `message-square.svg`, `minus.svg`, `moon.svg`, `package.svg`, `panel-right-open.svg`, `panels-top-left.svg`, `parentheses.svg`, `pin.svg`, `play.svg`, `plug.svg`, `redo.svg`, `refresh-cw.svg`, `regex.svg`, `rotate-ccw.svg`, `rows-2.svg`, `save.svg`, `scale.svg`, `scroll-text.svg`, `search.svg`, `server.svg`, `settings.svg`, `sigma.svg`, `sliders-horizontal.svg`, `snowflake.svg`, `sparkle.svg`, `sprout.svg`, `square-dashed-text.svg`, `square-function.svg`, `square-minus.svg`, `square-plus.svg`, `square-terminal.svg`, `square.svg`, `star.svg`, `table.svg`, `tag.svg`, `target.svg`, `terminal.svg`, `text-wrap.svg`, `toggle-right.svg`, `tree-deciduous.svg`, `triangle-alert.svg`, `triangle.svg`, `type.svg`, `undo.svg`, `unfold-vertical.svg`, `variable.svg`, `whole-word.svg`, `x.svg`, `zap.svg` |
| `kiln/crates/kiln-ui/assets/tabler-icons/` | [tabler/tabler-icons](https://github.com/tabler/tabler-icons) `icons/outline/` | v3.50.0 @ `6904efd245391ad396df17654738f52f6da8c40e` | MIT | `brand-astro.svg`, `brand-c-sharp.svg`, `brand-cpp.svg`, `brand-css3.svg`, `brand-deno.svg`, `brand-docker.svg`, `brand-git.svg`, `brand-github.svg`, `brand-golang.svg`, `brand-html5.svg`, `brand-javascript.svg`, `brand-kotlin.svg`, `brand-nodejs.svg`, `brand-npm.svg`, `brand-php.svg`, `brand-pnpm.svg`, `brand-powershell.svg`, `brand-python.svg`, `brand-react.svg`, `brand-rust.svg`, `brand-sass.svg`, `brand-svelte.svg`, `brand-swift.svg`, `brand-terraform.svg`, `brand-typescript.svg`, `brand-vue.svg`, `brand-yarn.svg`, `letter-c.svg`, `letter-e.svg`, `letter-f.svg`, `letter-g.svg`, `letter-j.svg`, `letter-k.svg`, `letter-o.svg`, `letter-p.svg`, `letter-r.svg`, `letter-v.svg`, `markdown.svg` |
| `kiln/crates/kiln-ui/assets/file-icons/` | [material-extensions/vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) `icons/`, as built into the npm package `material-icon-theme` 5.40.0 | v5.40.0 @ `045291ff033062a25441dd297fd51546326866f2` | MIT | `audio.svg`, `bun.svg`, `bun_light.svg`, `c.svg`, `certificate.svg`, `changelog.svg`, `cmake.svg`, `console.svg`, `cpp.svg`, `csharp.svg`, `css.svg`, `dart.svg`, `database.svg`, `diff.svg`, `docker.svg`, `document.svg`, `editorconfig.svg`, `elixir.svg`, `erlang.svg`, `exe.svg`, `file.svg`, `folder-benchmark-open.svg`, `folder-benchmark.svg`, `folder-config-open.svg`, `folder-config.svg`, `folder-css-open.svg`, `folder-css.svg`, `folder-dist-open.svg`, `folder-dist.svg`, `folder-docs-open.svg`, `folder-docs.svg`, `folder-examples-open.svg`, `folder-examples.svg`, `folder-font-open.svg`, `folder-font.svg`, `folder-git-open.svg`, `folder-git.svg`, `folder-github-open.svg`, `folder-github.svg`, `folder-i18n-open.svg`, `folder-i18n.svg`, `folder-images-open.svg`, `folder-images.svg`, `folder-include-open.svg`, `folder-include.svg`, `folder-lib-open.svg`, `folder-lib.svg`, `folder-mock-open.svg`, `folder-mock.svg`, `folder-node-open.svg`, `folder-node.svg`, `folder-open.svg`, `folder-packages-open.svg`, `folder-packages.svg`, `folder-public-open.svg`, `folder-public.svg`, `folder-resource-open.svg`, `folder-resource.svg`, `folder-rust-open.svg`, `folder-rust.svg`, `folder-scripts-open.svg`, `folder-scripts.svg`, `folder-src-open.svg`, `folder-src.svg`, `folder-target-open.svg`, `folder-target.svg`, `folder-temp-open.svg`, `folder-temp.svg`, `folder-test-open.svg`, `folder-test.svg`, `folder-theme-open.svg`, `folder-theme.svg`, `folder-tools-open.svg`, `folder-tools.svg`, `folder-utils-open.svg`, `folder-utils.svg`, `folder-vscode-open.svg`, `folder-vscode.svg`, `folder.svg`, `font.svg`, `gemfile.svg`, `git.svg`, `go-mod.svg`, `go.svg`, `gradle.svg`, `h.svg`, `haskell.svg`, `hpp.svg`, `html.svg`, `image.svg`, `java.svg`, `javascript.svg`, `json.svg`, `julia.svg`, `just.svg`, `key.svg`, `kotlin.svg`, `less.svg`, `license.svg`, `lock.svg`, `log.svg`, `lua.svg`, `makefile.svg`, `markdown.svg`, `maven.svg`, `nix.svg`, `nodejs.svg`, `npm.svg`, `ocaml.svg`, `pdf.svg`, `php.svg`, `pnpm.svg`, `pnpm_light.svg`, `powershell.svg`, `python-misc.svg`, `python.svg`, `r.svg`, `react.svg`, `react_ts.svg`, `readme.svg`, `ruby.svg`, `rust.svg`, `sass.svg`, `scala.svg`, `settings.svg`, `svelte.svg`, `svg.svg`, `swift.svg`, `table.svg`, `tex.svg`, `toml.svg`, `toml_light.svg`, `tune.svg`, `typescript-def.svg`, `typescript.svg`, `video.svg`, `vue.svg`, `xml.svg`, `yaml.svg`, `yarn.svg`, `zig.svg`, `zip.svg` |

The 134 SVGs are copied with one modification, which ISC permits: each root
`<svg>`'s `stroke-width="2"` is changed to `stroke-width="1.75"`, the design's
line weight; the paths are Lucide's. They are compiled into `kiln-ui` with
`include_bytes!`.
Lucide is ISC, "Copyright (c) 2026 Lucide Icons and Contributors". Icons derived
from Feather are MIT, "Copyright (c) 2013-present Cole Bemis". Of ours, these are
Feather-derived:

- named in Lucide's `LICENSE` Feather list: `arrow-left`, `arrow-right`,
  `check`, `chevron-right`, `code`, `database`, `hash`, `link`, `lock`,
  `minus`, `search`, `server`, `square`, `type`, `x`, `columns-2` (listed
  under its old name `columns`), `ellipsis` (listed as `more-horizontal`),
  `circle-x` (listed as `x-circle`), `triangle-alert` (listed as
  `alert-triangle`) and `chevron-down`;
- not in that list, but crediting Feather's author `colebemis` as a contributor
  in `icons/<name>.json` at the commit, so treated as MIT too: `box`, `copy`,
  `eye`, `file`, `file-text`, `folder`, `git-branch`, `list`, `message-square`,
  `package` and `table`;
- added by the 2026-10-31 audit (listed in no row before), not in the Feather
  list: `cloud`, `refresh-cw`, `rotate-ccw` and `tag` are Feather icons by name,
  so they are treated as MIT too, conservatively; `fold-vertical`, `list-x` and
  `unfold-vertical` are Lucide-only, so ISC;
- added for the which-key popup's row icons: `log-out` and `terminal` are in
  the Feather list; `panels-top-left` (Feather's `layout`), `play`, `save`,
  `settings` and `toggle-right` credit `colebemis` as a contributor, so are
  treated as MIT too; `app-window`, `bot`, `bug`, `flask-conical`, `redo`,
  `text-wrap` and `undo` are Lucide-only, so ISC.
- added for the design's B3 surfaces: `chevron-up`, `square-minus` (listed as
  `minus-square`) and `square-plus` (listed as `plus-square`) are in the
  Feather list; `bell` credits `colebemis` as a contributor, so is treated as
  MIT too; `case-sensitive`, `circle-check`, `git-commit-vertical`,
  `list-video`, `loader-circle`, `regex`, `square-dashed-text` (find in
  selection; `text-select` is its deprecated alias), `square-terminal` and
  `whole-word` are Lucide-only, so ISC.

- added for `TintedIcons`' language glyphs: `moon`, `target` and `triangle`
  are in the Feather list; `coffee`, `droplet`, `layers`, `star` and `zap`
  credit `colebemis` as a contributor, so are treated as MIT too; `code-xml`,
  `file-box`, `file-sliders`, `file-type`, `gem`, `hammer`, `lambda`,
  `list-checks`, `parentheses`, `scroll-text`, `sigma`, `sliders-horizontal`,
  `snowflake`, `sparkle`, `sprout` and `tree-deciduous` are Lucide-only, so
  ISC.
- added for `TintedIcons`' project files and folder roles: `folder-bookmark`
  and `folder-code` credit `colebemis` as a contributor, so are treated as
  MIT too; `croissant`, `file-clock`, `folder-archive`, `folder-check`,
  `folder-clock`, `folder-cog`, `folder-git`, `folder-git-2`, `folder-input`,
  `folder-kanban`, `folder-output` and `folder-pen` are Lucide-only, so ISC.

The rest are ISC only. Lucide's `LICENSE`, which carries both notices, is
copied beside them as `kiln/crates/kiln-ui/assets/icons/LICENSE` and must ship with
the binary's notices.

The 38 SVGs of Tabler Icons, `TintedIcons`' marks for the languages and tools
Lucide has none for (`kiln/crates/kiln-ui/src/file_icons_tinted.rs`), are copied
from `icons/outline/` with two modifications, which MIT permits: each file's
leading metadata comment (tags, category, version, unicode) is removed, and
the root `<svg>`'s `stroke-width="2"` is changed to `stroke-width="1.75"`,
the design's line weight; the paths are Tabler's. They are its `brand-*`
marks and, for languages with neither a mark nor a fitting symbol, its
`letter-*` monograms, plus `markdown`. They are compiled into `kiln-ui` with
`include_bytes!` and served beside Lucide's, so no name repeats across the
two sets. Tabler Icons is MIT, "Copyright (c) 2020-2026 Paweł Kuna"; its
`LICENSE` is copied beside them as
`kiln/crates/kiln-ui/assets/tabler-icons/LICENSE` and must ship with the
binary's notices. The logos among them (Rust, Go, Python, Docker, Git,
React, …) are their owners' marks, used only to identify file types; no
endorsement is implied.

The 142 SVGs of Material Icon Theme, drawn by `ColouredIcons`
(`kiln/crates/kiln-ui/src/file_icons_coloured.rs`), are copied unmodified
and compiled into `kiln-ui` with `include_bytes!`. They are a curated subset of
upstream's: an icon per highlighted language, per well-known file and per
well-known folder, open and closed, and the `_light` variants of those that
have one. The repository's `icons/` holds only the closed folders; the open
ones, `file.svg`, `folder.svg` and `folder-open.svg` are generated by its
build, so all are taken from the npm package published from the same commit
(its `gitHead`), whose other files are byte-identical to the repository's.

Material Icon Theme is MIT, "Copyright (c) 2025 Material Extensions", for
every file in it; its `LICENSE` is copied beside the icons as
`kiln/crates/kiln-ui/assets/file-icons/LICENSE` and must ship with
the binary's notices. Its README's "Icon sources" names Material Design
Icons (Pictogrammers, Apache-2.0) and Material Symbols (Google, Apache-2.0)
as where its designs come from; it distributes its own drawings under its
MIT licence, and none of the copied files carries other terms. A file whose
licence was unclear would have been left out, falling back to the default
icon.

Some icons are logos (Rust, Go, Python, Docker, Swift, Kotlin, Java, Git,
Node.js, …). Those marks belong to their owners and are used only to identify
file types; no endorsement is implied.

## Fonts

| Our path | Source | Commit | License | Files |
|---|---|---|---|---|
| `crates/fe-fonts/fonts/instrument-sans/` | [Instrument Sans](https://github.com/google/fonts/tree/86aced91de05cd4f96a06838124cca45be337b2b/ofl/instrumentsans) (google/fonts `ofl/instrumentsans/`), static instances of `InstrumentSans[wdth,wght].ttf` | `86aced91de05cd4f96a06838124cca45be337b2b` | OFL-1.1 | `InstrumentSans-Regular.ttf`, `InstrumentSans-Medium.ttf`, `InstrumentSans-SemiBold.ttf`, `OFL.txt` |
| `crates/fe-fonts/fonts/spline-sans-mono/` | [Spline Sans Mono](https://github.com/google/fonts/tree/86aced91de05cd4f96a06838124cca45be337b2b/ofl/splinesansmono) (google/fonts `ofl/splinesansmono/`), static instances of `SplineSansMono[wght].ttf` and `SplineSansMono-Italic[wght].ttf` | `86aced91de05cd4f96a06838124cca45be337b2b` | OFL-1.1 | `SplineSansMono-Regular.ttf`, `SplineSansMono-Medium.ttf`, `SplineSansMono-SemiBold.ttf`, `SplineSansMono-Italic.ttf`, `OFL.txt` |
| `crates/fe-fonts/fonts/instrument-serif/` | [Instrument Serif](https://github.com/google/fonts/tree/86aced91de05cd4f96a06838124cca45be337b2b/ofl/instrumentserif) (google/fonts `ofl/instrumentserif/`) | `86aced91de05cd4f96a06838124cca45be337b2b` | OFL-1.1 | `InstrumentSerif-Regular.ttf` (unmodified), `OFL.txt` |

Compiled into `fe-fonts` with `include_bytes!` and registered with the text
system by `fe_workbench::init`. Upstream ships Instrument Sans and Spline Sans
Mono as variable fonts, which GPUI's macOS text system registers at their
default weight only, so each weight we use is a static instance made with
fontTools' `instancer`; `crates/fe-fonts/fonts/README.md` has the commands and
the upstream files' SHA-256s. The SIL Open Font License 1.1 permits bundling
the fonts with software, including commercial software, and modified versions
under the same licence; none of the three declares a Reserved Font Name. Each
family's `OFL.txt`, copied unmodified, is beside its files, and
`scripts/bundle-macos.sh` copies it into Tenmoku.app as
`Contents/Resources/fonts/<family>/OFL.txt` (checked by
`scripts/test-bundle-macos.sh`).

## Shipped MPL-2.0 dependencies (unmodified)

MPL-2.0 requires telling recipients where to get the source of these crates.

| Crate | Version | Source | Pulled in by |
|---|---|---|---|
| dwrote | 0.11.5 | https://crates.io/crates/dwrote | zed-font-kit (Windows only) |
| option-ext | 0.2.0 | https://crates.io/crates/option-ext | dirs-sys via zed-font-kit |

`cbindgen` (MPL-2.0) is a build-time tool only and is not distributed.

## Shipped CDLA-Permissive-2.0 data

`webpki-roots` 1.0.11 (https://crates.io/crates/webpki-roots), pulled in by `ureq`
via `velopack`, is Mozilla's root certificate store as Rust data, under the
[Community Data License Agreement – Permissive 2.0](https://cdla.dev/permissive-2-0/).
It ships in the binary, so its licence text goes with the release's other licence
texts. `webpki-root-certs` (the same data, wasm32-only here) is not shipped.

## Binary frameworks in Tenmoku.app

Not committed and not a crate: `scripts/fetch-sparkle.sh` downloads the release, refuses
it unless its SHA-256 is the pinned one, and `scripts/bundle-macos.sh --sparkle` embeds
it. Its `LICENSE`, which also carries the licences of the code Sparkle includes, ships
beside it unmodified.

| Framework | Version | Source | SHA-256 | License | Licence text in the bundle |
|---|---|---|---|---|---|
| Sparkle | 2.10.1 | [sparkle-project/Sparkle](https://github.com/sparkle-project/Sparkle/releases/download/2.10.1/Sparkle-2.10.1.tar.xz) | `1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27` | MIT | `Contents/Resources/Sparkle-LICENSE.txt` |

We delete `Versions/B/XPCServices` from our copy (Tenmoku is not sandboxed) and re-sign the rest.

## Dependencies

Run `cargo deny list` for every crate and its license. Release builds must ship
the license texts. Generating `THIRD_PARTY_LICENSES` is a release-checklist item.

¹² Vendored in `vendor/tree-sitter-gitignore/` as a path dependency, because
crates.io has no upstream crate (only a third-party republish, which we don't
take). Upstream's `grammar.js` and generated `src/` are unchanged, at commit
`f4685bf11ac466dd278449bcfe5fd014e94aa504`; the Rust binding and the highlight
query are ours. The licence is upstream's. The vendored README says how to update it.

¹³ Vendored in `vendor/tree-sitter-git-commit/`, `-git-rebase/`, `-git-config/` and
`-gitattributes/` as path dependencies: crates.io has none of the first three, and
`tree-sitter-gitattributes` 0.1.6 there needs `tree-sitter` ~0.20. Upstream's
`grammar.js`, generated `src/` and query are unchanged, at commits
`5a50da19b3841ac51e9d483cd9c856a85232233d`,
`32686d6b72980b36f876ae2d07719c9c3ed154e2`,
`3a61756a81a86291a0f48e3eeeaa0692b9981aa9` and
`1b7af09d45b579f9f288453b95ad555f1f431645`; the Rust bindings are ours. The licences
are upstream's. Each vendored README says how to update it.

¹⁴ Vendored in `vendor/tree-sitter-just/` as a path dependency, because
`tree-sitter-just` 0.2.0 on crates.io depends on `tree-sitter` ~0.25.5, which can't
link beside our 0.27. Upstream's `grammar.js`, generated `src/` and queries are
unchanged, at commit `5685543a6e64f66335e25518c9ae8ffa1dae3d01`; the Rust binding is
ours. Upstream's `LICENSE` is Apache-2.0 although its `Cargo.toml` says MIT; we go by
the licence text and ship it. The vendored README says how to update it.

¹⁵ Vendored in `vendor/tree-sitter-json5/` as a path dependency, because
`tree-sitter-json5` 0.1.0 on crates.io needs `tree-sitter` ~0.20, which can't link
beside our 0.27. Upstream's `grammar.js`, `utils.js`, generated `src/` and query are
unchanged, at commit `248b8564567087d7866be76569b182f6dd7e14e9`; the Rust binding is
ours. The licence is upstream's. The vendored README says how to update it.

¹⁶ `crates/fe-grammar-csv` is our grammar, with nothing third-party in `grammar.js` or
the query. `crates/fe-grammar-csv/{csv,tsv}/src/` is what tree-sitter CLI 0.25
generates from it, as with every grammar here; its `tree_sitter/` headers come from
the CLI (MIT). We don't use `tree-sitter-csv` from crates.io.
