---
task_uid: "01m6j08g71yry6nv8eeyt7s88k"
task: "T275"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "preview_off_loads_no_plugin: with default settings a plugin in a user extension dir (the prebuilt crates/fe-plugin/tests/guests/editor/editor.wasm), even one already granted, is neither compiled (the component cache stays empty) nor loaded, no prompt shows, its commands are not registered and its state is PreviewOff (crates/fastedit/tests/plugins.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "the_preview_setting_applies_live: editing settings.toml to turn extensions.plugins_preview on loads the granted plugin with no restart; turning it off unloads it (its shutdown runs, its commands leave the palette) and keeps its grant record; on again loads it without asking (crates/fastedit/tests/plugins.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "a_workspace_extension_cannot_load_a_plugin: the same plugin under a trusted root .tenmoku/extensions is never loaded and gives a diagnostic, and a grant to the user-dir copy does not cover it (crates/fastedit/tests/plugins.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "plugin_host::plugin_host_tests::a_plugin_is_pending_until_granted: with the preview on, a new plugin loads nothing until its grant is recorded"
    required: true
    manual: false
  - id: "C5"
    text: "plugin_host::plugin_host_tests::the_prompt_follows_the_pending_grant_and_records_the_answer: the active window shows the prompt once; Allow records the grant in the GrantStore and loads the plugin; the second button records Declined and the same request is not asked again; dismissing leaves it pending"
    required: true
    manual: false
  - id: "C6"
    text: "plugin_host::plugin_host_tests::an_escalating_update_keeps_the_old_version_loaded: replacing the plugin with one asking for more leaves the old one running until Allow, then the new one runs"
    required: true
    manual: false
  - id: "C7"
    text: "plugin_host::plugin_host_tests::a_granted_plugin_registers_its_commands_and_runs_them: its command appears in the palette list as \u003cid\u003e.\u003cname\u003e, runs the plugin run-command off the main thread, and leaves the list when the plugin unloads"
    required: true
    manual: false
  - id: "C8"
    text: "plugin_host::plugin_host_tests::a_rebuilt_component_reloads_without_a_prompt: rewriting the .wasm with the same capabilities reloads the plugin (shutdown then init) and asks nothing"
    required: true
    manual: false
  - id: "C9"
    text: "plugin_host::plugin_host_tests::a_stuck_plugin_does_not_freeze_the_window: with a looping plugin the workbench keeps handling input and the plugin state ends Stopped with reason Timeout"
    required: true
    manual: false
  - id: "C10"
    text: "cargo nextest run -p fastedit -p fe-workbench and cargo clippy -p fastedit -p fe-workbench --all-targets -- -D warnings pass; cargo deny check bans licenses sources passes"
    required: true
    manual: false
---

# Run user-directory code plugins behind extensions.plugins_preview: discover them, ask for their grants, load them off the main thread, reload them, and register their commands (Mac T1062)

## Problem

The plugin host is built but headless: `kiln-plugin` (runner, limits, grants, commands, events, fs,
process, net, secrets, cache) and `fe-plugin` (the `fastedit:editor@1.0.0` world) load components in
tests only, and the install prompt (`crates/fe-workbench/src/plugin_prompt.rs`, T1061's work, which
landed on main as b166899b7 from helium's T301) is unused. Nothing in Tenmoku runs a plugin.

User decisions, 2026-11-03: code plugins ship as a **preview behind `extensions.plugins_preview`**
(default false, the setting card this depends on). With it off no plugin is compiled or loaded and no
prompt shows. Trust for the preview is local unsigned installs plus the existing grant prompts; no
signing or registry. Record: `docs/decisions/vmtnmkay62-extensions-and-plugins.md` §7.

This card is the core: the gate, discovery, grants and the prompt, loading off the main thread, hot
reload, and the plugin's own commands. Buffer access and invoking editor commands, events, and
decorations and status items are follow-up cards; so are the management list (T1063) and installs.

## Change

- `crates/fastedit/src/config.rs`, `with_extensions`: register `kiln_plugin::manifest::plugin_kind()`
  (reach it through a fe-workbench re-export, so `crates/fastedit/Cargo.toml` gains no dependency) as
  an extension kind, whatever the setting, so a plugin manifest checks quietly while the preview is
  off. It is a user-layer kind only: `WORKSPACE_KINDS` stays grammars, lsp and snippets, so a trusted
  root's `.tenmoku/extensions` plugin is refused (and `PluginManifest::from_extension` refuses an
  `ExtensionOrigin::Workspace` one too). Hand the set to the plugin host from `on_extensions`; list
  each plugin's `wasm` file in `extension_files` so a rebuilt component reloads. The extension API
  stays 1.1: bumping it would break `extension_snippets`' "a later API isn't loaded" case, and the
  setting is the gate.
- `crates/fe-workbench/src/plugin_host.rs` and `plugin_host_tests.rs` (`mod` and `pub use` in
  `lib.rs`): an app-global plugin host.
  - **Gate.** Follows `extensions.plugins_preview` live. Turned on: the same path as a reload
    (evaluate grants, prompt, load). Turned off: every plugin unloads (its `shutdown` runs, its
    commands are withdrawn); grant records are kept and nothing is asked. Off, nothing is compiled.
  - **Discovery.** A `PluginManifest` with `ExtensionOrigin::User` per plugin extension in the set
    (extensions the settings' `extensions.disabled` names are already left out, T1042). Grant state
    from a `GrantStore` at `<state dir>/plugin-grants.toml` (`kiln_plugin::grants::GRANTS_FILE`),
    decided with `GrantStore::decide`.
  - **Prompt.** A decision that needs the user becomes a `PendingPluginGrant`; `show_plugin_prompt`
    shows it once in the active window (a window activated later shows it if it is still pending), and
    the `PluginAnswer` is recorded with `GrantStore::grant` or `GrantStore::decline`. Dismissing it
    leaves it pending.
  - **Load.** Compile and instantiate on the background executor, never on the main thread or the
    paint path: `PluginEngine::shared`, a `ComponentCache` under `<cache dir>/plugins`, fe-plugin's
    linker, and a `fe_plugin::HostState` whose kiln slices are live: commands through a
    `CommandHost`, fs with the trusted roots of the open windows at load as the workspace (a plugin
    with an `fs` grant reloads when that set changes), secrets through the platform secret store,
    net through `FeNet`. The editor slices (buffer, decorations, status) stay at their default
    (`unimplemented`) until their cards.
  - **Reload.** A changed manifest or `.wasm` reloads the plugin; with the same grants it asks
    nothing. An update that asks for more keeps the old version loaded until the user answers
    (`PluginAsk::Escalation`, decision 3).
  - **Commands.** `CommandHost::reserve` every built-in namespace. A granted plugin's commands
    (`<id>.<name>`) are listed in the command palette and dispatched by the workbench through
    `RunCommand`, running `PluginCommand::run` off the main thread. Unloading withdraws them.
  - **For the list (T1063).** `plugin_states(cx)` (id, name, version, dir, requested and granted
    capabilities, and `Running`, `WaitingForGrant`, `Declined`, `Stopped(Diagnostic)` or
    `PreviewOff`), `unload_plugin(id, cx)` and `reevaluate(cx)`.
  - **Diagnostics.** A load failure or an unload is logged with its diagnostic. A component that
    does not link against this host's world says: the plugin API is a preview and can change in any
    release; rebuild the plugin against this release. (The WIT stays at @1.0.0 during the preview;
    see the SDK card.)
- `crates/fe-workbench/src/workbench.rs`: only what dispatching plugin commands and showing a pending
  prompt on activation need.
- `crates/fe-workbench/Cargo.toml`: `fe-plugin`; a dev-dependency on `wasmtime` with `wat`, so tests
  can reuse `kiln/crates/kiln-plugin/tests/fixtures/commands.wat` and `loop_forever.wat` by
  `include_str!`. `crates/fastedit/tests/plugins.rs` copies the prebuilt
  `crates/fe-plugin/tests/guests/editor/editor.wasm` into a temporary user extension directory.

## Notes

- Names after the Tenmoku rename (T1257, T1258): state dir `$XDG_STATE_HOME/tenmoku/`, workspace
  extensions `.tenmoku/extensions`, manifests `[engine] tenmoku = "1.1"`. The old spec's P7, P8,
  P10, P11 and P12 are `plugin_prompt.rs`, `crates/fe-editor/src/plugin_access.rs`,
  `kiln_plugin::events`, `fe_plugin::editor::BufferAccess` and `fe_plugin::decorations`.
- The earlier "do not touch crates/fe-gpui" note is stale and not needed here: this card does not
  touch fe-gpui.
- Host-independent: GPUI's test platform (prompts through `VisualTestContext::pending_prompt` and
  `simulate_prompt_answer`, as `plugin_prompt_tests.rs` does), no screenshots, no AppKit.

Project rules: keep tests lean and fast (hostile fixtures bounded under 2 s each); new tests go in new
files; the scope lists exact files.

## Implementation notes

- **A runner of the host's own.** kiln-plugin's `runner::Plugin` links only the `kiln:extension`
  world, and the editor guest imports `fastedit:editor/buffer`, so it can't load the plugins this
  card runs. `plugin_host.rs` carries `HostedPlugin`, the same runner (a store and thread per plugin,
  epoch deadline, memory limiter, async calls raced against the deadline) over a `Data` holding
  `fe_plugin::HostState` and WASI. Its linker adds fe-plugin's three editor interfaces through their
  generated `add_to_linker`, as `fe_plugin::add_to_linker` does, since that one is typed for a bare
  `HostState`, which has no WASI. kiln-plugin's epoch ticker only ticks for its own runner's calls
  (`ticking` is crate-private), so the host runs a small ticker of its own on the shared engine.
  `HostedPlugin` implements `CallPlugin`, so `CommandHost::commands` hands back ordinary
  `PluginCommand`s. Follow-up: make kiln-plugin's runner generic over the world and drop the copy.
- **Palette entries leave by context, not removal.** `kiln_shell::command_palette::add_commands`
  has no remove. Each load's commands are added once, with the context predicate
  `plugin_load_<n>` (and the plugin's own `when`). The workbench's root key context carries that
  flag only while the load runs (`plugin_host::key_context`), so an unloaded or replaced load's
  entries no longer apply. Follow-up: a `remove_commands` in kiln-shell.
- **States.** `PluginState::Stopped` holds a `PluginDiagnostic` with a `StopReason`: the runner's
  four reasons, plus `Load`, `Grants` and `Unloaded`. A plugin unloads in the background and reads
  `Running` until its `shutdown` has returned, then `PreviewOff` or whatever its want says. A stopped
  plugin isn't loaded again until its component, version, grants or (with an `fs` grant) roots
  change, or `reevaluate` is called. Turning the preview on counts as `reevaluate`.
- **Invoking commands.** The `CommandHost`'s dispatcher answers each invocation as not run until
  the card that lets plugins invoke editor commands lands.
- **Running commands.** Before a plugin's command is scheduled, the workbench checks it as the
  command registry would: its `when` against the invocation's context, and its args (none without an
  args schema; with one, a table with no null and with the schema's `required` keys). There's no
  JSON Schema validator in the tree, and the registry validates typed args through serde, so the
  rest of the schema (field types, constraints) isn't checked yet: follow-up T285.
- **Cancelling loads.** Each load carries a cancel flag. Turning the preview off, a plugin going,
  changing roots for a plugin that reaches files, and `unload_plugin` all set it at once. They also
  set each slot's want at once, so a stale want can't start another load before the next evaluation.
  A cancelled load stops before compiling, instantiating or `init`. A load that finishes after its
  slot stopped wanting it leaves the slot's stopped state alone and is unloaded. Under `cfg(test)`,
  `PluginHost::load_gate` holds each load before it starts, so tests can change things mid-load
  deterministically.
- **Kept versions follow the roots.** A version kept running while its update waits or was declined
  stays wanted: the slot's current `Load` want, else what it runs or is loading. Each keeps its
  artifact (manifest, component, grant record), so if it reaches files it reloads on new roots
  with its own grants. Root changes rewrite those wants at once, so a reload in flight for older
  roots is cancelled, however often the roots change or the plugins are decided again meanwhile.
- **Spawn identity.** A load's identity includes the programs its spawn grants are bound to and
  those suspended, so re-granting a changed program reloads the plugin. A spawn is killed at the
  call deadline (`process::State::with_deadline`), so a waiting program can't hold up a timeout,
  an unload or a shutdown.
- **Stopped states.** `stopped` keeps its diagnostic, so a plugin stopped by `unload_plugin`, a fault
  or a failed load shows why for as long as that same load is wanted, even when an evaluation
  lands before a cancelled load ends.
- **Spawn grants.** Each load's process slice is the decided `GrantRecord`, recorded in the
  `GrantStore`, so a granted program runs and a suspension is written back.
- **Prompts.** Each prompt shown is tracked with the request it shows. When the preview goes off,
  the plugin goes, or its request changes, the prompt is dropped (its task, and so any full-access
  prompt after it, stops). An answer is recorded only if it's still the plugin's request with the
  preview on, and still what its manifest asks for: a manifest replaced under the same id clears
  the pending request at once, before it's decided again. A prompt closed without an answer, with its window still open, isn't reopened there
  until that window is next activated; another window activated asks.
- **The ticker** fails loudly: if its thread can't start, every load is refused (`start_ticker`
  takes the spawn so a test can make it fail).
- **C2's shutdown.** The editor guest registers no commands and does nothing observable in
  `shutdown`, so `plugin_host_tests::the_preview_setting_shuts_plugins_down_live` drives the real
  settings file with the commands fixture. It checks the commands are listed, that `shutdown` has
  run by the time the plugin reads `PreviewOff`, and that the commands are withdrawn.
- **Directories.** The host takes the app's state and cache directories from `fe_config::paths`
  (main.rs is out of scope); `set_plugin_dirs` replaces them, and the tests use it. Secrets go to
  the keychain, opened on first use (memory under `cfg(test)`), with their index in
  `<state dir>/plugin-secrets.toml`.
- `fe_workbench::GrantStore` is re-exported so `crates/fastedit/tests/plugins.rs` can record a grant
  without a new dependency.
