---
task_uid: "01m5ffwy03qyxmb0c3nygzf5k5"
task: "T1042"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions_disabled_setting::disabled_key: the default is empty, the user file's [extensions] disabled = [\"acme.solarized\"] reads, a project file's [extensions] is ignored with a warning, and the template and schema document it (crates/fe-config/tests/extensions_disabled_setting.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "extension_disable::a_disabled_extension_contributes_nothing: kiln::extensions::set_disabled makes its theme, keymap and menu layers vanish and on_extensions stop receiving it, while loaded() still lists it; a selected theme of it falls back with the existing warning; enabling it again restores all without a restart (kiln/crates/kiln/tests/extension_disable.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "extension_disabled::the_setting_reaches_the_app: editing settings.toml to disable an extension stops its language server, grammar offers and snippets and enabling it again brings them back, its consent entry kept; an unknown id is not an error (crates/fastedit/tests/extension_disabled.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "extensions_picker_disable_tests::the_list_shows_disabled: a disabled extension's row has state Disabled"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check_kiln_boundary.py, cargo nextest run -p fe-config -p kiln -p fe-workbench -p fastedit and cargo clippy --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Disable an installed extension with [extensions] disabled in settings.toml (extensions record §10)

## Problem

Extensions record §10: "Disabling one: a later card adds `[extensions] disabled = ["acme.solarized"]`
to the user's settings." There is no way to turn an installed extension off without deleting its
directory.

## Change

- `crates/fe-config/src/settings.rs`: `Settings::extensions: ExtensionsSettings { disabled:
  Vec<String> }` (default empty), documented in `settings-template.toml` and `settings.schema.json`
  (regenerate as the neighbouring settings do). A user-file setting only: a project file's
  `[extensions]` is ignored with a warning, as `theme.dark` is (`crates/fe-config/tests/
  theme_names_setting.rs` is the model).
- kiln (`kiln/crates/kiln/src/extensions.rs`, `setup.rs`): `kiln::extensions::set_disabled(cx, ids)`.
  A disabled extension is still discovered and listed but contributes nothing: no themes (a selected
  theme that's gone falls back with the existing warning), keymaps, menus, and its app kinds aren't
  handed to `on_extensions`. Enabling it again applies without a restart, as a reload does.
- `crates/fastedit/src/config.rs`: follows the setting as `theme::init` follows `theme.dark`
  (`crates/fastedit/src/theme.rs:50`) and calls `set_disabled`. A disabled extension's language
  server stops and its consent is kept, its grammars stop being offered for newly opened files and
  its snippets stop being offered.
- `crates/fe-workbench/src/extensions_picker.rs`: a disabled extension shows state `Disabled`.

An unknown id in `disabled` is not an error (the extension may be uninstalled).

## Out of scope

A command or list action that toggles the setting; per-contribution disabling; a project-level
setting; `extensions.allowed_licenses` (Q10, deferred).

## Implementation notes

- kiln-core gained `ExtensionSet::filtered` (outside the listed scope): kiln needs a set without
  the disabled extensions to hand `on_extensions`, and the set's fields are private.
- `kiln::extensions::set_disabled` keeps the IDs in a global, read by `kiln::init` too, so IDs set
  before init never load. The source filters what it reads (themes, keymap and menu files), what it
  fingerprints and what `on_extensions` gets; `loaded()` and licence notices still cover every
  installed extension. A theme selection's fallback warning is published at once
  (`Layers::apply_publishing`, split out of `select` in `kiln/src/config.rs`), as the call comes
  between the config service's batches.
- `kiln::extensions::disabled` and `kiln::extensions::observe` let the extensions list show and
  follow the state.
- fastedit follows the setting from a config source added in `with_extensions`
  (`DisabledExtensions`), installed after the extensions' own, so the first `on_extensions` already
  lacks a disabled extension and `main.rs` needs no change. Servers, grammars and snippets go
  through the existing `on_extensions` path; consent entries are untouched.
- The carried T1039 findings on `extensions_picker.rs` are fixed, which reached
  `lsp_consent.rs` (`granted_extension_servers`), `lsp_consent_prompt.rs` (one coordinated
  `ask_in` for the window's own asking and the list's), `workbench.rs` (the held field renamed) and
  the existing picker tests (Enter on a row with nothing waiting now shows its details):
  - F1: Enter asks about each of the row's waiting servers in turn, undecided first, then declined.
  - F2: rows match consents by canonical extension directory and server name; a granted server an
    earlier extension's same-named one shadows says it isn't running.
  - F3: invocations show the resolved executable, the environment, and shell-quoted arguments.
  - F4: Enter on a row with nothing waiting shows its full details, a line each, in a prompt.
  - F5: the list's prompts go through the same `Asking` registry and per-window limit as the
    automatic ones.
  - F6: rows follow `ExtensionConsent` and kiln's extensions while the list shows, keeping the
    query and selection; Enter reads the consents as they are when pressed.
- Review round 1: kiln installs the app's config sources before the extension source, and a
  disable that arrives before the extension source is installed only re-reads its files, so
  `on_extensions` never gets an extension disabled from the start. A re-resolve asked for before
  contributions start (`contributions::resolve_again`, outside the listed scope) now runs once they
  start, so its keys and menus never land. The extensions list restores focus before showing its
  prompts. The C3 test drives a trusted workbench with fe-lsp's fake server (a new fastedit
  dev-dependency on `fe-lsp`'s `test-support`).
