---
task_uid: "01m6xwq21d8hvmpky6gtp8a6bx"
task: "T1615"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests tests::tinted_project_icons::material_file_names_are_never_blank and tests::tinted_project_icons::folder_roles_have_their_own_glyph (cargo nextest run -p kiln-ui) pass"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-ui passes, including tests::icon_stroke::every_bundled_icon_strokes_at_1_75, tests::assets::bundled_files_match_the_enum and tests::tinted_icon_set; cargo nextest run -p fe-config --test icon_theme_setting passes (Hakuji's folders still one grey)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fastedit --test notice passes with the Lucide and Tabler rows' file lists updated"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (file_icons_dark.png, file_icons_light.png, setup_wizard_dark.png, setup_wizard_light.png; workbench.png does not move)"
    required: true
    manual: false
---

# Project files and folder roles in the tenmoku icon set: manifests, lock files, licences and changelogs, and a Lucide folder glyph per role, at Material's breadth

# Project files and folder roles in the tenmoku icon set

Second half of the user's "per file type icons" request (the first card gave
every language its own line glyph, Lucide plus Tabler marks, and its sources,
licence rows and `assets/tabler-icons/` directory; build on them). This card
covers the well-known project files and folders Material Icon Theme's set
(`workbench.icon_theme = "material"`, `file_icons_coloured.rs`) recognises, so
the default `"tenmoku"` set reaches roughly its breadth, still as 16 px line
icons at 1.75 stroke tinted by the `file_icon` tokens.

Spec: `docs/design/design-system.md#workbench-anatomy` (Icons; folders tint by
role, source, tests, docs, and go neutral grey in Hakuji).

## Files (`file_icons_tinted.rs` name tables)

Every name in `file_icons_coloured::FILE_NAMES` gets a glyph other than the
blank `file`. Proposed (the worker may swap for a closer glyph from Lucide
1.52.0 or Tabler v3.50.0, noting each swap in `docs/decisions/<lane>-*.md`):

