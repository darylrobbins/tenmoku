---
task_uid: "01m5fknp6xeens86x08jq0cyb7"
task: "T201"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_authorize::a_host_actor_is_refused_with_no_policy: dispatching an Actor::Host invocation on a registry with no plugin policy returns a denial and runs nothing"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_authorize::an_undeclared_command_is_refused_to_plugins: a registered command without allow_plugins is denied to a plugin that holds every capability"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_authorize::needs_the_commands_capability_and_the_commands_own: a declared command needs commands plus each declared capability; the denial names the missing one"
    required: true
    manual: false
  - id: "C4"
    text: "plugin_authorize::path_args_must_stay_inside_the_workspace: a path argument with .., an absolute path outside the roots, and a symlink out of a root are each denied; one inside is allowed"
    required: true
    manual: false
  - id: "C5"
    text: "plugin_authorize::delegated_dispatch_keeps_the_actor_and_its_limits: a command that dispatches another through the registry runs it as the same Host actor, and the inner command is authorized against the same grants"
    required: true
    manual: false
  - id: "C6"
    text: "plugin_authorize::user_and_ai_actors_are_unaffected: User, Ai, Lsp and Format invocations dispatch exactly as before with or without a policy"
    required: true
    manual: false
  - id: "C7"
    text: "plugin_access::only_the_allowlisted_commands_are_plugin_callable: the set of editor commands with plugin access equals the ten listed in the spec (ids that do not exist are dropped and named in the finish notes), every other registered editor command is denied, editor.save and editor.undo included"
    required: true
    manual: false
  - id: "C8"
    text: "plugin_access::editor_insert_by_a_plugin_needs_buffer_write_and_uses_the_host_origin: with commands and buffer.write it inserts as Origin::Host(id); with commands alone it is denied and the buffer is unchanged"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p kiln-command -p fe-editor and cargo clippy -p kiln-command -p fe-editor --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Authorize every plugin-origin command dispatch in kiln-command against the plugin grants, default-deny, and annotate a ten-command editor allowlist as plugin-callable (Mac T1046)

## Problem

`Actor::Host("<extension id>")` is attribution, not authorization: `Registry::dispatch` checks context and arguments only
(§7). Before any plugin can invoke a command, the host must authorize each plugin-origin dispatch against the plugin's
grants, and must refuse any command whose authority is not declared. Nothing declares command authority yet.

## Change

- New `kiln/crates/kiln-command/src/authorize.rs` (`mod authorize;` and exports in `lib.rs`):
  `PluginAccess { capabilities: Vec<Capability>, path_args: Vec<&'static str> }` (the capabilities the command's effects
  need, and the names of arguments that are workspace paths), `trait PluginPolicy { fn grants(&self, plugin: &str) ->
  Option<Grants>; fn workspace_roots(&self) -> Vec<PathBuf>; }`, and `PluginDenied` naming the missing capability, the
  undeclared command or the path outside the workspace.
- `registry.rs`: `Registry::allow_plugins(&mut self, id: &str, access: PluginAccess) -> Result<(), UnknownCommand>`
  annotates an already-registered command; `Registry::set_plugin_policy(Arc<dyn PluginPolicy>)`. In `dispatch`, an
  invocation whose actor is `Actor::Host(_)` is authorized first: no policy installed means denied; the policy must grant
  the plugin `commands` and every capability the command declares; a command with no `PluginAccess` is denied; each
  declared path argument must resolve inside a workspace root (normalize `..`, resolve symlinks of the longest existing
  prefix, compare components). Other actors are unchanged. A nested dispatch that goes through the registry with the
  same actor is authorized by the same rule, so a delegated command runs with the plugin's own limits.
- New `crates/fe-editor/src/plugin_access.rs` (one `mod` line and one call in `commands.rs`): the allowlist, annotating
  only these existing commands (drop any whose id is absent): `editor.insert` and `editor.newline` with
  `buffer.write`; `editor.move`, `editor.move_line_start`, `editor.move_line_end`, `editor.move_document_start`,
  `editor.move_document_end`, `editor.select_all`, `editor.fold`, `editor.unfold` with no capability beyond
  `commands`. Everything else, `editor.save`, clipboard and undo/redo commands included, stays default-denied.

## Notes

This card is a prerequisite of the commands interface (P9). Out of scope: any plugin-registered commands (P9), an
allowlist outside the editor crate, settings. Tests are new files: `kiln/crates/kiln-command/tests/plugin_authorize.rs`
and `crates/fe-editor/tests/plugin_access.rs`.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

- A denial is `DispatchError::PluginDenied(PluginDenied)`. Adding the variant forced one match arm in
  `kiln/crates/kiln-core/src/keymap.rs` (its exhaustive match on `validate_args` errors), the only change outside the
  listed scope.
- Authorization runs after the command is looked up (an unknown command is still `UnknownCommand`) and before the
  context and args checks. `Registry::validate` authorizes the same way, so it predicts `dispatch`.
- Nested dispatch: while a plugin's dispatch runs, the registry remembers the plugin, and any dispatch nested in it on
  the same registry runs as `Actor::Host(<that plugin>)` whatever actor it names, so a delegating command can't launder
  a plugin's invocation as the user or as another plugin. A dispatch through a different registry isn't covered.
- Path args: a relative path is denied (nothing says what it's relative to). The path is resolved one component at a
  time as the OS would: each existing symlink is replaced by its target (relative targets from the link's directory),
  so `..` after a symlink is physical and a dangling symlink counts as where creating through it would write; a
  component that doesn't exist is taken as named. More than 40 symlinks (a loop) or an unreadable component is denied.
  Roots resolve the same way. A declared path arg that is absent, null or not a string is denied: one left out could
  default to anywhere. A symlink created between the check and the command's own open (TOCTOU) is out of reach of a
  check at dispatch.
- All ten allowlisted editor command ids exist; none was dropped.
