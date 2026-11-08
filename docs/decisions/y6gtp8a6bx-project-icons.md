# T1615: project files and folder roles in the tenmoku set

Decisions taken while implementing
`docs/specs/y6gtp8a6bx-project-files-and-folder-roles-in-the.md`.

## The spec's glyphs, unchanged

Every row takes the glyph and token the spec proposes; no swaps. The 14 new
Lucide files and 6 Tabler files are the ones the spec's scope lists, from the
same pinned tags as T1614 (Lucide 1.52.0 @ `500620a`, Tabler v3.50.0 @
`6904efd`), edited the same way (stroke `2` → `1.75`; Tabler's leading
metadata comment removed). Of the Lucide files, `folder-bookmark` and
`folder-code` credit `colebemis` in `icons/<name>.json`, so THIRD_PARTY.md
treats them as MIT; none is in Lucide's Feather list.

## Files the spec's table doesn't name

Material's `FILE_NAMES` has a few names outside the spec's rows. Each takes
the glyph of the tool or format it belongs to, in that family's token:

- `.ecrc` (editorconfig-checker) `sliders-horizontal`, `config`, as
  `.editorconfig`; `Kbuild` `hammer`, `config`, as Make;
  `CMakeCache.txt` and `CMakePresets.json` `triangle`, `config`, as CMake.
- `gradlew.bat` `package`, `code`, with `gradlew`.
- `py.typed` `brand-python`, `script`, with `.python-version`;
  `pylintrc`, `.pylintrc` and `.coveragerc` are INI files, so
  `sliders-horizontal` in `config`.
- `PKGBUILD` and `APKBUILD` are shell scripts: `file-terminal`, `script`.
- `.yarnrc.yaml` with `.yarnrc.yml`; `COPYRIGHT.md`/`.txt` with the
  licences, and `CHANGES.txt` with the changelogs, as the spec's "(and
  `.md`/`.txt`)" says.
- `composer.lock` is a lock file (`file-lock`, `data`), as the spec's lock
  row says, though Material draws it as JSON.
- `CODEOWNERS`, `deno.json(c)` are not in Material's table; they are in
  ours as the spec's rows give them.

Names Lucide's tables already map (`Cargo.toml`, `go.mod`, `pom.xml`,
`build.gradle(.kts)`, `pyproject.toml`, `Gemfile` to `package`; `LICENSE`,
`COPYING`, `NOTICE` to `scale`; the `lock` extension and
`package-lock.json`, `pnpm-lock.yaml`, `go.sum` to `file-lock`) keep
Lucide's glyph and only gain a token in `FILE_NAMES` where their extension's
was wrong (`Cargo.toml` `rust` not `toml`, `LICENSE.md` `default` not
`markdown`). `Gemfile`, `go.mod`, `pom.xml` and `build.gradle(.kts)` move from
`config` to their language's token, as the spec's rows give them.

## Folder roles

`FolderRole` is a closed enum, each role one Lucide glyph and one token;
`FOLDER_NAMES` maps names to roles. The spec's git row has two glyphs
(`folder-git` for Git's own folders, `folder-git-2` for `.github`/`.gitlab`),
so it is two roles, `Git` and `Forge`; no glyph serves two roles.

Lookup follows `ColouredIcons::folder_path`: as-is, then with leading `.`/`_`
and trailing `_` trimmed, so `.github`, `.vscode`, `.idea`, `.config`,
`.cargo` and `__tests__` need no entries of their own (`__tests__` was one
before and is gone).

Material's folder names outside the spec's rows follow the Material icon
they share, mapped to the spec's role for it:

- source: `srcs`, `sources`, `library`, `package`, `pkgs`, `include(s)`,
  `inc` (headers are sources), `util(s)`, `utility`, `utilities`, `rust`;
- tests: `test`, `testing`, `specs`, `fixture`, `mock`,
  `benchmark(s)`, `perf` (Material's benchmark icon, which T1517 put with
  tests through `benches`);
- config: `configs`, `configuration`, `options`, `prefs`, `meta-inf`, and
  `cargo` (`.cargo` holds Cargo's config; Material draws it as Rust);
- output: `output`, `builds`, `built`, `compiled`;
- dependencies: `node`, `nodejs`, `vendors`, `third-party`, `thirdparty`;
- examples: `example`, `demos`, `sample`; scripts and tools: `script`,
  `tooling`, `devtools`;
- assets: Material's images (`image`, `img`, `icon`, `figures`,
  `screenshots`, `pictures`, …), resources (`asset`, `resource`, `res`),
  public (`www`, `web`), fonts, CSS (`css`, `style`, `stylesheets`), themes
  (`theme`, `colors`, `colours`) and i18n (`l10n`, `locale`, `lang`,
  `translations`) folders.

## Goldens

`icons_scene.rs` adds `Cargo.toml`, `package.json`, `LICENSE`, and `tests/`,
`target/`, `.github/` (closed, each with a file). The window grows from
480 to 640 px tall so the tree's last rows (`package.json`, `README.md`)
aren't cut off. `UPDATE_SCREENSHOTS=1` rewrote the four claimed goldens, but
only `file_icons_{dark,light}.png` changed: the setup wizard's preview shows
no project tree and no project file, so `setup_wizard_*.png` are
byte-identical, as is `workbench.png`.
