---
task_uid: "01m6j5s6fz4r4hk8dgnbkyets6"
task: "T284"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "tests::coloured_icon_set::tables_point_at_bundled_files: every table entry names a bundled file and every file parses as SVG with a viewBox, under 8 KiB"
    required: true
    manual: false
  - id: "C2"
    text: "tests::coloured_icon_set::build_files_get_distinct_icons: Makefile, justfile, .editorconfig, Cargo.toml and a plain .toml all differ from each other and from the default"
    required: true
    manual: false
  - id: "C3"
    text: "tests::coloured_icon_set::every_syntax_language_has_an_icon: each fe-syntax language extension maps to a non-default icon"
    required: true
    manual: false
  - id: "C4"
    text: "tests::coloured_icon_set::folders_by_name_open_and_closed: src and node_modules differ from a plain folder and open differs from closed"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fastedit --test notice passes"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Bundle a curated Material Icon Theme subset (MIT) as ColouredIcons: per-language, per-filename and named open/closed folder icons, with THIRD_PARTY.md and NOTICE entries (Mac T1361)

## Problem

File icons are generic: Makefile, justfile, .editorconfig and every *.toml all get `file-cog`. User decision (2026-11-03): ship a coloured, per-language icon set as the default for files and folders and keep Lucide for UI chrome. Direction: Tenmoku is Apache-2.0, standalone and embeddable, curated JetBrains-lite, beautiful.

## Source

Material Icon Theme (PKief/vscode-material-icon-theme, MIT): per-language and per-filename icons, open/closed folder icons by name (src, test, docs, .git, node_modules, ...), mid-tone colours that read on both light and dark backgrounds. Take a curated subset of about 100-150 SVGs (not the whole set): one per language `fe-syntax` highlights (rust, go, python, js/ts/tsx/jsx, java, kotlin, c, cpp, csharp, scala, swift, php, ruby, sql, html, css/scss, json/json5/jsonc, yaml, toml, markdown, shell, lua, ...), plus Makefile, justfile, Dockerfile, .gitignore/.gitattributes, .editorconfig, lockfiles, package manifests (Cargo, package.json, go.mod, pyproject), README, LICENSE, images, fonts, archives, plain text and a default file; folders: default, src, test(s), docs, config, assets/images, scripts, .git, .github, node_modules, target/dist/build, vendor, each with an open variant. The implementer re-checks the licence of every copied file against the upstream README's attribution section before copying; brand logos (Docker, Go, Rust, Swift, Kotlin, etc.) may carry trademark terms: keep only the ones MIT covers and note in THIRD_PARTY.md that the marks belong to their owners and are used only to identify file types. A file whose licence is unclear is left out and falls back to a generic icon.

## Change

- Copy the chosen SVGs unmodified to `kiln/crates/kiln-ui/assets/file-icons/` with upstream `LICENSE` beside them; record source repo, tag/commit, licence and the file list in the Icons section of `THIRD_PARTY.md`; add the notice to `kiln/NOTICE` as Lucide's is (see `crates/fastedit/tests/notice.rs` for what is checked). Update `docs/design-language.md` Icons section (a second set, multi-colour, drawn as an image).
- New `kiln/crates/kiln-ui/src/file_icons_coloured.rs`: `ColouredIcons` implementing `IconSet`, answering `IconRef::Coloured` from tables of the same shape as `FILE_NAMES` / `EXTENSIONS` (exact name case-insensitive, `.env*` prefix, last two extensions, last extension, default) plus folder-name tables (`src`, `tests`, `docs`, `.git`, ...), with open and closed variants. `IconSet::folder(expanded)` takes no name, so extend the trait with a defaulted `folder_named(&self, name: &str, expanded: bool) -> IconRef` (default calls `folder`), and add `folder_icon_ref_named`; the project tree passes the folder's name (`crates/fe-workbench/src/project_panel.rs`, the two `folder_icon_ref` sites).
- Register the files with `Assets` (card A's table) via `include_bytes!`.

Not in this card: choosing the set in settings (card D); `ColouredIcons` is not installed by default yet.

## Tests

New `kiln/crates/kiln-ui/src/tests/coloured_icon_set.rs`: every table entry points at a bundled file; every bundled file is parseable SVG, 'viewBox' present, under 8 KiB; Makefile, justfile, .editorconfig, Cargo.toml and plain .toml get distinct icons from each other and from the default; every `fe-syntax` language extension maps to a non-default icon; 'src' and 'node_modules' differ from a plain folder; open differs from closed for each folder icon; lookup is case-insensitive. New `crates/fastedit/tests/` check is not needed: `notice.rs` already covers NOTICE.

## Light and dark

Material ships *_light.svg variants for icons that vanish on a light background. Copy those too; `ColouredIcons` picks the light variant when the active UI theme is light (read the appearance when answering, redraw on theme change). All other icons are fixed-colour mid-tones.

## Implementation notes

- Source: Material Icon Theme v5.40.0 (`045291ff`). The repository's `icons/` has only
  closed folders; open folders and the default `file`/`folder`/`folder-open` are made by
  its build, so the 142 SVGs come from the npm package `material-icon-theme` 5.40.0,
  published from that commit, whose other files match the repository's byte for byte.
  The README's attribution ("Icon sources": Material Design Icons, Material Symbols,
  both Apache-2.0) is design provenance; upstream ships every file under its MIT
  `LICENSE`, and no SVG carries other terms. None was left out for its licence.
- Upstream has no Cargo icon: `Cargo.toml` gets Rust's, `Cargo.lock` the lock.
- Tables map names to file stems; a folder stem's open icon is `<stem>-open`, and a
  stem with a `<stem>_light` file (here `toml`, `pnpm`, `bun`) draws it under a light
  theme. The appearance is a per-thread cell in `theme.rs`, set wherever the theme is
  (`activate`, `set_ui_theme`); `activate` already redraws every window.
- `Assets` serves the files from a static table (not `register_coloured`), so they
  draw under any host whose source delegates to it, with no registration step.
- `every_syntax_language_has_an_icon` copies fe-syntax's extension lists: kiln can't
  depend on an app crate.
- Carried T282 F3: a registration change now drops the stale images' atlas tiles from
  every window (`App::drop_image`, the drawing window included) and refreshes windows;
  `tests::coloured_icons::registering_again_drops_the_old_tile` uses a keyed atlas.
- Outside the listed scope, needed for C5 and the notice rule: root `NOTICE`,
  `kiln/THIRD_PARTY.md` (both copies of the rows), `crates/fastedit/tests/notice.rs`
  (reads the new `LICENSE`), `theme.rs` (appearance), `tests/coloured_icons.rs`.
