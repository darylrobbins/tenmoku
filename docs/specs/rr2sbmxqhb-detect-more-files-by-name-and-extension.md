---
task_uid: "01m79daadqqhq428rr2sbmxqhb"
task: "T341"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "xml_family_extensions_and_names, json_toml_yaml_family_names, ambiguous_config_and_manifest_stay_undetected, names_ignore_ascii_case pass"
    required: true
    manual: false
  - id: "C2"
    text: "extensionless_xml_and_json_are_sniffed, unknown_extension_is_never_sniffed, html_and_plain_text_are_not_sniffed, path_and_shebang_beat_the_sniff pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
---

# Detect more files by name and extension (XML, JSON, TOML, YAML families: .csproj, .props, .xaml, poetry.lock, .clang-format...) and sniff extensionless XML and JSON by content (Mac T1775)

# Detect more files by name and extension (XML, JSON, TOML, YAML, INI families) and sniff extensionless ones by content

Detection today: `fe_syntax::detect_language_name(path, head)` = `language_name_for_path` (language.rs `DEFS` extensions/file_names per Def, PATH_SUFFIXES, FILE_NAME_PREFIXES; registered grammars via extension.toml `extensions`/`file_names`) then `language_name_for_shebang` then `language_name_for_first_line` (detect.rs: `<?xml`/`<svg`/`<!doctype svg`, `#cloud-config`, Emacs shell-script). Callers read the first DETECT_BYTES=512 bytes (fe-gpui view.rs) in `EditorView::detect_language` (pane_item.rs: on open, after save-as, detect_language_from), diff_view.rs and commit_diff.rs. Already covered (do not re-add): json has jsonc, webmanifest, har, code-workspace, babelrc/eslintrc, composer.lock, bun.lock; toml has Cargo.lock; ini has .editorconfig, .npmrc; xml has xml, svg, plist. Only languages compiled into fe-syntax count (xml, json, toml, yaml, ini are default features; csharp/fsharp are wasm and keep their own claims).

## Change (tables, in language.rs `DEFS`; one test row per entry)
- xml extensions += csproj vbproj fsproj vcxproj jsproj proj props targets nuspec resx xaml axaml xsd xsl xslt wsdl xliff xlf rss atom kml gpx storyboard xib appxmanifest vsixmanifest pubxml ruleset csl; file_names += web.config app.config nuget.config packages.config. Do NOT claim bare `config` or `manifest` (ambiguous).
- toml file_names += poetry.lock uv.lock Pipfile pdm.lock; yaml file_names += .clang-format .clangd .clang-tidy CITATION.cff .gemrc .yamllint; json file_names += Pipfile.lock .prettierrc .lintstagedrc .stylelintrc; skip any name that another Def already claims; `rg` the tables first.
- Content sniff (new crates/fe-syntax/src/sniff.rs, called last in `detect_language_name`, only when the path is None or has NO extension at all, e.g. `Pipfile`-style or `bin/config`; a file with any unknown extension is never sniffed): XML when the first non-space, non-BOM text is `<` + a letter/`_`/`!--` and the head contains a matching closing `</name>` or a `/>` before byte 512 (a bare `<Project Sdk=` MSBuild root is covered by that rule; HTML is not claimed: `<html`, `<!doctype html` start excludes); JSON when it starts `{` followed by whitespace* then `"` or `}`, or `[` then whitespace* then `{`, `"`, a digit, `-`, `]`, no further parsing. Both only if the language is compiled in. Conservative: when unsure, None.
- Order stays path > shebang > first-line > sniff, so nothing that is detected today changes.
## Scope
crates/fe-syntax/src/language.rs, crates/fe-syntax/src/detect.rs (one call), new crates/fe-syntax/src/sniff.rs, crates/fe-syntax/src/lib.rs (mod), new tests crates/fe-syntax/tests/extension_families.rs and crates/fe-syntax/tests/content_sniff.rs (separate binaries).
## Criteria test ids
extension_families::xml_family_extensions_and_names, json_toml_yaml_family_names, ambiguous_config_and_manifest_stay_undetected, names_ignore_ascii_case; content_sniff::extensionless_xml_and_json_are_sniffed, unknown_extension_is_never_sniffed, html_and_plain_text_are_not_sniffed, path_and_shebang_beat_the_sniff; `cargo nextest run -p fe-syntax` passes (incl. existing config_file_names, first_line_languages, tests::).
Conflicts: T1765 (C# preprocessor) is queries-only; any ready card editing language.rs DEFS should be rebased on finish. Land before the modeline card (both touch `detect_language_name`).
Platform: linux-ok.
