---
task_uid: "01m6xwq0zsv2b42xrtq0syrybt"
task: "T1614"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests tests::tinted_language_icons::every_language_has_its_own_icon and tests::tinted_language_icons::material_extensions_are_never_blank (cargo nextest run -p kiln-ui) pass"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-ui passes, including tests::icon_stroke::every_bundled_icon_strokes_at_1_75, tests::assets::bundled_files_match_the_enum and tests::assets::every_icon_is_bundled_and_parses over both icon directories, and tests::tinted_icon_set::tables_name_file_icon_tokens with file_icon.code; no existing icon is renamed (cargo nextest run -p fe-gpui which_key_icons passes)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fastedit --test notice passes with the Tabler Icons rows, its LICENSE and the updated Lucide file lists"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-config passes (icon_theme_setting, design_language_doc with the file_icon.code row) and cargo nextest run -p fe-workbench icon_theme_install passes"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (file_icons_dark.png, file_icons_light.png, setup_wizard_dark.png, setup_wizard_light.png; workbench.png does not move)"
    required: true
    manual: false
---

# A line icon per language in the tenmoku icon set: Lucide plus Tabler's marks (MIT), tinted by family, a new file_icon.code tint

# A line icon per language in the tenmoku icon set

User request: "Why did we replace the nice per file type icons with the older
more limited set?" T1517 made `workbench.icon_theme = "tenmoku"` (the default)
Lucide's line icons tinted by file family, in place of Material Icon Theme's
coloured set (still there as `"material"`). Lucide has no language marks, so
every language now draws the same `file-code`, told apart only by tint, and Go,
Java, C and most others take the default grey. Chosen: keep the line style and
the `file_icon` tints, and give each language Tenmoku highlights a glyph of its
own, near Material's breadth. This card does the languages; the next one
(depends on this) does project files and folder roles.

Spec: `docs/design/design-system.md#workbench-anatomy` (Icons: 16 px line icons
on a 24 grid, 1.7–1.8 stroke; file-type icons sit in the palette).

## Icon sources

- Lucide 1.52.0 (same tag as T1516, ISC) for every glyph it has.
- **Tabler Icons v3.50.0** (`tabler/tabler-icons` @ `6904efd245391ad396df17654738f52f6da8c40e`,
  `icons/outline/`, MIT, "Copyright (c) 2020-2026 Paweł Kuna") for language
  marks Lucide lacks: its `brand-*` marks and, for languages with neither a mark
  nor a fitting symbol, its `letter-*` monograms. Same 24 grid, round caps and
  joins as Lucide. Strip each file's leading metadata comment and set
  `stroke-width="1.75"`; paths untouched. No hand-drawn icons.
- Tabler's SVGs go in their own directory,
  `kiln/crates/kiln-ui/assets/tabler-icons/`, with upstream's `LICENSE` beside
  them (the `notice` test looks for each Icons row's `LICENSE` in the row's
  directory). They're still `Icon` variants served at `kiln-ui/icons/<name>.svg`:
  give the `icons!` macro (or a second list) the source directory, and make
  `tests::assets` (`every_icon_is_bundled_and_parses`,
  `bundled_files_match_the_enum`) cover both directories.
  `tests::icon_stroke::every_bundled_icon_strokes_at_1_75` already walks
  `Icon::ALL`. Icon names are user-visible (which-key `icon=`): rename nothing,
  and no name may repeat across the two sets.

## Mapping (`file_icons_tinted.rs`)

`TintedIcons` gets its own icon tables (extensions and language-selecting file
names); `LucideIcons` (`"minimal"`) and its tables do not change. Proposed
glyphs below; the worker may swap one for a closer glyph from the two pinned
sets, noting each swap in `docs/decisions/<lane>-*.md`. Dialects of one
language share a glyph (js/mjs/cjs, c/h, cpp/hpp, sh/bash/zsh, ini/cfg/conf/properties,
json/jsonc/json5, Dockerfile/Containerfile, the git files).

