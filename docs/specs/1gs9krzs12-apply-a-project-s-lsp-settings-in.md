---
task_uid: "01m4pzqb7hx50kte1gs9krzs12"
task: "T65"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test trusted_lsp::project_lsp_keys_apply_unfiltered: trusted_lsp over a project file setting lsp.servers.\u003cname\u003e.command, args, env and settings returns them over the user file's values, while SettingsStore::load on the same files still drops them with the reworded UNTRUSTED_LSP warning"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test lsp::trusted_settings_tests::trusted_root_launches_with_project_settings: in a trusted root whose project file sets a server's command and settings, the FakeLauncher gets that command and workspace/configuration answers those settings; in an untrusted root it gets the filtered ones"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test lsp::trusted_settings_tests::trust_changes_and_reloads: trusting the root restarts the server with the project's command and untrusting goes back to the filtered settings; editing the project file's lsp settings then reloading sends didChangeConfiguration with the new values; a trusted read that finishes after the root changed is dropped; a workbench opened afterwards on an untrusted root gets only the filtered settings"
    required: true
    manual: false
  - id: "C4"
    text: "docs/decisions/lsp-client.md's Trust boundary says the project layer's lsp keys apply in trusted folders, and its 'left to a later card' sentence is gone"
    required: true
    manual: false
---

# Apply a project's LSP settings in trusted folders, and keep ignoring them in untrusted ones (Mac T395)

## Problem

A project's `.fastedit/settings.toml` may only turn language servers off: `narrow_lsp`
(`crates/fe-config/src/settings.rs:378`) drops every other `lsp.*` key with the `UNTRUSTED_LSP`
warning ("LSP settings in project files are ignored until workspace trust exists"). Workspace trust
exists since T177, and servers start only in trusted folders, but the filter still applies there:
docs/decisions/lsp-client.md:645-647, "Lifting this filter per trusted root is left to a later
card; for now it applies even in trusted folders." So a trusted project can't set a server's
`command`, `args`, `env`, `initialization_options` or `settings` (for example rust-analyzer's
`cargo.features` for that repository); the user has to put per-project values in their user file.

## Change

In a trusted root, the workbench's language servers get the project layer's `[lsp]` table
unfiltered; untrusted, nothing changes.

- `crates/fe-config/src/settings.rs`: add `pub fn trusted_lsp(user: Option<PathBuf>, project:
  Option<PathBuf>) -> LspSettings`: the `lsp` of the user and project files loaded with the same
  schema check (`schema_for`) and no project filter. It does blocking reads; callers run it off the
  main thread. Reword `UNTRUSTED_LSP` to "LSP settings in project files apply only in trusted
  folders". `SettingsStore` and the config service's store keep the filter: they are app-wide, and
  trust is per root.
- `crates/fe-workbench/src/lsp.rs`: while `root_trusted()`, the settings handed to the root's
  `LspManager` are `trusted_lsp(user_settings_path(), Some(project_settings_path(root)))`, read on
  the background executor: when the root becomes trusted (`apply_trust`), when a manager for a new
  root starts trusted, and on each `reload_lsp_settings`. A read that finishes after the root or
  its trust changed is dropped. Untrusting goes back to the filtered settings the workbench was
  given. The trusted settings are this workbench's only: `set_lsp_settings` (the default for new
  workbenches) keeps the filtered ones.
- docs/decisions/lsp-client.md, "Trust boundary": the project layer's `lsp` keys apply in trusted
  folders; untrusted, it may only narrow, as before.

Tests in the new files `crates/fe-config/tests/trusted_lsp.rs` and
`crates/fe-workbench/src/lsp_trusted_settings_tests.rs` (declared from `lsp.rs` with `#[path]`,
as `lsp_trust_tests.rs` is), with the existing FakeLauncher.

## Implementation notes

- After review (codex, round 1), the workbench doesn't call `trusted_lsp` itself: it keeps a
  `fe_config::settings::ProjectLsp` per root, the project file's `lsp` table checked as the store
  checks a file and kept at its last good value while the file has errors, and lays it over the
  filtered settings it was given, which keep the user file's last good table (the app's store
  holds it). That's what `trusted_lsp` computes over a fresh store, but an invalid file no longer
  drops values already applied, across reloads and trust changes.
- The app's store narrows the project file, so an edit to only the keys it drops reloads nothing:
  the workbench watches its root's project file (`ConfigWatcher`) while the root is trusted, and
  reads it again on each change.
- Becoming trusted reads the trusted settings first and lets the manager start servers
  (`set_trust`) only once they're applied, so no server launches with the filtered settings and
  is then restarted. Documents opened on the manager before then (parked, waiting for trust) keep
  the size limit they were opened under, so they're closed and opened again at the next frame.
- The LSP test harness (`tests/lsp.rs`) now sets `ConfigDirOverride` to a temporary folder, and its
  two in-memory `reload` helpers (`tests/lsp.rs`, `tests/lsp_aliases_reroute.rs`) write the user
  file too. `settings_are_per_window` puts the shared user file back after `s`'s reload.
- `Lsp::handed` (test only) records the settings last handed to a manager, so the untrusted case
  can be checked without a launch.
