---
task_uid: "01m6c9qkjz2crajypmy4w5xxng"
task: "T1298"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Cargo.lock has tree-sitter-ini 1.4.0 from crates.io and no crates.io tree-sitter-just; vendor/tree-sitter-just holds casey/tree-sitter-just at 5685543 with its Apache-2.0 LICENSE and a README saying how to update it"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-syntax/tests/config_file_names.rs: x.ini, setup.cfg, tox.ini, .editorconfig, .npmrc, .pypirc, .flake8, .coveragerc and .pylintrc are ini; .env, .env.local, .env.production, .env.example and prod.env are dotenv while .envrc and .env.sh stay bash; justfile, Justfile, .justfile and x.just are just, and a pathless buffer starting #!/usr/bin/env -S just --justfile is just"
    required: true
    manual: false
  - id: "C3"
    text: "Goldens ini.ini, dotenv.env and just.just show ini sections, keys, values and comments; dotenv names, unquoted and quoted values, ${VAR} expansions, export and comments; just recipes, parameters, dependencies, settings, variables, interpolations and a recipe body highlighted as bash"
    required: true
    manual: false
  - id: "C4"
    text: "each_language_highlights_a_snippet, candidates_per_language, every_listed_language_has_tokens and every_language_has_an_entry pass with ini, dotenv and just"
    required: true
    manual: false
  - id: "C5"
    text: "THIRD_PARTY.md lists ini and just; cargo deny check licenses bans sources passes (the RUSTSEC advisories already on main are T1301's); cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
  - id: "C6"
    text: "vendor/tree-sitter/src/parser.c's ts_parser__accept also admits a non-extra tree below the root only when it has children and state 1 goes back to itself on its symbol (ini's _blank); tests::ini_files_may_start_with_blank_lines passes for \"\\n\", \"; c\\n\\nk = v\\n\" and \"\\n[s]\\n\"; wasm_accepted_stack.rs still rejects its hostile stacks and its header and vendor/tree-sitter/README.md's parser.c paragraph describe the exception"
    required: true
    manual: false
---

# Highlight INI-style files (.ini, .cfg, .editorconfig, .npmrc), dotenv files and justfiles

## Problem

Everyday config files open as plain text: INI-style files (`.ini`, `.cfg`, `setup.cfg`, `tox.ini`,
`.editorconfig`, `.npmrc`), dotenv files (`.env`, `.env.local`, `prod.env`) and justfiles. The user approved
built-in highlighting for them on 2026-11-01. Detection (`crates/fe-syntax/src/language.rs`
`language_name_for_path`) matches a file name or an extension only, so `.env.local` / `.env.production`
(extension `local`, `production`) can't be matched.

## Change

Three languages in `crates/fe-syntax`, each a `Def` in `language.rs`:

- `ini`: crates.io `tree-sitter-ini` 1.4.0 (justinmk, the upstream author; Apache-2.0; ABI 15, depends on
  `tree-sitter-language` 0.1; about 3 KB of object code). New Cargo feature `ini`, on by default. Extensions
  `ini`, `cfg`; file names `.editorconfig`, `.npmrc`, `.pypirc`, `.flake8`, `.coveragerc`, `.pylintrc`,
  `pylintrc`. The crate's query leaves values plain: add `crates/fe-syntax/queries/ini.scm` after it with
  `(setting_value) @string`. Comment token `;` (VS Code's ini configuration). Settings must use `=`; a
  `key: value` line (allowed by Python's configparser) parses as an error, which is acceptable. A dedicated
  editorconfig grammar isn't needed: the ini grammar reads `.editorconfig` sections and `root = true`.
- `dotenv`: no new grammar. It reuses `tree_sitter_bash::LANGUAGE` (under `#[cfg(feature = "bash")]`), as
  `rustdoc_inline` reuses Markdown's, with `tree_sitter_bash::HIGHLIGHT_QUERY` then
  `crates/fe-syntax/queries/dotenv.scm`, which paints an assignment's unquoted value (`word`) as `string`.
  Names are property (bash's `variable_name`), `$VAR` / `${VAR}` expansions and quoted strings keep bash's
  colours. Zero binary cost. File name `.env`, extension `env`, and the prefix `.env.` through a new
  `const FILE_NAME_PREFIXES: &[(&str, &str)]` in `language.rs`, consulted only when no name, extension or path
  tail matched (so `.env.sh` stays bash). `.envrc` stays bash. LSP languageId passes `dotenv` through
  (no server).
- `just`: vendor casey/tree-sitter-just (the upstream, Apache-2.0 LICENSE though its manifest says MIT; pin
  `5685543a6e64f66335e25518c9ae8ffa1dae3d01`) as `vendor/tree-sitter-just` like T1282's gitignore crate:
  `src/parser.c` + `src/scanner.c`, its `queries-src/highlights.scm` and `injections.scm`, a `cc` build.rs,
  `LANGUAGE` / `HIGHLIGHTS_QUERY` / `INJECTIONS_QUERY` via `tree-sitter-language`. crates.io's
  `tree-sitter-just` 0.2.0 (tgross35, a maintainer) can't be used: it depends on `tree-sitter ~0.25.5`, which
  can't link beside our 0.27. ABI 15; about 41 KB of object code. File names `justfile`, `.justfile`
  (case-insensitive matching covers `Justfile`), extension `just`; `("just", "just")` in `detect.rs`
  `INTERPRETERS` for `#!/usr/bin/env -S just --justfile`. Recipe bodies inject bash (or a shebang's
  language) through the crate's injections query. Fold `recipe` (a `KINDS` entry in `fold.rs`).

Per-language tables: `comment.rs` (`ini` `;`, `dotenv` and `just` `#`; `auto_pairs`: `ini` and `dotenv` the
dockerfile/make row, `just` the javascript row with backticks) and `tests/auto_pairs.rs`; a case each in
`src/tests.rs` `each_language_highlights_a_snippet`; a sample each in `src/fold/tests.rs` `CASES`. Goldens
`tests/golden/ini.ini`, `dotenv.env`, `just.just`.

Dropped from this card: ssh config. The only grammar, tree-sitter-grammars/tree-sitter-ssh-config (MIT, not
on crates.io), enumerates every keyword: its parser.c is 1.15 MB, about 270 KB of object code, more than every
other grammar in these cards together. Leave it for T1287's on-demand WebAssembly set.

Record `ini` and `just` in `THIRD_PARTY.md` (ini under the crate table, Apache-2.0; just as vendored, with
the licence mismatch noted; `dotenv.scm` and `ini.scm` are ours), and note the release-binary growth in the
decision file.
