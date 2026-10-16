---
task_uid: "01m50ewh7a9xa5gc9k3vchj2yc"
task: "T95"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/macro_settings.rs: the generated Default carries each setting's default = .. value, and the schema lists each field with its doc line as its description"
    required: true
    manual: false
  - id: "C2"
    text: "macro_settings.rs: two section types read their own [section] tables of one settings.toml, neither reporting the other's keys as unknown, while an unknown key inside a section is still a diagnostic"
    required: true
    manual: false
  - id: "C3"
    text: "macro_settings.rs: in a kiln::testing app cx.setting::\u003cS\u003e() reads the section, and a view using it redraws after the file reloads"
    required: true
    manual: false
  - id: "C4"
    text: "Existing kiln-core settings tests pass unedited"
    required: true
    manual: false
---

# Add #[kiln::settings(section = ..)]: defaults, serde, a schema and hot reload from one struct, sections sharing one settings.toml (Mac T685)

Filed on argon from Mac T685 (card ~/coordination/argon-cards/T685.json rev 1; note zh). Mac task uid 01m4vbje66d4635mm2cmkb5j9q. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T94 (Mac T684). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T685)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). App settings today need `#[serde(crate = "kiln::serde", default)]`, a hand
`Default` (`examples/gallery/settings.rs`), `Config::settings(user, project)` with paths the app works out, and an
`observe_global::<GlobalSettings<S>>` to follow reloads (`examples/gallery/main.rs:50-56`). A schema means
depending on `schemars` directly. Each settings type is its own whole-file store (`kiln-core/src/settings.rs`),
so two types can't share one `settings.toml` without each seeing the other's keys as unknown.

## Change

In `kiln/crates/kiln-macros` (new `src/settings.rs`) and kiln:

```rust
/// `[notes]` in settings.toml.
#[kiln::settings(section = "notes")]
pub struct NotesSettings {
    /// The editor's font size, in points.
    #[setting(default = 14.0)]
    pub font_size: f32,
}
```

- The macro derives `Serialize`/`Deserialize` with `#[serde(crate = "kiln::serde", default)]`, a `Default` from the
  `default = ..` values (a field without one uses its type's `Default`), `PartialEq`, a JSON schema through
  `schemars` re-exported as `kiln::schemars` (doc lines become descriptions), and contributes the section
  (`Contributions::add_settings`).
- **Sections of one file.** Each section type reads its `[section]` table of the app's one `settings.toml`;
  unknown-key diagnostics are checked against the union of registered sections and each section's schema, so a
  key in another section isn't unknown. Add what `kiln-core/src/settings.rs` needs for a store over one table of a
  file.
- A reload of a section refreshes every window (`cx.refresh_windows()`); `cx.setting::<S>()` reads it.
- Rustdoc shows the expansion and the handwritten form.

New tests go in `kiln/crates/kiln/tests/macro_settings.rs`.

## Implementation notes

- kiln-core: `SettingsOptions::section(name, all)` makes a store read one table of a shared file. serde errors
  keep their location (a `DeserializeSeed` over the file that deserializes only that table); the schema check runs
  on the whole file against the section's schema wrapped as one property among the other sections' names, so a
  key of another section is known. Each store reports what's in its own table; the first section also reports what
  is outside every section (a syntax error, a table no section has), so nothing is reported twice. An error in one
  section keeps only that section at its last good table.
- kiln: `SettingsSection` (`const SECTION`, plus `JsonSchema`) and `kiln::settings_schema::<S>()`;
  `Config::settings_file(path)` names the shared file (`kiln::app` passes the user's `settings.toml`, and
  `user_files_in` moves it like the other user files); `Config::section::<S>()` adds a section, replacing an
  earlier store of the same type; two types with the same section name panic in `init`. `#[kiln::settings]`
  submits a `SettingsRecord`, which `add_recorded` / `kiln::module!` contribute through
  `Contributions::add_settings`.
- Out of the listed scope, needed for the wiring: `setup.rs` (Config), `app_builder.rs` (one line passing the
  settings file), `contributions.rs` (the record), a new `kiln/src/settings.rs`, and `testing.rs`:
  `TestApp::write_user_file(name, text)` writes a user file and sends the batch the watcher would (TestApp now
  keeps a batch sender rather than a closed channel), which the C3 test uses.
- A string-literal `default = ".."` is `Into::into`'d (for a `String`); any other expression is used as written.
