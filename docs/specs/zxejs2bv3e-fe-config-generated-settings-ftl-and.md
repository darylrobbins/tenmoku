---
task_uid: "01m6xrpz17pfyg7yzxejs2bv3e"
task: "T532"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The two goldens pass and regenerate together: UPDATE_GOLDEN=1 rewrites settings.schema.json, settings.ftl and settings-template.ftl; en-US user_settings_template() is byte-equal to today's template"
    required: true
    manual: false
  - id: "C2"
    text: "The fake-locale tests pass, including file creation through Open Settings and write_setting (cargo nextest run -p fe-workbench --test settings_template_fake_locale)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-config --test l10n passes with fe-config's lint on"
    required: true
    manual: false
---

# fe-config: generated settings.ftl and settings-template.ftl, setting_key, the template rendered per locale, keyed descriptions in the settings UI (i18n C1) (Mac T1530)

# fe-config: generated settings.ftl and settings-template.ftl, keyed descriptions (C1)

Spec: card **C1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for C1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- Lands after the B3 cards that change settings (the default theme, the icon
  theme's `material` value), so the generated files start from the new schema.

Platform: **Linux-OK**.

## Implementation notes

- **Keys.** `fe_config::setting_key(KeyKind, path)` with `KeyKind::{Description,
  Template, Group}`: `settings-<path>`, `settings-template-<path>`,
  `settings-group-<name>`. A value's description (the schema's `const`
  variants) is an attribute of its setting's message, named by the value
  (`""` is `.empty`). The generator rejects a name outside `[A-Za-z0-9_]`, a
  value no attribute can name, and a top-level table named `template`,
  `group` or `intro`.
- **`settings.ftl`** holds exactly what the Settings editor shows: each row's
  and group's description, each described value, and each table name's title
  (160 descriptions today). Descriptions of things the editor never lists
  (fields inside `vim.remap` items, `formatters.*`, `$defs` docs) stay in the
  schema only.
- **Template blocks.** A `#` line at either end of a prose run stays as it is
  (so the extensions and formatters separators are kept byte for byte); `#`
  lines inside a run are empty lines of its message. A translated line wraps
  at 80 columns keeping its indent; a block only en-US has is copied as
  written, so a chain whose first locale lacks the template is still
  byte-equal. 98 blocks today.
- **Generators are test-only** (`l10n/generate.rs`, `l10n/ftl.rs`, under
  `#[cfg(test)]`); the runtime needs only `l10n/template.rs`. `UPDATE_GOLDEN=1`
  rewrites `settings.schema.json` and `settings.ftl` in
  `tests::settings::schema_matches_golden`, and `settings-template.ftl` and the
  fake locale's fixture in `tests::l10n::template_ftl_matches_golden`.
- **Fake locale.** `src/tests/fixtures/qps-ploc/settings-template.ftl` is
  generated (every message a `⟦key⟧` marker long enough to wrap), kept outside
  `l10n/` so it never ships; fe-workbench's
  `tests/settings_template_fake_locale.rs` registers it through
  `fe_command::l10n::catalogue::resources!` (fe-workbench has no direct
  kiln-l10n dependency).
- **The toggle writer.** kiln-shell's `persist.rs` isn't in this card's scope,
  so `kiln_shell::persist::setting_writer` keeps its `&'static str` template.
  fe-workbench's `persist.rs` builds its writer with `Writer::new` around
  `fe_config::write_setting`, which calls `user_settings_template()` at each
  write on the background executor: the design's behaviour (a file created
  after a locale change gets the new locale's template) without the kiln
  change. The live-locale test is
  `a_toggle_after_a_locale_change_creates_that_locales_template` in
  `settings_template_fake_locale.rs` rather than a separate
  `l10n_settings_writer.rs`, which isn't in scope; kiln-shell's provider-timing
  test isn't written. `setting_writer` now has no caller in the app; a later
  card that owns kiln-shell's `persist.rs` can take the provider form or drop it.
- **The lint** is on for all of fe-config (`all = true`). The project-file
  warning constants (`PROJECT_*`, `UNTRUSTED_LSP`) stay as public en-US text,
  which other crates' tests compare with; diagnostics use their
  `config-project-*` messages, and a unit test checks each constant is its
  message's en-US text. `themes.rs` (bundled theme names) is allow-listed
  whole; file-name formats and the macro-generated doc comments literal by
  literal.
- **Carried T480 findings.** F1: `language_file` writes `%XX` for a separator,
  `%`, a control character, `<>:"|?*` and a leading `.`, so the file is always
  directly in the snippets directory; `file_language` decodes it, so the
  language still matches. F5: key positions are a map built once, and
  diagnostic locations come from one pass over the text.
