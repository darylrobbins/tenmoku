---
task_uid: "01m7k2aj1fzp16pwrjx6x5sc6v"
task: "T406"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-syntax/src/language.rs gives ruby the file names Dangerfile, Appfile, Matchfile, Pluginfile, Berksfile, Thorfile, Steepfile, .irbrc and .pryrc and the extension jbuilder, and extensions/grammars/ruby/extension.toml lists the same additions so its extensions and file_names equal the native ruby Def's; Procfile is a yaml file name and (\"Procfile.\", \"yaml\") is in FILE_NAME_PREFIXES; service, timer, socket, mount, path and target are ini extensions (NOT desktop: the INI grammar puts an ERROR node on a localized key such as Name[de]=Anwendung, which nearly every .desktop file has, so .desktop is left out per the card's fit-check rule, like CODEOWNERS' escape hatch); CODEOWNERS is a gitignore file name; avsc is a json extension; mdx is a markdown extension; nothing else in language.rs changes"
    required: true
    manual: false
  - id: "C2"
    text: "the new crates/fe-syntax/tests/project_file_names.rs pins every mapping above (mixed case and under a directory, such as .github/CODEOWNERS and Procfile.dev), that look-alikes keep their language or none (Procfile.sh is bash, Dangerfile.bak none, and a .desktop file stays unclaimed), and that the spec's Procfile and systemd unit samples parse with no ERROR node, as does the CODEOWNERS sample (or CODEOWNERS is left out with the parse tree in the decision file); the decision file records the .desktop INI parse tree with its ERROR node and says .desktop waits for an INI grammar fix or its own card"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --features ruby -E 'binary(project_file_names) | binary(language_file_names) | binary(config_file_names) | test(ruby_extension_is_as_built_in) | test(every_extension_is_well_formed)' passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Highlight Procfiles, systemd units, .desktop files, CODEOWNERS, Danger/fastlane/Thor Ruby files, .avsc and .mdx with the grammars we have (file name and extension mappings) (Mac T1899)

# Common project files open with the grammars we already have

The user asked (2026-11-16): "Procfiles don't seem to be syntax highlighted.
Are there any other file formats like this that we're missing that are common
within certain types of projects?" The lead proposed a set and the user said
"Go for it". This card is the part that needs no new grammar: file names and
extensions mapped to grammars fe-syntax already has. Prisma, HTTP request
files and pip requirements are their own cards (new WebAssembly grammars).

## Where the mappings live

- Built-in languages: `crates/fe-syntax/src/language.rs`, each `Def`'s
  `extensions` (no dot, ASCII case ignored) and `file_names` (whole names,
  case ignored), plus `FILE_NAME_PREFIXES` (~L161, `(prefix, language)`,
  checked last, only when no extension or name claims the file) and
  `PATH_SUFFIXES`. The `ALIASES` table (~L362) is Markdown fence and injection
  names only, not file detection: don't add file mappings there.
- Ruby is an extension by default (T427) and also built in under the `ruby`
  feature. Its files are listed in BOTH `extensions/grammars/ruby/extension.toml`
  (`extensions`, `file_names`) and the `ruby` `Def` in `language.rs`, and the
  two lists must stay equal: `wasm_parity`'s `ruby_extension_is_as_built_in`
  (only with `--features ruby`) asserts every extension and file name the
  manifest lists is the built-in's, and the default build (no `ruby` feature)
  only knows the manifest's list. Add each Ruby name to both, in the same order.
- Changing `extensions/grammars/ruby/extension.toml` changes the ruby registry
  artifact's content tag at the next registry build (the artifact includes the
  manifest), which is expected; `pack.toml` records only the wasm hash, so it
  doesn't change, and no committed index or golden pins the manifest. Don't
  touch `pack.toml`, `registry.toml`, the ruby queries or golden.

## Mappings to add

- Ruby (`file_names`): `Dangerfile`, `Appfile`, `Matchfile`, `Pluginfile`,
  `Berksfile`, `Thorfile`, `Steepfile`, `.irbrc`, `.pryrc`; (`extensions`):
  `jbuilder`. `.irbrc` and `.pryrc` must be file names (a dot file has no
  extension to `Path::extension`).
