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
| `crates/kiln-ui/assets/icons/` | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) `icons/` | 1.52.0 @ `500620a2e8123f8d1db191538886dc0c223f69a9` | ISC (Feather-derived icons: MIT) | `arrow-left.svg`, `arrow-right.svg`, `asterisk.svg`, `box.svg`, `braces.svg`, `check.svg`, `chevron-right.svg`, `circle-dot.svg`, `circle-x.svg`, `code.svg`, `columns-2.svg`, `component.svg`, `container.svg`, `copy.svg`, `database.svg`, `diamond.svg`, `ellipsis.svg`, `eye.svg`, `file-archive.svg`, `file-braces.svg`, `file-code.svg`, `file-cog.svg`, `file-diff.svg`, `file-image.svg`, `file-key.svg`, `file-lock.svg`, `file-music.svg`, `file-spreadsheet.svg`, `file-symlink.svg`, `file-terminal.svg`, `file-text.svg`, `file-video-camera.svg`, `file.svg`, `files.svg`, `folder-open.svg`, `folder.svg`, `git-branch.svg`, `git-graph.svg`, `hash.svg`, `heading.svg`, `link.svg`, `list-tree.svg`, `list.svg`, `lock.svg`, `message-square.svg`, `minus.svg`, `package.svg`, `panel-right-open.svg`, `pin.svg`, `plug.svg`, `rows-2.svg`, `scale.svg`, `search.svg`, `server.svg`, `square-function.svg`, `square.svg`, `table.svg`, `triangle-alert.svg`, `type.svg`, `variable.svg`, `x.svg` |
| `crates/kiln-ui/assets/file-icons/` | [material-extensions/vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme) `icons/`, as built into the npm package `material-icon-theme` 5.40.0 | v5.40.0 @ `045291ff033062a25441dd297fd51546326866f2` | MIT | `audio.svg`, `bun.svg`, `bun_light.svg`, `c.svg`, `certificate.svg`, `changelog.svg`, `cmake.svg`, `console.svg`, `cpp.svg`, `csharp.svg`, `css.svg`, `dart.svg`, `database.svg`, `diff.svg`, `docker.svg`, `document.svg`, `editorconfig.svg`, `elixir.svg`, `erlang.svg`, `exe.svg`, `file.svg`, `folder-benchmark-open.svg`, `folder-benchmark.svg`, `folder-config-open.svg`, `folder-config.svg`, `folder-css-open.svg`, `folder-css.svg`, `folder-dist-open.svg`, `folder-dist.svg`, `folder-docs-open.svg`, `folder-docs.svg`, `folder-examples-open.svg`, `folder-examples.svg`, `folder-font-open.svg`, `folder-font.svg`, `folder-git-open.svg`, `folder-git.svg`, `folder-github-open.svg`, `folder-github.svg`, `folder-i18n-open.svg`, `folder-i18n.svg`, `folder-images-open.svg`, `folder-images.svg`, `folder-include-open.svg`, `folder-include.svg`, `folder-lib-open.svg`, `folder-lib.svg`, `folder-mock-open.svg`, `folder-mock.svg`, `folder-node-open.svg`, `folder-node.svg`, `folder-open.svg`, `folder-packages-open.svg`, `folder-packages.svg`, `folder-public-open.svg`, `folder-public.svg`, `folder-resource-open.svg`, `folder-resource.svg`, `folder-rust-open.svg`, `folder-rust.svg`, `folder-scripts-open.svg`, `folder-scripts.svg`, `folder-src-open.svg`, `folder-src.svg`, `folder-target-open.svg`, `folder-target.svg`, `folder-temp-open.svg`, `folder-temp.svg`, `folder-test-open.svg`, `folder-test.svg`, `folder-theme-open.svg`, `folder-theme.svg`, `folder-tools-open.svg`, `folder-tools.svg`, `folder-utils-open.svg`, `folder-utils.svg`, `folder-vscode-open.svg`, `folder-vscode.svg`, `folder.svg`, `font.svg`, `gemfile.svg`, `git.svg`, `go-mod.svg`, `go.svg`, `gradle.svg`, `h.svg`, `haskell.svg`, `hpp.svg`, `html.svg`, `image.svg`, `java.svg`, `javascript.svg`, `json.svg`, `julia.svg`, `just.svg`, `key.svg`, `kotlin.svg`, `less.svg`, `license.svg`, `lock.svg`, `log.svg`, `lua.svg`, `makefile.svg`, `markdown.svg`, `maven.svg`, `nix.svg`, `nodejs.svg`, `npm.svg`, `ocaml.svg`, `pdf.svg`, `php.svg`, `pnpm.svg`, `pnpm_light.svg`, `powershell.svg`, `python-misc.svg`, `python.svg`, `r.svg`, `react.svg`, `react_ts.svg`, `readme.svg`, `ruby.svg`, `rust.svg`, `sass.svg`, `scala.svg`, `settings.svg`, `svelte.svg`, `svg.svg`, `swift.svg`, `table.svg`, `tex.svg`, `toml.svg`, `toml_light.svg`, `tune.svg`, `typescript-def.svg`, `typescript.svg`, `video.svg`, `vue.svg`, `xml.svg`, `yaml.svg`, `yarn.svg`, `zig.svg`, `zip.svg` |

The 60 SVGs are copied unmodified, compiled into `kiln-ui` with `include_bytes!`.
Lucide is ISC, "Copyright (c) 2026 Lucide Icons and Contributors". Icons derived
from Feather are MIT, "Copyright (c) 2013-present Cole Bemis". Of ours, these are
Feather-derived:

- named in Lucide's `LICENSE` Feather list: `arrow-left`, `arrow-right`,
  `check`, `chevron-right`, `code`, `database`, `hash`, `link`, `lock`,
  `minus`, `search`, `server`, `square`, `type`, `x`, `columns-2` (listed
  under its old name `columns`), `ellipsis` (listed as `more-horizontal`),
  `circle-x` (listed as `x-circle`) and `triangle-alert` (listed as
  `alert-triangle`);
- not in that list, but crediting Feather's author `colebemis` as a contributor
  in `icons/<name>.json` at the commit, so treated as MIT too: `box`, `copy`,
  `eye`, `file`, `file-text`, `folder`, `git-branch`, `list`, `message-square`,
  `package` and `table`.

The rest are ISC only. Lucide's `LICENSE`, which carries both notices, is
copied beside them as `crates/kiln-ui/assets/icons/LICENSE` and must ship with
an app's notices.

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