| Family token | Language: glyph (L Lucide, T Tabler) |
| --- | --- |
| `rust` | Rust: T `brand-rust` |
| `code` (new) | C: T `letter-c`; C++: T `brand-cpp`; C#: T `brand-c-sharp`; F#: T `letter-f`; Go: T `brand-golang`; Java: L `coffee`; Kotlin: T `brand-kotlin`; Scala: L `layers`; Groovy (`.groovy`, `.gradle`): T `letter-g`; Swift: T `brand-swift`; Dart: L `target`; Zig: L `zap`; Haskell: L `lambda`; OCaml: T `letter-o`; Elm: L `tree-deciduous`; Gleam: L `sparkle`; Erlang: T `letter-e`; Elixir: L `droplet`; Racket: L `parentheses`; Solidity: L `diamond` |
| `script` | Python: T `brand-python`; Ruby (and `.erb`, `.rake`, `.gemspec`): L `gem`; Lua: L `moon`; Perl: T `letter-p`; PHP: T `brand-php`; R: T `letter-r`; Julia: T `letter-j`; shells (incl. `.bashrc`, `.zshrc`, `.profile`, `.envrc`): L `file-terminal`; PowerShell: T `brand-powershell`; Vim script (`.vim`, `.vimrc`): T `letter-v` |
| `web` | JavaScript: T `brand-javascript`; TypeScript (and `.d.ts`): T `brand-typescript`; JSX/TSX: T `brand-react`; HTML: T `brand-html5`; CSS: T `brand-css3`; SCSS/Sass: T `brand-sass`; Vue: T `brand-vue`; Svelte: T `brand-svelte`; Astro: T `brand-astro`; Twig: L `sprout` |
| `markdown` | Markdown (and READMEs, per T1517's CH8 ruling): T `markdown`; LaTeX: L `sigma` |
| `toml` | TOML: L `file-cog` |
| `config` | YAML: L `file-sliders`; INI/properties/`.editorconfig`: L `sliders-horizontal`; Dockerfile: T `brand-docker`; Make (`Makefile`, `GNUmakefile`, `.mk`): L `hammer`; just: L `list-checks`; CMake: L `triangle`; Nginx (`nginx.conf`): L `server`; Nix: L `snowflake`; HCL/Terraform: T `brand-terraform`; Starlark (`.bzl`, `.star`, `BUILD.bazel`): L `star`; KDL: T `letter-k`; Git (`.gitignore`, `.gitattributes`, `.gitmodules`, `.gitconfig`, `COMMIT_EDITMSG`, `git-rebase-todo`): T `brand-git` |
| `data` | JSON: L `file-braces`; XML (`.xml`, `.xsd`, `.xsl`, `.plist`): L `code-xml`; CSV/TSV: L `file-spreadsheet`; SQL: L `database`; Protocol Buffers: L `file-box`; diff/patch: L `file-diff` |

Every other extension ColouredIcons' `EXTENSIONS` maps (fonts, PDF, logs,
archives, keys, media, `.ipynb`, `.ron`, …) takes a non-blank glyph from its
family's Lucide kind icons (`file-type`, `file-text`, `file-archive`,
`file-key`, `file-image`, `file-braces`, …), never the blank `file`.

New token `file_icon.code` (`Rule::Same("list.icon")` like the rest), set to
jun in Yohen `#c7a5e6` and Hakuji `#7b36a8` (the keyword glaze, which no
`file_icon` token uses yet). Go moves from `default` to `code`.

## Also

- `THIRD_PARTY.md` and `kiln/THIRD_PARTY.md`: the Lucide Icons row's file list
  gains the new Lucide files; a new Icons row for Tabler (path, tag and commit,
  MIT, file list) and a paragraph like Lucide's saying what was modified.
- `NOTICE` and `kiln/NOTICE`: a Tabler Icons entry (copyright, MIT, "stroke
  width set to 1.75; the logos among them are their owners' marks, used only to
  identify file types"), and Lucide's entry no longer says "unmodified" (T1516
  changed the stroke); the licence-texts line names Tabler's LICENSE.
  `crates/fastedit/tests/notice.rs` reads the new LICENSE (as `MATERIAL_TEXT`).
- `docs/design-language.md` `### file_icon`: a `file_icon.code` row; the
  `script`, `web`, `config` and `data` rows' "For" list the languages above.
- `docs/design/design-system.md` Icons paragraph: say each language has its own
  glyph (Lucide, with Tabler's marks where Lucide has none), tinted by family.
- `crates/fe-workbench/src/tests/icon_theme_install.rs` assumes the tenmoku set
  draws `main.rs` with Lucide's `file-code`: compare against
  `TintedIcons.file("main.rs")` instead, keeping what it checks.
- `crates/fe-workbench/examples/screenshots/icons_scene.rs`: add files that show
  the range (e.g. `main.go`, `app.ts`, `main.py`, `Dockerfile`, `.gitignore`,
  `index.html`), taller window if needed; update its doc comment.
- Regenerate on the Mac `file_icons_{dark,light}.png` and
  `setup_wizard_{dark,light}.png` (`UPDATE_SCREENSHOTS=1 cargo run -p
  fe-workbench --example screenshots`) and look at them. `workbench.png` must not
  move.

## Tests (new file)

`kiln/crates/kiln-ui/src/tests/tinted_language_icons.rs` (declared in
`tests/mod.rs`), with a table of (language, sample file names) covering every
row above:

- `every_language_has_its_own_icon`: samples of one language share an icon;
  icons of different languages differ; none is `Icon::File` or `Icon::FileCode`;
  each takes its family token above.
- `material_extensions_are_never_blank`: every extension in
  `file_icons_coloured::EXTENSIONS` gets a tinted icon other than `Icon::File`.

Update `tests/tinted_icon_set.rs` where its expectations change (`main.go` is
`code`, `.rs` no longer Lucide's icon, the README glyph); keep what it checks.

Platform: **Mac-only** (goldens).

## Implementation notes

- Every glyph is the one proposed above; no swaps. How the dialects, the
  Material extensions the table doesn't name, and the lookup order were
  decided is in `docs/decisions/rtq0syrybt-language-icons.md`.
- `TintedIcons` lays `ICON_NAMES` and `ICON_EXTENSIONS` over Lucide's
  tables, so an exact name only Lucide's tables know (`Cargo.toml`,
  `pom.xml`, `LICENSE`) keeps its icon for the next card.