- YAML: `Procfile` as a file name, and `("Procfile.", "yaml")` in
  `FILE_NAME_PREFIXES` so foreman's and Rails 7's `Procfile.dev`,
  `Procfile.local` and the like follow (their extension claims nothing). A
  Procfile is `name: command` lines, which parse as a YAML mapping of plain
  scalars.
- INI: systemd unit extensions `service`, `timer`, `socket`, `mount`, `path`,
  `target`, and freedesktop `desktop`. (`setup.cfg` is already INI by the
  `cfg` extension and `config_file_names.rs` pins it: nothing to do; keep it
  in the new test as a pin if you like.)
- gitignore: `CODEOWNERS` as a file name (GitHub's and GitLab's, wherever it
  sits: repo root, `.github/`, `docs/`).
- JSON: `avsc` (Avro schema) extension.
- Markdown: `mdx` extension, as a fallback (MDX's JSX and `import` lines stay
  plain Markdown; there's no MDX grammar).

Before adding each, check nothing else claims it: no built-in `Def`, no
`extensions/grammars/*/extension.toml` (`every_extension_is_well_formed`
fails on a clash with a built-in), and `FILE_NAME_PREFIXES` must not take a
file a built-in or extension already names. None of these is claimed at
a0c2e9057.

## Grammar fit checks

- Procfile: a sample with `web: bundle exec puma -C config/puma.rb`,
  `worker: bundle exec sidekiq -q default`, `release: bin/rails db:migrate`
  and a `# comment` parses as YAML with no ERROR node, and the process names
  are keys.
- CODEOWNERS: a sample with `# comment`, `* @org/team`,
  `/docs/ @alice @bob` and `*.js @org/frontend` parses with the gitignore
  grammar with no ERROR node and colours the comment. If it doesn't (owners
  after a space make an ERROR), drop CODEOWNERS from the card and say so in
  the decision file rather than shipping ERROR highlighting.
- A systemd unit (`[Unit]`, `Description=`, `[Service]`, `ExecStart=`,
  `[Install]`) and a `.desktop` entry (`[Desktop Entry]`, `Name=`, `Exec=`,
  `Name[de]=`) parse as INI with no ERROR node.

## Tests

A new test file, `crates/fe-syntax/tests/project_file_names.rs`, in the
`config_file_names.rs` style (`language_name_for_path`, each test under the
`cfg(feature = ...)` of the language it expects): every name and extension
above maps to its language, in mixed case and under a directory
(`.github/CODEOWNERS`, `app/views/x.json.jbuilder`, `Procfile.dev`); the
three grammar-fit samples above parse with no ERROR (as `yaml_long_list.rs`
checks `has_error`); and a few look-alikes stay unclaimed or keep their
language (`Procfile.sh` stays bash, `notes.target.bak`, `Dangerfile.bak`).
The Ruby tests go under `cfg(feature = "ruby")`, so they run with
`--features ruby` only (the gate runs default features).

## Scope

`crates/fe-syntax/src/language.rs`, `extensions/grammars/ruby/extension.toml`
and the new test file only. No new grammar, query, golden or fe-workbench icon
change. Lent card T1891 (argon) also edits `language.rs` (the `ruby` `Def`'s
`injections` line, ~10 lines below its `file_names`): keep the Ruby edit to
the `extensions`/`file_names` lists so the hunks stay apart.

## Implementation notes

`.desktop` is left out. The INI grammar gives an ERROR node on a localized key:

```
[Desktop Entry]
Name=App
Name[de]=Anwendung
```
parses as
`(document (section (section_name (text)) (setting (setting_name) (setting_value))) (ERROR (setting_name) (text) (UNEXPECTED 'A')))`.
Without the `Name[de]` line it parses clean. `.desktop` waits for an INI grammar
fix or its own card. The other fit checks (Procfile, systemd unit, CODEOWNERS)
parse with no ERROR node.
