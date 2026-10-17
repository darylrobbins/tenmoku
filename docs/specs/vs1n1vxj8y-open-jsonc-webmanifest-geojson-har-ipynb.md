---
task_uid: "01m557xpaap60zt0vs1n1vxj8y"
task: "T151"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-syntax::json_file_names vs_code_json_extensions_and_names_are_json (tests/json_file_names.rs, top level) passes: language_name_for_path gives json for x.jsonc, app.webmanifest, trace.har, x.jsonld, map.geojson, notes.ipynb, X.IPYNB, composer.lock, bun.lock, .babelrc, .eslintrc, .jshintrc, .swcrc, .watchmanconfig, .ember-cli and a.babelrc, toml for Cargo.lock and None for yarn.lock and x.jsonl; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Every other path keeps its language: fe-syntax::language_file_names login_shell_ruby_build_and_upper_case_names_are_detected, the fence alias tests and the golden highlight tests pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax passes with no existing test edited; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Open .jsonc, .webmanifest, .geojson, .har, .ipynb, .babelrc, composer.lock, bun.lock and VS Code's other JSON and JSON with Comments files as JSON, as VS Code does, instead of as plain text (Mac T790)

## Problem

`language_name_for_path` (`crates/fe-syntax/src/language.rs`) gives JSON only to `.json`: the `json`
`Def` has `extensions: &["json"]` and no file names. VS Code's JSON extension (extensions/json/package.json
in the VS Code app) opens many more files as JSON or JSON with Comments: extensions `.jsonc`,
`.webmanifest`, `.har`, `.jsonld`, `.geojson`, `.ipynb`, `.bowerrc`, `.jscsrc`, `.jslintrc`, `.vuerc`,
`.eslintrc`, `.jsfmtrc`, `.jshintrc`, `.swcrc`, `.hintrc` and `.babelrc`, and file names
`composer.lock`, `bun.lock`, `.watchmanconfig` and `.ember-cli` (plus multi-dot ones and patterns).

Reproduced at Mac main 3e3eaa03: `language_name_for_path` gives `None` for `x.jsonc`, `app.webmanifest`,
`trace.har`, `x.jsonld`, `map.geojson`, `notes.ipynb`, `composer.lock`, `bun.lock`, `.babelrc`,
`.eslintrc`, `.swcrc` and `.watchmanconfig`, so they open as plain text: no highlighting, folding,
brackets or `//` comments (T773), and no JSON language server.

## Change

In the `json` `Def`, add the single-dot extensions above to `extensions` (matched case-insensitively, as
now), and to `file_names` the dotfiles among them, because `Path::extension` of `.babelrc` is `None`
(`.bowerrc`, `.jscsrc`, `.jslintrc`, `.vuerc`, `.eslintrc`, `.jsfmtrc`, `.jshintrc`, `.swcrc`, `.hintrc`,
`.babelrc`), plus `composer.lock`, `bun.lock`, `.watchmanconfig` and `.ember-cli`. JSON with Comments has
no grammar of its own here; tree-sitter-json already parses comments, so they open as `json`. Out of
scope: `.js.map`-style multi-dot extensions, `**/.github/hooks/*.json`, JSON Lines (`.jsonl`, `.ndjson`:
a different VS Code language) and LSP language ids. `Cargo.lock` stays TOML and `yarn.lock` stays
unknown. Test in a new `crates/fe-syntax/tests/json_file_names.rs` (`#![cfg(feature = "json")]`).
Prototyped on 3e3eaa03: all of `cargo test -p fe-syntax` passed.
