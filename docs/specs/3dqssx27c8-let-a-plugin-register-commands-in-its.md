---
task_uid: "01m5jjkvmp9j051e3dqssx27c8"
task: "T283"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "commands::a_plugin_registers_a_namespaced_command: register during init yields a PluginCommand with id \u003cextension"
    required: true
    manual: false
  - id: "C2"
    text: "commands::bad_or_taken_names_are_refused: a name outside the namespace, a built-in id, another plugin id and a re"
    required: true
    manual: false
  - id: "C3"
    text: "commands::register_and_invoke_need_the_commands_capability: without it both fail with an error naming commands"
    required: true
    manual: false
  - id: "C4"
    text: "commands::running_a_plugin_command_calls_run_command_under_the_deadline: the guest export runs on the plugin thre"
    required: true
    manual: false
  - id: "C5"
    text: "commands::plugin_invocations_are_host_script_and_observed: an invoke dispatches Invocation{actor: Host(id), sourc"
    required: true
    manual: false
  - id: "C6"
    text: "commands::an_undeclared_command_is_refused_to_the_plugin: invoking a command with no allow_plugins annotation ret"
    required: true
    manual: false
  - id: "C7"
    text: "commands::unloading_returns_the_plugins_command_ids: after an unload the plugin commands list is the ids to remov"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Let a plugin register commands in its own namespace and invoke commands as Host with Script source, recorded and observed like a user's (Mac T1052)

## Problem

A plugin registers commands `<extension id>.<name>` and invokes commands through an `Invocation` with `actor =
Host("<extension id>")` and `source = Script`, so every plugin action is recordable, observable and replayable like a
user's (§7). The runner (P3) and the authorization (P8) exist; the commands interface does not.

## Change

`kiln/crates/kiln-plugin/src/commands.rs` (the `commands` host interface and its `State`) and `wit/commands.wit`:

- Registration: `register(name, title, args-schema, context-predicate, mutates-buffer)` is accepted only during `init`
  and only with the `commands` capability; the id is `<extension id>.<name>`, a name that is not a valid
  `CommandId` suffix, an id already taken (built-in or another plugin) or a reserved namespace is an error. The host
  collects them as `PluginCommand` values for the app to register on its `Registry` (P19); unloading a plugin returns
  the ids to remove.
- Running a plugin's command: a typed `PluginCommand::run(args) -> Result<Effect, CommandError>` that calls the guest
  exported `run-command` under the call deadline on the plugin's thread.
- Invoking: `invoke(command, args)` builds `Invocation { actor: Host(id), source: Script, .. }` and sends it to the
  host's dispatcher (a trait object the app supplies), which authorizes it through P8's policy; a refusal reaches the
  plugin as an error naming the missing capability. A plugin cannot invoke without the `commands` capability.
- `PluginPolicy` for a running plugin is implemented here from its `Grants` and the workspace roots.

## Notes

No editor command is added here and no allowlist changes (P8). Tests use `.wat` fixtures from `strings.wat`'s pattern
test `Registry<TestTarget>` and `Registry::observe`.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new

Lend note (helium lead): the lend file cut long lines (title, scope, some criteria and spec sentences); this card's title, scope and the ends of cut criteria were completed from the spec. The Mac copy is authoritative: if it differs, respec to it.

## Implementation notes

- **Scope.** `src/runner.rs` and `src/bindings.rs` were respecced in. A plugin registers during `init`, which runs
  inside `Plugin::load`, so its commands slice must already know its id, grants and host by then. The runner gains
  `Plugin::load_hosted(.., host: HostState)`, which loads with a `HostState` the caller set up (the other `load*`
  functions delegate to it). It calls `commands.begin_init()`/`end_init()` around the exported `init` only, so a
  core start function registers nothing (review F3). It also sets `HostState::deadline` for each call.
  `bindings.rs` builds `commands::State::default()`, as T282 did for secrets, and holds that deadline.
- **`CommandHost`** is what the app shares with every plugin's slice. It holds the built-in ids, the reserved
  namespaces (`reserve`), the workspace roots, each loaded plugin's grants and commands, and the app's `Dispatcher`.
  It implements `PluginPolicy`, so the app sets it on its `Registry`: a plugin it has unloaded has no grants, and the
  registry refuses it. `commands(plugin, handle)` returns the `PluginCommand`s, and `unload(plugin)` returns the ids
  to remove.
- **Names.** The WIT record keeps its full `id`, which must be `<extension id>.<name>`. The name is one or more
  `CommandId` segments, so plugin `acme` can register `acme.tool.x` and that id can clash with plugin `acme.tool`'s
  ("another plugin id"). The checks run in this order: invalid `CommandId` (`invalid-argument`), then outside the
  namespace, a built-in id, already registered (by this plugin or another), and a reserved namespace (all
  `denied`). A malformed `args-schema` or `when` is `invalid-argument`.
- **A store with no `CommandHost`** (`HostState::default()`) still answers both imports with `unimplemented`, so
  `tests/bindings.rs` passes unchanged.
- **`PluginCommand::run`** sends `Call::RunCommand` through a `PluginHandle` (`Arc<Mutex<dyn CallPlugin>>`,
  implemented for `Plugin` and `events::Recipient`). Its effect is `Changed` for a command that mutates the buffer
  and `Notify` otherwise. It fails without calling the plugin once the plugin is unloaded, or while the plugin's
  thread waits in an `invoke`.
- **Invoking never outlives the deadline (review F2).** The `Dispatcher` takes a `Request` and must not wait on the
  app. The app runs the request (`Request::run`, which skips a cancelled one) and answers it. The plugin waits for
  the answer on a condvar only until `HostState::deadline`. Past it, the request is cancelled and the plugin's call
  overruns, so the runner unloads it as timed out. A request dropped unanswered answers `failed`.
- **Attachments (review F1).** Each `State::new` is one attachment of a plugin id to the host. Registering,
  invoking and running check that the attachment is still the id's current one. `unload` revokes it, which cancels
  its requests, and the same id attaches again only once no request of a revoked attachment is out
  (`AttachError::RequestsOut`). So a store kept past an unload can't act under a reload's grants. A load that
  fails cleans up with `detach`, which checks the attachment and unloads it under one lock acquisition, so a reload
  that replaced it stays (review F4). `unloading_returns_the_plugins_command_ids` races that cleanup against an
  unload and reload for 2000 rounds. The gap the old two-lock code left is too short for a thread to land in, so
  the race test guards against regressions without reproducing the old bug; the single lock is what makes it
  correct.
- **Not done here (P19).** Registering `PluginCommand`s on the app's `Registry`, removing them on unload, and running
  `Request`s on the UI thread are the app's job.
