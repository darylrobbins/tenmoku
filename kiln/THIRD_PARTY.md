# Third-party code in kiln

Paths are relative to this directory. While kiln lives inside a host
repository, that repository's own `THIRD_PARTY.md` carries these rows too;
change both together.

## Code copied or adapted

| Our path | Source | Commit | License |
|---|---|---|---|
| `crates/kiln-ui/src/text_field.rs` | [gpui-pre](https://crates.io/crates/gpui-pre) 0.3.7 `examples/input.rs` (Zed's Apache-2.0 GPUI) | zed `1a28cff` | Apache-2.0 |

kiln's `TextField` adapts the text-input example that ships with GPUI
(Copyright Zed Industries, Apache-2.0): its line shaping, selection and caret
quads, IME marked-range underline and input-handler methods. The file's
header names the source and licence and its module docs say what was changed.
gpui-pre ships no `NOTICE` file (see [`NOTICE`](NOTICE)). Ship the Apache-2.0
license text with it.

Nothing else in kiln is copied or adapted: the rest is written for kiln
(see [`CLEAN_ROOM.md`](CLEAN_ROOM.md)).

## Icons

| Our path | Source | Tag / commit | License | Files |
|---|---|---|---|---|
| `crates/kiln-ui/assets/icons/` | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) `icons/` | 1.52.0 @ `500620a2e8123f8d1db191538886dc0c223f69a9` | ISC (Feather-derived icons: MIT) | `app-window.svg`, `arrow-left.svg`, `arrow-right.svg`, `asterisk.svg`, `bell.svg`, `bot.svg`, `box.svg`, `braces.svg`, `bug.svg`, `case-sensitive.svg`, `check.svg`, `chevron-down.svg`, `chevron-right.svg`, `chevron-up.svg`, `circle-check.svg`, `circle-dot.svg`, `circle-x.svg`, `cloud.svg`, `code-xml.svg`, `code.svg`, `coffee.svg`, `columns-2.svg`, `component.svg`, `container.svg`, `copy.svg`, `database.svg`, `diamond.svg`, `droplet.svg`, `ellipsis.svg`, `eye.svg`, `file-archive.svg`, `file-box.svg`, `file-braces.svg`, `file-code.svg`, `file-cog.svg`, `file-diff.svg`, `file-image.svg`, `file-key.svg`, `file-lock.svg`, `file-music.svg`, `file-sliders.svg`, `file-spreadsheet.svg`, `file-symlink.svg`, `file-terminal.svg`, `file-text.svg`, `file-type.svg`, `file-video-camera.svg`, `file.svg`, `files.svg`, `flask-conical.svg`, `fold-vertical.svg`, `folder-open.svg`, `folder.svg`, `gem.svg`, `git-branch.svg`, `git-commit-vertical.svg`, `git-graph.svg`, `hammer.svg`, `hash.svg`, `heading.svg`, `lambda.svg`, `layers.svg`, `link.svg`, `list-checks.svg`, `list-tree.svg`, `list-video.svg`, `list-x.svg`, `list.svg`, `loader-circle.svg`, `lock.svg`, `log-out.svg`, `message-square.svg`, `minus.svg`, `moon.svg`, `package.svg`, `panel-right-open.svg`, `panels-top-left.svg`, `parentheses.svg`, `pin.svg`, `play.svg`, `plug.svg`, `redo.svg`, `refresh-cw.svg`, `regex.svg`, `rotate-ccw.svg`, `rows-2.svg`, `save.svg`, `scale.svg`, `scroll-text.svg`, `search.svg`, `server.svg`, `settings.svg`, `sigma.svg`, `sliders-horizontal.svg`, `snowflake.svg`, `sparkle.svg`, `sprout.svg`, `square-dashed-text.svg`, `square-function.svg`, `square-minus.svg`, `square-plus.svg`, `square-terminal.svg`, `square.svg`, `star.svg`, `table.svg`, `tag.svg`, `target.svg`, `terminal.svg`, `text-wrap.svg`, `toggle-right.svg`, `tree-deciduous.svg`, `triangle-alert.svg`, `triangle.svg`, `type.svg`, `undo.svg`, `unfold-vertical.svg`, `variable.svg`, `whole-word.svg`, `x.svg`, `zap.svg` |
| `crates/kiln-ui/assets/tabler-icons/` | [tabler/tabler-icons](https://github.com/tabler/tabler-icons) `icons/outline/` | v3.50.0 @ `6904efd245391ad396df17654738f52f6da8c40e` | MIT | `brand-astro.svg`, `brand-c-sharp.svg`, `brand-cpp.svg`, `brand-css3.svg`, `brand-docker.svg`, `brand-git.svg`, `brand-golang.svg`, `brand-html5.svg`, `brand-javascript.svg`, `brand-kotlin.svg`, `brand-php.svg`, `brand-powershell.svg`, `brand-python.svg`, `brand-react.svg`, `brand-rust.svg`, `brand-sass.svg`, `brand-svelte.svg`, `brand-swift.svg`, `brand-terraform.svg`, `brand-typescript.svg`, `brand-vue.svg`, `letter-c.svg`, `letter-e.svg`, `letter-f.svg`, `letter-g.svg`, `letter-j.svg`, `letter-k.svg`, `letter-o.svg`, `letter-p.svg`, `letter-r.svg`, `letter-v.svg`, `markdown.svg` |
| `crates/kiln-ui/assets/file-icons/` | [material-extensions/vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) `icons/`, as built into the npm package `material-icon-theme` 5.40.0 | v5.40.0 @ `045291ff033062a25441dd297fd51546326866f2` | MIT | `audio.svg`, `bun.svg`, `bun_light.svg`, `c.svg`, `certificate.svg`, `changelog.svg`, `cmake.svg`, `console.svg`, `cpp.svg`, `csharp.svg`, `css.svg`, `dart.svg`, `database.svg`, `diff.svg`, `docker.svg`, `document.svg`, `editorconfig.svg`, `elixir.svg`, `erlang.svg`, `exe.svg`, `file.svg`, `folder-benchmark-open.svg`, `folder-benchmark.svg`, `folder-config-open.svg`, `folder-config.svg`, `folder-css-open.svg`, `folder-css.svg`, `folder-dist-open.svg`, `folder-dist.svg`, `folder-docs-open.svg`, `folder-docs.svg`, `folder-examples-open.svg`, `folder-examples.svg`, `folder-font-open.svg`, `folder-font.svg`, `folder-git-open.svg`, `folder-git.svg`, `folder-github-open.svg`, `folder-github.svg`, `folder-i18n-open.svg`, `folder-i18n.svg`, `folder-images-open.svg`, `folder-images.svg`, `folder-include-open.svg`, `folder-include.svg`, `folder-lib-open.svg`, `folder-lib.svg`, `folder-mock-open.svg`, `folder-mock.svg`, `folder-node-open.svg`, `folder-node.svg`, `folder-open.svg`, `folder-packages-open.svg`, `folder-packages.svg`, `folder-public-open.svg`, `folder-public.svg`, `folder-resource-open.svg`, `folder-resource.svg`, `folder-rust-open.svg`, `folder-rust.svg`, `folder-scripts-open.svg`, `folder-scripts.svg`, `folder-src-open.svg`, `folder-src.svg`, `folder-target-open.svg`, `folder-target.svg`, `folder-temp-open.svg`, `folder-temp.svg`, `folder-test-open.svg`, `folder-test.svg`, `folder-theme-open.svg`, `folder-theme.svg`, `folder-tools-open.svg`, `folder-tools.svg`, `folder-utils-open.svg`, `folder-utils.svg`, `folder-vscode-open.svg`, `folder-vscode.svg`, `folder.svg`, `font.svg`, `gemfile.svg`, `git.svg`, `go-mod.svg`, `go.svg`, `gradle.svg`, `h.svg`, `haskell.svg`, `hpp.svg`, `html.svg`, `image.svg`, `java.svg`, `javascript.svg`, `json.svg`, `julia.svg`, `just.svg`, `key.svg`, `kotlin.svg`, `less.svg`, `license.svg`, `lock.svg`, `log.svg`, `lua.svg`, `makefile.svg`, `markdown.svg`, `maven.svg`, `nix.svg`, `nodejs.svg`, `npm.svg`, `ocaml.svg`, `pdf.svg`, `php.svg`, `pnpm.svg`, `pnpm_light.svg`, `powershell.svg`, `python-misc.svg`, `python.svg`, `r.svg`, `react.svg`, `react_ts.svg`, `readme.svg`, `ruby.svg`, `rust.svg`, `sass.svg`, `scala.svg`, `settings.svg`, `svelte.svg`, `svg.svg`, `swift.svg`, `table.svg`, `tex.svg`, `toml.svg`, `toml_light.svg`, `tune.svg`, `typescript-def.svg`, `typescript.svg`, `video.svg`, `vue.svg`, `xml.svg`, `yaml.svg`, `yarn.svg`, `zig.svg`, `zip.svg` |

The 120 SVGs are copied with one modification, which ISC permits: each root
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

The rest are ISC only. Lucide's `LICENSE`, which carries both notices, is
copied beside them as `crates/kiln-ui/assets/icons/LICENSE` and must ship with
an app's notices.

The 32 SVGs of Tabler Icons, `TintedIcons`' marks for the languages Lucide
has none for (`crates/kiln-ui/src/file_icons_tinted.rs`), are copied
from `icons/outline/` with two modifications, which MIT permits: each file's
leading metadata comment (tags, category, version, unicode) is removed, and
the root `<svg>`'s `stroke-width="2"` is changed to `stroke-width="1.75"`,
the design's line weight; the paths are Tabler's. They are its `brand-*`
marks and, for languages with neither a mark nor a fitting symbol, its
`letter-*` monograms, plus `markdown`. They are compiled into `kiln-ui` with
`include_bytes!` and served beside Lucide's, so no name repeats across the
two sets. Tabler Icons is MIT, "Copyright (c) 2020-2026 Paweł Kuna"; its
`LICENSE` is copied beside them as
`crates/kiln-ui/assets/tabler-icons/LICENSE` and must ship with the
binary's notices. The logos among them (Rust, Go, Python, Docker, Git,
React, …) are their owners' marks, used only to identify file types; no
endorsement is implied.

