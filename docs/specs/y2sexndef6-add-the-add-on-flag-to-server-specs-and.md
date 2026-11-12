---
task_uid: "01m77zy6qgq9wdj1y2sexndef6"
task: "T358"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`tests::registry::add_on::add_on_never_chosen_as_lead` : a spec with add_on = true is skipped by server_for and returned after the lead by servers_for"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::registry::add_on::lead_is_first_non_add_on_and_enabled_false_drops_each` and `tests::registry::add_on::needs_restart_when_add_on_set_changes` pass (extension and host add-ons and typescript_server included)"
    required: true
    manual: false
  - id: "C3"
    text: "In crates/fe-config/tests/lsp_add_on_setting.rs: `add_on_setting_parses`, `add_on_in_schema_and_template`, `add_on_in_project_layer_is_filtered` pass"
    required: true
    manual: false
  - id: "C4"
    text: "docs/decisions/q5j03ax2af-lsp-add-on-servers.md section 1 reads \"approved by the user 2026-11-12\" and section 9 records Q1 keep, Q2 one-time info toast naming the npm package, Q3 fixed list, Q4 covered by trust; sections 6.3, 7 and 8 (W3) no longer say a missing add-on is silent"
    required: true
    manual: false
---

# Add the add-on flag to server specs and settings and pick the lead server of a language, with no routing change (LSP add-ons A1, T1645) (Mac T1731)

# Add the add-on flag to server specs and settings and pick the lead server of a language, with no routing change (LSP add-ons A1, T1645)

Spec: docs/decisions/q5j03ax2af-lsp-add-on-servers.md §3 (spec model), §8 row A1.

Row: `ServerSpec::add_on` and `LspServerSettings::add_on` (schema, template, `apply`), plus `registry::servers_for` / `Servers::lead`. `server_for` skips add-ons. No routing change.

Also edit the decision document: update §1 Status to "approved by the user 2026-11-12" and record the §9 answers: Q1 keep (javascript, typescript, tsx stay in the languages), Q2 CHANGED to a one-time info toast naming the npm package when a built-in add-on is missing (not silence), Q3 fixed list (target, build, out, vendor, .worktrees in blacklistFolders), Q4 covered by trust. Also correct §6.3, §7 UI list and the §8 W3 row where they say a missing add-on is silent: they now say a one-time info toast naming the npm package.

Tests to add: crates/fe-lsp/src/tests/registry/add_on.rs (primary/add-on/lead choice, `enabled`, `typescript_server`, extension and host add-ons, `needs_restart`); crates/fe-config/tests/lsp_add_on_setting.rs (parse, schema, project-layer filter). Declare new test files from the directory's mod.rs. If a settings label .ftl file is needed for the new key and is outside --scope, stop and say so.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

Lead additions (2026-11-12): add an optional `package` field to `ServerSpec` (the npm or other package that provides the server's command), so W3's one-time "missing add-on" toast can name it from the spec; cover it in add_on.rs. The settings label .ftl files (crates/fe-config/l10n/en-US/settings.ftl, settings-template.ftl, crates/fe-workbench/l10n/en-US/settings.ftl) are in scope for the new key's labels.