| Names | Glyph | Token |
| --- | --- | --- |
| `Cargo.toml` | L `package` | `rust` |
| `go.mod`, `go.work`, `pom.xml`, `build.gradle(.kts)`, `gradle.properties`, `gradlew` | L `package` | `code` |
| `pyproject.toml`, `requirements.txt`, `Pipfile`, `Gemfile` | L `package` | `script` |
| `.python-version` | T `brand-python` | `script` |
| `Rakefile`, `Podfile`, `Brewfile`, `Guardfile`, `Capfile`, `.ruby-version` | L `gem` | `script` |
| `package.json`, `.nvmrc`, `.node-version` | T `brand-nodejs` | `web` |
| `.npmrc`, `.npmignore` | T `brand-npm` | `config` |
| `pnpm-workspace.yaml`, `.pnpmfile.cjs` | T `brand-pnpm` | `config` |
| `.yarnrc`, `.yarnrc.yml` | T `brand-yarn` | `config` |
| `bunfig.toml`, `.bun-version` | L `croissant` | `config` |
| `deno.json`, `deno.jsonc` | T `brand-deno` | `config` |
| `Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `bun.lock(b)`, `go.sum`, `go.work.sum`, `composer.lock` | L `file-lock` | `data` |
| `docker-compose.yml`, `compose.yaml` (and `.yml`/`.yaml` pairs), `.containerignore` | T `brand-docker` | `config` |
| `.gitkeep`, `.git-blame-ignore-revs`, `MERGE_MSG`, `TAG_EDITMSG` | T `brand-git` | `config` |
| `CODEOWNERS` | T `brand-github` | `config` |
| `.babelrc`, `.eslintrc`, `.swcrc` and Material's other JSON rc names | L `file-braces` | `data` |
| `.env*` | L `file-key` | `config` |
| `LICENSE`, `LICENCE`, `COPYING`, `NOTICE`, `COPYRIGHT` (and `.md`/`.txt`) | L `scale` | `default` |
| `CHANGELOG`, `CHANGES` (and `.md`/`.txt`) | L `file-clock` | `markdown` |

READMEs keep the Markdown glyph (T1517's CH8 ruling).

## Folders

Every name in `file_icons_coloured::FOLDER_NAMES` (Material's folder roles; looked up
as-is, then with leading dots and underscores and trailing underscores
trimmed, as `ColouredIcons::folder_path` does) gets a role glyph other than the plain
folder. Lucide folder glyphs only, so every folder has one shape:

| Role (Material's names) | Glyph | Token |
| --- | --- | --- |
| source: `src`, `source`, `code`, `lib`, `libs`, `crates`, `packages`, `pkg`, `app` | `folder-code` | `folder_source` |
| tests: `tests`, `spec`, `fixtures`, `mocks`, `testdata`, `snapshots`, `benches`, `bench` | `folder-check` | `folder_tests` |
| docs: `docs`, `doc`, `documentation`, `wiki` | `folder-pen` | `folder_docs` |
| config: `config`, `cfg`, `conf`, `settings`, `.vscode`, `.idea`, `.config` | `folder-cog` | `folder` |
| git: `.git`, `.githooks`, `patches`, `submodules`; `.github`, `.gitlab` | `folder-git`; `folder-git-2` | `folder` |
| build output: `target`, `dist`, `out`, `build`, `bin`, `release` | `folder-output` | `folder` |
| dependencies: `node_modules`, `vendor`, `third_party`, `external` | `folder-archive` | `folder` |
| examples: `examples`, `demo`, `samples` | `folder-kanban` | `folder` |
| scripts and tools: `scripts`, `xtask`, `tools` | `folder-input` | `folder` |
| assets: `assets`, `resources`, `static`, `public`, `images`, `icons`, `fonts`, `themes`, `styles`, `i18n`, `locales` | `folder-bookmark` | `folder` |
| temporary: `tmp`, `temp`, `cache` | `folder-clock` | `folder` |

A role folder draws its glyph open or closed (Lucide has no open forms; the
tree's chevron shows the state); a folder with no role keeps `folder` /
`folder-open`. Tints: source, tests and docs keep their tokens; the new roles
take `file_icon.folder`, so no new tokens and Hakuji's folders stay one grey.
Where T1517 mapped a name to a role (`lib`, `crates`, `packages`, `app` source;
`benches` tests), it keeps that role.

## Also

- `THIRD_PARTY.md` and `kiln/THIRD_PARTY.md`: the Lucide and Tabler rows' file
  lists gain the new files.
- `docs/design-language.md` `### file_icon`: the folder rows' "For" name the
  new roles that share `file_icon.folder`.
- `crates/fe-workbench/examples/screenshots/icons_scene.rs`: add a few of these
  (`Cargo.toml`, `package.json`, `LICENSE`, `tests/`, `target/`, `.github/`);
  update its doc comment.
- Regenerate on the Mac `file_icons_{dark,light}.png` and
  `setup_wizard_{dark,light}.png` and look at them. `workbench.png` must not
  move.

## Tests (new file)

`kiln/crates/kiln-ui/src/tests/tinted_project_icons.rs` (declared in
`tests/mod.rs`):

- `material_file_names_are_never_blank`: every name in
  `file_icons_coloured::FILE_NAMES` gets a tinted icon other than `Icon::File`,
  in the token the table above gives its row.
- `folder_roles_have_their_own_glyph`: every name in
  `file_icons_coloured::FOLDER_NAMES` gets a glyph other than `Icon::Folder` /
  `Icon::FolderOpen`, the same open and closed; no glyph serves two of the roles
  above; a name with
  no role is the plain folder, open or closed.

Update `tests/tinted_icon_set.rs` where its folder expectations change.

Platform: **Mac-only** (goldens).

## Implementation notes

- No glyph swaps; the names and folders outside the tables, the git row's two
  roles (`Git`, `Forge`) and the goldens are in
  `docs/decisions/y6gtp8a6bx-project-icons.md`.
- `setup_wizard_{dark,light}.png` were regenerated but are byte-identical: the
  wizard's preview draws no project tree. The icons scene's window grew to
  640 px tall to fit the added rows.
