---
task_uid: "01m48zxkgm1tgvr72pf8ehyg0n"
task: "T115"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-ui test tests::file_icons::names_map_by_name_then_extension: Cargo.toml→package, Cargo.lock→file-lock, main.rs and MAIN.RS→file-code, README.md→file-text, data.json→file-braces, archive.tar.gz→file-archive, .gitignore→git-branch (not an extension), Dockerfile→container, .env.local→file-key, LICENSE→scale, Makefile→file-cog, noext, 'trailing.' and '.hidden'→file"
    required: true
    manual: false
  - id: "C2"
    text: "fe-ui test tests::file_icons::folders_and_long_names: folder_icon(false)→folder, folder_icon(true)→folder-open; a 300-byte name ending .rs→file-code; a name of 64+ bytes equal (case-insensitively) to no table entry→file; every key in FILE_NAMES and EXTENSIONS is ASCII lowercase with no duplicate keys"
    required: true
    manual: false
  - id: "C3"
    text: "fe-ui test tests::assets::every_icon_is_bundled_and_parses: for each Icon::ALL (31 icons), Assets.load(icon.path()) is Some, and gpui's SvgRenderer::parse_svg accepts the bytes; Assets.load('fe-ui/icons/nope.svg') and load('icons/file.svg') are Ok(None); Assets.list('fe-ui/icons/') returns exactly the 26 paths"
    required: true
    manual: false
  - id: "C4"
    text: "fe-ui test tests::assets::bundled_files_match_the_enum: the .svg files in crates/fe-ui/assets/icons/ (read via CARGO_MANIFEST_DIR) are exactly Icon::ALL's names, each byte-identical to what Assets.load returns, and assets/icons/LICENSE exists and contains both 'ISC License' and 'Cole Bemis'"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fastedit/src/main.rs registers fe_ui::Assets with with_assets; Cargo.lock changes only by fe-ui in fastedit's dependency list; cargo deny check still passes; THIRD_PARTY.md has an Icons section naming lucide-icons/lucide 1.52.0 @500620a2e8123f8d1db191538886dc0c223f69a9, ISC with Feather-derived MIT, unmodified, listing all 31 files including files, search and list-tree; NOTICE has the Lucide (ISC) and Feather (MIT, Cole Bemis) copyright lines"
    required: true
    manual: false
  - id: "C6"
    text: "fe-ui test tests::file_icons::rail_icons_are_bundled_but_unmapped: Icon::Files, Icon::Search and Icon::ListTree have paths fe-ui/icons/files.svg, search.svg and list-tree.svg, Assets loads each one, and no FILE_NAMES or EXTENSIONS entry maps to them"
    required: true
    manual: false
  - id: "C7"
    text: "fe-ui test tests::file_icons::toolbar_icons_are_bundled_but_unmapped: Icon::ArrowLeft, ArrowRight, Columns2, Rows2 and Ellipsis have paths fe-ui/icons/{arrow-left,arrow-right,columns-2,rows-2,ellipsis}.svg, Assets loads each one, and no file-name or extension maps to them"
    required: true
    manual: false
---

# Bundle a Lucide icon subset in fe-ui and map file names to icons

# Bundle a Lucide icon subset in fe-ui and map file names to icons

