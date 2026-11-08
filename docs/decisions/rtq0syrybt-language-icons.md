# T1614: a line icon per language in the tenmoku set

Decisions taken while implementing
`docs/specs/rtq0syrybt-a-line-icon-per-language-in-the-tenmoku.md`.

## The spec's glyphs, unchanged

Every language row takes the glyph the spec proposes; no swaps. The 24 new
Lucide files and 32 Tabler files are the ones the spec's scope lists, from
the pinned tags (Lucide 1.52.0 @ `500620a`, Tabler v3.50.0 @ `6904efd`).
Lucide's are edited `stroke-width="2"` → `"1.75"` only; Tabler's lose their
leading `<!-- tags … -->` comment and take the same stroke edit.

## Lookup order

`TintedIcons::file_icon` runs `by_name` over Lucide's tables with
`ICON_NAMES` and `ICON_EXTENSIONS` laid over them (built once into two hash
maps). So an exact name Lucide's tables know and ours don't keeps Lucide's
icon even when our extension table has its extension: `Cargo.toml` stays
`package`, `pom.xml` and `build.gradle.kts` stay `package`, `LICENSE` stays
`scale`. Those are project files, the next card's.

## Dialects and the extensions the spec doesn't name

Material Icon Theme's `EXTENSIONS` lists a few extensions the spec's table
doesn't name. They follow the language Material files them under:

- `.cxx`, `.c++`, `.hh`, `.hxx`, `.h++`, `.inl` with C++; `.csx` with C#;
  `.sc` with Scala; `.zon` with Zig; `.lhs` with Haskell; `.mli` with OCaml;
  `.hrl` with Erlang; `.heex` with Elixir; `.pyi`/`.pyw` with Python;
  `.ru`/`.podspec` with Ruby; `.pm` with Perl (Material has no `.pm`, but it
  is Perl's module extension); `.rmd` with R; `.ksh`, `.csh`, `.nu` with the
  shells; `.psm1`/`.psd1` with PowerShell; `.mts`/`.cts` with TypeScript;
  `.xhtml` with HTML; `.xslt` with XML; `.config` with INI; `.tab`/`.psv`
  with CSV; `.rej` with diffs; `.ltx`/`.sty` with LaTeX.
- `.gradle.kts` is Kotlin (`brand-kotlin`, `code`), not Groovy: it's a
  Kotlin script.
- `.less` takes CSS's `brand-css3`: neither set has a Less mark, and it is a
  CSS dialect.
- `.ron` is data (`file-braces`, `data`), though Material draws it as Rust.
- `.pyc` is `file-code` and `.whl` `file-archive`, both in `script`
  (Python's family). They aren't languages, so `file-code` is allowed here.
- `.env` (as `x.env`) is `file-key` in `config`, as `.env*` names already are.
- Every other kind takes a Lucide kind icon in its existing tint: fonts
  `file-type`, PDFs `file-text`, logs `scroll-text`, packages (`.deb`,
  `.rpm`) `file-archive`, keys and certificates `file-key`, more images,
  video and sound `file-image`/`file-video-camera`/`file-music`. Executables
  (`.exe`, `.msi`) take the existing `app-window`: Lucide has no file-kind
  icon for a program, and `app-window` is already bundled.

Language-selecting file names also cover `.justfile`, `.containerignore`,
`.dockerignore`, `.gitkeep`, `MERGE_MSG`, `TAG_EDITMSG`, `_vimrc`, `.gvimrc`
and the other bash/zsh startup files Material lists, with their language.

## Tests

`tests::assets::bundled_files_match_the_enum` now walks `assets/icons` and
`assets/tabler-icons` separately (each directory's SVGs equal the `Icon`s the
macro puts there, and each `LICENSE` is the set's); `Icon::dir` exists for it
and is `#[cfg(test)]`. `every_icon_is_bundled_and_parses` also checks a
Tabler icon is served at `kiln-ui/icons/` and not under its own directory.
`crates/fastedit/tests/notice.rs` gains `repository_icons_table_is_checked`,
asserting both THIRD_PARTY.md copies carry the three Icons rows and the Tabler
LICENSE is read.