The 142 SVGs of Material Icon Theme, drawn by `ColouredIcons`
(`crates/kiln-ui/src/file_icons_coloured.rs`), are copied unmodified
and compiled into `kiln-ui` with `include_bytes!`. They are a curated subset of
upstream's: an icon per highlighted language, per well-known file and per
well-known folder, open and closed, and the `_light` variants of those that
have one. The repository's `icons/` holds only the closed folders; the open
ones, `file.svg`, `folder.svg` and `folder-open.svg` are generated by its
build, so all are taken from the npm package published from the same commit
(its `gitHead`), whose other files are byte-identical to the repository's.

Material Icon Theme is MIT, "Copyright (c) 2025 Material Extensions", for
every file in it; its `LICENSE` is copied beside the icons as
`crates/kiln-ui/assets/file-icons/LICENSE` and must ship with
an app's notices. Its README's "Icon sources" names Material Design
Icons (Pictogrammers, Apache-2.0) and Material Symbols (Google, Apache-2.0)
as where its designs come from; it distributes its own drawings under its
MIT licence, and none of the copied files carries other terms. A file whose
licence was unclear would have been left out, falling back to the default
icon.

Some icons are logos (Rust, Go, Python, Docker, Swift, Kotlin, Java, Git,
Node.js, …). Those marks belong to their owners and are used only to identify
file types; no endorsement is implied.

## Dependencies

Run `cargo deny list` for every crate and its license; `deny.toml` holds the
policy. An app built on kiln ships its dependencies' licence texts with its
own. These MPL-2.0 crates are in kiln's dependency graph, unmodified, through
GPUI; MPL-2.0 requires telling recipients where to get their source:

| Crate | Version | Source | Pulled in by |
|---|---|---|---|
| dwrote | 0.11.5 | https://crates.io/crates/dwrote | zed-font-kit (Windows only) |
| option-ext | 0.2.0 | https://crates.io/crates/option-ext | dirs-sys via zed-font-kit |

`cbindgen` (MPL-2.0) is a build-time tool only (gpui-pre-apple's build
script) and is not distributed.
