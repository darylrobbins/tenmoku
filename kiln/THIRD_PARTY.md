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
| `crates/kiln-ui/assets/icons/` | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) `icons/` | 1.52.0 @ `500620a2e8123f8d1db191538886dc0c223f69a9` | ISC (Feather-derived icons: MIT) | `arrow-left.svg`, `arrow-right.svg`, `check.svg`, `chevron-right.svg`, `circle-x.svg`, `columns-2.svg`, `container.svg`, `copy.svg`, `database.svg`, `ellipsis.svg`, `file.svg`, `file-archive.svg`, `file-braces.svg`, `file-code.svg`, `file-cog.svg`, `file-diff.svg`, `file-image.svg`, `file-key.svg`, `file-lock.svg`, `file-music.svg`, `file-spreadsheet.svg`, `file-symlink.svg`, `file-terminal.svg`, `file-text.svg`, `file-video-camera.svg`, `files.svg`, `folder.svg`, `folder-open.svg`, `git-branch.svg`, `git-graph.svg`, `list-tree.svg`, `lock.svg`, `minus.svg`, `package.svg`, `rows-2.svg`, `scale.svg`, `search.svg`, `server.svg`, `square.svg`, `triangle-alert.svg`, `x.svg` |

The 40 SVGs are copied unmodified, compiled into `kiln-ui` with `include_bytes!`.
Lucide is ISC, "Copyright (c) 2026 Lucide Icons and Contributors". Icons derived
from Feather are MIT, "Copyright (c) 2013-present Cole Bemis". Of ours, these are
Feather-derived:

- named in Lucide's `LICENSE` Feather list: `arrow-left`, `arrow-right`,
  `check`, `chevron-right`, `database`, `lock`, `minus`, `search`, `server`,
  `square`, `x`, `columns-2` (listed under its old name `columns`), `ellipsis`
  (listed as `more-horizontal`), `circle-x` (listed as `x-circle`) and
  `triangle-alert` (listed as `alert-triangle`);
- not in that list, but crediting Feather's author `colebemis` as a contributor
  in `icons/<name>.json` at the commit, so treated as MIT too: `copy`, `file`,
  `file-text`, `folder`, `git-branch` and `package`.

The rest are ISC only. Lucide's `LICENSE`, which carries both notices, is
copied beside them as `crates/kiln-ui/assets/icons/LICENSE` and must ship with
an app's notices.

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