Polish for the project tree, asked for by the user ("pretty up the file explorer: icons, bigger
triangles, a nice modern file explorer"). First of two cards. This one adds the icons and the
name-to-icon mapping, with no visible change. The tree-restyle card draws them.

It builds on:

- T63's fe-ui, which depends on GPUI alone
- GPUI's `svg()` element (gpui-pre 0.3.7). `svg().path(p)` loads `p` through the app's
  `AssetSource` only when the sprite atlas misses. It rasterises an **alpha mask**, keyed by path and
  size, and tints it with the element's text colour. So a monochrome SVG set tinted from `UiTheme`
  costs one parse and one raster per icon and size, never per frame, and works in light and dark
  themes.

## Problem

The tree shows names only, with no file-type or folder icons. GPUI's `svg()` needs an `AssetSource`,
and the app registers none (`gpui_platform::application()` is used bare in
`crates/fastedit/src/main.rs`).

## The icon set: Lucide

Use [Lucide](https://github.com/lucide-icons/lucide) at tag **1.52.0** (commit
`500620a2e8123f8d1db191538886dc0c223f69a9`).

- **Licence: ISC** ("Copyright (c) 2026 Lucide Icons and Contributors"). Icons derived from Feather
  are **MIT** ("Copyright (c) 2013-present Cole Bemis"). Of the icons below, those are
  `chevron-right`, `database` and `search`. Check each copied icon's `icons/<name>.json` at the commit, and record any other Feather-derived icon as MIT too. Lucide's `LICENSE` file carries both notices. ISC and MIT
  are both in `deny.toml`'s allow list.
- Lucide is chosen because it's monochrome, stroke-based and consistent on a 24×24 grid, so GPUI's
  alpha-mask tinting suits it. It has file-shaped variants (`file-code`, `file-braces`,
  `file-text`, …) and `folder`/`folder-open`.
- Not chosen:
  - **Material Icon Theme** (MIT): multicolour, and `svg()` would flatten it to a silhouette.
  - **Seti UI** (MIT): language logos, but a dated look.
  - **Codicons** (CC BY 4.0): attribution-only, but CC-BY-4.0 is not on our allow list.
  - **vscode-icons**: the code is MIT, but the icons are CC BY-SA, which is share-alike.
  - Never Zed's icon assets (clean room).

Copy each SVG **unmodified** from `icons/<name>.svg` at that commit into
`crates/fe-ui/assets/icons/<name>.svg`. Copy Lucide's `LICENSE` to
`crates/fe-ui/assets/icons/LICENSE`. Copy no other file.

## Change

### Assets: `crates/fe-ui/src/assets.rs` (new)

- `pub struct Assets;`, which implements `gpui::AssetSource`:
  - `load(path)` returns `Ok(Some(Cow::Borrowed(bytes)))` for a bundled icon, and `Ok(None)` for
    anything else.
  - `list(prefix)` returns the bundled paths starting with `prefix`.
- The bytes are `include_bytes!`, so nothing is read from disk at run time.
- Paths are namespaced `fe-ui/icons/<name>.svg`, so they can't collide with a host's own assets.
- The doc comment tells an embedding host that already has an `AssetSource` to delegate unknown
  paths to `fe_ui::Assets`, or the tree draws no icons.

### Icons and mapping: `crates/fe-ui/src/file_icons.rs` (new)

- `pub enum Icon` (Copy, Eq, Hash, Debug):
  - `const ALL: &[Icon]`
  - `fn path(self) -> &'static str` (`"fe-ui/icons/file-code.svg"`)
  - `fn name(self) -> &'static str` (`"file-code"`)
- `pub fn file_icon(name: &str) -> Icon` and `pub fn folder_icon(expanded: bool) -> Icon`.
- **Lookup order:**
  1. exact file name, ASCII case-insensitive
  2. a name starting with `.env` gives `file-key`
  3. the last two dot-separated extensions (`tar.gz`)
  4. the last extension
  5. `file`

  A leading dot doesn't start an extension: `.gitignore` is a name, not the extension `gitignore`.
- **The tables are Rust data, not code paths:** `const FILE_NAMES: &[(&str, Icon)]` and
  `const EXTENSIONS: &[(&str, Icon)]`, built once into `LazyLock<HashMap<&'static str, Icon>>`.
  - Each lookup is at most 4 hash probes, with no heap allocation. Lowercase into a stack buffer of
    64 bytes. A longer name skips the name lookup and uses only its extension.
  - A themeable TOML mapping is left for M5 themes, which would load over these defaults.
- **Bundled icons (26) and what maps to them:**
  - `chevron-right`: the disclosure chevron
  - `folder`, `folder-open`
  - `file`: the default
  - `file-code`: rs c h cc cpp hpp go py rb java kt kts scala swift php cs lua js mjs cjs jsx ts tsx
    html htm css scss xml vue svelte zig ex exs erl hs ml dart r jl
  - `file-braces`: json jsonc json5
  - `file-text`: md markdown txt rst adoc org
  - `file-cog`: toml yaml yml ini cfg conf properties, and the names `.editorconfig`, `Makefile`,
    `justfile` and `CMakeLists.txt`
  - `package`: `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `Gemfile`, `pom.xml`,
    `build.gradle`, `build.gradle.kts`
  - `file-lock`: lock, plus `package-lock.json`, `pnpm-lock.yaml` and `go.sum` by name
    (`Cargo.lock` and `yarn.lock` map by their extension)
  - `git-branch`: `.gitignore`, `.gitattributes`, `.gitmodules`, `.git-blame-ignore-revs`
  - `container`: `Dockerfile`, `Containerfile`, `.dockerignore`, and the extension dockerfile
  - `file-terminal`: sh bash zsh fish ps1 bat cmd
  - `file-key`: pem key crt p12, and `.env*`
  - `file-image`: png jpg jpeg gif webp bmp ico svg tiff avif
  - `file-archive`: zip tar gz tgz xz bz2 zst 7z rar, `tar.gz`, `tar.xz`
  - `file-spreadsheet`: csv tsv xlsx xls ods
  - `database`: sql sqlite db
  - `file-diff`: diff patch
  - `file-video-camera`: mp4 mov webm mkv avi
  - `file-music`: mp3 wav flac ogg m4a
  - `scale`: `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE`, `LICENCE`, `COPYING`, `NOTICE`
  - `file-symlink`: used by the tree for symlink entries, not mapped from names

  The worker may move a mapping between these icons if one reads badly at 16 px. They may not add
  icons beyond this list.
- **Activity-rail icons, not mapped from names:** `files` (Project), `search` (Search) and
  `list-tree` (Outline). The activity-rail card (T120) draws them. They're bundled here so the icon
  set and its licence records change in one card. They are `Icon::Files`, `Icon::Search` and
  `Icon::ListTree`.

### Wiring

- `crates/fe-ui/src/lib.rs`:
  - `mod` and `pub use` lines (`Assets`, `Icon`, `file_icon`, `folder_icon`)
  - `#[cfg(test)] mod tests;`
- `crates/fastedit/src/main.rs`: `gpui_platform::application().with_assets(fe_ui::Assets)`. Add
  `fe-ui.workspace = true` to `crates/fastedit/Cargo.toml` if the crate isn't re-exported. That
  file is shared and append-only. `Cargo.lock` gains only that one dependency line under
  `fastedit`.

### Licence records

- `THIRD_PARTY.md`: a new section, "Icons", with one row: our path `crates/fe-ui/assets/icons/`,
  the source `lucide-icons/lucide` `icons/`, the tag and commit above, `ISC (Feather-derived
  icons: MIT)`, and the 26 file names. Note that the files are unmodified, and that the `LICENSE`
  beside them must ship with the binary's notices.
- `NOTICE`: a Lucide entry with both copyright lines, ISC and MIT.

## Tests

New files `crates/fe-ui/src/tests/mod.rs`, `crates/fe-ui/src/tests/file_icons.rs` and
`crates/fe-ui/src/tests/assets.rs`. They're plain `#[test]`s with no GPUI app.

## Out of scope

- Drawing anything. `TreeRow` and `ProjectPanel` are the tree-restyle card's.
- Per-language colours.
- Special folder icons (`src`, `.git`, `node_modules`).
- A TOML icon theme.
- Brand logos (Simple Icons is CC0, but carries trademark caveats).

## Decided (lead defaults; the user may revisit)

- Lucide, monochrome, tinted from `UiTheme`, bundled with `include_bytes!` and served through an
  `AssetSource`. Not `svg().data()`, which hashes and copies the bytes on every frame.
- The mapping is a Rust table now, and a TOML icon theme with M5 themes.


## Added for the pane toolbar (T127), lead rev

Also bundle five more Lucide icons, unmodified, bringing the set to 31: arrow-left, arrow-right, columns-2, rows-2, ellipsis (Icon::ArrowLeft, ArrowRight, Columns2, Rows2, Ellipsis). Like the rail icons they are not mapped to file names. Check each icon's metadata JSON for Feather derivation (arrow-left, arrow-right, ellipsis and probably columns-2 are Feather, MIT) and credit THIRD_PARTY.md/NOTICE accordingly.
