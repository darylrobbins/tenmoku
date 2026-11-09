---
task_uid: "01m6z27nk4pv45v1k6vj7ctgt1"
task: "T1628"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test data_file_keys passes: data_file_keys_are_string_special_key (YAML block and flow keys, plain and quoted; TOML bare, dotted and quoted pair keys; INI setting names; JSON and JSON5 member names all highlight as string.special.key, and their values do not)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test data_file_keys passes: headers_and_code_fields_keep_their_highlights (TOML [a.b] and [[bin]] components and INI section names stay type; a Rust field access and a struct field definition, CSS color, a .env NAME and a .gitconfig name stay property)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax passes with the updated expectations, including toml_keys_are_covered_to_eleven_components and the x.yaml, x.ini and Cargo.toml rows of src/tests.rs"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-config passes, including yohen_hakuji_values every_design_role_has_the_design_value with docs/design/design-system.md's new Syntax row 'Data-file key | #8db1f2 | #2457c5 | Lapis' mapped to syntax.string.special.key, plus yohen_hakuji_contrast and design_role_tokens"
    required: true
    manual: false
  - id: "C5"
    text: "Under crates/fe-config/themes only yohen.toml and hakuji.toml change (their string.special.key lines); the only screenshot goldens that change are fe-chat-view's chat_diff_dark.png and chat_diff_light.png (TOML keys recoloured), and only where keys are drawn; the fe-syntax goldens toml.toml, yaml.yaml, ini.ini and markdown.md (YAML front matter) are regenerated with keys as string.special.key; cargo clippy -p fe-syntax -p fe-config --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Data-file keys get their own colour: YAML, TOML and INI keys join JSON's string.special.key, Lapis in Yohen and Hakuji, with a design-doc row

## Decision (user, 2026-11-08, verbatim)
"Data-file keys get own colour" — "New token for keys in YAML, TOML, JSON and similar files, coloured Lapis (#8db1f2 Yohen / #2457c5 Hakuji), like VS Code's key blue. Code properties (Rust fields etc.) stay quiet Shino. Design doc gets a row."

## Why
Yohen and Hakuji colour YAML/TOML keys `property` (Shino #dccbb8 / #3d444d), almost the ink (#ede4d6 / #1f2328), so keys read as uncoloured.

## Token
Reuse `string.special.key`; add no new name. It is already in fe-syntax's NAMES list, JSON's crate query and queries/json5.scm already capture keys with it, and every bundled theme already sets it. Do not edit crates/fe-syntax/src/highlight.rs or any theme other than Yohen and Hakuji.

## Changes
- YAML: a new crates/fe-syntax/queries/yaml.scm ("Ours", like json5.scm), appended after tree_sitter_yaml::HIGHLIGHTS_QUERY in language.rs (earlier_patterns_win is false, so it wins). It captures the keys of block and flow mappings, plain and quoted, as @string.special.key. Do not edit vendor/.
- TOML (queries/toml.scm): every `key = value` key component, bare or quoted and at every covered depth, becomes @string.special.key. Table-header components ([a.b], [[bin]]) stay @type. The plain `(bare_key)` fallback for components past the 11-component limit becomes @string.special.key (quoted components still fall back to @string). Update the file's comments to match.
- INI (queries/ini.scm): `(setting (setting_name) @string.special.key)`. Section names stay type.
- JSON, JSON5: no query change.
- Yohen `string.special.key` = #8db1f2, Hakuji = #2457c5.
- docs/design/design-system.md Syntax table: add a row after Property: `| Data-file key | \`#8db1f2\` | \`#2457c5\` | Lapis | |`. Map it in yohen_hakuji_values.rs ROLES to syntax.string.special.key and bump the row count (7 + 11 + 10 + 3).
- Update the existing expectations that pin these keys as property: src/tests.rs (Cargo.toml, x.ini, x.yaml), src/tests/query_overrides.rs (toml_* tests, key_forms, json_and_yaml_keys_differ_from_values), src/injection/tests.rs (php_heredocs_keep_lines_for_yaml).

## Stays as is
Code properties and fields (Rust, JS, CSS `color`, …) stay `property` (Shino). dotenv names (bash grammar), .gitconfig names and the Java properties extension (extensions/grammars) are out of scope.
