---
task_uid: "01m6j08g1s3w30pw6dmpbxmda5"
task: "T274"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "plugins_preview_key: the default is false; the user file [extensions] plugins_preview = true reads true; a project file [extensions] plugins_preview is ignored with the PROJECT_EXTENSIONS warning; the template documents # plugins_preview = false under [extensions]; the schema gives a boolean, default false, whose description says it is a preview (crates/fe-config/tests/extensions_plugins_preview_setting.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config and cargo clippy -p fe-config --all-targets -- -D warnings pass, with settings.schema.json regenerated"
    required: true
    manual: false
---

# Add the extensions.plugins_preview setting, default false: code plugins run only when it is on, and a project file cannot set it (Mac T1345)

## Problem

Code plugins ship in the first release as a preview, off by default (user, 2026-11-03): declarative
extensions are the supported path. The app needs a user setting that gates them before the plugin
host is wired in (T1062).

## Change

- `crates/fe-config/src/settings.rs`: `ExtensionsSettings` gains `plugins_preview: bool`, default
  `false`, documented: "Run code plugins (WebAssembly) from installed extensions. A preview: their
  API can change in any release. Each plugin still asks for what it may do before it runs." It sits
  in `ExtensionsSettings` so `drop_extensions` already drops it from a project file with the
  `PROJECT_EXTENSIONS` warning: a cloned repo must never turn plugins on.
- `crates/fe-config/settings-template.toml`: under `[extensions]`, the commented default
  `# plugins_preview = false` with a short comment saying it is a preview and off by default.
- `crates/fe-config/settings.schema.json`: regenerated (`UPDATE_GOLDEN=1`, the schema golden test).
- `crates/fe-config/tests/extensions_plugins_preview_setting.rs`: modelled on
  `extensions_disabled_setting.rs`.

Nothing reads the key yet; T1062 gates the plugin host on it, live.

## Notes

Key choice: `extensions.plugins_preview`, not a new `[plugins]` table, because `[extensions]` is
already user-only (a new table would need its own project filter and warning). Edits stay inside
`ExtensionsSettings`, its `$defs/ExtensionsSettings` schema entry and the template's `[extensions]`
section, so they merge textually apart from the lent `[formatters]` (T1324) and spell-check (T1332)
settings work.

Host-independent (fe-config only).

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.
