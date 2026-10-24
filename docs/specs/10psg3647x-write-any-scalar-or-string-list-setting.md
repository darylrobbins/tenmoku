---
task_uid: "01m5n96x792fpayt10psg3647x"
task: "T294"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln_core::write::SettingValue gains Int(i64), Float(f64) and StrList(\u0026[\u0026str]); written as TOML 123, 1.5 (always with a fractional part or exponent, never 13 for a float) and [\"a\", \"b\"]; a float that is not finite is an InvalidInput error. Existing Bool/Str behaviour is byte-for-byte unchanged. Tests int_float_and_list_values_are_written, float_keeps_a_point, comments_and_order_survive, non_finite_float_is_rejected in kiln-core/tests/write_setting_values.rs"
    required: true
    manual: false
  - id: "C2"
    text: "fe_config::settings_edit::{set_setting, reset_setting}(scope, root, key, value) pick the file: User -\u003e user_settings_path(), Workspace -\u003e project_settings_path(root); set writes through write_settings (atomic, template for a missing user file; a missing workspace file is created empty, not from the user template); reset removes the key and removes a table left empty by it, leaving a table that still holds other keys or comments. Tests set_in_user_file, set_in_workspace_file_creates_it_empty, reset_removes_key_and_empty_table, reset_keeps_a_table_with_other_keys, comments_survive_set_and_reset, reset_of_an_absent_key_writes_nothing Limit (Known limits, review round 3): a reset whose key sits in an inline table that the exact key-and-comma cut cannot take (for example a dotted key such as `minimap.enabled` inside `editor = { ... }`) and whose removal through toml_edit would drop any comment returns Err(Invalid(message saying to edit settings.toml by hand)) and leaves the file byte-for-byte untouched; it never discards a comment. Applies to reset_settings and to write_settings(.., None). Tests reset_of_a_dotted_key_in_a_commented_inline_table_is_refused (file unchanged, error) and reset_of_a_dotted_key_without_comments_still_works"
    required: true
    manual: false
  - id: "C3"
    text: "A workspace write of a key the project layer ignores (settings_model's workspace_allowed is false; here: the PROJECT_CHAT, PROJECT_TERMINAL_SHELL and PROJECT_VIM_USER_ONLY keys) returns Err(NotAllowedInWorkspace) and leaves the file untouched. Test workspace_restricted_keys_are_refused"
    required: true
    manual: false
  - id: "C4"
    text: "A set that the schema would reject (out-of-range number, enum variant not in the schema, wrong type) returns Err(Invalid(message)) without writing; validation is settings_schema() checked on the one value. Tests out_of_range_is_refused, unknown_enum_variant_is_refused"
    required: true
    manual: false
  - id: "C5"
    text: "Headless, no new dependency (toml_edit is already a dependency of kiln-core and fe-config); tests run on Linux"
    required: true
    manual: false
---

# Write any scalar or string-list setting back to the user or workspace settings.toml keeping comments and layout, and reset one to default by removing its key (Mac T1095)

## Problem
`write_setting`/`write_settings` (kiln-core `write.rs`, wrapped in `crates/fe-config/src/write.rs`) can set only booleans and strings, and a key can be removed only through `write_settings(.., None)`. A Settings editor must set numbers and string lists, reset a setting to default, and write the workspace file too, all without disturbing the user's comments (VS Code edits settings.json in place for the same reason). The write engine (atomic rename, 8-round re-read on concurrent change, template for a missing file) is reusable as it is.

## Change
- `kiln/crates/kiln-core/src/write.rs`: add the three `SettingValue` variants and their `toml()` rendering (`basic_string` for list items). `SettingValue` is `Copy`; `StrList(&'a [&'a str])` keeps it so. Callers matching it: `crates/fe-workbench/src/theme_picker.rs` only constructs it, `crates/fe-config/src/lib.rs` re-exports it; check with `rg SettingValue` that no `match` needs a new arm.
- `crates/fe-config/src/write.rs`: no change beyond docs unless a helper is needed for an empty-file (no template) write; add `write_settings_plain(path, changes)` for the workspace file if so.
- New `crates/fe-config/src/settings_edit.rs`: `SettingScope { User, Workspace }` (defined here only; card 1's `ValueSource` is a separate enum that also has Default), `set_setting`, `reset_setting`, `SettingEditError { NotAllowedInWorkspace, Invalid(String), Io(io::Error) }`. Validation: build the one-key table, run it through the same `jsonschema`-free checker `load_layer` uses (see `fe_config::layers::parse_layer`/`Diagnostic` for how the schema is applied; reuse it rather than adding a validator dependency).
- Removing a now-empty `[table]` is done in `write.rs` of kiln-core inside the existing `edit` function so it stays one atomic write.

## Tests
`kiln/crates/kiln-core/tests/write_setting_values.rs` and `crates/fe-config/tests/settings_edit.rs` (tempfile dirs, already a dev-dependency). Headless, Linux-OK. Does not touch settings.rs or the schema.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.

## Implementation notes

- kiln-core `write.rs`: `SettingValue` loses `Eq` (a float isn't), keeps `Copy`. A float is written with `{:?}`, which always has a point or an exponent (`13.0`, `1e300`); a non-finite one fails the whole write with `InvalidInput` before anything is read. Removal with pruning is a new `reset_settings(path, keys, template)` beside `write_settings`, sharing its write loop through a private `Change` (set / remove / reset), so `write_settings(.., None)` — the theme picker's — keeps its old behaviour of leaving an emptied table in place.
- Pruning: after the key goes, each table on its path, innermost first, is removed if it has no keys left: an inline table by removing its key, unless a comment is inside its braces, after it on its line or on the line before (review F1); a `[header]` if only blank lines are under it and no comment is on its line or the line just before it (that comment is taken as the table's). A last table takes the blank lines before it with it.
- A table added to an empty file (the workspace file, created from an empty template) has no blank lines before it.
- `settings_edit` validates the one-key table by deserializing it as `Settings` (catches wrong types and unknown enum variants), then `Layer::check_schema(&settings_schema())` (ranges, `minLength`), then the model's enum variants for string settings. A workspace reset of a key the project can't set is refused too, as a write.
- Review F1 (round 1): a key removed from an inline table written over several lines (`  font_size = 14,`) now takes just its line when its trailing comma is all that follows, so the comments beside it in the table stay; the existing check that the cut parses to the same table as toml_edit's removal still guards it. This also applies to `write_settings(.., None)`.
- Review round 2: F1 (a value sharing the closing brace's line, `font_size = 14 }`, lost the comment above it through toml_edit's removal). A key in an inline table is now removed by cutting its own `key = value` and one comma, the one after it or else the one before, keeping every comment; an only key leaves `{}`. As with the line cut, it applies only when it parses to the same table toml_edit's removal gives. F2: a comment is now a `#` outside any string, found by a scan that skips basic, literal and multiline strings, for inline tables, headers and the line before them.

## Known limits

- Review round 3 (F1, medium; C2 narrowed by the lead): a key in an inline table that neither the line cut nor the exact key-and-comma cut can take — a dotted key such as `minimap.enabled` inside `editor = { ... }` sharing a line with the closing brace or another key — is removed by toml_edit, which drops the comments before it. When that removal would leave fewer comments than the file had (counted by the string-aware comment scan), kiln-core refuses with `io::ErrorKind::Unsupported` and writes nothing; this applies to `reset_settings` and to `write_settings(.., None)`. `fe_config::reset_setting` turns it into `SettingEditError::Invalid("<key>: can't be removed without losing a comment; edit settings.toml by hand")`. The same key with no comment to lose is still removed. Cutting the whole dotted assignment textually is not done here.
