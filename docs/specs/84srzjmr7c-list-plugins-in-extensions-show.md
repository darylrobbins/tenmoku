---
task_uid: "01m6j08gbmd895h084srzjmr7c"
task: "T276"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_manage::plugin_manage_tests::rows_show_capabilities_state_and_a_full_access_marker: a plugin granted net:* shows Full access and a scoped one does not; rows show Running, Waiting for grant, Declined, or Stopped with the diagnostic; capabilities read in plain words"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_manage::plugin_manage_tests::with_the_preview_off_a_plugin_is_listed_as_needing_it: with extensions.plugins_preview off a plugin extension is listed with state Needs plugins preview and a detail naming the setting"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_manage::plugin_manage_tests::the_uninstall_confirmation_says_it_deletes_secrets: the prompt names the plugin and states that its stored secrets are deleted"
    required: true
    manual: false
  - id: "C4"
    text: "plugin_manage::plugin_manage_tests::confirming_uninstall_removes_everything: the plugin is unloaded, PluginSecrets::delete_all runs, its grant record is forgotten and its directory removed; for a symlinked directory only the link goes"
    required: true
    manual: false
  - id: "C5"
    text: "plugin_manage::plugin_manage_tests::cancelling_uninstall_changes_nothing: no unload and no deletion"
    required: true
    manual: false
  - id: "C6"
    text: "plugin_manage::plugin_manage_tests::a_failed_secret_delete_keeps_the_plugin_installed: an Unavailable secret store is reported and the plugin, its grants and its directory remain"
    required: true
    manual: false
  - id: "C7"
    text: "plugin_manage::plugin_manage_tests::revoke_removes_grants_and_unloads: after Revoke Plugin Grants the grant record is gone, the plugin is stopped and asks again on the next load"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# List plugins in Extensions: Show Installed with their capabilities, state and a full-access marker, or as needing the plugins preview, and revoke or uninstall them, the uninstall confirmation saying it deletes their secrets (Mac T1063)

## Problem

Users need to see and remove plugins. Extensions: Show Installed (`workbench.show_extensions`,
`crates/fe-workbench/src/extensions_picker.rs`) must show each plugin's capabilities and state and
mark a wildcard grant as full access (decision 2); a plugin can be revoked or uninstalled, and
uninstalling deletes its secrets and says so in its confirmation (user decision, 2026-10-21). With
`extensions.plugins_preview` off (the default, user decision 2026-11-03) a plugin extension is still
listed, as needing the preview.

## Change

Builds on the extensions picker and T1062's plugin host (`plugin_states`, `unload_plugin`,
`reevaluate`):

- `crates/fe-workbench/src/plugin_manage.rs` and `plugin_manage_tests.rs` (`mod` in `lib.rs`):
  plugin rows from `plugin_states`: requested and granted capabilities, each in plain words
  (`kiln_plugin::warnings::describe`), a "Full access" marker when
  `kiln_plugin::warnings::is_full_access` holds for the granted set, and the state: Running,
  Waiting for grant, Declined, Stopped (with its diagnostic), or Needs plugins preview (its detail
  names `extensions.plugins_preview`).
- `extensions_picker.rs`: `ExtensionState` cases for those states. Enter on a Waiting for grant row
  shows its grant prompt, as Enter on a Needs consent row opens the language server consent prompt.
- Two typed commands in `workbench_commands.rs` (ids added where `typed_command_ids_tests.rs` and
  `command_catalog_tests.rs` require), each picking from the plugins in the user's extensions
  directory: `workbench.revoke_plugin_grants`, "Extensions: Revoke Plugin Grants", and
  `workbench.uninstall_plugin`, "Extensions: Uninstall Plugin".
- Revoke: `GrantStore::forget` and unload; the next load asks again.
- Uninstall: a native confirmation (`window.prompt`) that names the plugin and says its stored
  secrets are deleted. Confirming unloads it, calls `PluginSecrets::delete_all`, `GrantStore::forget`
  and removes the extension directory (for a symlinked directory, the link only, never its target).
  Cancelling changes nothing. A failed secret delete (`SecretError::Unavailable`) is reported in a
  toast and leaves the plugin, its grant record and its directory in place.

## Notes

T1061 (the prompt and warnings) is on main as b166899b7 (helium's T301): `plugin_prompt.rs`,
`kiln/crates/kiln-plugin/src/warnings.rs`. Prompts are tested through
`VisualTestContext::pending_prompt` and `simulate_prompt_answer`, secrets through a fake
`SecretStore`. Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- `plugin_manage` keeps its own view of the state directory (`fe_config::paths::state_dir`, for
  `GRANTS_FILE` and `SECRETS_INDEX_FILE`) and the keychain, opened only when a plugin has a secret
  to delete. Tests set both with `plugin_manage::set_stores`, beside `set_plugin_dirs`.
- The two commands open the extensions picker over the user's plugins only, disabled ones
  included (from their manifests, as the host doesn't run them): `extensions_picker::open_next_for`
  sets what the next `opened` is for, as the picker entity is the workbench's own. With no plugin,
  they say so in a toast.
- Review round 1 widened the scope to `plugin_host.rs`: revoke and uninstall hold the plugin in the
  host (`hold_plugin`): it unloads, its pending request and any prompt showing it are withdrawn, and
  until `release_plugin` it isn't loaded or asked about, nor an answer to it recorded. They act once
  `plugin_settled` says no load, unload or answer being recorded remains, so a plugin's `init` or
  `shutdown` can't write a secret after `delete_all`, nor an answer recreate its grants. A revoked
  plugin's row shows no grants at once.
- A hold is exclusive: `hold_plugin` hands back a token, or nothing if the plugin is held already
  (the second operation says so in a toast), and only that token releases it. Holding and releasing
  each move the plugin's generation on, and an evaluation applies nothing to a plugin whose
  generation changed since it started, so one that decided before a revoke or uninstall can't load
  it or bring its grants back afterwards. A failed operation releases it as `Restored`: its earlier
  stopped state comes back and the host decides again without forcing, so only that plugin runs
  again, and only if it ran.
- A user plugin the host hasn't decided yet (with the preview on, its first evaluation still
  running) is listed as Starting, from its manifest, so the commands never miss an installed one.
- Uninstall checks the directory's manifest still names the plugin before deleting anything (it
  might have been replaced while the confirmation showed), and forgets the grant record before
  removing the directory: a removal that fails leaves a plugin that asks again, never a grant record
  for one that's gone. On any failure the toast says why.
- Enter on a row whose plugin waits for its grant asks through the host's prompt
  (`grant_to_ask`, `ask_for_grant`), even if it was dismissed in that window; nothing if a window
  is asking about it already. That comes before the row's server consents, which Enter reaches
  next. A row whose servers' state isn't its plugin's shows both: `Needs consent · Plugin: Needs
  plugins preview`.
- A plugin revoked or uninstalled before the host's first evaluation of it gets its slot from the
  hold, so it shows Stopped rather than Starting once that evaluation is rejected.
- Documented limit (review round 2, F13, filed as T286): when an update awaits more grants or was
  declined while its old version runs, a failed revoke or uninstall doesn't bring that old version
  back; it stays stopped until the plugin is decided again with the update granted, or reloaded.
