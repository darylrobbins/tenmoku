---
task_uid: "01m5n96wyq8rtmf6sk057w5x17"
task: "T293"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "settings_schema() is flattened into SettingEntry rows (dotted key path, group, kind, default, description) with kinds Bool, Enum(variants), Int{min,max}, Float{min,max}, Text, TextList, and Other for tables or arrays of tables the editor cannot edit (those link to settings.toml). Every leaf the schema declares gets exactly one row; no row is invented. Tests every_schema_leaf_has_one_row, kinds_follow_the_schema, ranges_come_from_schemars_attributes"
    required: true
    manual: false
  - id: "C2"
    text: "SettingsModel::new(user_text, project_text) reads the two layers as TOML without writing anything and gives per row: effective value, source it comes from (ValueSource::Default, User, Workspace), is_modified (set in the user or workspace file, even to the default value), and workspace_allowed (false for the keys a project layer ignores: PROJECT_CHAT, PROJECT_TERMINAL_SHELL, PROJECT_VIM_USER_ONLY, project lsp keys other than turning a server off). A file that does not parse yields rows at default and an error string, never a panic. Tests source_precedence_is_workspace_over_user_over_default, modified_even_when_equal_to_default, bad_toml_leaves_defaults, project_only_restrictions_are_flagged"
    required: true
    manual: false
  - id: "C3"
    text: "search(query) filters rows case-insensitively: every whitespace-separated term must appear in the key path, the group title or the description; results keep schema order; empty query returns all; extra filters @modified and @user / @workspace as VS Code has. groups() returns the top-level tables as titled groups ('Editor', 'Editor \u003e Minimap', ...) with row counts. Tests search_matches_key_description_and_group, search_terms_are_anded, at_modified_filter, groups_are_titled_and_nested"
    required: true
    manual: false
  - id: "C4"
    text: "No GPUI, no new dependency; tests are headless and run on Linux"
    required: true
    manual: false
---

# Add a headless settings model that lists every setting from settings.schema.json with its type, default, description and group, searches them, and reports each one's user and workspace value and modified state, for the Settings editor (Mac T1094)

## Problem
Settings live only in `settings.toml` (schema `crates/fe-config/settings.schema.json`, derived from `Settings` by schemars in `settings.rs` `settings_schema()`); there is no Settings editor. VS Code's Settings editor (workbench.action.openSettings) is a searchable, grouped list of every setting with its default, description, a "modified" mark and per-scope values. This card is its headless model; the view (card 3/4) and write-back (card 2) come separately so hosts can run them in parallel. Settings stay TOML; the schema is the single source for what is listed, so a setting added later appears with no extra work.

## Change
New `crates/fe-config/src/settings_model.rs`, `pub mod settings_model;` and re-exports (`SettingsModel`, `SettingEntry`, `SettingKind`, `ValueSource`, `SettingGroup`) in `lib.rs`. fe-config already depends on serde_json and toml; add nothing.
- `SettingEntry { key: Vec<String>, group: String, kind: SettingKind, default: serde_json::Value, description: String }` built by walking `settings_schema()`: resolve `$ref` into `$defs`, take `description`/`default`/`enum`/`oneOf` string variants/`minimum`/`maximum`/`minLength` as schemars emits them (see the `#[schemars(range(...))]` uses in settings.rs), nested tables recurse with the table name as a sub-group.
- `SettingsModel::new(user: Option<&str>, project: Option<&str>)` parses each layer with `toml::Table` and exposes `entries()`, `value(&entry) -> EffectiveValue { value, source }`, `is_modified(&entry)`, `workspace_allowed(&entry)`, `errors()`.
- `search`, `groups` as in the criteria. Group titles come from the key segments (`editor` -> Editor, `lsp` -> LSP via a small table for acronyms: LSP, AI, UI).
- Do not edit settings.rs or the schema (T877/T1031/T1042 change them; this card must read whatever is there).

## Tests
New `crates/fe-config/tests/settings_model.rs` with the tests named in the criteria. Fixtures are inline TOML strings; no files. Headless, Linux-OK.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.

## Implementation notes

- `SettingKind::Text` carries the schema's `minLength` (`Text { min_length }`), and `Enum` carries each variant's description from schemars' `oneOf` (`EnumVariant { value, description }`) for the view's enum descriptions. A setting unset by default (`editor.font_family`) has a null `default`.
- `user_value` and `workspace_value` give each file's value as written; `value` gives the effective one. The project layer is filtered as `SettingsStore`'s project filters filter it (chat and theme dropped, `terminal.shell`/`shell_args`, `vim.shell`/`remap` dropped, `lsp` narrowed to `enabled = false`), so a key the project sets but may not isn't its source, though it is modified (set in the file). A table or array-of-tables row's effective value gets the schema defaults of the keys its items leave out (`lsp.servers.<name>.enabled`, `vim.remap[].recursive`), as the store's deserializing does. `theme` is not `workspace_allowed` either, since the store drops it. Trusted folders' LSP keys are not modelled.
- `values_agree_with_the_settings_store` checks every row's effective value against `SettingsStore::load` for the same text (written to a tempdir, the only test that touches files).
- Only a TOML syntax error is reported in `errors()`; schema errors stay the store's diagnostics.
- `@workspace` and `@modified` find what the project's file sets, even where it's ignored. Group counts are direct rows, not nested groups'.
